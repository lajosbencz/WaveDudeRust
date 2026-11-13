//! The probe graph boxes over a picture (docs/ui/40-preview-pane.md, Probe graph box): the data at the frame shown
//! (the run's recording cut at the frame's step), and the box's drag and resize in canvas pixels with snap and
//! hysteresis; where a box shows is [`wavedude_view::graph`]'s.
//!
//! Coordinates: box positions and sizes in canvas pixels; screen values in logical pixels of the picture's cell.

use std::rc::Rc;

use wavedude_app_state::commands::Edit;
use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::model::{BoxSize, ItemKind, ItemRef, Layer, Probe, ProbeEdit, Project};
use wavedude_core::spec::Schedule;
use wavedude_view::graph;

use crate::App;

/// A box's anchor within this many screen pixels of point A lands on it.
pub const SNAP_RADIUS: f32 = 12.;
/// An attached box stays on A until dragged this far, screen pixels.
pub const RELEASE_DISTANCE: f32 = 24.;
/// A resized box within this many screen pixels of a fixed size takes it.
pub const SIZE_SNAP: f32 = 8.;

/// A probe's recording as it stood at the frame shown, owned so the box can lay it out at any size.
#[derive(Clone, Debug, Default)]
pub struct Recording {
	pub series: Rc<[f32]>,
	pub profile: Option<Rc<[f32]>>,
	pub length: f64,
	pub sampled: u64,
	/// Smallest top of the graph's fitted range.
	pub floor: f64,
}

impl Recording {
	/// The recording of `trace` up to `step`, its profile at `frame`.
	pub fn at(trace: &ProbeTrace, step: u64, frame: usize) -> Self {
		let d = graph::GraphData::at(trace, step, frame);
		Self {
			series: d.series.into(),
			profile: d.profile.map(Rc::from),
			length: d.profile_length,
			sampled: d.steps_sampled,
			floor: 0.0,
		}
	}

	/// The recording as the graph layout reads it.
	pub fn data(&self) -> graph::GraphData<'_> {
		graph::GraphData {
			series: &self.series,
			profile: self.profile.as_deref(),
			profile_length: self.length,
			steps_sampled: self.sampled,
			floor: self.floor,
		}
	}
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

/// Whether `layer` shows graph boxes: an active probe measures it.
pub fn has_boxes(project: &Project, layer: Layer) -> bool {
	project.probes.iter().any(|p| graph::shows_box(p, layer))
}

/// One box to draw: its probe, the recording at the frame shown and its place in canvas pixels.
pub struct BoxView {
	pub index: usize,
	pub probe: Probe,
	pub recording: Recording,
	pub origin: (f32, f32),
	pub size: (f32, f32),
	pub selected: bool,
}

/// What the boxes' data is cut from: the project, the selection, the frame shown, the settings, the run's frames
/// and its traces.
pub type BoxKey = (u64, u64, u64, u64, Option<(u64, usize)>, u32);

/// The [`BoxKey`] of the state as it stands.
pub fn box_key(app: &App) -> BoxKey {
	let r = app.revisions();
	(r.document, r.selection, r.playback, r.settings, app.live.progress_key(), app.live.traces_steps())
}

/// The boxes of the active probes measuring `layer` with the data of the frame shown, the selected ones last.
pub fn boxes(app: &App, layer: Layer) -> Vec<BoxView> {
	if !has_boxes(&app.document.project, layer) {
		return Vec::new();
	}
	let live = &app.live;
	let mut out = match (live.shown(), live.run_request()) {
		(Some(k), Some(request)) => {
			let names: Vec<&str> = request.probes().map(|p| p.name.as_str()).collect();
			let cut = shown_cut(&request.schedule(), k);
			let mut out = Vec::new();
			live.with_traces(&mut |t| out = views(app, layer, &names, t, Some(cut)));
			out
		}
		_ => views(app, layer, &[], &[], None),
	};
	out.sort_by_key(|v| v.selected);
	out
}

/// The (step, frame) a graph is cut at when timeline frame `k` is shown: the picture shows the stored frame, which
/// is the timeline frame a reduced frame divisor rounds down to.
fn shown_cut(schedule: &Schedule, k: u32) -> (u64, usize) {
	let shown = schedule.stored_index_of(k) * schedule.frame_divisor();
	(u64::from(schedule.step_of(shown)), shown as usize)
}

/// The boxes from `traces`, recorded for the probes `names`; `cut` is the frame shown (step, frame) of a run's
/// traces.
fn views(app: &App, layer: Layer, names: &[&str], traces: &[ProbeTrace], cut: Option<(u64, usize)>) -> Vec<BoxView> {
	let project = &app.document.project;
	let canvas = project.canvas_size();
	let selected = app.selection.items();
	project
		.probes
		.iter()
		.enumerate()
		.filter(|(_, p)| graph::shows_box(p, layer))
		.map(|(i, p)| {
			let trace = names.iter().position(|n| n.eq_ignore_ascii_case(&p.name)).and_then(|t| traces.get(t));
			let recording = match (trace, cut) {
				(Some(t), Some((step, frame))) => {
					Recording { floor: graph::full_scale(p, &project.display), ..Recording::at(t, step, frame) }
				}
				_ => Recording::default(),
			};
			BoxView {
				index: i,
				probe: p.clone(),
				recording,
				origin: graph::box_origin(p, canvas),
				size: graph::box_size(p, canvas),
				selected: selected.contains(&ItemRef::new(ItemKind::Probe, i)),
			}
		})
		.collect()
}

/// The edits that put probe `index`'s box at `to`; none when it is there.
pub fn moved_box_edits(index: usize, probe: &Probe, to: (i32, i32)) -> Vec<Edit> {
	if to == (probe.box_x, probe.box_y) {
		return Vec::new();
	}
	vec![Edit::Probe(index, ProbeEdit::BoxX(to.0)), Edit::Probe(index, ProbeEdit::BoxY(to.1))]
}

/// The edits that size probe `index`'s box `w` x `h` screen pixels at `scale` ([`resized_box`]); none for its size.
pub fn resized_box_edits(index: usize, probe: &Probe, w: f32, h: f32, scale: f32) -> Vec<Edit> {
	let (size, bw, bh) = resized_box(w / scale, h / scale, scale);
	let mut edits = Vec::new();
	if size != probe.box_size {
		edits.push(Edit::Probe(index, ProbeEdit::BoxSize(size)));
	}
	if size == BoxSize::Custom && (bw, bh) != (probe.box_width, probe.box_height) {
		edits.extend([Edit::Probe(index, ProbeEdit::BoxWidth(bw)), Edit::Probe(index, ProbeEdit::BoxHeight(bh))]);
	}
	edits
}

#[cfg(test)]
mod tests {
	use super::*;
	use wavedude_core::model::{Probe, ProbeKind};
	use wavedude_core::quality::PreviewQuality;
	use wavedude_view::TextMetrics;

	struct Fixed;

	impl TextMetrics for Fixed {
		fn line_height(&mut self) -> f32 {
			13.
		}

		fn width(&mut self, text: &str) -> f32 {
			text.len() as f32 * 6.
		}
	}

	#[test]
	fn graphs_are_cut_at_the_stored_frame_shown() {
		let quality = PreviewQuality { frame_divisor: 4, ..PreviewQuality::default() };
		let schedule = Schedule { steps: 1000, visualize_every: 5, quality };
		assert_eq!(shown_cut(&schedule, 11), (40, 8));
		assert_eq!(shown_cut(&schedule, 8), (40, 8));
		let whole = Schedule { quality: PreviewQuality::default(), ..schedule };
		assert_eq!(shown_cut(&whole, 11), (55, 11));
	}

	#[test]
	fn a_line_probe_reads_its_maximum_as_next_writes_it() {
		let p = Probe { kind: ProbeKind::Line, ..Probe::default() };
		let recording = Recording {
			series: Rc::from([]),
			profile: Some(Rc::from([0., 1.2e-5, 3.08e-5, 2e-6])),
			length: 80.,
			sampled: 10,
			floor: 0.,
		};
		let g = graph::layout(&p, &recording.data(), 100, 300., 150., &mut Fixed);
		assert_eq!(g.readout, "max 3.08E-05");
		assert!(g.has_plot);
		let labels: Vec<&str> = g.x_ticks.iter().map(|t| t.label.as_str()).collect();
		assert_eq!(labels, ["0", "20", "40", "60", "80"]);
		assert!(g.y_ticks.iter().any(|t| t.label == "2E-05"), "{:?}", g.y_ticks);
	}

	#[test]
	fn boxes_snap_onto_their_probe() {
		let size = (100., 50.);
		assert_eq!(dragged_box((0, 0), (40, 40), (35., 38.), 1., (500, 500), size), (40, 40), "within the snap radius");
		assert_eq!(dragged_box((0, 0), (40, 40), (20., 20.), 1., (500, 500), size), (20, 20));
		assert_eq!(dragged_box((40, 40), (40, 40), (15., 0.), 1., (500, 500), size), (40, 40), "held until released");
		assert_eq!(dragged_box((40, 40), (40, 40), (30., 0.), 1., (500, 500), size), (70, 40));
		assert_eq!(dragged_box((0, 0), (300, 300), (900., -10.), 1., (500, 500), size), (400, 0), "kept inside");
	}

	#[test]
	fn box_edits_change_only_what_moved() {
		let p = Probe { box_x: 5, box_y: 6, box_size: BoxSize::Small, ..Probe::default() };
		assert!(moved_box_edits(0, &p, (5, 6)).is_empty());
		assert_eq!(
			moved_box_edits(2, &p, (7, 6)),
			[Edit::Probe(2, ProbeEdit::BoxX(7)), Edit::Probe(2, ProbeEdit::BoxY(6))]
		);
		assert!(resized_box_edits(0, &p, 602., 300., 2.).is_empty(), "still Small");
		assert_eq!(
			resized_box_edits(0, &p, 640., 294., 2.),
			[
				Edit::Probe(0, ProbeEdit::BoxSize(BoxSize::Custom)),
				Edit::Probe(0, ProbeEdit::BoxWidth(320)),
				Edit::Probe(0, ProbeEdit::BoxHeight(147)),
			]
		);
	}

	#[test]
	fn resizing_snaps_to_the_fixed_sizes() {
		assert_eq!(resized_box(304., 147., 1.), (BoxSize::Small, 300, 150));
		assert_eq!(resized_box(320., 147., 1.), (BoxSize::Custom, 320, 147));
		assert_eq!(resized_box(320., 147., 0.25).0, BoxSize::Small, "the tolerance is in screen pixels");
	}
}
