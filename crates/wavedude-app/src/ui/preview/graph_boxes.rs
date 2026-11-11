//! The probe graph boxes over a picture (docs/ui/40-preview-pane.md, Probe graph box): the data at the frame shown
//! (the run's recording cut at the frame's step), the axes, the box's place and size in
//! canvas pixels with snap and hysteresis, and the boxes as Freya elements.
//!
//! Coordinates: box positions and sizes in canvas pixels; screen values in logical pixels of the picture's cell.
//! Threads: the UI thread.

use std::cell::RefCell;
use std::rc::Rc;

use freya::prelude::*;
use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::model::{BoxSize, Layer, Probe, ProbeEdit, Project};
use wavedude_export::graph;
use wavedude_widgets::graph_box::GraphBoxEvent;
use wavedude_widgets::menu::ContextMenuRequest;
use wavedude_widgets::picture::CanvasTransform;
use wavedude_widgets::{
	Callback, GraphBox, GraphData, GraphLayout, GraphTick, MenuItem, MenuModel, TestIdExt, TextMeasure, Tip,
};

use super::{PaneStates, PictureMenu, PreviewAction, layer_name};
use crate::ui::shared::{Shared, Topics};
use wavedude_app_state::commands::{Command, Edit};
use wavedude_app_state::selection::{ItemKind, ItemRef, Row};
use wavedude_app_state::{AppState, Event as AppEvent};

/// A box's anchor within this many screen pixels of point A lands on it.
pub const SNAP_RADIUS: f32 = 12.;
/// An attached box stays on A until dragged this far, screen pixels.
pub const RELEASE_DISTANCE: f32 = 24.;
/// A resized box within this many screen pixels of a fixed size takes it.
pub const SIZE_SNAP: f32 = 8.;

/// A probe's recording as it stood at the frame shown, owned so the box can lay it out at any size.
#[derive(Clone, Debug, Default)]
struct Recording {
	series: Rc<[f32]>,
	profile: Option<Rc<[f32]>>,
	length: f64,
	sampled: u64,
}

impl Recording {
	fn at(trace: &ProbeTrace, step: u64, frame: usize) -> Self {
		let d = graph::GraphData::at(trace, step, frame);
		Self {
			series: d.series.into(),
			profile: d.profile.map(Rc::from),
			length: d.profile_length,
			sampled: d.steps_sampled,
		}
	}

	fn data(&self) -> graph::GraphData<'_> {
		graph::GraphData {
			series: &self.series,
			profile: self.profile.as_deref(),
			profile_length: self.length,
			steps_sampled: self.sampled,
		}
	}
}

/// The box's text metrics for the shared layout.
struct Metrics<'a>(&'a TextMeasure<'a>);

impl graph::TextMetrics for Metrics<'_> {
	fn line_height(&self) -> f32 {
		self.0.line_height
	}

	fn width(&self, text: &str) -> f32 {
		(self.0.width)(text)
	}
}

/// Probe `probe`'s graph over `recording` in a box `width` x `height`, laid out as an export lays it out.
fn graph_layout(
	probe: &Probe,
	recording: &Recording,
	steps: u64,
	text: &TextMeasure<'_>,
	size: (f32, f32),
) -> GraphLayout {
	let g = graph::layout(probe, &recording.data(), steps, size.0, size.1, &Metrics(text));
	let ticks = |ticks: Vec<graph::Tick>| {
		ticks.into_iter().map(|t| GraphTick { position: t.position, label: t.label, zero: t.zero }).collect()
	};
	GraphLayout {
		has_plot: g.visible && g.has_plot,
		plot: (g.plot_x, g.plot_y, g.plot_width, g.plot_height),
		line_width: g.line_width,
		y_ticks: ticks(g.y_ticks),
		x_ticks: ticks(g.x_ticks),
		curve: g.curve,
		title: g.title,
		readout: g.readout,
	}
}

/// The box's size in canvas pixels: the probe's, shrunk with its aspect kept to fit the canvas.
pub fn box_size(probe: &Probe, canvas: (u32, u32)) -> (f32, f32) {
	let (w, h) = probe.box_pixels();
	let (w, h) = (w as f32, h as f32);
	let f = (canvas.0 as f32 / w).min(canvas.1 as f32 / h).min(1.);
	(w * f, h * f)
}

/// The box's top-left corner in canvas pixels, held inside the canvas.
pub fn box_origin(probe: &Probe, canvas: (u32, u32)) -> (f32, f32) {
	let (w, h) = box_size(probe, canvas);
	let x = (probe.box_x as f32).clamp(0., (canvas.0 as f32 - w).max(0.));
	let y = (probe.box_y as f32).clamp(0., (canvas.1 as f32 - h).max(0.));
	(x, y)
}

/// Where a box dragged by `delta` canvas pixels from `start` lands: held inside the canvas, snapped onto point A
/// within [`SNAP_RADIUS`] screen pixels, and kept on A while it started there and moved less than
/// [`RELEASE_DISTANCE`].
pub fn dragged_box(
	start: (i32, i32),
	a: (i32, i32),
	delta: (f32, f32),
	scale: f32,
	canvas: (u32, u32),
	size: (f32, f32),
) -> (i32, i32) {
	let x = (start.0 as f32 + delta.0).round().clamp(0., (canvas.0 as f32 - size.0).max(0.).floor()) as i32;
	let y = (start.1 as f32 + delta.1).round().clamp(0., (canvas.1 as f32 - size.1).max(0.).floor()) as i32;
	let screen = |p: (i32, i32)| ((p.0 - a.0) as f32).hypot((p.1 - a.1) as f32) * scale;
	let attached = start == a;
	let moved = delta.0.hypot(delta.1) * scale;
	if (attached && moved < RELEASE_DISTANCE) || screen((x, y)) <= SNAP_RADIUS { a } else { (x, y) }
}

/// The box size a resize to `w` x `h` canvas pixels gives: a fixed size within [`SIZE_SNAP`] screen pixels, else
/// Custom at whole pixels.
pub fn resized_box(w: f32, h: f32, scale: f32) -> (BoxSize, u32, u32) {
	let tolerance = SIZE_SNAP / scale.max(1e-3);
	for size in [BoxSize::Tiny, BoxSize::Small, BoxSize::Medium, BoxSize::Large, BoxSize::ExtraLarge] {
		let (nw, nh) = size.nominal().expect("a fixed size");
		if (nw as f32 - w).abs() <= tolerance && (nh as f32 - h).abs() <= tolerance {
			return (size, nw, nh);
		}
	}
	(BoxSize::Custom, w.round().max(1.) as u32, h.round().max(1.) as u32)
}

/// One box to draw: its probe and graph.
#[derive(Clone, Debug, PartialEq)]
pub struct BoxView {
	pub index: usize,
	pub data: GraphData,
	pub origin: (f32, f32),
	pub size: (f32, f32),
	pub selected: bool,
}

/// Whether `layer` shows graph boxes: an active probe measures it.
pub fn has_boxes(project: &Project, layer: Layer) -> bool {
	project.probes.iter().any(|p| p.active && p.layer == layer)
}

/// The boxes of the active probes measuring `layer`, with the data of the frame shown.
pub fn boxes<B: wavedude_app_state::live::Backend>(app: &AppState<B>, layer: Layer) -> Vec<BoxView> {
	if !has_boxes(&app.document.project, layer) {
		return Vec::new();
	}
	let live = &app.live;
	match (live.shown(), live.run_request()) {
		(Some(k), Some(request)) => {
			let names: Vec<&str> = request.probes().map(|p| p.name.as_str()).collect();
			let cut = (request.schedule().step_of(k) as usize + 1, k as usize);
			let mut out = Vec::new();
			live.with_traces(&mut |t| out = views(app, layer, &names, t, Some(cut)));
			out
		}
		_ => views(app, layer, &[], &[], None),
	}
}

/// The boxes from `traces`, recorded for the probes `names`; `cut` is the frame shown (samples, profile index)
/// of a run's traces.
fn views<B: wavedude_app_state::live::Backend>(
	app: &AppState<B>,
	layer: Layer,
	names: &[&str],
	traces: &[ProbeTrace],
	cut: Option<(usize, usize)>,
) -> Vec<BoxView> {
	let project = &app.document.project;
	let canvas = project.canvas_size();
	let selected = app.selection.items();
	let steps = u64::from(project.simulation.steps);
	project
		.probes
		.iter()
		.enumerate()
		.filter(|(_, p)| p.active && p.layer == layer)
		.map(|(i, p)| {
			let trace = names.iter().position(|n| n.eq_ignore_ascii_case(&p.name)).and_then(|t| traces.get(t));
			let recording = match (trace, cut) {
				(Some(t), Some((samples, frame))) => Recording::at(t, samples.saturating_sub(1) as u64, frame),
				_ => Recording::default(),
			};
			let probe = p.clone();
			BoxView {
				index: i,
				data: GraphData::new(p.name.clone(), move |text, w, h| {
					graph_layout(&probe, &recording, steps, text, (w, h))
				}),
				origin: box_origin(p, canvas),
				size: box_size(p, canvas),
				selected: selected.contains(&ItemRef::new(ItemKind::Probe, i)),
			}
		})
		.collect()
}

/// A box drag in progress: the probe, its box at the press.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxGrab {
	pub index: usize,
	pub start: (i32, i32),
}

fn set_probe(shared: &Shared, index: usize, edit: ProbeEdit) {
	shared.send(AppEvent::Command(Command::Edit(Edit::Probe(index, edit))));
}

/// The boxes of `layer` over the picture's cell mapped by `transform`: the frame shown, its traces, the
/// probes and the selection redraw them; the picture itself does not.
#[derive(Clone, PartialEq)]
pub struct GraphBoxes {
	pub layer: Layer,
	pub states: PaneStates,
	pub transform: CanvasTransform,
}

impl Component for GraphBoxes {
	fn render(&self) -> impl IntoElement {
		let shared = Shared::get();
		shared.subscribe(Topics::DOCUMENT | Topics::SELECTION | Topics::PLAYBACK | Topics::PROGRESS | Topics::SETTINGS);
		// The data is rebuilt when what it is cut from moved: the project, the selection, the frame shown, the
		// run's frames or its traces; a heartbeat that moved none of them reuses it.
		let (key, layer) = shared.peek(|app| {
			let r = app.revisions();
			let live = &app.live;
			((r.document, r.selection, r.playback, r.settings, live.progress_key(), live.traces_steps()), self.layer)
		});
		let built = use_hook(|| Rc::new(RefCell::new(None::<(BoxKey, Vec<BoxView>)>)));
		let views = {
			let mut built = built.borrow_mut();
			match &*built {
				Some((k, v)) if *k == key => v.clone(),
				_ => {
					let mut views = shared.peek(|app| boxes(app, layer));
					views.sort_by_key(|v| v.selected);
					*built = Some((key, views.clone()));
					views
				}
			}
		};
		let t = self.transform;
		let boxes = views.into_iter().map(|view| {
			let at = t.to_screen(view.origin.0, view.origin.1);
			let size = ((view.size.0 * t.scale).round(), (view.size.1 * t.scale).round());
			ProbeBox {
				view,
				canvas: (t.canvas_width, t.canvas_height),
				scale: t.scale,
				at,
				size,
				layer: self.layer,
				states: self.states,
			}
		});
		rect()
			.position(Position::new_absolute().left(0.).top(0.))
			.width(Size::px(0.))
			.height(Size::px(0.))
			.children(boxes.map(IntoElement::into_element))
	}
}

/// What the boxes' data is cut from.
type BoxKey = (u64, u64, u64, u64, Option<(u64, usize)>, u32);

/// One box at `at` of `size` (logical pixels); its handler reads the geometry from a state, so it is made once
/// and the box compares equal while its data stays.
#[derive(Clone, PartialEq)]
struct ProbeBox {
	view: BoxView,
	canvas: (u32, u32),
	scale: f32,
	at: (f32, f32),
	size: (f32, f32),
	layer: Layer,
	states: PaneStates,
}

impl Component for ProbeBox {
	fn render(&self) -> impl IntoElement {
		let shared = Shared::get();
		let index = self.view.index;
		let item = ItemRef::new(ItemKind::Probe, index);
		let states = self.states;
		let mut geometry = use_state(|| (index, self.canvas, self.scale));
		geometry.set_if_modified((index, self.canvas, self.scale));
		let on_event = use_hook(|| {
			let shared = shared.clone();
			Callback::from(move |e: GraphBoxEvent| {
				let (index, canvas, scale) = *geometry.peek();
				on_box_event(&shared, states, index, canvas, scale, e);
			})
		});
		let (w, h) = self.size;
		let (sx, sy) = self.at;
		let shared_menu = shared.clone();
		let mut menu = states.menu;
		let (tip, grip_tip, export_text) = shared.peek(|app| {
			let s = &app.strings;
			(
				s.format("TipProbeBoxFormat", &[&self.view.data.title]),
				s.format("TipProbeBoxCornerFormat", &[&self.view.data.title]),
				s.get("MenuExportProbeCsv").to_owned(),
			)
		});
		rect()
			.position(Position::new_absolute().left(sx.round()).top(sy.round()))
			.width(Size::px(w))
			.height(Size::px(h))
			.on_pointer_down(move |e: Event<PointerEventData>| {
				if e.button() == Some(MouseButton::Right) {
					e.stop_propagation();
					shared_menu.update(|app| {
						if !app.selection.contains(item) {
							app.selection.select(Row::Item(item));
						}
					});
					let g = e.global_location();
					menu.set(Some(ContextMenuRequest {
						at: (g.x, g.y),
						model: MenuModel::new(vec![
							MenuItem::with_action(
								format!("probe-csv:{index}"),
								export_text.clone(),
								PictureMenu::Shell(PreviewAction::ExportProbeCsv(index)),
							)
							.enabled(!shared_menu.peek(|app| app.live.export_busy()))
							.into(),
						]),
					}));
				}
			})
			.child(
				Tip::new(tip).child(
					GraphBox::new(self.view.data.clone(), w, h, on_event)
						.selected(self.view.selected)
						.grip_tooltip(grip_tip)
						.test_id(format!("ProbeGraph:{}:{}", layer_name(self.layer), index)),
				),
			)
			.test_id(format!("ProbeGraphBox:{index}"))
	}
}

/// What a box's event does to probe `index`: moves and sizes come in logical pixels of a cell at `scale`.
fn on_box_event(shared: &Shared, states: PaneStates, index: usize, canvas: (u32, u32), scale: f32, e: GraphBoxEvent) {
	let item = ItemRef::new(ItemKind::Probe, index);
	let mut grab = states.box_grab;
	let probe = shared.peek(|app| app.document.project.probes.get(index).cloned());
	let Some(probe) = probe else { return };
	match e {
		GraphBoxEvent::Move { dx, dy, done } => {
			let held = *grab.peek();
			let start = match held {
				Some(g) if g.index == index => g.start,
				_ => {
					let start = (probe.box_x, probe.box_y);
					if !done {
						grab.set(Some(BoxGrab { index, start }));
						shared.send(AppEvent::MouseDown);
					}
					start
				}
			};
			let size = box_size(&probe, canvas);
			let (bx, by) = if done && grab.peek().is_none() {
				let x = (start.0 + dx as i32).clamp(0, (canvas.0 as f32 - size.0).max(0.) as i32);
				let y = (start.1 + dy as i32).clamp(0, (canvas.1 as f32 - size.1).max(0.) as i32);
				(x, y)
			} else {
				dragged_box(start, (probe.ax, probe.ay), (dx / scale, dy / scale), scale, canvas, size)
			};
			if (bx, by) != (probe.box_x, probe.box_y) {
				set_probe(shared, index, ProbeEdit::BoxX(bx));
				set_probe(shared, index, ProbeEdit::BoxY(by));
			}
			if done && grab.peek().is_some() {
				grab.set(None);
				shared.send(AppEvent::MouseUp);
			}
		}
		GraphBoxEvent::Resize { width, height, done } => {
			if grab.peek().is_none() && !done {
				grab.set(Some(BoxGrab { index, start: (probe.box_x, probe.box_y) }));
				shared.send(AppEvent::MouseDown);
			}
			let (size, bw, bh) = resized_box(width / scale, height / scale, scale);
			if size != probe.box_size {
				set_probe(shared, index, ProbeEdit::BoxSize(size));
			}
			if size == BoxSize::Custom && (bw, bh) != (probe.box_width, probe.box_height) {
				set_probe(shared, index, ProbeEdit::BoxWidth(bw));
				set_probe(shared, index, ProbeEdit::BoxHeight(bh));
			}
			if done && grab.peek().is_some() {
				grab.set(None);
				shared.send(AppEvent::MouseUp);
			}
		}
		GraphBoxEvent::Delete => {
			shared.send(AppEvent::Command(Command::Remove { target: item }));
		}
		GraphBoxEvent::Focused => {
			shared.update(|app| {
				if !app.selection.contains(item) {
					app.selection.select(Row::Item(item));
				}
			});
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use wavedude_core::model::ProbeKind;

	#[test]
	fn a_line_probe_reads_its_maximum_as_next_writes_it() {
		let p = Probe { kind: ProbeKind::Line, ..Probe::default() };
		let recording = Recording {
			series: Rc::from([]),
			profile: Some(Rc::from([0., 1.2e-5, 3.08e-5, 2e-6])),
			length: 80.,
			sampled: 10,
		};
		let width = |t: &str| t.len() as f32 * 6.;
		let text = TextMeasure { line_height: 13., width: &width };
		let g = graph_layout(&p, &recording, 100, &text, (300., 150.));
		assert_eq!(g.readout, "max 3.08E-05");
		assert!(g.has_plot);
		let labels: Vec<&str> = g.x_ticks.iter().map(|t| t.label.as_str()).collect();
		assert_eq!(labels, ["0", "20", "40", "60", "80"]);
		assert!(g.y_ticks.iter().any(|t| t.label == "2E-05"), "{:?}", g.y_ticks);
	}

	#[test]
	fn boxes_fit_the_canvas_and_snap_onto_their_probe() {
		let p = Probe { box_size: BoxSize::Medium, box_x: 350, box_y: -5, ..Probe::default() };
		assert_eq!(box_size(&p, (1000, 1000)), (400., 200.));
		assert_eq!(box_size(&p, (200, 400)), (200., 100.), "shrunk with its aspect kept");
		assert_eq!(box_origin(&p, (600, 300)), (200., 0.));
		let size = (100., 50.);
		assert_eq!(dragged_box((0, 0), (40, 40), (35., 38.), 1., (500, 500), size), (40, 40), "within the snap radius");
		assert_eq!(dragged_box((0, 0), (40, 40), (20., 20.), 1., (500, 500), size), (20, 20));
		assert_eq!(dragged_box((40, 40), (40, 40), (15., 0.), 1., (500, 500), size), (40, 40), "held until released");
		assert_eq!(dragged_box((40, 40), (40, 40), (30., 0.), 1., (500, 500), size), (70, 40));
		assert_eq!(dragged_box((0, 0), (300, 300), (900., -10.), 1., (500, 500), size), (400, 0), "kept inside");
	}

	#[test]
	fn resizing_snaps_to_the_fixed_sizes() {
		assert_eq!(resized_box(304., 147., 1.), (BoxSize::Small, 300, 150));
		assert_eq!(resized_box(320., 147., 1.), (BoxSize::Custom, 320, 147));
		assert_eq!(resized_box(320., 147., 0.25).0, BoxSize::Small, "the tolerance is in screen pixels");
	}
}
