//! Where a picture lies (docs/ui/40-preview-pane.md, Zoom): the canvas fitted uniformly into its area, or zoomed in
//! on a [`View`] shared by both pictures.
//!
//! Coordinates: canvas pixels as continuous values; screen positions in points of the window.

use wavedude_view::epaint::{Pos2, Rect, Vec2, pos2, vec2};

/// Zoom over the fit: 1 is the fit, more shows the part of the canvas around `centre` (canvas pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
	pub zoom: f32,
	pub centre: (f32, f32),
}

impl View {
	pub const MAX_ZOOM: f32 = 64.;
	/// One wheel notch.
	pub const STEP: f32 = 1.25;

	/// The fit of `canvas`.
	pub fn fit(canvas: (u32, u32)) -> Self {
		Self { zoom: 1., centre: (canvas.0 as f32 / 2., canvas.1 as f32 / 2.) }
	}

	/// Zoomed by `factor` about the canvas point `at`, which stays under the pointer; never out past the fit.
	pub fn zoomed(self, factor: f32, at: (f32, f32), canvas: (u32, u32)) -> Self {
		let zoom = (self.zoom * factor).clamp(1., Self::MAX_ZOOM);
		if zoom <= 1. {
			return Self::fit(canvas);
		}
		let k = zoom / self.zoom;
		Self { zoom, centre: (at.0 - (at.0 - self.centre.0) / k, at.1 - (at.1 - self.centre.1) / k) }
	}

	/// The fit of `canvas` while not zoomed in, so the centre is the canvas's whatever it was.
	pub fn normalised(self, canvas: (u32, u32)) -> Self {
		if self.zoom <= 1. { Self::fit(canvas) } else { self }
	}

	/// Moved by a pointer drag of `delta` points at `scale` points per canvas pixel.
	pub fn panned(self, delta: Vec2, scale: f32) -> Self {
		if self.zoom <= 1. || scale <= 0. {
			return self;
		}
		Self { centre: (self.centre.0 - delta.x / scale, self.centre.1 - delta.y / scale), ..self }
	}
}

impl Default for View {
	fn default() -> Self {
		Self { zoom: 1., centre: (0., 0.) }
	}
}

/// A canvas in an area: fitted uniformly and centred, or zoomed in on a [`View`], when the picture is larger than
/// the area and shows the part around the view's centre, held so that no area is left empty.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasTransform {
	/// Points per canvas pixel.
	pub scale: f32,
	/// The picture's top-left; left of or above the area when zoomed.
	pub origin: Pos2,
	pub canvas: (u32, u32),
	/// Where the picture shows.
	pub area: Rect,
}

impl CanvasTransform {
	pub fn new(area: Rect, canvas: (u32, u32), view: View) -> Self {
		let (cw, ch) = (canvas.0.max(1) as f32, canvas.1.max(1) as f32);
		let size = area.size().max(Vec2::ZERO);
		let scale = (size.x / cw).min(size.y / ch) * view.zoom.max(1.);
		let picture = vec2(cw, ch) * scale;
		// Centred where the picture fits the axis; where it is larger, around the view's centre, held so the
		// picture covers the area.
		let place = |a: f32, p: f32, centre: f32| {
			if p <= a { (a - p) / 2. } else { (a / 2. - centre * scale).clamp(a - p, 0.) }
		};
		let origin = area.min + vec2(place(size.x, picture.x, view.centre.0), place(size.y, picture.y, view.centre.1));
		Self { scale, origin, canvas, area }
	}

	/// The whole picture; larger than the area when zoomed.
	pub fn picture(&self) -> Rect {
		Rect::from_min_size(self.origin, vec2(self.canvas.0 as f32, self.canvas.1 as f32) * self.scale)
	}

	/// The part of the picture inside the area.
	pub fn visible(&self) -> Rect {
		self.picture().intersect(self.area)
	}

	/// The part of the canvas the visible picture shows, canvas pixels; None when all of it.
	pub fn window(&self) -> Option<Rect> {
		let visible = self.visible();
		if visible == self.picture() || self.scale <= 0. {
			return None;
		}
		let (x0, y0) = self.to_canvas(visible.min);
		let (x1, y1) = self.to_canvas(visible.max);
		Some(Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)))
	}

	/// Canvas pixel coordinates (pixel centres at +0.5) to the screen.
	pub fn to_screen(&self, x: f32, y: f32) -> Pos2 {
		self.origin + vec2(x, y) * self.scale
	}

	pub fn to_canvas(&self, p: Pos2) -> (f32, f32) {
		if self.scale <= 0. {
			return (0., 0.);
		}
		let c = (p - self.origin) / self.scale;
		(c.x, c.y)
	}

	/// The visible picture's size in whole device pixels at `pixels_per_point`.
	pub fn device_size(&self, pixels_per_point: f32) -> (u32, u32) {
		let size = self.visible().size().max(Vec2::ZERO) * pixels_per_point;
		(size.x.round() as u32, size.y.round() as u32)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn area(w: f32, h: f32) -> Rect {
		Rect::from_min_size(pos2(4., 4.), vec2(w, h))
	}

	#[test]
	fn fit_is_uniform_and_centred() {
		let t = CanvasTransform::new(area(200., 100.), (400, 100), View::default());
		assert_eq!(t.scale, 0.5);
		assert_eq!(t.picture(), Rect::from_min_size(pos2(4., 29.), vec2(200., 50.)));
		assert_eq!(t.to_screen(200., 50.), pos2(104., 54.));
		assert_eq!(t.to_canvas(pos2(104., 54.)), (200., 50.));
		assert_eq!(t.device_size(1.5), (300, 75));
		assert_eq!(t.window(), None);
		let tall = CanvasTransform::new(area(100., 200.), (100, 100), View::default());
		assert_eq!(tall.picture(), Rect::from_min_size(pos2(4., 54.), vec2(100., 100.)));
	}

	#[test]
	fn a_zoomed_view_keeps_the_point_under_the_pointer_and_covers_the_area() {
		let canvas = (400, 200);
		let fit = View::fit(canvas);
		let at = |view| CanvasTransform::new(area(200., 100.), canvas, view);
		assert_eq!(at(fit), at(View::default()));
		// Zoom in twice about the canvas point (200, 100): it stays where it was on screen.
		let screen = at(fit).to_screen(200., 100.);
		let zoomed = fit.zoomed(2., (200., 100.), canvas);
		let after = at(zoomed);
		assert_eq!(after.scale, 1.);
		assert_eq!(after.to_screen(200., 100.), screen);
		assert_eq!(after.visible(), area(200., 100.), "the picture fills the area");
		assert_eq!(after.window(), Some(Rect::from_min_size(pos2(100., 50.), vec2(200., 100.))));
		assert_eq!(after.device_size(1.), (200, 100));
		// Zooming out below the fit is the fit; a centre near the edge is held so the picture covers the area.
		assert_eq!(zoomed.zoomed(0.25, (0., 0.), canvas), fit);
		let edge = at(View { zoom: 2., centre: (390., 190.) });
		assert_eq!(edge.window(), Some(Rect::from_min_size(pos2(200., 100.), vec2(200., 100.))));
		let panned = View { zoom: 2., centre: (200., 100.) }.panned(vec2(-50., 0.), 1.);
		assert_eq!(panned.centre, (250., 100.));
		assert_eq!(fit.panned(vec2(-50., 0.), 1.), fit);
	}

	#[test]
	fn degenerate_areas_do_not_divide_by_zero() {
		let t = CanvasTransform::new(Rect::from_min_size(pos2(4., 4.), Vec2::ZERO), (0, 0), View::default());
		assert_eq!(t.scale, 0.);
		assert_eq!(t.to_canvas(pos2(3., 3.)), (0., 0.));
		assert_eq!(t.device_size(2.), (0, 0));
	}
}
