//! The live run in another agent (the browser's run worker, decision 0024): the messages between the page and
//! the worker ([`protocol`]), the worker's end ([`Host`]) and the page's ([`Remote`], a live-run [`Backend`]).
//!
//! The worker's stores keep no frames: each frame crosses to the page as one buffer, into a mirror store that the
//! preview, presenter and status read. The page acknowledges a frame once its mirror holds it, so the worker is
//! at most the spill's writes in flight ([`StoreLayout::writes_in_flight`]) ahead. Messages arrive in the order posted.
//!
//! In the browser the ends are `web::PagePort` and `web::WorkerPort` over `postMessage`.
//!
//! Threads: none; each end is driven by its owner's calls (`Host::pump`, `Backend::pump`).
//!
//! [`Backend`]: crate::live::Backend
//! [`StoreLayout::writes_in_flight`]: wavedude_core::run::StoreLayout::writes_in_flight

mod host;
mod page;
pub mod protocol;
#[cfg(target_arch = "wasm32")]
pub mod web;

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
	port.post(envelope(message, buffer));
}

fn envelope(message: &impl Serialize, buffer: Option<Vec<u8>>) -> Envelope {
	Envelope { message: postcard::to_allocvec(message).expect("a message of the protocol encodes"), buffer }
}

fn decode<M: DeserializeOwned>(envelope: &Envelope) -> postcard::Result<M> {
	postcard::from_bytes(&envelope.message)
}

/// Queues `envelope` behind `held` for a worker not listening yet. A goal or a rebuild replaces the one held
/// before, which the worker would take up only to drop; the rest keep their order, so an image stays before the
/// goal naming it.
#[cfg(any(test, target_arch = "wasm32"))]
fn hold(held: &mut Vec<Envelope>, envelope: Envelope) {
	let replacing = |e: &Envelope| {
		let message: protocol::ToWorker = decode(e).expect("the page posts messages of the protocol");
		matches!(message, protocol::ToWorker::Goal { .. } | protocol::ToWorker::Rebuild { .. })
			.then(|| std::mem::discriminant(&message))
	};
	if let Some(kind) = replacing(&envelope) {
		held.retain(|e| replacing(e) != Some(kind));
	}
	held.push(envelope);
}

#[cfg(test)]
mod tests {
	use wavedude_core::run::Goal;

	use super::protocol::ToWorker;
	use super::*;

	#[test]
	fn a_held_goal_or_rebuild_replaces_the_one_before_and_the_rest_keep_their_order() {
		let goal = |revision| ToWorker::Goal { goal: Goal { revision, ..Goal::default() }, image: None };
		let messages = [
			ToWorker::Init { use_gpu: true },
			goal(1),
			ToWorker::Rebuild { use_gpu: false },
			ToWorker::Image { key: 7 },
			goal(2),
			ToWorker::Discard,
			ToWorker::Rebuild { use_gpu: true },
		];
		let mut held = Vec::new();
		for message in &messages {
			hold(&mut held, envelope(message, None));
		}
		let held: Vec<String> = held.iter().map(|e| format!("{:?}", decode::<ToWorker>(e).unwrap())).collect();
		let wanted: Vec<String> = [&messages[0], &messages[3], &messages[4], &messages[5], &messages[6]]
			.iter()
			.map(|m| format!("{m:?}"))
			.collect();
		assert_eq!(held, wanted);
	}
}
