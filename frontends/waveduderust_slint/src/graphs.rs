//! The probe graph boxes over the pictures (docs/ui/40, Probe graph box): the core's layout, shared with an export,
//! as Slint rows in logical pixels of the box.

use std::fmt::Write as _;
use std::rc::Rc;

use slint::{ModelRc, VecModel};
use waveduderust::ProbeTrace;
use waveduderust::model::{Layer, Project, View};
use waveduderust::unstable::view::epaint::text::{Fonts, TextOptions};
use waveduderust::unstable::view::fonts;
use waveduderust::unstable::view::graph::{self, GraphData, GraphLayout};

use crate::models;
use crate::ui;

/// What the boxes show: the probe traces in the active probes' order, at a step and stored frame, and the steps
/// of the run.
pub(crate) struct Traces<'a> {
	pub(crate) traces: &'a [ProbeTrace],
	/// The run's active probes, the traces' order.
	pub(crate) probes: Vec<waveduderust::model::ProbeId>,
	pub(crate) step: u64,
	pub(crate) frame: usize,
	pub(crate) total_steps: u64,
}

/// A box as a picture shows it, for hit-testing: its probe (index), corner and size in canvas pixels.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placed {
	pub(crate) probe: usize,
	pub(crate) at: (f32, f32),
	pub(crate) size: (f32, f32),
}

impl Placed {
	pub(crate) fn contains(&self, at: (f32, f32)) -> bool {
		(self.at.0..=self.at.0 + self.size.0).contains(&at.0) && (self.at.1..=self.at.1 + self.size.1).contains(&at.1)
	}
}

pub(crate) struct Graphs {
	/// Measures the text as an export lays it out.
	fonts: Fonts,
	rows: [Rc<VecModel<ui::GraphBox>>; 2],
	/// The boxes of each picture, bottom first.
	placed: [Vec<Placed>; 2],
}

impl Default for Graphs {
	fn default() -> Self {
		Self {
			fonts: Fonts::new(TextOptions::default(), fonts::definitions()),
			rows: Default::default(),
			placed: Default::default(),
		}
	}
}

fn path_of(lines: impl Iterator<Item = ((f32, f32), (f32, f32))>) -> String {
	let mut out = String::new();
	for ((x0, y0), (x1, y1)) in lines {
		let _ = write!(out, "M {x0} {y0} L {x1} {y1} ");
	}
	out
}

fn ticks(ticks: &[graph::Tick]) -> ModelRc<ui::GraphTick> {
	let rows: Vec<ui::GraphTick> = ticks
		.iter()
		.filter(|t| !t.label.is_empty())
		.map(|t| ui::GraphTick { label: t.label.as_str().into(), x: t.label_at.0, y: t.label_at.1 })
		.collect();
	ModelRc::new(VecModel::from(rows))
}

/// A laid-out box as the picture draws it, at canvas pixel `at`.
fn row(g: &GraphLayout, probe: usize, at: (f32, f32), selected: bool) -> ui::GraphBox {
	let (px, py, pw, ph) = (g.plot_x, g.plot_y, g.plot_width, g.plot_height);
	let horizontal = |zero: bool| {
		path_of(g.y_ticks.iter().filter(|t| t.zero == zero).map(|t| ((px, t.position), (px + pw, t.position))))
	};
	let mut grid = horizontal(false);
	grid.push_str(&path_of(g.x_ticks.iter().map(|t| ((t.position, py), (t.position, py + ph)))));
	let mut curve = String::new();
	match g.curve.as_slice() {
		[] => {}
		[(x, y)] => {
			let _ = write!(curve, "M {} {y} a 2 2 0 1 0 4 0 a 2 2 0 1 0 -4 0 ", x - 2.);
		}
		points => {
			for (i, (x, y)) in points.iter().enumerate() {
				let _ = write!(curve, "{} {x} {y} ", if i == 0 { 'M' } else { 'L' });
			}
		}
	}
	ui::GraphBox {
		probe: models::index_to_ui(Some(probe)),
		x: at.0,
		y: at.1,
		width: g.width,
		height: g.height,
		has_plot: g.has_plot,
		plot: ui::Rect { x: px, y: py, width: pw, height: ph },
		grid: grid.into(),
		zero: horizontal(true).into(),
		curve: curve.into(),
		dot: g.curve.len() == 1,
		line_width: g.line_width,
		y_ticks: ticks(&g.y_ticks),
		x_ticks: ticks(&g.x_ticks),
		title: g.title.as_str().into(),
		readout: g.readout.as_str().into(),
		selected,
	}
}

impl Graphs {
	/// The probe (index) of the topmost box of `layer`'s picture under canvas pixel `at`.
	pub(crate) fn under(&self, layer: ui::Layer, at: (f32, f32)) -> Option<usize> {
		self.placed(layer).iter().rev().find(|b| b.contains(at)).map(|b| b.probe)
	}

	/// The boxes of `layer`'s picture, bottom first.
	pub(crate) fn placed(&self, layer: ui::Layer) -> &[Placed] {
		&self.placed[usize::from(layer == ui::Layer::Intensity)]
	}

	pub(crate) fn wire(&self, canvas: &ui::Canvas<'_>) {
		crate::canvas::edit_layer(canvas, ui::Layer::Field, |v| v.graphs = ModelRc::from(self.rows[0].clone()));
		crate::canvas::edit_layer(canvas, ui::Layer::Intensity, |v| v.graphs = ModelRc::from(self.rows[1].clone()));
	}

	/// Lays out the boxes of the active probes on both pictures, `scales` logical pixels per canvas pixel; the
	/// probes of `selected` (indices) drawn last with the accent edge. Without `data`, empty boxes.
	pub(crate) fn show(
		&mut self,
		project: &Project,
		view: &View,
		selected: &[usize],
		data: Option<&Traces<'_>>,
		scales: [f32; 2],
	) {
		let canvas = project.canvas_size();
		let total_steps = data.map_or(0, |d| d.total_steps);
		let mut fonts = self.fonts.with_pixels_per_point(1.);
		let pictures = self.rows.iter().zip(self.placed.iter_mut()).zip(scales);
		for (layer, ((rows, placed), scale)) in [Layer::Field, Layer::Intensity].into_iter().zip(pictures) {
			let mut boxes: Vec<(bool, ui::GraphBox, Placed)> = Vec::new();
			for (i, probe) in project.probes.iter().enumerate().filter(|(_, p)| p.active) {
				if !graph::shows_box(probe, layer) {
					continue;
				}
				let floor = graph::full_scale(probe, view.into());
				let trace = data.and_then(|d| {
					let n = d.probes.iter().position(|&id| id == probe.id())?;
					d.traces.get(n).map(|t| GraphData::at(t, d.step, d.frame))
				});
				let shown = trace.unwrap_or_default().with_floor(floor);
				let (w, h) = graph::box_size(probe, canvas);
				let g = graph::layout_with(&mut fonts, probe, &shown, total_steps, w * scale, h * scale);
				if g.visible {
					let picked = selected.contains(&i);
					let at = graph::box_origin(probe, canvas);
					let size = (g.width / scale, g.height / scale);
					boxes.push((picked, row(&g, i, at, picked), Placed { probe: i, at, size }));
				}
			}
			// the selected boxes over the rest
			boxes.sort_by_key(|(picked, ..)| *picked);
			*placed = boxes.iter().map(|(.., p)| *p).collect();
			models::update(rows, boxes.into_iter().map(|(_, b, _)| b));
		}
	}
}
