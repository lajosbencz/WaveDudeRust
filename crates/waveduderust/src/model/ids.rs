//! Session-stable item ids: minted on add, load, paste and duplicate from one process-wide counter, never saved.

use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

fn mint() -> NonZeroU64 {
	NonZeroU64::new(NEXT.fetch_add(1, Ordering::Relaxed)).expect("the id counter wrapped")
}

macro_rules! id {
	($(#[$meta:meta])* $name:ident) => {
		$(#[$meta])*
		#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
		pub struct $name(NonZeroU64);

		impl $name {
			pub(crate) fn mint() -> Self {
				Self(mint())
			}

			/// The session-local number: for agent keys and logs, never saved.
			pub fn get(self) -> u64 {
				self.0.get()
			}
		}

		/// One line, also pretty-printed.
		impl std::fmt::Debug for $name {
			fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
				write!(f, concat!(stringify!($name), "({})"), self.0)
			}
		}
	};
}

id!(
	/// A source of a project.
	SourceId
);
id!(
	/// A part of a project.
	PartId
);
id!(
	/// A probe of a project.
	ProbeId
);
id!(
	/// A colour scheme of a project.
	SchemeId
);

/// `PartialEq` by value: every field but `id`, destructured so a new field must be listed.
macro_rules! eq_by_value {
	($t:ident { $($field:ident),* $(,)? }) => {
		impl PartialEq for $t {
			fn eq(&self, other: &Self) -> bool {
				let Self { id: _, $($field),* } = self;
				$(*$field == other.$field)&&*
			}
		}
	};
}
pub(crate) use eq_by_value;

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn mints_distinct_ids() {
		let (a, b) = (SourceId::mint(), SourceId::mint());
		assert_ne!(a, b);
		assert!(a < b);
	}

	#[cfg(feature = "serde")]
	#[test]
	fn serde_skips_the_id_and_mints_one() {
		use crate::model::Source;
		let s = Source::default();
		let json = serde_json::to_string(&s).unwrap();
		assert!(!json.contains("\"id\""), "{json}");
		let back: Source = serde_json::from_str(&json).unwrap();
		assert_eq!(back, s);
		assert_ne!(back.id(), s.id());
	}
}
