//! The colour scheme's gradient (docs/ui/30, Colour scheme): its stops kept sorted, the bar with draggable stop
//! markers (a press on the bar adds a stop there, a double click on a marker or Del removes one, a marker pulled
//! far above or below the bar is removed at the release; the last stop stays), and the left-to-right mesh the
//! bar and the swatches are painted with.

use std::borrow::Cow;

use eframe::egui::{
	Color32, Id, Key, Mesh, Modifiers, Pos2, Rect, Role, Sense, Shape, Stroke, StrokeKind, Ui, WidgetInfo, pos2, vec2,
};
use wavedude_core::model::{Rgb, Stop};

use super::color_picker::{color32, lerp};
use super::theme::Palette;

/// Colour stops sorted by position.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Gradient<'a> {
	pub(super) stops: Cow<'a, [Stop]>,
}

impl<'a> Gradient<'a> {
	/// `stops` in place when sorted, else a sorted copy with the positions clamped.
	pub(super) fn of(stops: &'a [Stop]) -> Self {
		if stops.is_sorted_by(|a, b| a.at <= b.at) && stops.iter().all(|s| (0. ..=1.).contains(&s.at)) {
			return Self { stops: Cow::Borrowed(stops) };
		}
		let mut v: Vec<Stop> = stops.iter().map(|s| Stop { at: s.at.clamp(0., 1.), ..*s }).collect();
		v.sort_by(|a, b| a.at.total_cmp(&b.at));
		Self { stops: Cow::Owned(v) }
	}

	/// The stops, owned for an edit.
	pub(super) fn into_stops(self) -> Vec<Stop> {
		self.stops.into_owned()
	}

	fn stops_mut(&mut self) -> &mut Vec<Stop> {
		self.stops.to_mut()
	}

	/// The colour at `t`, straight sRGB interpolation between neighbours; held beyond the ends.
	pub(super) fn color_at(&self, t: f64) -> Rgb {
		let (Some(first), Some(last)) = (self.stops.first(), self.stops.last()) else { return Rgb::default() };
		if t <= first.at {
			return first.color;
		}
		for pair in self.stops.windows(2) {
			let (a, b) = (pair[0], pair[1]);
			if t <= b.at {
				let span = b.at - a.at;
				return if span <= 0. { b.color } else { lerp(a.color, b.color, (t - a.at) / span) };
			}
		}
		last.color
	}

	/// The middle of the widest gap, the ends counting as gaps.
	fn widest_gap(&self) -> f64 {
		let mut best = (0., 0.5);
		let mut left = 0.;
		for right in self.stops.iter().map(|s| s.at).chain([1.]) {
			if right - left > best.0 {
				best = (right - left, (left + right) / 2.);
			}
			left = right;
		}
		best.1
	}

	/// Inserts a stop and returns its index.
	pub(super) fn insert(&mut self, at: f64, color: Rgb) -> usize {
		let at = at.clamp(0., 1.);
		let index = self.stops.iter().position(|s| s.at > at).unwrap_or(self.stops.len());
		self.stops_mut().insert(index, Stop { at, color });
		index
	}

	/// Add stop: in the widest gap, with the gradient's colour there.
	pub(super) fn add_in_widest_gap(&mut self) -> usize {
		let t = self.widest_gap();
		let c = self.color_at(t);
		self.insert(t, c)
	}

	/// Removes a stop; the last one stays.
	pub(super) fn remove(&mut self, index: usize) -> bool {
		if self.stops.len() <= 1 || index >= self.stops.len() {
			return false;
		}
		self.stops_mut().remove(index);
		true
	}

	/// Moves a stop and returns its index after re-sorting.
	pub(super) fn set_position(&mut self, index: usize, at: f64) -> usize {
		let Some(stop) = self.stops.get(index).copied() else { return index };
		self.stops_mut().remove(index);
		self.insert(at, stop.color)
	}

	pub(super) fn set_color(&mut self, index: usize, color: Rgb) {
		if let Some(s) = self.stops_mut().get_mut(index) {
			s.color = color;
		}
	}

	/// The stop whose marker is within `tolerance` of `t`: the nearest, among equals the one drawn on top.
	fn stop_near(&self, t: f64, tolerance: f64) -> Option<usize> {
		self.stops
			.iter()
			.enumerate()
			.rev()
			.map(|(i, s)| (i, (s.at - t).abs()))
			.filter(|(_, d)| *d <= tolerance)
			.min_by(|a, b| a.1.total_cmp(&b.1))
			.map(|(i, _)| i)
	}
}

/// `stops` (sorted) left to right over `rect`, held beyond the ends.
pub(super) fn gradient_mesh(rect: Rect, stops: &[Stop]) -> Mesh {
	let color = |s: &Stop| color32(s.color);
	let mut mesh = Mesh::default();
	let x = |at: f64| rect.left() + rect.width() * at.clamp(0., 1.) as f32;
	let mut quad = |x0: f32, x1: f32, c0: Color32, c1: Color32| {
		let base = mesh.vertices.len() as u32;
		for (p, c) in [(pos2(x0, rect.top()), c0), (pos2(x1, rect.top()), c1)] {
			mesh.colored_vertex(p, c);
			mesh.colored_vertex(pos2(p.x, rect.bottom()), c);
		}
		mesh.add_triangle(base, base + 1, base + 2);
		mesh.add_triangle(base + 1, base + 2, base + 3);
	};
	if let (Some(first), Some(last)) = (stops.first(), stops.last()) {
		quad(rect.left(), x(first.at), color(first), color(first));
		for pair in stops.windows(2) {
			quad(x(pair[0].at), x(pair[1].at), color(&pair[0]), color(&pair[1]));
		}
		quad(x(last.at), rect.right(), color(last), color(last));
	}
	mesh
}

const BAR_HEIGHT: f32 = 20.;
const MARKER: f32 = 7.;
/// The bar's room: the bar, the markers under it and a margin.
const HEIGHT: f32 = BAR_HEIGHT + MARKER + 4.;
/// Pixels above or below the bar at which a dragged marker is pulled away: released there, its stop is removed.
const PULL_AWAY: f32 = 24.;
/// How much of a pulled-away marker shows.
const PULLED_OPACITY: f32 = 0.35;
/// Seconds and pixels within which a second press makes a double click.
const DOUBLE: (f64, f32) = (0.3, 6.);

/// The editor's state between frames.
#[derive(Clone, Copy, Default)]
struct State {
	selected: Option<usize>,
	dragging: bool,
	pulled: bool,
	/// The last press: when and where.
	pressed: Option<(f64, Pos2)>,
}

/// The bar `id` of `gradient` across the row; the stops it sets.
pub(super) fn gradient_editor(ui: &mut Ui, id: Id, gradient: &Gradient<'_>, tip: &str) -> Option<Vec<Stop>> {
	let palette = Palette::of(ui.ctx().theme());
	let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEIGHT), Sense::hover());
	let response = ui.interact(rect, id, Sense::click_and_drag()).on_hover_text(tip);
	response.widget_info(|| WidgetInfo::labeled(Role::Slider, ui.is_enabled(), tip));
	let state_id = id.with("state");
	let mut st = ui.data(|d| d.get_temp::<State>(state_id)).unwrap_or_default();
	let count = gradient.stops.len();
	st.selected = st.selected.filter(|&i| i < count);
	let bar = Rect::from_min_max(
		pos2(rect.left() + MARKER, rect.top()),
		pos2(rect.right() - MARKER, rect.top() + BAR_HEIGHT),
	);
	let to_t = |x: f32| f64::from(((x - bar.left()) / bar.width().max(1.)).clamp(0., 1.));
	let tolerance = f64::from(MARKER / bar.width().max(1.));
	let mut edited: Option<Gradient<'_>> = None;

	let (pressed, press, now, latest, down) = ui.input(|i| {
		(
			i.pointer.primary_pressed(),
			i.pointer.press_origin(),
			i.time,
			i.pointer.latest_pos(),
			i.pointer.primary_down(),
		)
	});
	if pressed && let Some(p) = press.filter(|p| rect.contains(*p) && response.hovered()) {
		response.request_focus();
		let t = to_t(p.x);
		let on_marker = p.y > bar.bottom() - 4.;
		let double = st.pressed.is_some_and(|(at, q)| now - at < DOUBLE.0 && q.distance(p) < DOUBLE.1);
		st.pressed = Some((now, p));
		if double {
			// the second press: on a marker it removes the stop; on the bar the first press added one
			if let Some(i) = on_marker.then(|| gradient.stop_near(t, tolerance)).flatten() {
				let mut g = gradient.clone();
				if g.remove(i) {
					st.selected = Some(i.min(g.stops.len() - 1));
					edited = Some(g);
				}
			}
			st.pressed = None;
		} else {
			let near = gradient.stop_near(t, tolerance);
			let index = match near {
				Some(i) if on_marker => Some(i),
				_ if p.y <= bar.bottom() => {
					let mut g = gradient.clone();
					let i = g.insert(t, g.color_at(t));
					edited = Some(g);
					Some(i)
				}
				other => other,
			};
			if let Some(i) = index {
				st.selected = Some(i);
				st.dragging = true;
			}
		}
	} else if st.dragging
		&& down
		&& let (Some(i), Some(p)) = (st.selected, latest)
	{
		let mut g = gradient.clone();
		st.pulled = g.stops.len() > 1 && (p.y < rect.top() - PULL_AWAY || p.y > rect.bottom() + PULL_AWAY);
		let t = to_t(p.x);
		if (g.stops[i].at - t).abs() > f64::EPSILON {
			st.selected = Some(g.set_position(i, t));
			edited = Some(g);
		}
	} else if st.dragging && !down {
		if st.pulled
			&& let Some(i) = st.selected
		{
			let mut g = gradient.clone();
			if g.remove(i) {
				st.selected = Some(i.min(g.stops.len() - 1));
				edited = Some(g);
			}
		}
		st.dragging = false;
		st.pulled = false;
	}
	if response.has_focus() && count > 0 {
		let i = st.selected.unwrap_or(0).min(count - 1);
		let (left, right, up, down, del, shift) = ui.input_mut(|inp| {
			let shift = inp.modifiers.shift;
			let m = if shift { Modifiers::SHIFT } else { Modifiers::NONE };
			(
				inp.consume_key(m, Key::ArrowLeft),
				inp.consume_key(m, Key::ArrowRight),
				inp.consume_key(Modifiers::NONE, Key::PageUp),
				inp.consume_key(Modifiers::NONE, Key::PageDown),
				inp.consume_key(Modifiers::NONE, Key::Delete),
				shift,
			)
		});
		let d = if shift { 0.1 } else { 0.01 };
		if left || right {
			let mut g = gradient.clone();
			let at = g.stops[i].at + if left { -d } else { d };
			st.selected = Some(g.set_position(i, at));
			edited = Some(g);
		} else if up {
			st.selected = Some(i.saturating_sub(1));
		} else if down {
			st.selected = Some((i + 1).min(count - 1));
		} else if del {
			let mut g = gradient.clone();
			if g.remove(i) {
				st.selected = Some(i.min(g.stops.len() - 1));
				edited = Some(g);
			}
		}
	}
	ui.data_mut(|d| d.insert_temp(state_id, st));

	let shown = edited.as_ref().unwrap_or(gradient);
	let painter = ui.painter();
	painter.add(Shape::mesh(gradient_mesh(bar, &shown.stops)));
	painter.rect_stroke(bar, 0., Stroke::new(1., palette.hairline), StrokeKind::Inside);
	for (i, s) in shown.stops.iter().enumerate() {
		let x = (bar.left() + bar.width() * s.at as f32).round() + 0.5;
		let selected = st.selected == Some(i);
		let alpha = if selected && st.pulled { PULLED_OPACITY } else { 1. };
		let tri = vec![
			pos2(x, BAR_HEIGHT + 1.),
			pos2(x + MARKER - 1., BAR_HEIGHT + MARKER + 2.),
			pos2(x - MARKER + 1., BAR_HEIGHT + MARKER + 2.),
		]
		.into_iter()
		.map(|p| pos2(p.x, rect.top() + p.y))
		.collect();
		let edge = if selected { Stroke::new(2., palette.accent) } else { Stroke::new(1., palette.hairline) };
		let edge = Stroke::new(edge.width, edge.color.gamma_multiply(alpha));
		painter.add(Shape::convex_polygon(tri, color32(s.color).gamma_multiply(alpha), edge));
	}
	if response.has_focus() {
		painter.rect_stroke(rect, 0., Stroke::new(1., palette.accent), StrokeKind::Inside);
	}
	if st.dragging || edited.is_some() {
		ui.ctx().request_repaint();
	}
	edited.map(Gradient::into_stops)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn bw() -> Vec<Stop> {
		vec![Stop { at: 1., color: Rgb::new(255, 255, 255) }, Stop { at: 0., color: Rgb::new(0, 0, 0) }]
	}

	#[test]
	fn stops_sort_and_interpolate() {
		let stops = bw();
		let g = Gradient::of(&stops);
		assert_eq!(g.stops[0].at, 0.);
		assert_eq!(g.color_at(0.5), Rgb::new(128, 128, 128));
		assert_eq!(g.color_at(-1.), Rgb::new(0, 0, 0));
		assert_eq!(g.color_at(2.), Rgb::new(255, 255, 255));
		assert_eq!(Gradient::of(&[]).color_at(0.3), Rgb::default());
		let sorted = g.into_stops();
		assert!(matches!(Gradient::of(&sorted).stops, Cow::Borrowed(_)), "sorted stops are not copied");
	}

	#[test]
	fn add_goes_into_the_widest_gap() {
		let stops = bw();
		let mut g = Gradient::of(&stops);
		assert_eq!(g.widest_gap(), 0.5);
		assert_eq!(g.add_in_widest_gap(), 1);
		assert_eq!(g.stops[1].at, 0.5);
		assert_eq!(g.stops[1].color, Rgb::new(128, 128, 128));
		let i = g.add_in_widest_gap();
		assert_eq!((i, g.stops[i].at), (1, 0.25));
		let one = [Stop { at: 0.2, color: Rgb::default() }];
		assert_eq!(Gradient::of(&one).widest_gap(), 0.6);
	}

	#[test]
	fn move_remove_and_hit() {
		let stops = bw();
		let mut g = Gradient::of(&stops);
		g.insert(0.5, Rgb::new(255, 0, 0));
		assert_eq!(g.set_position(1, 0.9), 1);
		assert_eq!(g.set_position(1, 1.5), 2);
		assert_eq!(g.stops[2].color, Rgb::new(255, 0, 0));
		assert_eq!(g.stop_near(0.98, 0.05), Some(2));
		assert_eq!(g.stop_near(0.5, 0.05), None);
		assert!(g.remove(0));
		assert!(g.remove(0));
		assert!(!g.remove(0));
		assert_eq!(g.stops.len(), 1);
	}
}
