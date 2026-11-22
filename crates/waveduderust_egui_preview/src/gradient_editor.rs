//! The colour scheme's gradient (docs/ui/30, Colour scheme): its stops kept sorted, the bar with draggable stop
//! markers (a press on the bar adds a stop there, a double click on a marker or Del removes one, a marker pulled
//! far above or below the bar is removed at the release; the last stop stays), and the left-to-right mesh the
//! bar and the swatches are painted with.

use eframe::egui::{
	Color32, Id, Key, Mesh, Modifiers, Pos2, Rect, Role, Sense, Shape, Stroke, StrokeKind, Ui, WidgetInfo, pos2, vec2,
};
use waveduderust::model::{Rgb, Stop};
use waveduderust_app::gradient::Gradient;

/// `c` as egui's colour.
pub fn color32(c: Rgb) -> Color32 {
	Color32::from_rgb(c.r, c.g, c.b)
}

/// `stops` (sorted) left to right over `rect`, held beyond the ends.
pub fn gradient_mesh(rect: Rect, stops: &[Stop]) -> Mesh {
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

/// The editor's colours: the bar's and the markers' outline, a selected marker's and the focus.
#[derive(Clone, Copy, Debug)]
pub struct GradientColors {
	pub hairline: Color32,
	pub accent: Color32,
}

/// The bar `id` of `gradient` across the row; the stops it sets.
pub fn gradient_editor(
	ui: &mut Ui,
	id: Id,
	gradient: &Gradient<'_>,
	tip: &str,
	palette: GradientColors,
) -> Option<Vec<Stop>> {
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
