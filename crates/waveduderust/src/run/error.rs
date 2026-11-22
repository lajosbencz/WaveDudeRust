//! Why a run failed.

use std::io;
use std::sync::Arc;

use crate::engine::EngineError;

use super::store::StoreError;

/// Why a run failed.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
	#[error("frame store: {0}")]
	Store(#[from] StoreError),
	#[error("{0}")]
	Spec(#[from] crate::spec::RunSpecError),
	#[error("engine: {0}")]
	Engine(#[from] EngineError),
	#[error("{0} panicked: {1}")]
	Panic(&'static str, String),
	#[error(transparent)]
	Failed(Arc<RunError>),
	#[error("the grid needs {needs} bytes of memory for the CPU engine, more than the {memory} bytes there are")]
	EngineMemory { needs: u64, memory: u64 },
	/// A failure of a run in another agent (the browser's run worker), as its message.
	#[error("{0}")]
	Worker(String),
}

/// Whether `kind` says the storage is full: a full disk or the browser's quota.
#[cfg_attr(not(feature = "unstable-run"), allow(unreachable_pub, reason = "public only with unstable-run"))]
pub fn storage_full(kind: io::ErrorKind) -> bool {
	matches!(kind, io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded)
}

impl RunError {
	/// Whether the storage refused the frames past memory.
	pub fn storage_full(&self) -> bool {
		match self {
			Self::Store(StoreError::Io(e)) => storage_full(e.kind()),
			Self::Failed(e) => e.storage_full(),
			_ => false,
		}
	}

	/// The panic of `what`, its payload's message when it has one.
	pub fn panic(what: &'static str, payload: &(dyn std::any::Any + Send)) -> Self {
		let message = payload
			.downcast_ref::<String>()
			.cloned()
			.or_else(|| payload.downcast_ref::<&str>().map(ToString::to_string))
			.unwrap_or_else(|| "a payload that is not text".to_owned());
		Self::Panic(what, message)
	}
}
