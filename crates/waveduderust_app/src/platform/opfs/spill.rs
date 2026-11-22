//! The frames past a store's memory ([`waveduderust::run::SpillBackend`]) in the spill worker (decision 0020), which
//! keeps each store generation in one file of this tab's session folder, `<session>/<store>-<generation>`. The page
//! posts each write with the frame's planes, each read and each removal ([`crate::spill`] has the
//! messages), and completes an operation's [`Token`] as the worker answers it.
//!
//! The start, each write and each read wake the window as they complete ([`crate::wake`]). A store's spill is
//! refused when its frames do not fit the free storage the worker answers as the store is about to open. A spill worker that does not start,
//! stops or speaks another protocol version is ended and fails every operation in flight and after with the reason.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::io;
use std::sync::Mutex;
use std::task::{Context, Poll};

use crate::spill::{FromSpill, ToSpill, VERSION};
use js_sys::Uint8Array;
use wasm_bindgen_futures::spawn_local;
use waveduderust::run::{SpillBackend, SpillFactory, StoreLayout, Token};
use waveduderust::unstable::sync::lock;

use super::session::{prepare, session};
use crate::platform::Instant;
use crate::remote::protocol::Lost;
use crate::remote::web::{PagePort, Wire};
use crate::remote::{Buffer, Envelope, Port};

/// The spill worker's script, beside the page.
const SPILL_WORKER: &str = "spill-worker.js";

/// The spill worker's URL: [`SPILL_WORKER`] with the page's build as its query, which the script passes on to its
/// files, so the browser's cache never serves the worker of another build.
fn spill_worker_url() -> String {
	format!("{SPILL_WORKER}?build={}", js_sys::encode_uri_component(&crate::build_id()))
}

/// The spills of this tab's stores.
pub struct Opfs {
	/// This tab's session taken.
	prepared: Token<()>,
	taken: Mutex<Option<io::Result<()>>>,
	/// The free storage for the next store, asked for once ready is polled.
	free: Mutex<Option<Token<u64>>>,
}

impl Opfs {
	/// Starts the spill worker, takes this tab's session and starts removing the sessions of tabs gone; the worker
	/// is told the session once it is ready. Once per page: a second start panics.
	pub fn start() -> Self {
		Link::start();
		let prepared = Token::default();
		let done = prepared.clone();
		spawn_local(async move {
			let result = prepare().await;
			match &result {
				Ok(()) => {
					let name = session().expect("prepared").name();
					post(&ToSpill::Session { name }, None);
				}
				Err(e) => tracing::warn!("frames past memory cannot be spilled: {e}"),
			}
			done.complete(result);
			crate::wake::wake();
		});
		Self { prepared, taken: Mutex::new(None), free: Mutex::new(None) }
	}
}

impl SpillFactory for Opfs {
	fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<()> {
		if self.prepared.poll_done(cx).is_pending() {
			return Poll::Pending;
		}
		let mut free = lock(&self.free);
		let free = free.get_or_insert_with(|| {
			let token = Token::default();
			request(Pending::Free(token.clone()), |id| ToSpill::Free { id }, None);
			token
		});
		free.poll_done(cx)
	}

	fn open(&self, store: u64, _layout: &StoreLayout, bytes: u64) -> io::Result<Box<dyn SpillBackend>> {
		let mut taken = lock(&self.taken);
		if taken.is_none() {
			*taken = self.prepared.take();
		}
		if let Err(e) = taken.as_ref().expect("open follows ready") {
			return Err(io::Error::new(e.kind(), e.to_string()));
		}
		let free = lock(&self.free).take().and_then(|free| free.take()).expect("open follows ready");
		match free {
			Err(e) => Err(e),
			Ok(free) if bytes > free => {
				let gb = |b: u64| b as f64 / f64::from(1u32 << 30);
				Err(io::Error::new(
					io::ErrorKind::StorageFull,
					format!(
						"the frames past memory need {:.2} GB of browser storage, {:.2} GB are free",
						gb(bytes),
						gb(free)
					),
				))
			}
			Ok(_) => match gone() {
				Some(why) => Err(io::Error::other(why)),
				None => Ok(Box::new(Spill { store })),
			},
		}
	}

	fn not_needed(&self) {
		lock(&self.free).take();
	}
}

/// One store's frames.
struct Spill {
	store: u64,
}

impl SpillBackend for Spill {
	fn write(&self, generation: u64, frame: usize, planes: &[&[u8]]) -> Token<()> {
		let data = Uint8Array::new_with_length(planes.iter().map(|p| p.len() as u32).sum());
		let mut at = 0;
		for plane in planes {
			let end = at + plane.len() as u32;
			data.subarray(at, end).copy_from(plane);
			at = end;
		}
		let token = Token::default();
		let store = self.store;
		request(
			Pending::Write(token.clone()),
			|id| ToSpill::Write { id, store, generation, frame },
			Some(Buffer::from(data)),
		);
		token
	}

	fn read(&self, generation: u64, frame: usize) -> Token<Vec<u8>> {
		let token = Token::default();
		let store = self.store;
		let pending = Pending::Read { token: token.clone(), frame, started: Instant::now() };
		request(pending, |id| ToSpill::Read { id, store, generation, frame }, None);
		token
	}

	fn drop_generation(&self, generation: u64) {
		post(&ToSpill::DropGeneration { store: self.store, generation }, None);
	}
}

impl Drop for Spill {
	fn drop(&mut self) {
		post(&ToSpill::DropStore { store: self.store }, None);
	}
}

/// The spill worker's [`Wire`]: a loss arrives as [`FromSpill::Gone`], and what the page posts before the worker
/// listens waits in order.
struct SpillWire;

impl Wire for SpillWire {
	fn lost(lost: Lost) -> Envelope {
		envelope(&FromSpill::Gone(lost.to_string()), None)
	}

	fn hold(held: &mut Vec<Envelope>, envelope: Envelope) {
		held.push(envelope);
	}
}

fn envelope(message: &impl serde::Serialize, buffer: Option<Buffer>) -> Envelope {
	Envelope { message: postcard::to_allocvec(message).expect("a message of the spill protocol encodes"), buffer }
}

thread_local! {
	/// The page's end of the spill worker, once the start made it.
	static LINK: OnceCell<Link> = const { OnceCell::new() };
}

/// The spill worker and the operations it has not answered.
struct Link {
	port: PagePort<SpillWire>,
	state: RefCell<Requests>,
}

#[derive(Default)]
struct Requests {
	/// The id of the next operation.
	next: u64,
	/// By id.
	pending: HashMap<u64, Pending>,
	/// Why the worker answers nothing more.
	gone: Option<String>,
}

/// An operation in flight.
enum Pending {
	Write(Token<()>),
	Read { token: Token<Vec<u8>>, frame: usize, started: Instant },
	Free(Token<u64>),
}

/// The worker's answer to an operation.
enum Answer {
	/// A write's or read's, a read's planes the buffer.
	Done(Option<Buffer>),
	Free(u64),
}

impl Pending {
	/// Completes the operation with `result` and wakes the window.
	fn settle(self, result: io::Result<Answer>) {
		let mismatch = || io::Error::other("the spill worker answered another operation");
		match self {
			Self::Write(token) => token.complete(result.map(drop)),
			Self::Read { token, frame, started } => {
				let planes = result.and_then(|answer| {
					let Answer::Done(buffer) = answer else { return Err(mismatch()) };
					let buffer = buffer.ok_or_else(|| io::Error::other("a spilled frame came back without planes"))?;
					Ok(buffer.to_vec(0..buffer.len()))
				});
				tracing::debug!(frame, ms = started.elapsed().as_secs_f64() * 1e3, "spilled frame read back");
				token.complete(planes);
			}
			Self::Free(token) => token.complete(result.and_then(|answer| match answer {
				Answer::Free(bytes) => Ok(bytes),
				Answer::Done(_) => Err(mismatch()),
			})),
		}
		crate::wake::wake();
	}
}

impl Link {
	/// Starts the spill worker; once per tab.
	fn start() {
		let port = PagePort::start(&spill_worker_url(), receive);
		let made = LINK.with(|link| link.set(Self { port, state: RefCell::default() }).is_ok());
		assert!(made, "one start per tab");
		// A worker that could not be made is lost already.
		receive();
	}

	/// Handles `envelope`: the operations it completes, with their outcomes.
	fn handle(&self, envelope: Envelope) -> Vec<(Pending, io::Result<Answer>)> {
		let mut state = self.state.borrow_mut();
		if state.gone.is_some() {
			return Vec::new();
		}
		let why = match postcard::from_bytes::<FromSpill>(&envelope.message) {
			Ok(FromSpill::Ready { version }) if version == VERSION => return Vec::new(),
			Ok(FromSpill::Ready { version }) => {
				format!("the spill worker speaks protocol version {version}, the page {VERSION}")
			}
			Ok(FromSpill::Done { id, result }) => match state.pending.remove(&id) {
				Some(pending) => {
					return vec![(pending, result.map(|()| Answer::Done(envelope.buffer)).map_err(io::Error::from))];
				}
				None => format!("the spill worker answered operation {id}, which is not in flight"),
			},
			Ok(FromSpill::Free { id, result }) => match state.pending.remove(&id) {
				Some(pending) => return vec![(pending, result.map(Answer::Free).map_err(io::Error::from))],
				None => format!("the spill worker answered operation {id}, which is not in flight"),
			},
			Ok(FromSpill::Gone(why)) => format!("the spill worker stopped: {why}"),
			Err(e) => format!("the spill worker sent an unreadable message: {e}"),
		};
		tracing::warn!("{why}");
		self.port.stop();
		let failed = state.pending.drain().map(|(_, pending)| (pending, Err(io::Error::other(why.clone())))).collect();
		state.gone = Some(why);
		failed
	}
}

fn with_link<T>(f: impl FnOnce(&Link) -> T) -> T {
	LINK.with(|link| f(link.get().expect("the spill worker is started first")))
}

/// Why the spill worker answers nothing more.
fn gone() -> Option<String> {
	with_link(|link| link.state.borrow().gone.clone())
}

/// Handles the spill worker's messages waiting.
fn receive() {
	while let Some(envelope) = with_link(|link| link.port.receive()) {
		for (pending, result) in with_link(|link| link.handle(envelope)) {
			pending.settle(result);
		}
	}
}

/// Posts `message` with `buffer`, unless the worker is gone.
fn post(message: &ToSpill, buffer: Option<Buffer>) {
	with_link(|link| {
		if link.state.borrow().gone.is_none() {
			link.port.post(envelope(message, buffer));
		}
	});
}

/// Starts `pending`: posts `message` of its id with `buffer`, or fails it at once when the worker is gone.
fn request(pending: Pending, message: impl FnOnce(u64) -> ToSpill, buffer: Option<Buffer>) {
	let refused = with_link(|link| {
		let mut state = link.state.borrow_mut();
		if let Some(why) = &state.gone {
			return Some((pending, io::Error::other(why.clone())));
		}
		let id = state.next;
		state.next += 1;
		state.pending.insert(id, pending);
		drop(state);
		link.port.post(envelope(&message(id), buffer));
		None
	});
	if let Some((pending, e)) = refused {
		pending.settle(Err(e));
	}
}
