//! The timeline's arithmetic (docs/ui/50-timeline-transport.md, Timeline): the model from the playback state, the
//! mapping between frames and the bar's x, the tick spacing, the bracket under the pointer and the keys' targets.
//! Pure, tested without a window.
//!
//! Units: frames; x in logical pixels from the bar's left edge.

use crate::Event;
use crate::commands::{Command, Edit};
use crate::playback::Playback;
use waveduderust::model::{Step, ViewEdit};

/// Least distance between two labelled ticks, logical pixels.
const MIN_LABEL_SPACING: f32 = 64.;

/// What the bar shows. `frames` is the count planned, the last frame `frames - 1`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TimelineModel<'a> {
	pub frames: u32,
	pub position: u32,
	pub in_point: Option<u32>,
	pub out_point: Option<u32>,
	/// The frames a run has made, as spans of timeline frames.
	pub available: &'a [(u32, u32)],
	/// The playhead may not leave the span it is in (a run the user follows).
	pub bounded: bool,
}

impl<'a> TimelineModel<'a> {
	/// The model of `playback` with the frames made `available`, `bounded` while following a run.
	pub fn of(playback: &Playback, available: &'a [(u32, u32)], bounded: bool) -> Self {
		let (from, to) = playback.range_frames();
		let (in_step, out_step) = playback.range_steps();
		Self {
			frames: playback.frame_count(),
			position: playback.position(),
			in_point: (in_step != Step(0)).then_some(from),
			out_point: out_step.map(|_| to),
			available,
			bounded,
		}
	}

	pub fn last(&self) -> u32 {
		self.frames.saturating_sub(1)
	}

	/// The export range, the whole run where a point is unset.
	pub fn range(&self) -> (u32, u32) {
		let last = self.last();
		let a = self.in_point.unwrap_or(0).min(last);
		let b = self.out_point.unwrap_or(last).min(last).max(a);
		(a, b)
	}

	/// The range is narrower than the run, so the brackets show.
	pub fn has_range(&self) -> bool {
		self.range() != (0, self.last())
	}

	/// `frame` held inside the run, and inside the position's span of frames made when bounded.
	pub fn clamp(&self, frame: i64) -> u32 {
		let mut max = i64::from(self.last());
		if self.bounded
			&& let Some(&(_, to)) = self.available.iter().find(|&&(a, b)| a <= self.position && self.position <= b)
		{
			max = max.min(i64::from(to));
		}
		frame.clamp(0, max) as u32
	}

	/// The frame `key` moves to.
	pub fn key_target(&self, key: TimelineKey) -> u32 {
		let p = i64::from(self.position);
		self.clamp(match key {
			TimelineKey::Back => p - 1,
			TimelineKey::Forward => p + 1,
			TimelineKey::PageForward => p + 10,
			TimelineKey::PageBack => p - 10,
			TimelineKey::Start => i64::from(self.range().0),
			TimelineKey::End => i64::from(self.range().1),
		})
	}
}

/// The bar's keys: Left/Right a frame, `PageUp` forward ten, `PageDown` back ten, Home/End the range's ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimelineKey {
	Back,
	Forward,
	PageForward,
	PageBack,
	Start,
	End,
}

/// Horizontal mapping between frames and the bar's x: frame 0 on the first column inside the outline, the last
/// frame on the last.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimelineGeometry {
	pub left: f32,
	pub right: f32,
	pub last: u32,
}

impl TimelineGeometry {
	/// For a bar `width` logical pixels wide with a 1 px outline.
	pub fn new(width: f32, last: u32) -> Self {
		Self { left: 1.5, right: (width - 1.5).max(1.5), last }
	}

	pub fn px_per_frame(&self) -> f32 {
		if self.last == 0 { 0. } else { (self.right - self.left) / self.last as f32 }
	}

	pub fn x_of(&self, frame: u32) -> f32 {
		self.left + self.px_per_frame() * frame.min(self.last) as f32
	}

	/// The nearest frame to `x`, inside the run.
	pub fn frame_at(&self, x: f32) -> u32 {
		if self.last == 0 {
			return 0;
		}
		((x - self.left) / self.px_per_frame()).round().clamp(0., self.last as f32) as u32
	}
}

/// Frames between labels: the smallest candidate at least 64 px apart.
pub fn label_step(px_per_frame: f32) -> u32 {
	const CANDIDATES: [u32; 13] = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000];
	CANDIDATES.into_iter().find(|&c| c as f32 * px_per_frame >= MIN_LABEL_SPACING).unwrap_or(CANDIDATES[12])
}

/// Frames between minor ticks under a label step, a fifth of it; none (0) under a step of 1.
pub fn minor_step(label_step: u32) -> u32 {
	match label_step {
		10.. => label_step / 5,
		2.. => 1,
		_ => 0,
	}
}

/// An export range bracket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bracket {
	In,
	Out,
}

/// The bracket within `tolerance` of `x`, the nearer one when both are; none without a range.
pub fn bracket_hit(model: &TimelineModel<'_>, geo: &TimelineGeometry, x: f32, tolerance: f32) -> Option<Bracket> {
	if !model.has_range() {
		return None;
	}
	let (a, b) = model.range();
	let da = (geo.x_of(a) - x).abs();
	let db = (geo.x_of(b) - x).abs();
	match (da <= tolerance, db <= tolerance) {
		(true, true) => Some(if da <= db { Bracket::In } else { Bracket::Out }),
		(true, false) => Some(Bracket::In),
		(false, true) => Some(Bracket::Out),
		_ => None,
	}
}

/// What a press or drag at `frame` asks for: the position, or `bracket`'s point in steps of `every` (the last frame's
/// out point is stored as 0, to the end).
pub fn press_event(model: &TimelineModel<'_>, bracket: Option<Bracket>, frame: u32, every: u32) -> Event {
	let edit = match bracket {
		None => return Event::Scrub(model.clamp(i64::from(frame))),
		Some(Bracket::In) => ViewEdit::ExportFrom(frame * every),
		Some(Bracket::Out) => ViewEdit::ExportTo(if frame >= model.last() { 0 } else { frame * every }),
	};
	Event::Command(Command::Edit(Edit::View(edit)))
}
