//! The main window's place on the screen: restored at start, saved at exit, resized with the UI zoom and kept on
//! its monitor. winit's window is reachable from a Freya plugin alone, so [`Placement`] carries the requests to
//! it and the place it last saw back.
//!
//! Threads: the plugin runs on the event loop thread, the views on the UI thread; a mutex holds the few fields.

use std::sync::{Arc, Mutex};
use wavedude_core::sync::lock;

use freya::winit::dpi::{LogicalPosition, LogicalSize};
use freya::winit::window::{Window, WindowId};
use freya_winit::plugins::{FreyaPlugin, PluginEvent, PluginHandle};

use wavedude_app_state::settings::{Settings, WindowState};

/// Where the window is, in the monitor's logical pixels (the UI zoom does not scale them).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Place {
	pub position: Option<(f64, f64)>,
	pub maximized: bool,
}

#[derive(Default)]
struct Inner {
	main: Option<WindowId>,
	seen: Place,
	/// A size to apply, logical pixels of the monitor.
	resize: Option<(f64, f64)>,
	min_size: Option<(f64, f64)>,
}

/// The handle the views and `main` share with the plugin.
#[derive(Clone, Default)]
pub struct Placement(Arc<Mutex<Inner>>);

impl Placement {
	fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
		lock(&self.0)
	}

	/// The place last seen.
	pub fn seen(&self) -> Place {
		self.lock().seen
	}

	/// Asks for the window to be `width` x `height` logical pixels at the next redraw.
	pub fn resize(&self, width: f64, height: f64) {
		self.lock().resize = Some((width, height));
	}

	/// Asks for the smallest size the window may take.
	pub fn set_min_size(&self, width: f64, height: f64) {
		self.lock().min_size = Some((width, height));
	}

	/// The plugin to register with the launch.
	pub fn plugin(&self) -> PlacementPlugin {
		PlacementPlugin(self.clone())
	}

	/// Writes the place seen into the settings; the size of a maximised window is not the window's own.
	pub fn save(&self, settings: &mut Settings) {
		let seen = self.seen();
		settings.window_state = if seen.maximized { WindowState::Maximized } else { WindowState::Normal };
		if let Some((x, y)) = seen.position {
			settings.window_left = Some(x);
			settings.window_top = Some(y);
		}
	}
}

/// The winit window attributes a saved place asks for.
pub fn attributes(
	settings: &Settings,
	attrs: freya::winit::window::WindowAttributes,
) -> freya::winit::window::WindowAttributes {
	let mut attrs = attrs.with_maximized(settings.window_state == WindowState::Maximized);
	if let (Some(x), Some(y)) = (settings.window_left, settings.window_top) {
		attrs = attrs.with_position(LogicalPosition::new(x, y));
	}
	attrs
}

/// Keeps a window of `size` at `position` inside `monitor` (top-left and size in logical pixels).
pub fn kept_inside(position: (f64, f64), size: (f64, f64), monitor: ((f64, f64), (f64, f64))) -> (f64, f64) {
	let ((mx, my), (mw, mh)) = monitor;
	let x = position.0.min(mx + mw - size.0).max(mx);
	let y = position.1.min(my + mh - size.1).max(my);
	(x, y)
}

pub struct PlacementPlugin(Placement);

impl PlacementPlugin {
	fn after_redraw(&self, window: &Window) {
		let mut inner = self.0.lock();
		let main = *inner.main.get_or_insert_with(|| window.id());
		if window.id() != main {
			return;
		}
		let scale = window.scale_factor();
		if let Some((w, h)) = inner.min_size.take() {
			window.set_min_inner_size(Some(LogicalSize::new(w, h)));
		}
		if let Some((w, h)) = inner.resize.take() {
			let _ = window.request_inner_size(LogicalSize::new(w, h));
			if let (Ok(p), Some(m)) = (window.outer_position(), window.current_monitor()) {
				let p = p.to_logical::<f64>(scale);
				let mp = m.position().to_logical::<f64>(scale);
				let ms = m.size().to_logical::<f64>(scale);
				let kept = kept_inside((p.x, p.y), (w, h), ((mp.x, mp.y), (ms.width, ms.height)));
				if kept != (p.x, p.y) {
					window.set_outer_position(LogicalPosition::new(kept.0, kept.1));
				}
			}
		}
		inner.seen = Place {
			position: window.outer_position().ok().map(|p| {
				let p = p.to_logical::<f64>(scale);
				(p.x, p.y)
			}),
			maximized: window.is_maximized(),
		};
	}
}

impl FreyaPlugin for PlacementPlugin {
	fn plugin_id(&self) -> &'static str {
		"wavedude-placement"
	}

	fn on_event(&mut self, event: &mut PluginEvent<'_>, _: PluginHandle) {
		if let PluginEvent::AfterRedraw { window, .. } = event {
			self.after_redraw(window);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_window_is_pulled_back_onto_its_monitor() {
		let monitor = ((0., 0.), (1920., 1080.));
		assert_eq!(kept_inside((100., 100.), (800., 600.), monitor), (100., 100.));
		assert_eq!(kept_inside((1500., 900.), (800., 600.), monitor), (1120., 480.));
		assert_eq!(kept_inside((-50., -50.), (800., 600.), monitor), (0., 0.));
		assert_eq!(
			kept_inside((0., 0.), (3000., 600.), monitor),
			(0., 0.),
			"a window wider than the monitor keeps its left edge"
		);
	}

	#[test]
	fn the_place_seen_reaches_the_settings() {
		let placement = Placement::default();
		placement.lock().seen = Place { position: Some((10., 20.)), maximized: true };
		let mut s = Settings::default();
		placement.save(&mut s);
		assert_eq!((s.window_left, s.window_top, s.window_state), (Some(10.), Some(20.), WindowState::Maximized));
	}
}
