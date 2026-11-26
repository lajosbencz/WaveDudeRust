//! Points, sizes, rectangles and directions, in logical pixels with y downwards.

/// A position.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
	/// Horizontal position.
	pub x: f32,
	/// Vertical position.
	pub y: f32,
}

impl Point {
	/// The point at `x`, `y`.
	pub const fn new(x: f32, y: f32) -> Self {
		Self { x, y }
	}

	pub(crate) fn along(self, axis: Axis) -> f32 {
		match axis {
			Axis::Horizontal => self.x,
			Axis::Vertical => self.y,
		}
	}
}

/// An extent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
	/// Horizontal extent.
	pub width: f32,
	/// Vertical extent.
	pub height: f32,
}

impl Size {
	/// No extent.
	pub const ZERO: Self = Self::new(0., 0.);

	/// The size `width` by `height`.
	pub const fn new(width: f32, height: f32) -> Self {
		Self { width, height }
	}

	pub(crate) fn along(self, axis: Axis) -> f32 {
		match axis {
			Axis::Horizontal => self.width,
			Axis::Vertical => self.height,
		}
	}

	pub(crate) fn max(self, other: Self) -> Self {
		Self::new(self.width.max(other.width), self.height.max(other.height))
	}
}

/// An axis-aligned rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
	/// Left edge.
	pub x: f32,
	/// Top edge.
	pub y: f32,
	/// Horizontal extent.
	pub width: f32,
	/// Vertical extent.
	pub height: f32,
}

impl Rect {
	/// The rectangle at `x`, `y` of `width` by `height`.
	pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
		Self { x, y, width, height }
	}

	/// Right edge.
	pub fn right(self) -> f32 {
		self.x + self.width
	}

	/// Bottom edge.
	pub fn bottom(self) -> f32 {
		self.y + self.height
	}

	/// Whether `p` lies inside; the right and bottom edges are outside.
	pub fn contains(self, p: Point) -> bool {
		(self.x..self.right()).contains(&p.x) && (self.y..self.bottom()).contains(&p.y)
	}

	/// Start and extent along `axis`.
	pub(crate) fn span(self, axis: Axis) -> (f32, f32) {
		match axis {
			Axis::Horizontal => (self.x, self.width),
			Axis::Vertical => (self.y, self.height),
		}
	}

	/// This rectangle with its span along `axis` replaced.
	pub(crate) fn with_span(self, axis: Axis, start: f32, extent: f32) -> Self {
		match axis {
			Axis::Horizontal => Self { x: start, width: extent, ..self },
			Axis::Vertical => Self { y: start, height: extent, ..self },
		}
	}

	/// The part at `side` taking `share` of the extent across it.
	pub(crate) fn part(self, side: Side, share: f32) -> Self {
		let axis = side.axis();
		let (start, extent) = self.span(axis);
		let depth = extent * share;
		let start = if side.leads() { start } else { start + extent - depth };
		self.with_span(axis, start, depth)
	}

	/// How far `p` lies inside from `side`.
	pub(crate) fn depth(self, side: Side, p: Point) -> f32 {
		match side {
			Side::Left => p.x - self.x,
			Side::Right => self.right() - p.x,
			Side::Top => p.y - self.y,
			Side::Bottom => self.bottom() - p.y,
		}
	}
}

/// The direction a split lays out its children in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Axis {
	/// Left to right.
	Horizontal,
	/// Top to bottom.
	Vertical,
}

/// A side of a rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
	/// The left side.
	Left,
	/// The right side.
	Right,
	/// The top side.
	Top,
	/// The bottom side.
	Bottom,
}

impl Side {
	/// Every side.
	pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Top, Self::Bottom];

	/// The axis a split beside this side lays out along.
	pub fn axis(self) -> Axis {
		match self {
			Self::Left | Self::Right => Axis::Horizontal,
			Self::Top | Self::Bottom => Axis::Vertical,
		}
	}

	/// Whether this side comes first along its axis.
	pub(crate) fn leads(self) -> bool {
		matches!(self, Self::Left | Self::Top)
	}
}

/// Sizes of `items` (weight, minimum) sharing `room`: proportional to the weights, each at least its minimum;
/// when the minimums do not fit they shrink in proportion.
pub(crate) fn distribute(room: f32, items: &[(f32, f32)]) -> Vec<f32> {
	let room = room.max(0.);
	let mins: f32 = items.iter().map(|&(_, min)| min).sum();
	if mins >= room {
		let scale = if mins > 0. { room / mins } else { 0. };
		return items.iter().map(|&(_, min)| min * scale).collect();
	}
	let mut fixed = vec![false; items.len()];
	loop {
		let free = room - items.iter().zip(&fixed).filter(|(_, f)| **f).map(|((_, min), _)| min).sum::<f32>();
		let weights: f32 = items.iter().zip(&fixed).filter(|(_, f)| !**f).map(|((w, _), _)| w).sum();
		let unfixed = fixed.iter().filter(|f| !**f).count() as f32;
		let share = |w: f32| if weights > 0. { free * w / weights } else { free / unfixed };
		let sizes: Vec<f32> = items.iter().zip(&fixed).map(|(&(w, min), &f)| if f { min } else { share(w) }).collect();
		let mut changed = false;
		for ((size, &(_, min)), f) in sizes.iter().zip(items).zip(&mut fixed) {
			if !*f && *size < min {
				*f = true;
				changed = true;
			}
		}
		if !changed {
			return sizes;
		}
	}
}
