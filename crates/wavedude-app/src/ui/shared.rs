//! The one handle every view uses to reach the application state.
//!
//! The state lives on the UI thread in a `RefCell`; views read it inside a closure and never hold a borrow
//! across an await. The state keeps a change counter per part ([`Revisions`]); after every mutation the handle
//! mirrors the counters that moved into Freya states, so a view that reads with [`Topics`] re-renders only when
//! a part it shows changed.

use std::cell::RefCell;
use std::ops::BitOr;
use std::rc::Rc;
use std::time::Instant;

use freya::prelude::*;

use wavedude_app_state::live::CoreBackend;
use wavedude_app_state::{AppState, Event, Response, Revisions};

pub type App = AppState<CoreBackend>;

/// The parts of the state a read subscribes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Topics(u8);

impl Topics {
	pub const NONE: Self = Self(0);
	/// The project, its file, undo and redo.
	pub const DOCUMENT: Self = Self(1);
	pub const SELECTION: Self = Self(2);
	/// Position, frame shown, frame count.
	pub const PLAYBACK: Self = Self(4);
	/// Run state, playing, loop, speed, range, follow.
	pub const RUN: Self = Self(8);
	/// The user settings and the language.
	pub const SETTINGS: Self = Self(16);
	/// A run's steps done and published frames: every tick of a run.
	pub const PROGRESS: Self = Self(32);
	/// A scratch frame in flight.
	pub const SCRATCH: Self = Self(64);

	fn has(self, other: Self) -> bool {
		self.0 & other.0 != 0
	}
}

impl BitOr for Topics {
	type Output = Self;

	fn bitor(self, other: Self) -> Self {
		Self(self.0 | other.0)
	}
}

/// The Freya mirror of [`Revisions`], one state per topic.
#[derive(Clone, Copy)]
struct Signals {
	document: State<u64>,
	selection: State<u64>,
	playback: State<u64>,
	progress: State<u64>,
	scratch: State<u64>,
	run: State<u64>,
	settings: State<u64>,
}

impl Signals {
	fn subscribe(&self, topics: Topics) {
		let pairs = [
			(Topics::DOCUMENT, self.document),
			(Topics::SELECTION, self.selection),
			(Topics::PLAYBACK, self.playback),
			(Topics::PROGRESS, self.progress),
			(Topics::SCRATCH, self.scratch),
			(Topics::RUN, self.run),
			(Topics::SETTINGS, self.settings),
		];
		for (topic, state) in pairs {
			if topics.has(topic) {
				let _ = state.read();
			}
		}
	}

	fn mirror(&self, r: Revisions) {
		let pairs = [
			(self.document, r.document),
			(self.selection, r.selection),
			(self.playback, r.playback),
			(self.progress, r.progress),
			(self.scratch, r.scratch),
			(self.run, r.run),
			(self.settings, r.settings),
		];
		for (mut state, value) in pairs {
			state.set_if_modified(value);
		}
	}
}

/// Shared application state plus its change signals.
#[derive(Clone)]
pub struct Shared {
	app: Rc<RefCell<App>>,
	signals: Signals,
}

impl PartialEq for Shared {
	fn eq(&self, other: &Self) -> bool {
		Rc::ptr_eq(&self.app, &other.app)
	}
}

impl Shared {
	/// Provides the handle to the whole tree; call once in the root component.
	pub fn provide(app: App) -> Self {
		let r = app.revisions();
		let signals = Signals {
			document: use_state(|| r.document),
			selection: use_state(|| r.selection),
			playback: use_state(|| r.playback),
			progress: use_state(|| r.progress),
			scratch: use_state(|| r.scratch),
			run: use_state(|| r.run),
			settings: use_state(|| r.settings),
		};
		use_provide_context(move || Shared { app: Rc::new(RefCell::new(app)), signals })
	}

	/// The handle from context, subscribing to nothing.
	pub fn get() -> Self {
		consume_context::<Shared>()
	}

	/// Reads the state without subscribing to changes.
	pub fn peek<R>(&self, f: impl FnOnce(&App) -> R) -> R {
		f(&self.app.borrow())
	}

	/// Reads the state and subscribes the caller to `topics`.
	pub fn read<R>(&self, topics: Topics, f: impl FnOnce(&App) -> R) -> R {
		self.signals.subscribe(topics);
		f(&self.app.borrow())
	}

	/// Subscribes the caller to `topics` without reading.
	pub fn subscribe(&self, topics: Topics) {
		self.signals.subscribe(topics);
	}

	/// Mutates the state without a re-render: for bookkeeping no view shows.
	pub fn peek_mut<R>(&self, f: impl FnOnce(&mut App) -> R) -> R {
		f(&mut self.app.borrow_mut())
	}

	/// Mutates the state and re-renders the views subscribed to the parts that changed.
	pub fn update<R>(&self, f: impl FnOnce(&mut App) -> R) -> R {
		let (r, revisions) = {
			let mut app = self.app.borrow_mut();
			let r = f(&mut app);
			app.sync_revisions();
			(r, app.revisions())
		};
		self.signals.mirror(revisions);
		r
	}

	/// Sends an event to the state at the current time.
	pub fn send(&self, event: Event) -> Response {
		self.update(|app| app.handle(event, Instant::now()))
	}

	/// Advances the state's timers; the root component calls it on its tick. True when a view's part changed.
	pub fn tick(&self) -> bool {
		self.update(|app| app.tick(Instant::now()))
	}

	/// The counters as of the last mutation, without subscribing.
	pub fn revisions(&self) -> Revisions {
		self.app.borrow().revisions()
	}
}

#[cfg(test)]
thread_local! {
	static RUNS: RefCell<std::collections::HashMap<&'static str, usize>> = RefCell::default();
}

/// Counts a memo's runs by name, for the tests that prove a memo's topics.
#[cfg(test)]
pub(crate) fn count_run(name: &'static str) {
	RUNS.with_borrow_mut(|r| *r.entry(name).or_default() += 1);
}

#[cfg(test)]
pub(crate) fn runs(name: &str) -> usize {
	RUNS.with_borrow(|r| r.get(name).copied().unwrap_or(0))
}
