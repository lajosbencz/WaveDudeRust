//! Writing the user settings outside the state's own saves (window size and pane arrangement at exit, the part
//! description's open state).

use super::app::Cx;

/// Saves the settings file now; a failure is logged.
pub fn persist(cx: &Cx) {
	cx.shared.peek(|a| {
		if let Err(e) = a.settings.save(&cx.info.dirs.settings_file()) {
			tracing::warn!("{e}");
		}
	});
}
