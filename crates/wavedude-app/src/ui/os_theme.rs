//! The OS colour scheme, read through mundy: winit reports none on Linux (X11 never, Wayland only its own
//! decorations), where mundy asks the XDG desktop portal.
//!
//! Threads: mundy calls back on its own thread; the UI thread reads the last value.

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use mundy::{ColorScheme, Interest, Preferences, Subscription};
use wavedude_widgets::ThemeKind;

const UNKNOWN: u8 = 0;
const LIGHT: u8 = 1;
const DARK: u8 = 2;
/// The wait for the first answer, so the first frame has the right theme.
const FIRST_ANSWER: Duration = Duration::from_millis(200);

static SCHEME: AtomicU8 = AtomicU8::new(UNKNOWN);

fn store(prefs: Preferences) {
	let v = match prefs.color_scheme {
		ColorScheme::Light => LIGHT,
		ColorScheme::Dark => DARK,
		ColorScheme::NoPreference => UNKNOWN,
	};
	SCHEME.store(v, Ordering::Relaxed);
}

/// Reads the scheme once, then follows its changes while the subscription lives. Call from the main thread.
pub fn watch() -> Subscription {
	if let Some(prefs) = Preferences::once_blocking(Interest::ColorScheme, FIRST_ANSWER) {
		store(prefs);
	}
	Preferences::subscribe(Interest::ColorScheme, store)
}

/// The OS preference, if it states one.
pub fn current() -> Option<ThemeKind> {
	match SCHEME.load(Ordering::Relaxed) {
		LIGHT => Some(ThemeKind::Light),
		DARK => Some(ThemeKind::Dark),
		_ => None,
	}
}
