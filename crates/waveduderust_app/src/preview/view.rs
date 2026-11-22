//! Where a picture lies (docs/ui/40-preview-pane.md, Zoom): the canvas fitted uniformly into its area, or zoomed in
//! on a [`View`] shared by both pictures.
//!
//! Coordinates: canvas pixels as continuous values; screen positions in points of the window.

use waveduderust::unstable::view::epaint::{Pos2, Rect, Vec2, pos2, vec2};

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

	/// A pinch's frame: zoomed by `factor` about the canvas point `at` (under the fingers' centre before the frame),
	/// then moved by the fingers' `drag` points,
	/// `scale` points per canvas pixel before the zoom. Fingers on one spot give a factor of 0/0, taken as 1.
	pub fn pinched(self, factor: f32, at: (f32, f32), drag: Vec2, scale: f32, canvas: (u32, u32)) -> Self {
		let factor = if factor.is_finite() && factor > 0. { factor } else { 1. };
		let from = self.normalised(canvas);
		let zoomed = from.zoomed(factor, at, canvas);
		zoomed.panned(drag, scale * zoomed.zoom / from.zoom)
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

	#[test]
	fn degenerate_areas_do_not_divide_by_zero() {
		let t = CanvasTransform::new(Rect::from_min_size(pos2(4., 4.), Vec2::ZERO), (0, 0), View::default());
		assert_eq!(t.scale, 0.);
		assert_eq!(t.to_canvas(pos2(3., 3.)), (0., 0.));
		assert_eq!(t.device_size(2.), (0, 0));
	}
}
