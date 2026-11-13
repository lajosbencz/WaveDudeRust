//! The state of a run, batch or live.

use std::sync::Arc;

use super::RunError;

/// Equality compares the variant; two failures are equal when they share the error.
#[derive(Clone, Debug)]
pub enum RunState {
	Created,
	Running,
	Paused,
	Completed,
	Cancelled,
	Failed(Arc<RunError>),
}

impl PartialEq for RunState {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(Self::Failed(a), Self::Failed(b)) => Arc::ptr_eq(a, b),
			_ => std::mem::discriminant(self) == std::mem::discriminant(other),
		}
	}
}

impl Eq for RunState {}

impl RunState {
	pub fn is_finished(&self) -> bool {
		matches!(self, Self::Completed | Self::Cancelled | Self::Failed(_))
	}

	pub fn is_active(&self) -> bool {
		matches!(self, Self::Running | Self::Paused)
	}
}
