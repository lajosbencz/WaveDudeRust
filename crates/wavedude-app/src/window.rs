//! The main window's constants: its smallest size, the state's tick and the dock's panes.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The tick period of the state loop while something moves; while nothing does, the window sleeps until an input
/// or [`wavedude_app_state::wake`] wakes it.
pub const TICK: Duration = Duration::from_millis(16);
/// The smallest window, unzoomed logical pixels.
pub const MIN_SIZE: (f64, f64) = (800.0, 500.0);
/// The dock's panes, in the order View > Panes lists them; the Preview is the centre, the others dock around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pane {
	Preview,
	Project,
	Properties,
	Messages,
}

impl Pane {
	pub const ALL: [Pane; 4] = [Pane::Preview, Pane::Project, Pane::Properties, Pane::Messages];

	/// The string key of its title.
	pub fn title_key(self) -> &'static str {
		match self {
			Pane::Preview => "PanePreview",
			Pane::Project => "PaneProject",
			Pane::Properties => "PaneProperties",
			Pane::Messages => "PaneMessages",
		}
	}
}
