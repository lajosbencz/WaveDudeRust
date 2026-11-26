//! The messages between the page and the spill worker (decision 0020). A message is postcard; a frame's planes travel
//! beside it as its buffer, a transferred `ArrayBuffer`. The worker answers each write and read by its id, in the
//! order posted.

use std::io;

use serde::{Deserialize, Serialize};

/// The version of [`ToSpill`] and [`FromSpill`], bumped when either changes; the page refuses a worker of another.
pub const VERSION: u32 = 2;

/// The folder of the origin private file system's root that holds the tabs' session folders.
pub const FOLDER: &str = "wavedude-spill";

/// From the page to the spill worker.
#[derive(Debug, Serialize, Deserialize)]
pub enum ToSpill {
	/// The tab's session folder in [`FOLDER`], which the page made and holds; posted before the first write. A folder
	/// the worker cannot open ends it with [`FromSpill::Gone`].
	Session { name: String },
	/// Writes frame `frame` of `generation` of store `store`, its planes the buffer.
	Write { id: u64, store: u64, generation: u64, frame: usize },
	/// Reads frame `frame` of `generation` of store `store`.
	Read { id: u64, store: u64, generation: u64, frame: usize },
	/// Removes the frames of `generation` of store `store`.
	DropGeneration { store: u64, generation: u64 },
	/// Removes the frames of store `store`.
	DropStore { store: u64 },
	/// The bytes a new store may spill: the storage free, with the frames this tab holds counted as free.
	Free { id: u64 },
}

/// From the spill worker to the page.
#[derive(Debug, Serialize, Deserialize)]
pub enum FromSpill {
	/// The worker listens: its [`VERSION`]. Stays the first variant with the version alone, so the page reads it from
	/// a worker of any version.
	Ready { version: u32 },
	/// Write or read `id` completed; a read's planes are the buffer.
	Done { id: u64, result: Result<(), Failure> },
	/// The worker answers nothing more: why.
	Gone(String),
	/// [`ToSpill::Free`] `id`'s bytes.
	Free { id: u64, result: Result<u64, Failure> },
}

/// A failed storage operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
	pub kind: FailureKind,
	pub message: String,
}

/// The [`io::ErrorKind`]s a [`Failure`] tells apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureKind {
	QuotaExceeded,
	NotFound,
	Unsupported,
	Other,
}

impl FailureKind {
	/// The kind of a failed storage call by its error's `name`, a `DOMException`'s.
	pub fn of_name(name: Option<&str>) -> Self {
		match name {
			Some("QuotaExceededError") => Self::QuotaExceeded,
			Some("NotFoundError") => Self::NotFound,
			_ => Self::Other,
		}
	}
}

impl From<FailureKind> for io::ErrorKind {
	fn from(kind: FailureKind) -> Self {
		match kind {
			FailureKind::QuotaExceeded => Self::QuotaExceeded,
			FailureKind::NotFound => Self::NotFound,
			FailureKind::Unsupported => Self::Unsupported,
			FailureKind::Other => Self::Other,
		}
	}
}

impl From<Failure> for io::Error {
	fn from(failure: Failure) -> Self {
		Self::new(failure.kind.into(), failure.message)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_ready_message_reads_from_its_variant_and_version_alone() {
		let bytes = postcard::to_allocvec(&(0u32, 7u32)).unwrap();
		assert!(matches!(postcard::from_bytes(&bytes).unwrap(), FromSpill::Ready { version: 7 }));
	}

	#[test]
	fn a_storage_errors_name_gives_its_kind() {
		let kind = |name| io::ErrorKind::from(FailureKind::of_name(name));
		assert_eq!(kind(Some("QuotaExceededError")), io::ErrorKind::QuotaExceeded);
		assert_eq!(kind(Some("NotFoundError")), io::ErrorKind::NotFound);
		assert_eq!(kind(Some("InvalidStateError")), io::ErrorKind::Other);
		assert_eq!(kind(None), io::ErrorKind::Other);
	}
}
