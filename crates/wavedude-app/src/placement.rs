//! The main window's place on the screen: restored at start, saved at exit, resized with the UI zoom and kept on
//! its monitor. Positions are the monitor's logical pixels, which the UI zoom does not scale; sizes are unzoomed.

use eframe::egui;
use wavedude_app_state::settings::{Settings, WindowState};
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::window::Window;

/// Where the window is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Place {
	pub position: Option<(f64, f64)>,
	/// Unzoomed; `None` while maximised, whose size is not the window's own.
	pub size: Option<(f64, f64)>,
	pub maximized: bool,
}

impl Place {
	/// The place of `window` shown at `zoom`.
	pub fn of(window: &Window, zoom: f64) -> Self {
		let scale = window.scale_factor();
		let maximized = window.is_maximized();
		Self {
			position: window.outer_position().ok().map(|p| p.to_logical::<f64>(scale).into()),
			size: (!maximized).then(|| {
				let s = window.inner_size().to_logical::<f64>(scale);
				(s.width / zoom, s.height / zoom)
			}),
			maximized,
		}
	}

	/// Writes the place into the settings.
	pub fn save(&self, settings: &mut Settings) {
		settings.window_state = if self.maximized { WindowState::Maximized } else { WindowState::Normal };
		if let Some((x, y)) = self.position {
			settings.window_left = Some(x);
			settings.window_top = Some(y);
		}
		if let Some((w, h)) = self.size {
			settings.window_width = w;
			settings.window_height = h;
		}
	}
}

/// The window the settings ask for, at their zoom, at least `min` (unzoomed) large; built before egui zooms.
pub fn viewport(settings: &Settings, min: (f64, f64)) -> egui::ViewportBuilder {
	let zoom = settings.ui_zoom;
	let zoomed = |(w, h): (f64, f64)| [(w * zoom) as f32, (h * zoom) as f32];
	let v = egui::ViewportBuilder::default()
		.with_inner_size(zoomed((settings.window_width, settings.window_height)))
		.with_min_inner_size(zoomed(min))
		.with_maximized(settings.window_state == WindowState::Maximized);
	match (settings.window_left, settings.window_top) {
		(Some(x), Some(y)) => v.with_position([x as f32, y as f32]),
		_ => v,
	}
}

/// Scales `window` by `ratio` after a zoom change, keeps it on its monitor and sets its smallest size `min`.
pub fn rezoom(window: &Window, ratio: f64, min: (f64, f64)) {
	window.set_min_inner_size(Some(LogicalSize::new(min.0, min.1)));
	if window.is_maximized() {
		return;
	}
	let scale = window.scale_factor();
	let s = window.inner_size().to_logical::<f64>(scale);
	let size = (s.width * ratio, s.height * ratio);
	let _ = window.request_inner_size(LogicalSize::new(size.0, size.1));
	if let (Ok(p), Some(m)) = (window.outer_position(), window.current_monitor()) {
		let p = p.to_logical::<f64>(scale);
		let mp = m.position().to_logical::<f64>(scale);
		let ms = m.size().to_logical::<f64>(scale);
		let kept = kept_inside((p.x, p.y), size, ((mp.x, mp.y), (ms.width, ms.height)));
		if kept != (p.x, p.y) {
			window.set_outer_position(LogicalPosition::new(kept.0, kept.1));
		}
	}
}

/// Keeps a window of `size` at `position` inside `monitor` (top-left and size in logical pixels).
pub fn kept_inside(position: (f64, f64), size: (f64, f64), monitor: ((f64, f64), (f64, f64))) -> (f64, f64) {
	let ((mx, my), (mw, mh)) = monitor;
	let x = position.0.min(mx + mw - size.0).max(mx);
	let y = position.1.min(my + mh - size.1).max(my);
	(x, y)
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
		let mut s = Settings::default();
		Place { position: Some((10., 20.)), size: None, maximized: true }.save(&mut s);
		assert_eq!((s.window_left, s.window_top, s.window_state), (Some(10.), Some(20.), WindowState::Maximized));
		assert_eq!((s.window_width, s.window_height), (1280., 800.), "a maximised window keeps its own size");
		Place { position: None, size: Some((900., 600.)), maximized: false }.save(&mut s);
		assert_eq!((s.window_width, s.window_height, s.window_state), (900., 600., WindowState::Normal));
	}
}
