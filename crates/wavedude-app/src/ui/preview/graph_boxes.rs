//! The probe graph boxes over a picture (docs/ui/40-preview-pane.md, Probe graph box): the data at the frame shown
//! (the scratch frame's recording, else the run's cut at the frame's step), the axes, the box's place and size in
//! canvas pixels with snap and hysteresis, and the boxes as Freya elements.
//!
//! Coordinates: box positions and sizes in canvas pixels; screen values in logical pixels of the picture's cell.
//! Threads: the UI thread.

use std::rc::Rc;

use freya::prelude::*;
use serde_json::json;
use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::model::{BoxSize, Layer, Measurement, Probe, ProbeKind, Project, ScaleMode, YScale};
use wavedude_widgets::graph_box::GraphBoxEvent;
use wavedude_widgets::menu::ContextMenuRequest;
use wavedude_widgets::picture::CanvasTransform;
use wavedude_widgets::{GraphBox, GraphData, MenuItem, MenuModel, TestIdExt, ValueAxis};

use super::{PaneStates, PictureMenu, PreviewAction, layer_name};
use crate::ui::shared::Shared;
use wavedude_app_state::commands::{Command, Target};
use wavedude_app_state::live::Shown;
use wavedude_app_state::selection::{ItemKind, ItemRef, Row};
use wavedude_app_state::{AppState, Event as AppEvent};

/// A box's anchor within this many screen pixels of point A lands on it.
pub const SNAP_RADIUS: f32 = 12.;
/// An attached box stays on A until dragged this far, screen pixels.
pub const RELEASE_DISTANCE: f32 = 24.;
/// A resized box within this many screen pixels of a fixed size takes it.
pub const SIZE_SNAP: f32 = 8.;

/// The values a graph shows and where they sit on its step (or distance) axis.
#[derive(Clone, Debug, PartialEq)]
pub struct Series {
	pub values: Vec<f32>,
	pub x_range: (f64, f64),
	pub x_per_sample: f64,
}

/// A point probe's series cut after `samples` values (the frame's step index plus one): the last `window` steps
/// (Running) or all (Full); with `auto_x` the step axis follows the samples, else it spans the window or the run.
pub fn point_series(series: &[f32], samples: usize, mode: ScaleMode, window: u32, auto_x: bool, steps: u32) -> Series {
	let end = samples.min(series.len());
	let (start, x_range) = match mode {
		ScaleMode::Running => {
			let window = window as usize;
			let start = end.saturating_sub(window);
			let range = if auto_x {
				(start as f64, end.max(start + 1) as f64)
			} else if end >= window {
				(start as f64, (start + window) as f64)
			} else {
				(0., window as f64)
			};
			(start, range)
		}
		ScaleMode::Full => (0, if auto_x { (0., end.max(1) as f64) } else { (0., f64::from(steps)) }),
	};
	Series { values: series[start..end].to_vec(), x_range, x_per_sample: 1. }
}

/// A line probe's profile along A..B, `length` canvas pixels.
pub fn line_series(profile: &[f32], length: f64) -> Series {
	let per = if profile.len() > 1 { length / (profile.len() - 1) as f64 } else { 0. };
	Series { values: profile.to_vec(), x_range: (0., length.max(1e-9)), x_per_sample: per }
}

/// The value axis: amplitude symmetric about zero, an intensity linear or in decades; fitted or the probe's bounds.
pub fn axis_for(probe: &Probe, values: &[f32]) -> ValueAxis {
	if probe.measurement == Measurement::Amplitude {
		return if probe.auto_scale_y {
			ValueAxis::fit_symmetric(values)
		} else {
			ValueAxis::Symmetric { max: probe.y_min.abs().max(probe.y_max.abs()) }
		};
	}
	match (probe.y_scale, probe.auto_scale_y) {
		(YScale::Logarithmic, true) => ValueAxis::fit_logarithmic(values, probe.decades),
		(YScale::Logarithmic, false) => ValueAxis::Logarithmic { top: probe.y_max.max(1e-30), decades: probe.decades },
		(YScale::Linear, true) => ValueAxis::fit_linear(values),
		(YScale::Linear, false) => ValueAxis::Linear { min: probe.y_min, max: probe.y_max },
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

fn enum_name(size: BoxSize) -> &'static str {
	match size {
		BoxSize::Tiny => "Tiny",
		BoxSize::Small => "Small",
		BoxSize::Medium => "Medium",
		BoxSize::Large => "Large",
		BoxSize::ExtraLarge => "ExtraLarge",
		BoxSize::Custom => "Custom",
	}
}

fn readout(values: &[f32]) -> String {
	values.last().map_or_else(String::new, |v| {
		let v = f64::from(*v);
		if v != 0. && (v.abs() >= 1e4 || v.abs() < 1e-2) { format!("{v:.2e}") } else { format!("{v:.3}") }
	})
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
	match (live.graph_scratch(), live.shown()) {
		(Some(r), _) => {
			let names: Vec<&str> = r.spec.probes.iter().map(|p| p.name.as_str()).collect();
			views(app, layer, &names, &r.traces, None)
		}
		(None, Some(Shown::Run(k))) if live.run_spec().is_some() => {
			let spec = live.run_spec().expect("checked above");
			let names: Vec<&str> = spec.probes.iter().map(|p| p.name.as_str()).collect();
			let cut = (spec.step_of(k) as usize + 1, k as usize);
			let mut out = Vec::new();
			live.with_run_traces(&mut |t| out = views(app, layer, &names, t, Some(cut)));
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
	let steps = project.simulation.steps;
	project
		.probes
		.iter()
		.enumerate()
		.filter(|(_, p)| p.active && p.layer == layer)
		.map(|(i, p)| {
			let trace = names.iter().position(|n| n.eq_ignore_ascii_case(&p.name)).and_then(|t| traces.get(t));
			let series = match (p.kind, trace) {
				(ProbeKind::Point, Some(t)) => {
					let samples = cut.map_or(t.series.len(), |c| c.0);
					point_series(&t.series, samples, p.scale_mode, p.window, p.auto_scale_x, steps)
				}
				(ProbeKind::Line, Some(t)) => {
					let profile = match cut {
						Some((_, k)) => t.profiles.get(k.min(t.profiles.len().saturating_sub(1))),
						None => t.profiles.last(),
					};
					line_series(profile.unwrap_or_default(), t.length)
				}
				(_, None) => Series { values: Vec::new(), x_range: (0., 1.), x_per_sample: 1. },
			};
			let axis = axis_for(p, &series.values);
			BoxView {
				index: i,
				data: GraphData {
					title: p.name.clone(),
					readout: readout(&series.values),
					readout_color: Color::from_rgb(0xFF, 0xD7, 0x00),
					values: Rc::from(series.values),
					x_range: series.x_range,
					x_per_sample: series.x_per_sample,
					axis,
				},
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

fn set_probe(shared: &Shared, index: usize, path: &str, value: serde_json::Value) {
	shared.send(AppEvent::Command(Command::Set {
		target: Target::Item(ItemRef::new(ItemKind::Probe, index)),
		path: path.into(),
		value,
	}));
}

/// The boxes of `layer` as absolutely placed elements over the picture's cell mapped by `t`.
pub fn box_elements(shared: &Shared, states: PaneStates, layer: Layer, t: &CanvasTransform) -> Vec<Element> {
	let views = shared.peek(|app| boxes(app, layer));
	let mut views = views;
	views.sort_by_key(|v| v.selected);
	let canvas = (t.canvas_width, t.canvas_height);
	let scale = t.scale;
	views
		.into_iter()
		.map(|view| {
			let (sx, sy) = t.to_screen(view.origin.0, view.origin.1);
			let (w, h) = ((view.size.0 * scale).round(), (view.size.1 * scale).round());
			let index = view.index;
			let item = ItemRef::new(ItemKind::Probe, index);
			let shared_ev = shared.clone();
			let mut grab = states.box_grab;
			let on_event = move |e: GraphBoxEvent| {
				let shared = &shared_ev;
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
							set_probe(shared, index, "box_x", json!(bx));
							set_probe(shared, index, "box_y", json!(by));
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
							set_probe(shared, index, "box_size", json!(enum_name(size)));
						}
						if size == BoxSize::Custom && (bw, bh) != (probe.box_width, probe.box_height) {
							set_probe(shared, index, "box_width", json!(bw));
							set_probe(shared, index, "box_height", json!(bh));
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
			};
			let shared_menu = shared.clone();
			let mut menu = states.menu;
			let tip = shared.peek(|app| app.strings.format("TipProbeBoxFormat", &[&view.data.title]));
			let export_text = shared.peek(|app| app.strings.get("MenuExportProbeCsv").to_owned());
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
				.child(TooltipContainer::new(Tooltip::new_text(tip)).child(
					GraphBox::new(view.data, w, h, on_event).selected(view.selected).test_id(format!(
						"ProbeGraph:{}:{}",
						layer_name(layer),
						index
					)),
				))
				.test_id(format!("ProbeGraphBox:{index}"))
				.into_element()
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn running_graphs_slide_a_window_over_the_steps() {
		let series: Vec<f32> = (0..100).map(|i| i as f32).collect();
		let s = point_series(&series, 60, ScaleMode::Running, 20, false, 100);
		assert_eq!(s.values.first(), Some(&40.));
		assert_eq!(s.values.len(), 20);
		assert_eq!(s.x_range, (40., 60.));
		let early = point_series(&series, 5, ScaleMode::Running, 20, false, 100);
		assert_eq!(early.values.len(), 5);
		assert_eq!(early.x_range, (0., 20.), "the window keeps its width before it fills");
		let auto = point_series(&series, 5, ScaleMode::Running, 20, true, 100);
		assert_eq!(auto.x_range, (0., 5.));
	}

	#[test]
	fn full_graphs_span_the_run_or_follow_the_samples() {
		let series: Vec<f32> = vec![1.; 50];
		assert_eq!(point_series(&series, 30, ScaleMode::Full, 20, false, 200).x_range, (0., 200.));
		assert_eq!(point_series(&series, 30, ScaleMode::Full, 20, true, 200).x_range, (0., 30.));
		assert_eq!(
			point_series(&series, 500, ScaleMode::Full, 20, true, 200).values.len(),
			50,
			"cut at what was recorded"
		);
		assert!(point_series(&[], 10, ScaleMode::Running, 20, false, 10).values.is_empty());
	}

	#[test]
	fn line_profiles_span_the_probe_length() {
		let s = line_series(&[0., 1., 2., 3., 4.], 8.);
		assert_eq!(s.x_per_sample, 2.);
		assert_eq!(s.x_range, (0., 8.));
	}

	#[test]
	fn axes_follow_the_measurement() {
		let mut p = Probe::default();
		assert_eq!(axis_for(&p, &[0.5, -2.]), ValueAxis::Symmetric { max: 2. });
		p.auto_scale_y = false;
		p.y_min = -3.;
		p.y_max = 1.;
		assert_eq!(axis_for(&p, &[0.5]), ValueAxis::Symmetric { max: 3. });
		p.measurement = Measurement::Value;
		p.y_scale = YScale::Logarithmic;
		p.auto_scale_y = true;
		p.decades = 4;
		assert_eq!(axis_for(&p, &[0., 0.02, 3.]), ValueAxis::Logarithmic { top: 10., decades: 4 });
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
