//! The page's end of the spill worker: its port, the session's start and the stores' spills.

use std::cell::{OnceCell, RefCell};
use std::io;
use std::task::{Context, Poll};

use js_sys::Uint8Array;
use wasm_bindgen_futures::spawn_local;
use waveduderust::run::{SpillBackend, SpillFactory, StoreLayout, Token};
use web_time::Instant;

use super::requests::{Opened, Pending, Requests, Settled};
use super::session;
use crate::protocol::Lost;
use crate::spill::{FromSpill, ToSpill};
use crate::web::{PagePort, Wire};
use crate::{Buffer, Envelope, Port, envelope_of};

/// The spill worker's script, beside the page.
const SPILL_WORKER: &str = "spill-worker.js";

/// The spill worker's URL: [`SPILL_WORKER`] with the page's build as its query, which the script passes on to its
/// files, so the browser's cache never serves the worker of another build.
fn spill_worker_url() -> String {
	format!("{SPILL_WORKER}?build={}", js_sys::encode_uri_component(&crate::build_id()))
}

/// This tab's spill: the spill worker and the session folder.
pub struct Opfs {
	/// The session made, or found impossible.
	prepared: Token<()>,
}

impl Opfs {
	/// Starts the spill worker, takes this tab's session and starts removing the sessions of tabs gone; the worker
	/// is told the session once it is made. `wake` is called as an operation completes. Once per page: a second
	/// start panics.
	pub fn start(wake: impl Fn() + 'static) -> Self {
		Link::start(Box::new(wake));
		let prepared = Token::default();
		let done = prepared.clone();
		spawn_local(async move {
			match session::prepare().await {
				Ok(name) => post(&ToSpill::Session { name }, None),
				Err(e) => fail(format!("the browser storage cannot be used: {e}")),
			}
			done.complete(Ok(()));
			with_link(|link| (link.wake)());
		});
		Self { prepared }
	}

	/// A factory of one run's spills; the factories share the worker and the session.
	pub fn factory(&self) -> Box<dyn SpillFactory> {
		Box::new(Factory { prepared: self.prepared.clone(), free: RefCell::default() })
	}
}

/// One run's spills: the free storage asked for its next store.
struct Factory {
	prepared: Token<()>,
	/// The free storage for the next store, asked for once ready is polled.
	free: RefCell<Option<Token<u64>>>,
}

impl SpillFactory for Factory {
	fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<()> {
		if self.prepared.poll_done(cx).is_pending() {
			return Poll::Pending;
		}
		if gone() {
			return Poll::Ready(());
		}
		self.free
			.borrow_mut()
			.get_or_insert_with(|| {
				let token = Token::default();
				request(Pending::Free(token.clone()), |id| ToSpill::Free { id }, None);
				token
			})
			.poll_done(cx)
	}

	fn open(&self, store: u64, _layout: &StoreLayout, bytes: u64) -> io::Result<Box<dyn SpillBackend>> {
		let free = self.free.take().and_then(|free| free.take());
		Ok(match with_link(|link| link.state.borrow_mut().open(free, bytes))? {
			Opened::Spill => Box::new(Spill { store }),
			Opened::Dropped => Box::new(Dropped),
		})
	}

	fn not_needed(&self) {
		self.free.take();
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

/// A spill that keeps nothing.
struct Dropped;

impl SpillBackend for Dropped {
	fn write(&self, _: u64, _: usize, _: &[&[u8]]) -> Token<()> {
		let token = Token::default();
		token.complete(Ok(()));
		token
	}

	fn read(&self, _: u64, frame: usize) -> Token<Vec<u8>> {
		let token = Token::default();
		token.complete(Err(io::Error::other(format!("frame {frame} was past memory and dropped"))));
		token
	}

	fn drop_generation(&self, _: u64) {}
}

/// The spill worker's [`Wire`]: a loss arrives as [`FromSpill::Gone`], and what the page posts before the worker
/// listens waits in order.
struct SpillWire;

impl Wire for SpillWire {
	fn lost(lost: Lost) -> Envelope {
		envelope_of(&FromSpill::Gone(lost.to_string()), None)
	}

	fn hold(held: &mut Vec<Envelope>, envelope: Envelope) {
		held.push(envelope);
	}
}

thread_local! {
	/// The page's end of the spill worker, once the start made it.
	static LINK: OnceCell<Link> = const { OnceCell::new() };
}

/// The spill worker and the operations it has not answered.
struct Link {
	port: PagePort<SpillWire>,
	state: RefCell<Requests>,
	wake: Box<dyn Fn()>,
}

impl Link {
	/// Starts the spill worker; once per tab.
	fn start(wake: Box<dyn Fn()>) {
		let port = PagePort::start(&spill_worker_url(), receive);
		let made = LINK.with(|link| link.set(Self { port, state: RefCell::default(), wake }).is_ok());
		assert!(made, "one start per tab");
		// A worker that could not be made is lost already.
		receive();
	}

	/// Ends the worker for `why`: the operations in flight, failed with it.
	fn fail(&self, why: String) -> Vec<Settled> {
		let failed = self.state.borrow_mut().fail(why.clone());
		if failed.is_some() {
			tracing::debug!("{why}");
			self.port.stop();
		}
		failed.unwrap_or_default()
	}
}

fn with_link<T>(f: impl FnOnce(&Link) -> T) -> T {
	LINK.with(|link| f(link.get().expect("the spill worker is started first")))
}

/// Whether the spill worker answers nothing more.
fn gone() -> bool {
	with_link(|link| link.state.borrow().gone().is_some())
}

/// Settles `settled` and calls the page's wake.
fn settle(settled: Vec<Settled>) {
	if settled.is_empty() {
		return;
	}
	for (pending, result) in settled {
		pending.settle(result);
	}
	with_link(|link| (link.wake)());
}

/// Ends the worker for `why`, settling the operations in flight.
fn fail(why: String) {
	settle(with_link(|link| link.fail(why)));
}

/// Handles the spill worker's messages waiting.
fn receive() {
	while let Some(envelope) = with_link(|link| link.port.receive()) {
		let handled = with_link(|link| link.state.borrow_mut().handle(envelope));
		match handled {
			Ok(settled) => settle(settled),
			Err(why) => fail(why),
		}
	}
}

/// Posts `message` with `buffer`, unless the worker is gone.
fn post(message: &ToSpill, buffer: Option<Buffer>) {
	with_link(|link| {
		if link.state.borrow().gone().is_none() {
			link.port.post(envelope_of(message, buffer));
		}
	});
}

/// Starts `pending`: posts `message` of its id with `buffer`, or fails it at once when the worker is gone.
fn request(pending: Pending, message: impl FnOnce(u64) -> ToSpill, buffer: Option<Buffer>) {
	let refused = with_link(|link| {
		let id = link.state.borrow_mut().start(pending)?;
		link.port.post(envelope_of(&message(id), buffer));
		Ok(())
	});
	if let Err((pending, e)) = refused {
		settle(vec![(pending, Err(e))]);
	}
}
