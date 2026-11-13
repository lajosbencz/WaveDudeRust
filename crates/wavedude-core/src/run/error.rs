//! Why a run failed, for the batch run and the live run.

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
	#[error("could not start a thread: {0}")]
	Thread(#[source] std::io::Error),
	#[error("{0} panicked: {1}")]
	Panic(&'static str, String),
	#[error("the run was cancelled")]
	Cancelled,
	#[error(transparent)]
	Failed(Arc<RunError>),
}

impl RunError {
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
