//! The scrub bar: tick labels, minor and major ticks, playhead with its frame number, in/out brackets and
//! band, the bands of frames made, a hover line; click/drag scrub, bracket drag, keys and wheel.

use std::rc::Rc;

use freya::engine::prelude::{PathBuilder, SkRect};
use freya::prelude::*;

use freya::engine::prelude::Weight;

use crate::paint::{DeviceGrid, DeviceGridExt, fill, stroke, text_line, text_width, weighted_text_line};
use crate::picture::Callback;
use crate::theme::use_wave_theme;
use crate::util::{TestIdExt, focus_border};

/// What the bar shows; all values in frames. `frames` is the count planned, the last frame `frames - 1`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TimelineModel {
	pub frames: u32,
	pub position: u32,
	pub in_point: Option<u32>,
	pub out_point: Option<u32>,
	/// The frames a run has made, as spans of timeline frames.
	pub available: Rc<[(u32, u32)]>,
	/// The playhead may not leave the span it is in (a run the user follows).
	pub bounded: bool,
}

impl TimelineModel {
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

	/// The frame a key moves to: Left/Right one, `PageUp` forward ten, `PageDown` back ten, Home/End the range.
	pub fn key_target(&self, key: &Key) -> Option<u32> {
		let p = i64::from(self.position);
		let target = match key {
			Key::Named(NamedKey::ArrowLeft) => p - 1,
			Key::Named(NamedKey::ArrowRight) => p + 1,
			Key::Named(NamedKey::PageUp) => p + 10,
			Key::Named(NamedKey::PageDown) => p - 10,
			Key::Named(NamedKey::Home) => i64::from(self.range().0),
			Key::Named(NamedKey::End) => i64::from(self.range().1),
			_ => return None,
		};
		Some(self.clamp(target))
	}
}

/// Horizontal mapping between frames and the bar's logical x: frame 0 on the first column inside the
/// outline, the last frame on the last.
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

/// Frames between labels: the smallest candidate at least `MIN_LABEL_SPACING` px apart.
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

/// The bracket within `tolerance` px of `x`, the nearer one when both are; none without a range.
pub fn bracket_hit(model: &TimelineModel, geo: &TimelineGeometry, x: f32, tolerance: f32) -> Option<Bracket> {
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

/// What the user did on the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimelineEvent {
	Seek(u32),
	SetIn(u32),
	SetOut(u32),
	PlayPause,
}

const LABEL_ROW: f32 = 14.;
const BAR_HEIGHT: f32 = 22.;
/// Least distance between two labelled ticks, logical pixels.
const MIN_LABEL_SPACING: f32 = 64.;

#[derive(Clone, Copy, PartialEq)]
enum Drag {
	Scrub,
	Bracket(Bracket),
}

/// The timeline bar, controlled: it shows `model` and reports [`TimelineEvent`]s.
#[derive(Clone, PartialEq)]
pub struct Timeline {
	model: TimelineModel,
	on_event: Callback<TimelineEvent>,
	tooltip: Option<String>,
}

impl Timeline {
	pub fn new(model: TimelineModel, on_event: impl Into<Callback<TimelineEvent>>) -> Self {
		Self { model, on_event: on_event.into(), tooltip: None }
	}

	pub fn tooltip(mut self, text: impl Into<String>) -> Self {
		self.tooltip = Some(text.into());
		self
	}
}

impl Component for Timeline {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let id = use_a11y();
		let focus = use_focus(id);
		let mut width = use_state(|| 0f32);
		let mut hover = use_state(|| None::<u32>);
		let mut drag = use_state(|| None::<Drag>);
		let mut origin_x = use_state(|| 0f64);
		let model = self.model.clone();
		let on_event = self.on_event.clone();
		let geo = TimelineGeometry::new(*width.read(), model.last());

		let emit_at = {
			let model = model.clone();
			let on_event = on_event.clone();
			move |kind: Drag, x: f32| {
				let g = TimelineGeometry::new(*width.peek(), model.last());
				let f = g.frame_at(x);
				on_event.call(match kind {
					Drag::Scrub => TimelineEvent::Seek(model.clamp(i64::from(f))),
					Drag::Bracket(Bracket::In) => TimelineEvent::SetIn(f),
					Drag::Bracket(Bracket::Out) => TimelineEvent::SetOut(f),
				});
			}
		};

		let on_down = {
			let model = model.clone();
			let emit_at = emit_at.clone();
			move |e: Event<PointerEventData>| {
				if e.button() != Some(MouseButton::Left) {
					return;
				}
				id.request_focus();
				origin_x.set(e.global_location().x - e.element_location().x);
				let x = e.element_location().x as f32;
				let g = TimelineGeometry::new(*width.peek(), model.last());
				let kind = bracket_hit(&model, &g, x, 4.).map_or(Drag::Scrub, Drag::Bracket);
				drag.set(Some(kind));
				emit_at(kind, x);
			}
		};
		let on_global_move = {
			let emit_at = emit_at.clone();
			move |e: Event<PointerEventData>| {
				if let Some(kind) = *drag.peek() {
					emit_at(kind, (e.global_location().x - *origin_x.peek()) as f32);
				}
			}
		};
		let on_move = {
			let model = model.clone();
			move |e: Event<PointerEventData>| {
				let g = TimelineGeometry::new(*width.peek(), model.last());
				hover.set_if_modified(Some(g.frame_at(e.element_location().x as f32)));
			}
		};
		let on_key = {
			let model = model.clone();
			let on_event = on_event.clone();
			move |e: Event<KeyboardEventData>| {
				if let Some(f) = model.key_target(&e.key) {
					e.stop_propagation();
					on_event.call(TimelineEvent::Seek(f));
				} else if e.key == Key::Character(" ".into()) {
					e.stop_propagation();
					on_event.call(TimelineEvent::PlayPause);
				}
			}
		};
		let on_wheel = {
			let model = model.clone();
			let on_event = on_event.clone();
			move |e: Event<WheelEventData>| {
				if e.delta_y == 0. {
					return;
				}
				e.stop_propagation();
				let step = if e.delta_y > 0. { 1 } else { -1 };
				on_event.call(TimelineEvent::Seek(model.clamp(i64::from(model.position) + step)));
			}
		};

		let hover_frame = *hover.read();
		let colors = (
			theme.control_background,
			theme.border,
			theme.timeline_tick,
			theme.text,
			theme.text_disabled,
			theme.accent,
		);
		let painter = RenderCallback::new(move |ctx: &mut FillRenderContext<'_>| {
			let grid = DeviceGrid::from_ctx(ctx);
			let families = ctx.text_style_state.font_families.clone();
			let RenderTarget::Background { canvas, font_collection } = &mut ctx.target else { return };
			let (bar_bg, line, tick_ink, text, dim, accent) = colors;
			grid.enter(canvas);
			let hl = grid.hairline();
			let font = 11. * grid.scale;
			let top = grid.width(LABEL_ROW);
			let bottom = grid.width(LABEL_ROW + BAR_HEIGHT);
			let x0 = grid.x(0.).floor();
			let x1 = grid.x(geo.right + 1.5).floor();
			// inside the outline
			let (inner_top, inner_bottom) = (top + hl, bottom - hl);
			canvas.draw_rect(SkRect::new(x0, top, x1, bottom), &fill(bar_bg));
			let col = |f: u32| grid.snap(grid.x(geo.x_of(f)));
			for &(a, b) in model.available.iter() {
				canvas.draw_rect(
					SkRect::new(col(a) - hl / 2., inner_top, col(b) + hl / 2., inner_bottom),
					&fill(accent.with_a(0x60)),
				);
			}
			if model.has_range() {
				let (a, b) = model.range();
				let (xa, xb) = (col(a), col(b));
				canvas.draw_rect(
					SkRect::new(xa - hl / 2., inner_top, xb + hl / 2., inner_bottom),
					&fill(accent.with_a(0x40)),
				);
			}
			canvas
				.draw_rect(SkRect::new(x0 + hl / 2., top + hl / 2., x1 - hl / 2., bottom - hl / 2.), &stroke(line, hl));
			if model.frames > 1 {
				let step = label_step(geo.px_per_frame());
				let minor = minor_step(step);
				let tick = stroke(tick_ink, hl);
				let mut f = 0;
				while f <= model.last() {
					let major = f % step == 0;
					let x = col(f);
					canvas.draw_line((x, inner_top), (x, inner_top + grid.width(if major { 6. } else { 3. })), &tick);
					if major {
						let p = text_line(font_collection, &families, &f.to_string(), font, dim);
						let lx = (x + grid.width(2.)).min(x1 - text_width(&p) - grid.width(1.)).round();
						p.paint(canvas, (lx, 0.));
					}
					match f.checked_add(if minor > 0 { minor } else { step }) {
						Some(next) => f = next,
						None => break,
					}
				}
			}
			if model.has_range() {
				let (a, b) = model.range();
				let (xa, xb) = (col(a) - hl / 2., col(b) + hl / 2.);
				let w2 = grid.width(2.).max(2.);
				let foot = grid.width(6.);
				let p = fill(accent);
				for (l, r) in [(xa, xa + w2), (xb - w2, xb)] {
					canvas.draw_rect(SkRect::new(l, inner_top, r, inner_bottom), &p);
				}
				for (l, r) in [(xa, xa + foot), (xb - foot, xb)] {
					canvas.draw_rect(SkRect::new(l, inner_top, r, inner_top + w2), &p);
					canvas.draw_rect(SkRect::new(l, inner_bottom - w2, r, inner_bottom), &p);
				}
			}
			if let Some(h) = hover_frame.filter(|&h| h != model.position) {
				let x = col(h);
				canvas.draw_line((x, top), (x, bottom), &stroke(tick_ink.with_a(0x80), hl));
			}
			let x = col(model.position.min(model.last()));
			canvas.draw_line((x, top), (x, bottom), &stroke(text, hl));
			let (hw, head) = (grid.width(5.), grid.width(6.));
			let mut handle = PathBuilder::new();
			handle.move_to((x - hw, top - head)).line_to((x + hw, top - head)).line_to((x, top)).close();
			canvas.draw_path(&handle.detach(), &fill(text));
			let p = weighted_text_line(
				font_collection,
				&families,
				&model.position.to_string(),
				font,
				text,
				Weight::SEMI_BOLD,
			);
			let w = text_width(&p);
			let gap = grid.width(8.);
			let lx = if x + gap + w <= x1 { x + gap } else { x - gap - w }.round();
			let pad = grid.width(3.);
			canvas.draw_rect(SkRect::new(lx - pad, 0., lx + w + pad, top), &fill(bar_bg));
			p.paint(canvas, (lx, 0.));
			canvas.restore();
		});

		let bar = rect()
			.a11y_id(id)
			.a11y_focusable(true)
			.a11y_role(AccessibilityRole::Slider)
			.a11y_builder({
				let m = self.model.clone();
				move |node| {
					node.set_numeric_value(f64::from(m.position));
					node.set_max_numeric_value(f64::from(m.last()));
					let (a, b) = m.range();
					node.set_value(format!("range {a}..{b}"));
				}
			})
			.test_id("Timeline")
			.width(Size::fill())
			.height(Size::px(LABEL_ROW + BAR_HEIGHT))
			.background(painter)
			.border(focus_border(focus(), theme.accent))
			.on_sized(move |e: Event<SizedEventData>| width.set_if_modified(e.area.width()))
			.on_pointer_down(on_down)
			.on_pointer_move(on_move)
			.on_pointer_leave(move |_| hover.set_if_modified(None))
			.on_global_pointer_move(on_global_move)
			.on_global_pointer_up(move |_| drag.set_if_modified(None))
			.on_key_down(on_key)
			.on_wheel(on_wheel);
		match self.tooltip.clone() {
			Some(tip) => crate::util::wide_tooltip(tip, bar),
			None => bar.into_element(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn frames_map_to_first_and_last_columns() {
		let g = TimelineGeometry::new(103., 100);
		assert_eq!(g.x_of(0), 1.5);
		assert_eq!(g.x_of(100), 101.5);
		assert_eq!(g.frame_at(1.5), 0);
		assert_eq!(g.frame_at(51.9), 50);
		assert_eq!(g.frame_at(-20.), 0);
		assert_eq!(g.frame_at(500.), 100);
		assert_eq!(TimelineGeometry::new(50., 0).frame_at(30.), 0);
	}

	#[test]
	fn label_and_minor_steps() {
		assert_eq!(label_step(0.75), 100, "1250 frames on 940 px label every 100");
		assert_eq!(label_step(0.19), 500);
		assert_eq!(label_step(64.), 1);
		assert_eq!(label_step(0.001), 10000);
		assert_eq!(minor_step(100), 20);
		assert_eq!(minor_step(5), 1);
		assert_eq!(minor_step(1), 0);
	}

	#[test]
	fn keys_and_bounds() {
		let mut m = TimelineModel { frames: 101, position: 50, ..Default::default() };
		assert_eq!(m.key_target(&Key::Named(NamedKey::ArrowRight)), Some(51));
		assert_eq!(m.key_target(&Key::Named(NamedKey::PageUp)), Some(60));
		assert_eq!(m.key_target(&Key::Named(NamedKey::PageDown)), Some(40));
		assert_eq!(m.key_target(&Key::Named(NamedKey::End)), Some(100));
		m.in_point = Some(10);
		m.out_point = Some(70);
		assert_eq!(m.key_target(&Key::Named(NamedKey::Home)), Some(10));
		assert_eq!(m.key_target(&Key::Named(NamedKey::End)), Some(70));
		m.available = Rc::from([(40, 55)]);
		m.bounded = true;
		assert_eq!(m.key_target(&Key::Named(NamedKey::PageUp)), Some(55));
		assert_eq!(m.key_target(&Key::Character("x".into())), None);
	}

	#[test]
	fn brackets_hit_the_nearer() {
		let m = TimelineModel { frames: 101, in_point: Some(10), out_point: Some(12), ..Default::default() };
		let g = TimelineGeometry::new(103., 100);
		assert_eq!(bracket_hit(&m, &g, 11.6, 4.), Some(Bracket::In));
		assert_eq!(bracket_hit(&m, &g, 13.4, 4.), Some(Bracket::Out));
		assert_eq!(bracket_hit(&m, &g, 60., 4.), None);
		let whole = TimelineModel { frames: 101, ..Default::default() };
		assert_eq!(bracket_hit(&whole, &g, 1.5, 4.), None);
		assert_eq!(whole.range(), (0, 100));
	}
}
