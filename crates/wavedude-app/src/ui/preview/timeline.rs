//! The timeline (docs/ui/50-timeline-transport.md, Timeline): the scrub bar's ticks and labels, the frames made,
//! the export range's band and brackets, the hover line and the playhead with its frame number, on whole device
//! pixels. A press scrubs or takes the bracket under it and a drag follows; the keys and the wheel step.

use eframe::egui::{
	Color32, EventFilter, FontId, Id, Key, Modifiers, Painter, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui,
	WidgetInfo,
};
use wavedude_app_state::Event;
use wavedude_app_state::commands::{Command, Edit};
use wavedude_core::model::{DisplayEdit, ProjectEdit};
use wavedude_view::DeviceGrid;

use super::transport::play_or_pause;
use crate::App;
use crate::preview::timeline::{
	Bracket, TimelineGeometry, TimelineKey, TimelineModel, bracket_hit, label_step, minor_step,
};
use crate::ui::number_box::wheel_notches;
use crate::ui::theme::Palette;

const LABEL_ROW: f32 = 14.;
const BAR_HEIGHT: f32 = 22.;
/// The timeline's height, logical pixels.
pub(super) const HEIGHT: f32 = LABEL_ROW + BAR_HEIGHT;
const LABEL_SIZE: f32 = 11.;
/// How near a bracket a press takes it, logical pixels.
const BRACKET_TOLERANCE: f32 = 4.;

const KEYS: [(Key, TimelineKey); 6] = [
	(Key::ArrowLeft, TimelineKey::Back),
	(Key::ArrowRight, TimelineKey::Forward),
	(Key::PageUp, TimelineKey::PageForward),
	(Key::PageDown, TimelineKey::PageBack),
	(Key::Home, TimelineKey::Start),
	(Key::End, TimelineKey::End),
];

/// A press in progress: the bracket it moves (none: it scrubs) and the frame it reported last.
#[derive(Clone, Copy)]
struct Scrub {
	bracket: Option<Bracket>,
	frame: u32,
}

/// The timeline's state between frames.
#[derive(Default)]
pub(super) struct Timeline {
	scrub: Option<Scrub>,
	/// The frames made, as of the run's progress key.
	spans: Vec<(u32, u32)>,
	spans_key: Option<(u64, usize)>,
}

impl Timeline {
	/// The timeline in `rect`; a press focuses it as `pointer_focus`, which shows no focus ring.
	pub(super) fn ui(
		&mut self,
		ui: &mut Ui,
		rect: Rect,
		app: &App,
		pointer_focus: &mut Option<Id>,
		out: &mut Vec<Event>,
	) {
		let key = app.live.progress_key();
		if key != self.spans_key {
			self.spans_key = key;
			self.spans = app.live.present_spans();
		}
		let model = TimelineModel::of(&app.live.playback, &self.spans, app.live.is_live());
		let geo = TimelineGeometry::new(rect.width(), model.last());
		let id = ui.scope_id().with("PreviewTimeline");
		let response =
			ui.interact(rect, id, Sense::click_and_drag()).on_hover_text(app.strings.get("TipPreviewTimeline"));
		describe(&response, &model);
		super::hold(&response);
		let (pressed, down, at) =
			ui.input(|i| (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.interact_pos()));
		let x = at.map(|p| p.x - rect.left());
		let every = app.document.project.simulation.visualize_every;
		if let (true, true, Some(x)) = (pressed, response.hovered(), x) {
			if !response.has_focus() {
				response.request_focus();
			}
			*pointer_focus = Some(id);
			let scrub = Scrub { bracket: bracket_hit(&model, &geo, x, BRACKET_TOLERANCE), frame: geo.frame_at(x) };
			out.push(report(&model, scrub, every));
			self.scrub = Some(scrub);
		} else if let (Some(scrub), Some(x)) = (&mut self.scrub, x)
			&& geo.frame_at(x) != scrub.frame
		{
			scrub.frame = geo.frame_at(x);
			out.push(report(&model, *scrub, every));
		}
		if !down {
			self.scrub = None;
		}
		if response.lost_focus() && *pointer_focus == Some(id) {
			*pointer_focus = None;
		}
		if response.has_focus() {
			ui.memory_mut(|m| {
				m.set_focus_lock_filter(id, EventFilter { horizontal_arrows: true, ..Default::default() });
			});
			let (target, play) = ui.input_mut(|i| {
				let mut m = model;
				let mut moved = false;
				for (key, step) in KEYS {
					for _ in 0..i.count_and_consume_key(Modifiers::NONE, key) {
						m.position = m.key_target(step);
						moved = true;
					}
				}
				(moved.then_some(m.position), i.consume_key(Modifiers::NONE, Key::Space))
			});
			out.extend(target.map(Event::Scrub));
			if play {
				out.push(play_or_pause(app));
			}
		}
		let notches = wheel_notches(ui, rect);
		if notches != 0 {
			out.push(Event::Scrub(model.clamp(i64::from(model.position) - i64::from(notches))));
		}
		let hover = response.hover_pos().map(|p| geo.frame_at(p.x - rect.left()));
		let ring = response.has_focus() && *pointer_focus != Some(id);
		paint(ui, rect, &model, &geo, hover, ring);
	}
}

/// What a press or drag at `scrub` asks for: the position, or the bracket's point in steps (the last frame's out
/// point is stored as 0, to the end).
fn report(model: &TimelineModel<'_>, scrub: Scrub, every: u32) -> Event {
	let f = scrub.frame;
	let edit = match scrub.bracket {
		None => return Event::Scrub(model.clamp(i64::from(f))),
		Some(Bracket::In) => DisplayEdit::ExportFrom(f * every),
		Some(Bracket::Out) => DisplayEdit::ExportTo(if f >= model.last() { 0 } else { f * every }),
	};
	Event::Command(Command::Edit(Edit::Project(ProjectEdit::Display(edit))))
}

/// A slider of the position up to the last frame, its value text "range a..b".
fn describe(response: &Response, model: &TimelineModel<'_>) {
	response.widget_info(|| WidgetInfo::slider(true, f64::from(model.position), ""));
	response.ctx.accesskit_node_builder(response.id, |node| {
		let (a, b) = model.range();
		node.set_min_numeric_value(0.);
		node.set_max_numeric_value(f64::from(model.last()));
		node.set_value(format!("range {a}..{b}"));
	});
}

fn with_alpha(c: Color32, alpha: u8) -> Color32 {
	Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

/// Fills the device rectangle between `l, t` and `r, b`.
fn fill_device(painter: &Painter, grid: &DeviceGrid, (l, t, r, b): (f32, f32, f32, f32), color: Color32) {
	painter.rect_filled(grid.rect((l, t), (r, b)), 0., color);
}

/// The bar, in device units from the device pixel under `rect`'s corner.
fn paint(ui: &Ui, rect: Rect, model: &TimelineModel<'_>, geo: &TimelineGeometry, hover: Option<u32>, ring: bool) {
	let palette = Palette::of(ui.ctx().theme());
	let painter = ui.painter_at(rect);
	let grid = DeviceGrid::new(ui.ctx().pixels_per_point(), rect.left(), rect.top());
	let hl = grid.hairline();
	let top = grid.width(LABEL_ROW);
	let bottom = grid.width(LABEL_ROW + BAR_HEIGHT);
	let x0 = grid.x(0.).floor();
	let x1 = grid.x(geo.right + 1.5).floor();
	let (inner_top, inner_bottom) = (top + hl, bottom - hl);
	let col = |f: u32| grid.snap(grid.x(geo.x_of(f)));
	let fill = |area, color| fill_device(&painter, &grid, area, color);
	let line = |x: f32, from: f32, to: f32, color| fill((x - hl / 2., from, x + hl / 2., to), color);
	fill((x0, top, x1, bottom), palette.control_background);
	for &(a, b) in model.available {
		fill((col(a) - hl / 2., inner_top, col(b) + hl / 2., inner_bottom), with_alpha(palette.accent, 0x60));
	}
	let range = model.has_range().then(|| model.range());
	if let Some((a, b)) = range {
		fill((col(a) - hl / 2., inner_top, col(b) + hl / 2., inner_bottom), with_alpha(palette.accent, 0x40));
	}
	let outline = Stroke::new(hl / grid.scale, palette.border);
	painter.rect_stroke(grid.rect((x0, top), (x1, bottom)), 0., outline, StrokeKind::Inside);
	let font = FontId::proportional(LABEL_SIZE);
	if model.frames > 1 {
		let step = label_step(geo.px_per_frame());
		let minor = minor_step(step);
		let mut f = 0;
		while f <= model.last() {
			let major = f % step == 0;
			let x = col(f);
			line(x, inner_top, inner_top + grid.width(if major { 6. } else { 3. }), palette.timeline_tick);
			if major {
				let label = painter.layout_no_wrap(f.to_string(), font.clone(), palette.text_disabled);
				let lx = (x + grid.width(2.)).min(x1 - label.size().x * grid.scale - grid.width(1.)).round();
				painter.galley(grid.point(lx, 0.), label, palette.text_disabled);
			}
			match f.checked_add(if minor > 0 { minor } else { step }) {
				Some(next) => f = next,
				None => break,
			}
		}
	}
	if let Some((a, b)) = range {
		let (xa, xb) = (col(a) - hl / 2., col(b) + hl / 2.);
		let (bar, foot) = (grid.width(2.).max(2.), grid.width(6.));
		for (l, r) in [(xa, xa + bar), (xb - bar, xb)] {
			fill((l, inner_top, r, inner_bottom), palette.accent);
		}
		for (l, r) in [(xa, xa + foot), (xb - foot, xb)] {
			fill((l, inner_top, r, inner_top + bar), palette.accent);
			fill((l, inner_bottom - bar, r, inner_bottom), palette.accent);
		}
	}
	if let Some(h) = hover.filter(|&h| h != model.position) {
		line(col(h), top, bottom, with_alpha(palette.timeline_tick, 0x80));
	}
	let x = col(model.position.min(model.last()));
	line(x, top, bottom, palette.text);
	let (hw, head) = (grid.width(5.), grid.width(6.));
	let handle = vec![grid.point(x - hw, top - head), grid.point(x + hw, top - head), grid.point(x, top)];
	painter.add(Shape::convex_polygon(handle, palette.text, Stroke::NONE));
	let number = painter.layout_no_wrap(
		model.position.to_string(),
		FontId::new(LABEL_SIZE, wavedude_view::fonts::bold()),
		palette.text,
	);
	let w = number.size().x * grid.scale;
	let gap = grid.width(8.);
	let lx = if x + gap + w <= x1 { x + gap } else { x - gap - w }.round();
	let pad = grid.width(3.);
	fill((lx - pad, 0., lx + w + pad, top), palette.control_background);
	painter.galley(grid.point(lx, 0.), number, palette.text);
	if ring {
		painter.rect_stroke(rect, 0., Stroke::new(1., palette.accent), StrokeKind::Inside);
	}
}
