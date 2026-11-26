//! The live run in another agent (the browser's run worker, decision 0024): the messages between the page and the
//! worker ([`protocol`]), the worker's end ([`Host`]) and the page's ([`Remote`]).
//!
//! The worker's stores keep no frames: each frame crosses to the page as one buffer, into a mirror store that the
//! preview and status read. The page acknowledges a frame once its mirror holds it, so the worker is at most the
//! spill's writes in flight ([`StoreLayout::writes_in_flight`]) ahead. Messages arrive in the order posted. In the
//! browser the ends are `web::PagePort` and `web::WorkerPort` over `postMessage`.
//!
//! Threads: none; each end is driven by its owner's calls ([`Host::pump`], [`Remote::pump`]).
//!
//! [`StoreLayout::writes_in_flight`]: waveduderust::run::StoreLayout::writes_in_flight

mod host;
mod page;
pub mod protocol;
#[cfg(target_arch = "wasm32")]
pub mod web;

use std::collections::VecDeque;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde::de::DeserializeOwned;
use waveduderust::unstable::sync::lock;

pub use host::Host;
pub use page::Remote;
use protocol::Lost;

/// This build: the version and, after a `+`, the build `tools/build-web.sh` names in `WAVEDUDE_BUILD` (its commit),
/// so the browser's page and run worker of one build tell each other apart from those of another.
pub fn build_id() -> String {
	match option_env!("WAVEDUDE_BUILD") {
		Some(build) => format!("{}+{build}", env!("CARGO_PKG_VERSION")),
		None => env!("CARGO_PKG_VERSION").to_owned(),
	}
}

/// Why the run worker runs nothing: no run starts until the page is reloaded.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WorkerGone {
	#[error("the run worker could not start: {0}")]
	NotStarted(Lost),
	#[error("the run worker stopped: {0}")]
	Stopped(Lost),
	#[error("the run worker speaks protocol version {worker}, the page {page}")]
	OtherVersion { page: u32, worker: u32 },
	#[error("the run worker is build {worker}, the page {page}")]
	OtherBuild { page: String, worker: String },
}

/// A message as it crosses: the encoded message and the buffer beside it.
#[derive(Debug, Default)]
pub struct Envelope {
	pub message: Vec<u8>,
	/// Moved, not copied, where the transport allows (a transferred `ArrayBuffer` on the web).
	pub buffer: Option<Buffer>,
}

/// The bytes beside a message: on wasm32 a JS `Uint8Array` outside the wasm memory, so a frame received is
/// copied once, into the planes that keep it.
#[derive(Debug)]
pub struct Buffer(#[cfg(target_arch = "wasm32")] js_sys::Uint8Array, #[cfg(not(target_arch = "wasm32"))] Vec<u8>);

impl Buffer {
	#[cfg(not(target_arch = "wasm32"))]
	pub fn len(&self) -> usize {
		self.0.len()
	}

	#[cfg(target_arch = "wasm32")]
	pub fn len(&self) -> usize {
		self.0.length() as usize
	}

	pub fn is_empty(&self) -> bool {
		self.len() == 0
	}

	/// Copies the bytes from `from` on into `out`; panics past the end.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn copy_to(&self, from: usize, out: &mut [u8]) {
		out.copy_from_slice(&self.0[from..from + out.len()]);
	}

	/// Copies the bytes from `from` on into `out`; panics past the end.
	#[cfg(target_arch = "wasm32")]
	pub fn copy_to(&self, from: usize, out: &mut [u8]) {
		self.0.subarray(from as u32, (from + out.len()) as u32).copy_to(out);
	}

	/// A copy of the bytes in `range`; panics past the end.
	pub fn to_vec(&self, range: Range<usize>) -> Vec<u8> {
		let mut out = vec![0; range.len()];
		self.copy_to(range.start, &mut out);
		out
	}
}

impl From<Vec<u8>> for Buffer {
	#[cfg(not(target_arch = "wasm32"))]
	fn from(bytes: Vec<u8>) -> Self {
		Self(bytes)
	}

	#[cfg(target_arch = "wasm32")]
	fn from(bytes: Vec<u8>) -> Self {
		Self(js_sys::Uint8Array::from(&bytes[..]))
	}
}

#[cfg(target_arch = "wasm32")]
impl From<js_sys::Uint8Array> for Buffer {
	fn from(bytes: js_sys::Uint8Array) -> Self {
		Self(bytes)
	}
}

/// One end of the channel between the page and the worker.
pub trait Port {
	fn post(&self, envelope: Envelope);
	/// The next envelope the other end posted, if one is waiting.
	fn receive(&self) -> Option<Envelope>;
}

/// One end of an in-process pair: what one end posts, the other receives.
pub struct Loopback {
	inbox: Arc<Mutex<VecDeque<Envelope>>>,
	outbox: Arc<Mutex<VecDeque<Envelope>>>,
}

impl Loopback {
	pub fn pair() -> (Self, Self) {
		let (a, b) = (Arc::<Mutex<VecDeque<Envelope>>>::default(), Arc::<Mutex<VecDeque<Envelope>>>::default());
		(Self { inbox: a.clone(), outbox: b.clone() }, Self { inbox: b, outbox: a })
	}
}

impl Port for Loopback {
	fn post(&self, envelope: Envelope) {
		lock(&self.outbox).push_back(envelope);
	}

	fn receive(&self) -> Option<Envelope> {
		lock(&self.inbox).pop_front()
	}
}

fn post(port: &impl Port, message: &impl Serialize, buffer: Option<Vec<u8>>) {
	port.post(envelope(message, buffer));
}

fn envelope(message: &impl Serialize, buffer: Option<Vec<u8>>) -> Envelope {
	let message = postcard::to_allocvec(message).expect("a message of the protocol encodes");
	Envelope { message, buffer: buffer.map(Buffer::from) }
}

fn decode<M: DeserializeOwned>(envelope: &Envelope) -> postcard::Result<M> {
	postcard::from_bytes(&envelope.message)
}

/// The protocol version of a first message ([`protocol::ToWorker::Init`], [`protocol::ToPage::Ready`]), read in
/// every version: their variant and version lead; `None` for another message.
fn version_of(envelope: &Envelope) -> Option<u32> {
	let ((variant, version), _) = postcard::take_from_bytes::<(u32, u32)>(&envelope.message).ok()?;
	(variant == 0).then_some(version)
}

/// Queues `envelope` behind `held` for a worker not listening yet. A goal replaces the one held before, which the
/// worker would take up only to drop; the rest keep their order, so an image stays before the goal naming it.
#[cfg(any(test, target_arch = "wasm32"))]
fn hold(held: &mut Vec<Envelope>, envelope: Envelope) {
	let goal = |e: &Envelope| {
		let message: protocol::ToWorker = decode(e).expect("the page posts messages of the protocol");
		matches!(message, protocol::ToWorker::Goal { .. })
	};
	if goal(&envelope) {
		held.retain(|e| !goal(e));
	}
	held.push(envelope);
}

#[cfg(test)]
mod tests {
	use waveduderust::run::Goal;

	use super::protocol::{ToPage, ToWorker};
	use super::*;

	#[test]
	fn a_held_goal_replaces_the_one_before_and_the_rest_keep_their_order() {
		let goal = |revision| ToWorker::Goal { goal: Goal { revision, ..Goal::default() }, image: None };
		let messages = [
			ToWorker::Init { version: protocol::VERSION, build: build_id() },
			goal(1),
			ToWorker::Image { key: 7 },
			ToWorker::Discard,
			goal(2),
			ToWorker::Image { key: 8 },
		];
		let mut held = Vec::new();
		for message in &messages {
			hold(&mut held, envelope(message, None));
		}
		let held: Vec<String> = held.iter().map(|e| format!("{:?}", decode::<ToWorker>(e).unwrap())).collect();
		let wanted: Vec<String> = [&messages[0], &messages[2], &messages[3], &messages[4], &messages[5]]
			.iter()
			.map(|m| format!("{m:?}"))
			.collect();
		assert_eq!(held, wanted);
	}

	#[test]
	fn a_first_message_names_its_version_and_no_other_does() {
		let init = ToWorker::Init { version: protocol::VERSION, build: build_id() };
		assert_eq!(version_of(&envelope(&init, None)), Some(protocol::VERSION));
		let ready = ToPage::Ready { version: 7, build: build_id() };
		assert_eq!(version_of(&envelope(&ready, None)), Some(7));
		assert_eq!(version_of(&envelope(&ToWorker::Discard, None)), None);
	}
}
