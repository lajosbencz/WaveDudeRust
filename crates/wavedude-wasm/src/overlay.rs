//! The shapes over a picture: what the desktop preview draws over the same frame with nothing selected
//! (docs/ui/40-preview-pane.md, Handles and overlays, Probe graph box), in drawing order: the edge line, the handles
//! and labels the project's Display settings show, clipped to the picture, then the graph boxes of the active probes
//! measuring the layer, their curves cut at the frame.
//!
//! Coordinates: relative to the picture's top-left; a [`Group`] says how its units map to device pixels.

use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::model::{Layer, Project};
use wavedude_core::spec::RunSpec;
use wavedude_view::draw::{DeviceGrid, Rect, Shape, TextMetrics};
use wavedude_view::graph::{self, GraphData};
use wavedude_view::overlay::{self as view, HANDLE};

use crate::session::SessionError;

/// Shapes drawn in one coordinate system: `scale` device pixels per unit, the unit origin `origin` device pixels
/// from the picture's top-left.
#[derive(Debug, PartialEq)]
pub(crate) struct Group {
	pub(crate) scale: f32,
	pub(crate) origin: (f32, f32),
	pub(crate) shapes: Vec<Shape>,
}

impl Group {
	fn device(origin: (f32, f32), shapes: Vec<Shape>) -> Self {
		Self { scale: 1., origin, shapes }
	}
}

/// The longest side of a picture in device pixels, half the 32767 a side Chrome's and Firefox's canvases allow.
pub(crate) const MAX_SIDE: u32 = 16384;

/// A picture on the page: `width` x `height` CSS pixels on a screen of `dpr` device pixels per CSS pixel, its
/// top-left on the device grid.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Screen {
	width: f32,
	height: f32,
	dpr: f32,
	device: (u32, u32),
}

impl Screen {
	/// Refuses a ratio that is not finite and positive, or a size of fewer than 1 or more than [`MAX_SIDE`] device
	/// pixels a side.
	pub(crate) fn new(width: f32, height: f32, dpr: f32) -> Result<Self, SessionError> {
		let side = |css: f32| Some((css * dpr).round()).filter(|d| (1. ..=MAX_SIDE as f32).contains(d));
		match (side(width), side(height)) {
			(Some(w), Some(h)) if dpr.is_finite() && dpr > 0. => {
				Ok(Self { width, height, dpr, device: (w as u32, h as u32) })
			}
			_ => Err(SessionError::Screen { width, height, dpr }),
		}
	}

	/// The picture's size in whole device pixels.
	pub(crate) fn device(&self) -> (u32, u32) {
		self.device
	}

	/// Device pixels of the edge line, drawn outside the picture.
	pub(crate) fn hairline(&self) -> f32 {
		DeviceGrid::new(self.dpr, 0., 0.).hairline()
	}
}

/// Text metrics in CSS pixels: of the labels at [`view::LABEL_SIZE`] in [`view::LABEL_STYLE`], of the graph boxes
/// at [`graph::FONT_SIZE`] in [`TextStyle::GraphBox`](wavedude_view::draw::TextStyle::GraphBox).
#[derive(Clone, Copy)]
pub(crate) struct Fonts<'a> {
	pub(crate) labels: &'a dyn TextMetrics,
	pub(crate) boxes: &'a dyn TextMetrics,
}

/// The groups over the picture of `layer` at timeline frame `frame` of the run `spec` of `project`, the canvas
/// fitted into `screen`; `traces` are the spec's probes' recordings.
pub(crate) fn groups(
	project: &Project,
	spec: &RunSpec,
	traces: &[ProbeTrace],
	frame: u32,
	layer: Layer,
	screen: Screen,
	fonts: Fonts<'_>,
) -> Vec<Group> {
	let Screen { width, height, dpr, .. } = screen;
	let canvas = (spec.scene.canvas_width, spec.scene.canvas_height);
	let scale = (width / canvas.0 as f32).min(height / canvas.1 as f32);
	let picture = (0., 0., width, height);
	let mut out = Vec::new();
	if let Some(color) = view::edge_color(&project.scene) {
		out.push(Group::device((0., 0.), vec![view::edge_line(DeviceGrid::new(dpr, 0., 0.), picture, color)]));
	}

	let d = &project.display;
	let model = view::build(project, &[], None, layer, scale, d.handles_in_preview, d.labels_in_preview);
	let mut shapes = vec![Shape::Clip { rect: Rect::new(0., 0., width, height), antialias: true }];
	shapes.extend(view::shapes(&model, scale, picture, None, HANDLE, fonts.labels));
	shapes.push(Shape::Unclip);
	out.push(Group { scale: dpr, origin: (0., 0.), shapes });

	let (step, total) = (u64::from(spec.step_of(frame)), u64::from(spec.steps));
	for (probe, trace) in spec.probes.iter().zip(traces).filter(|(p, _)| graph::shows_box(p, layer)) {
		let origin = graph::box_origin(probe, canvas);
		let (at, size) = graph::box_on_picture(origin, graph::box_size(probe, canvas), (0., 0.), scale);
		let g = graph::layout(probe, &GraphData::at(trace, step, frame as usize), total, size.0, size.1, fonts.boxes);
		let shapes = graph::box_shapes(&g, DeviceGrid::new(dpr, at.0, at.1), false, None, fonts.boxes);
		out.push(Group::device(((at.0 * dpr).floor(), (at.1 * dpr).floor()), shapes));
	}
	out
}
