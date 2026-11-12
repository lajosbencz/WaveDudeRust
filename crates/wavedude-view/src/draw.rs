//! A draw list without a toolkit: shapes in the units of the canvas they are drawn on, in drawing order, for the
//! desktop's Skia and the browser's Canvas2D. Colours are RGBA8 with straight alpha.
//!
//! The overlay's shapes are in an element's logical pixels; a graph box's are in device pixels of a
//! [`DeviceGrid`], so its hairlines and text land on whole device pixels.

use serde::{Serialize, Serializer};

/// An RGBA8 colour, alpha not premultiplied; serialised as a CSS colour, `#rrggbbaa`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgba {
	pub r: u8,
	pub g: u8,
	pub b: u8,
	pub a: u8,
}

impl Rgba {
	pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
		Self { r, g, b, a }
	}

	/// Opaque.
	pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
		Self::new(r, g, b, 255)
	}
}

impl Serialize for Rgba {
	fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.collect_str(&format_args!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a))
	}
}

/// An axis-aligned rectangle by its edges.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Rect {
	pub left: f32,
	pub top: f32,
	pub right: f32,
	pub bottom: f32,
}

impl Rect {
	pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
		Self { left, top, right, bottom }
	}
}

/// A stroke. `antialias` is the desktop's and not serialised; a browser always antialiases.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Pen {
	pub color: Rgba,
	pub width: f32,
	/// On and off lengths, starting with on.
	pub dash: Option<[f32; 2]>,
	/// Round joins and caps, else miter joins and butt caps.
	pub round: bool,
	#[serde(skip_serializing)]
	pub antialias: bool,
}

impl Pen {
	/// Solid and antialiased.
	pub const fn new(color: Rgba, width: f32) -> Self {
		Self { color, width, dash: None, round: false, antialias: true }
	}

	pub const fn dashed(self, on: f32, off: f32) -> Self {
		Self { dash: Some([on, off]), ..self }
	}

	pub const fn round(self) -> Self {
		Self { round: true, ..self }
	}

	pub const fn aliased(self) -> Self {
		Self { antialias: false, ..self }
	}
}

/// How a circle or rectangle is painted.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum Ink {
	Stroke(Pen),
	/// `antialias` as on a [`Pen`].
	Fill {
		color: Rgba,
		#[serde(skip_serializing)]
		antialias: bool,
	},
}

impl Ink {
	/// An antialiased fill.
	pub const fn fill(color: Rgba) -> Self {
		Self::Fill { color, antialias: true }
	}
}

/// The stroke drawn under a text: round joins, antialiased.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Halo {
	pub color: Rgba,
	pub width: f32,
}

/// Which line of a text its position names, and with it the font.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Baseline {
	/// The alphabetic baseline, in the platform's default typeface (a generic sans-serif in a browser).
	Alphabetic,
	/// The top of the line box, the font's ascent above the alphabetic baseline, in the element's font families.
	/// Canvas2D draws it with `textBaseline = "alphabetic"` at `y + fontBoundingBoxAscent`, not with `"top"`.
	Top,
}

/// A filled antialiased disc.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Disc {
	pub radius: f32,
	pub color: Rgba,
}

/// One drawing operation.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum Shape {
	Line {
		from: (f32, f32),
		to: (f32, f32),
		pen: Pen,
	},
	/// Straight segments through `points`.
	Path {
		points: Vec<(f32, f32)>,
		closed: bool,
		pen: Pen,
	},
	Circle {
		centre: (f32, f32),
		radius: f32,
		ink: Ink,
	},
	Rect {
		rect: Rect,
		ink: Ink,
	},
	/// One line of text at `size` in the font of its [`Baseline`], its halo first. The desktop draws a halo under
	/// [`Baseline::Alphabetic`] text only and panics on a halo under [`Baseline::Top`] text, and on `Top` text
	/// drawn without the element's fonts.
	Text {
		text: String,
		at: (f32, f32),
		baseline: Baseline,
		size: f32,
		color: Rgba,
		halo: Option<Halo>,
	},
	/// A `halo` disc under every point, then a `dot` disc on every point; points relative to `origin`, clipped to
	/// `size` from it. Many dots that change rarely: the desktop draws them through a cached bitmap and panics when
	/// given none.
	Dots {
		origin: (f32, f32),
		size: (f32, f32),
		points: Vec<(f32, f32)>,
		halo: Disc,
		dot: Disc,
	},
	/// Clips what follows up to the matching [`Shape::Unclip`].
	Clip {
		rect: Rect,
		/// The desktop's, as on a [`Pen`].
		#[serde(skip_serializing)]
		antialias: bool,
	},
	Unclip,
}

/// Text metrics of the renderer, in the pixels of the layout or shapes made with them.
pub trait TextMetrics {
	fn line_height(&self) -> f32;
	fn width(&self, text: &str) -> f32;
}

/// An element's logical pixels mapped onto whole device pixels: `scale` device pixels per logical pixel and the
/// fraction of a device pixel by which the element's origin lies past the device grid.
///
/// In device units integers lie on the screen's pixel grid; [`DeviceGrid::x`] and [`DeviceGrid::y`] map the
/// element's logical coordinates into them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeviceGrid {
	pub scale: f32,
	pub fx: f32,
	pub fy: f32,
}

impl DeviceGrid {
	/// The grid of an element at `origin_x`, `origin_y` logical pixels in a window of `scale`.
	pub fn new(scale: f32, origin_x: f32, origin_y: f32) -> Self {
		let ox = origin_x * scale;
		let oy = origin_y * scale;
		Self { scale, fx: ox - ox.floor(), fy: oy - oy.floor() }
	}

	pub fn x(&self, logical: f32) -> f32 {
		logical * self.scale + self.fx
	}

	pub fn y(&self, logical: f32) -> f32 {
		logical * self.scale + self.fy
	}

	/// The whole number of device pixels nearest to one logical pixel, at least 1.
	pub fn hairline(&self) -> f32 {
		self.scale.round().max(1.)
	}

	/// Centre of a hairline of [`Self::hairline`] width whose first device column is the one under `device`.
	pub fn snap(&self, device: f32) -> f32 {
		device.floor() + self.hairline() / 2.
	}

	pub fn width(&self, logical: f32) -> f32 {
		(logical * self.scale).round()
	}

	/// The rectangle between the logical corners `from` and `to`, its edges on the nearest whole device pixels.
	pub fn rect(&self, from: (f32, f32), to: (f32, f32)) -> Rect {
		Rect::new(self.x(from.0).round(), self.y(from.1).round(), self.x(to.0).round(), self.y(to.1).round())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn snapping_lands_on_pixel_centres() {
		let g = DeviceGrid::new(1., 10.25, 0.);
		assert_eq!(g.fx, 0.25);
		assert_eq!(g.snap(g.x(3.)), 3.5);
		let g2 = DeviceGrid::new(2., 0., 0.);
		assert_eq!(g2.hairline(), 2.);
		assert_eq!(g2.snap(g2.x(3.)), 7.);
		let g15 = DeviceGrid::new(1.5, 0., 0.);
		assert_eq!(g15.hairline(), 2.);
	}
}
