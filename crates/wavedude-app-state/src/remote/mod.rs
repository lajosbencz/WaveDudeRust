//! The live run in another agent (the browser's run worker, decision 0024): the messages between the page and
//! the worker ([`protocol`]), the worker's end ([`Host`]) and the page's ([`Remote`], a live-run [`Backend`]).
//!
//! The worker's stores keep no frames: each frame crosses to the page as one buffer, into a mirror store that the
//! preview, presenter and status read. The page acknowledges a frame once its mirror holds it, so the worker is
//! at most the spill's writes in flight ([`WRITES_IN_FLIGHT`]) ahead. Messages arrive in the order posted.
//!
//! Threads: none; each end is driven by its owner's calls (`Host::pump`, `Backend::pump`).
//!
//! [`Backend`]: crate::live::Backend
//! [`WRITES_IN_FLIGHT`]: wavedude_core::run::WRITES_IN_FLIGHT

mod host;
mod page;
pub mod protocol;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde::de::DeserializeOwned;
use wavedude_core::sync::lock;

pub use host::Host;
pub use page::Remote;

/// A message as it crosses: the encoded message and the buffer beside it.
#[derive(Debug, Default)]
pub struct Envelope {
	pub message: Vec<u8>,
	/// Moved, not copied, where the transport allows (a transferred `ArrayBuffer` on the web).
	pub buffer: Option<Vec<u8>>,
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
	let message = postcard::to_allocvec(message).expect("a message of the protocol encodes");
	port.post(Envelope { message, buffer });
}

fn decode<M: DeserializeOwned>(envelope: &Envelope) -> postcard::Result<M> {
	postcard::from_bytes(&envelope.message)
}
