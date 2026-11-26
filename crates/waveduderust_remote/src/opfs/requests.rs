//! The page's operations in flight with the spill worker, what the worker's answers do to them, and the spill a store
//! opens with: the worker's, or none once the worker is gone.

use std::collections::HashMap;
use std::io;

use waveduderust::run::Token;
use web_time::Instant;

use crate::Envelope;
use crate::spill::{FromSpill, VERSION};

/// An operation in flight.
pub(super) enum Pending {
	Write(Token<()>),
	Read { token: Token<Vec<u8>>, frame: usize, started: Instant },
	Free(Token<u64>),
}

/// The worker's answer to an operation.
pub(super) enum Answer {
	/// A write's or read's, a read's planes the buffer.
	Done(Option<crate::Buffer>),
	Free(u64),
}

/// An operation and its outcome, to be settled.
pub(super) type Settled = (Pending, io::Result<Answer>);

impl Pending {
	/// Completes the operation with `result`.
	pub(super) fn settle(self, result: io::Result<Answer>) {
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
	}
}

/// The operations in flight and why the worker answers nothing more.
#[derive(Default)]
pub(super) struct Requests {
	/// The id of the next operation.
	next: u64,
	/// By id.
	pending: HashMap<u64, Pending>,
	gone: Option<String>,
	/// A store dropping its frames past memory said why.
	warned: bool,
}

impl Requests {
	/// Why the worker answers nothing more.
	pub(super) fn gone(&self) -> Option<&str> {
		self.gone.as_deref()
	}

	/// Puts `pending` in flight: its id, or it back with the reason when the worker is gone.
	pub(super) fn start(&mut self, pending: Pending) -> Result<u64, (Pending, io::Error)> {
		if let Some(why) = &self.gone {
			return Err((pending, io::Error::other(why.clone())));
		}
		let id = self.next;
		self.next += 1;
		self.pending.insert(id, pending);
		Ok(id)
	}

	/// The operations `envelope` completes, with their outcomes; why the worker is to end when it said it ended or
	/// broke the protocol. Nothing once it is gone.
	pub(super) fn handle(&mut self, envelope: Envelope) -> Result<Vec<Settled>, String> {
		if self.gone.is_some() {
			return Ok(Vec::new());
		}
		let not_in_flight = |id| format!("the spill worker answered operation {id}, which is not in flight");
		match postcard::from_bytes::<FromSpill>(&envelope.message) {
			Ok(FromSpill::Ready { version }) if version == VERSION => Ok(Vec::new()),
			Ok(FromSpill::Ready { version }) => {
				Err(format!("the spill worker speaks protocol version {version}, the page {VERSION}"))
			}
			Ok(FromSpill::Done { id, result }) => match self.pending.remove(&id) {
				Some(pending) => {
					Ok(vec![(pending, result.map(|()| Answer::Done(envelope.buffer)).map_err(io::Error::from))])
				}
				None => Err(not_in_flight(id)),
			},
			Ok(FromSpill::Free { id, result }) => match self.pending.remove(&id) {
				Some(pending) => Ok(vec![(pending, result.map(Answer::Free).map_err(io::Error::from))]),
				None => Err(not_in_flight(id)),
			},
			Ok(FromSpill::Gone(why)) => Err(format!("the spill worker stopped: {why}")),
			Err(e) => Err(format!("the spill worker sent an unreadable message: {e}")),
		}
	}

	/// The worker is gone for `why`: the operations in flight fail with it; `None` when it was gone already.
	pub(super) fn fail(&mut self, why: String) -> Option<Vec<Settled>> {
		if self.gone.is_some() {
			return None;
		}
		let failed = self.pending.drain().map(|(_, pending)| (pending, Err(io::Error::other(why.clone())))).collect();
		self.gone = Some(why);
		Some(failed)
	}

	/// The spill of a store of `bytes` past memory, `free` the storage answer readied for it: the worker's, or none
	/// once the worker is gone, the first such store warning why.
	pub(super) fn open(&mut self, free: Option<io::Result<u64>>, bytes: u64) -> io::Result<Opened> {
		if let Some(why) = &self.gone {
			if !std::mem::replace(&mut self.warned, true) {
				tracing::warn!("frames past memory are dropped, not spilled: {why}");
			}
			return Ok(Opened::Dropped);
		}
		let free = free.expect("open follows ready")?;
		if bytes > free {
			let gb = |b: u64| b as f64 / f64::from(1u32 << 30);
			return Err(io::Error::new(
				io::ErrorKind::StorageFull,
				format!(
					"the frames past memory need {:.2} GB of browser storage, {:.2} GB are free",
					gb(bytes),
					gb(free)
				),
			));
		}
		Ok(Opened::Spill)
	}
}

/// The spill a store opens with.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Opened {
	/// The worker's files.
	Spill,
	/// None: the frames past memory are dropped.
	Dropped,
}

#[cfg(test)]
mod tests {
	use serde::Serialize;

	use super::*;
	use crate::envelope_of;
	use crate::spill::Failure;

	fn from(message: &impl Serialize) -> Envelope {
		envelope_of(message, None)
	}

	fn settle(settled: Vec<Settled>) {
		for (pending, result) in settled {
			pending.settle(result);
		}
	}

	#[test]
	fn a_session_the_worker_cannot_open_fails_the_free_storage_asked_and_the_next_store_drops() {
		let mut requests = Requests::default();
		let free = Token::default();
		requests.start(Pending::Free(free.clone())).ok().unwrap();
		let why = requests
			.handle(from(&FromSpill::Gone("the session folder s could not be opened".into())))
			.map(drop)
			.unwrap_err();
		settle(requests.fail(why).unwrap());
		assert!(free.is_done());
		assert_eq!(requests.gone(), Some("the spill worker stopped: the session folder s could not be opened"));
		let free = free.take();
		assert!(free.as_ref().unwrap().is_err());
		assert_eq!(requests.open(free, 1 << 30).unwrap(), Opened::Dropped);
		assert_eq!(requests.open(None, 1 << 30).unwrap(), Opened::Dropped, "every store after drops too");
		assert!(requests.start(Pending::Free(Token::default())).is_err(), "nothing is posted any more");
	}

	#[test]
	fn the_free_storage_answered_opens_a_store_that_fits_and_refuses_one_that_does_not() {
		let mut requests = Requests::default();
		let free = Token::default();
		let id = requests.start(Pending::Free(free.clone())).ok().unwrap();
		settle(requests.handle(from(&FromSpill::Free { id, result: Ok(100) })).unwrap());
		assert_eq!(requests.open(free.take(), 100).unwrap(), Opened::Spill);
		let e = requests.open(Some(Ok(99)), 100).unwrap_err();
		assert_eq!(e.kind(), io::ErrorKind::StorageFull);
		let failed = Failure { kind: crate::spill::FailureKind::Unsupported, message: "no storage".into() };
		let free = Token::default();
		let id = requests.start(Pending::Free(free.clone())).ok().unwrap();
		settle(requests.handle(from(&FromSpill::Free { id, result: Err(failed) })).unwrap());
		assert_eq!(requests.open(free.take(), 1).unwrap_err().kind(), io::ErrorKind::Unsupported);
	}

	#[test]
	fn a_write_and_a_read_complete_by_their_ids_the_read_with_its_planes() {
		let mut requests = Requests::default();
		let (write, read) = (Token::default(), Token::default());
		let w = requests.start(Pending::Write(write.clone())).ok().unwrap();
		let r = requests.start(Pending::Read { token: read.clone(), frame: 3, started: Instant::now() }).ok().unwrap();
		let answer = envelope_of(&FromSpill::Done { id: r, result: Ok(()) }, Some(vec![1, 2, 3].into()));
		settle(requests.handle(answer).unwrap());
		assert!(!write.is_done());
		assert_eq!(read.take().unwrap().unwrap(), [1, 2, 3]);
		settle(requests.handle(from(&FromSpill::Done { id: w, result: Ok(()) })).unwrap());
		assert!(write.take().unwrap().is_ok());
	}

	#[test]
	fn another_version_or_an_answer_not_in_flight_ends_the_worker() {
		let mut requests = Requests::default();
		assert!(requests.handle(from(&FromSpill::Ready { version: VERSION })).unwrap().is_empty());
		assert!(requests.handle(from(&FromSpill::Ready { version: VERSION + 1 })).is_err());
		assert!(requests.handle(from(&FromSpill::Done { id: 7, result: Ok(()) })).is_err());
	}
}
