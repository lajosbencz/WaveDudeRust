//! The pictures' overlays (docs/ui/40, Handles and overlays): the core's overlay model shown as Slint rows, and the
//! selection and drags its handles make.

use std::fmt::Write as _;
use std::rc::Rc;

use slint::{Model, ModelRc, VecModel};
use waveduderust::model::{ItemRef, Layer, ProbeKind, Project, View};
use waveduderust::unstable::view::overlay::{self, HandleKind, OverlayModel};

use crate::selection::{Selection, groupable};
use crate::tree::Node;
use crate::ui;

/// A layer's overlay as the window shows it; its rows change in place.
#[derive(Default)]
struct Shown {
	model: OverlayModel,
	handles: Rc<VecModel<ui::HandleData>>,
	labels: Rc<VecModel<ui::LabelData>>,
	/// Logical pixels per canvas pixel.
	scale: f32,
}

impl Shown {
	/// Logical pixels per canvas pixel; 1 until the picture reports it.
	fn scale(&self) -> f32 {
		if self.scale > 0. { self.scale } else { 1. }
	}
}

#[derive(Default)]
pub(crate) struct Overlays {
	shown: [Shown; 2],
}

/// Where a dragged item started.
#[derive(Clone, Copy)]
enum Start {
	Source(f64, f64),
	Part(i32, i32),
	/// A probe's A and B; `which` moves A, B or both.
	Probe([i32; 4], Which),
}

#[derive(Clone, Copy, PartialEq)]
enum Which {
	A,
	B,
	Both,
}

/// A handle drag under way: what it moves from where, one undo step.
pub(crate) struct Drag {
	key: String,
	node: Node,
	starts: Vec<(ItemRef, Start)>,
	/// The whole-pixel move applied last.
	by: (f64, f64),
}

fn index(layer: ui::Layer) -> usize {
	match layer {
		ui::Layer::Field => 0,
		ui::Layer::Intensity => 1,
	}
}

pub(crate) fn item_of(project: &Project, node: Node) -> Option<ItemRef> {
	match node {
		Node::Source(i) => project.sources.get(i).map(|s| ItemRef::Source(s.id())),
		Node::Part(i) => project.parts.get(i).map(|p| ItemRef::Part(p.id())),
		Node::Probe(i) => project.probes.get(i).map(|p| ItemRef::Probe(p.id())),
		_ => None,
	}
}

pub(crate) fn node_of(project: &Project, item: ItemRef) -> Option<Node> {
	match item {
		ItemRef::Source(id) => project.sources.iter().position(|s| s.id() == id).map(Node::Source),
		ItemRef::Part(id) => project.parts.iter().position(|p| p.id() == id).map(Node::Part),
		ItemRef::Probe(id) => project.probes.iter().position(|p| p.id() == id).map(Node::Probe),
		_ => None,
	}
}

fn glyph(kind: HandleKind) -> ui::HandleGlyph {
	match kind {
		HandleKind::Source => ui::HandleGlyph::Source,
		HandleKind::SourceLever | HandleKind::PartLever => ui::HandleGlyph::Lever,
		HandleKind::PartCentre => ui::HandleGlyph::Part,
		HandleKind::Grip => ui::HandleGlyph::Grip,
		HandleKind::Point(_) | HandleKind::Node(_) => ui::HandleGlyph::Point,
		HandleKind::InHandle(_) | HandleKind::OutHandle(_) => ui::HandleGlyph::Bezier,
		HandleKind::ProbeA | HandleKind::ProbeB => ui::HandleGlyph::Probe,
	}
}

/// Replaces `rows` with `new`, row by row while the count holds.
fn update<T: Clone + PartialEq + 'static>(rows: &VecModel<T>, new: Vec<T>) {
	if rows.row_count() != new.len() {
		rows.set_vec(new);
		return;
	}
	for (i, row) in new.into_iter().enumerate() {
		if rows.row_data(i).as_ref() != Some(&row) {
			rows.set_row_data(i, row);
		}
	}
}

/// SVG path commands of small circles of `r` canvas pixels around `points`.
fn circles(points: impl Iterator<Item = (f32, f32)>, r: f32) -> String {
	let mut out = String::new();
	for (x, y) in points {
		let _ = write!(out, "M {} {} a {r} {r} 0 1 0 {} 0 a {r} {r} 0 1 0 {} 0 ", x - r, y, 2. * r, -2. * r);
	}
	out
}

fn polyline(points: &[(f32, f32)], closed: bool, out: &mut String) {
	for (i, (x, y)) in points.iter().enumerate() {
		let _ = write!(out, "{} {x} {y} ", if i == 0 { 'M' } else { 'L' });
	}
	if closed && !points.is_empty() {
		out.push_str("Z ");
	}
}

impl Overlays {
	pub(crate) fn wire(&self, canvas: &ui::Canvas<'_>) {
		let [field, intensity] = &self.shown;
		canvas.set_field_handles(ModelRc::from(field.handles.clone()));
		canvas.set_field_labels(ModelRc::from(field.labels.clone()));
		canvas.set_intensity_handles(ModelRc::from(intensity.handles.clone()));
		canvas.set_intensity_labels(ModelRc::from(intensity.labels.clone()));
	}

	/// Notes the pictures' scale; whether it changed.
	pub(crate) fn rescale(&mut self, layer: ui::Layer, scale: f32) -> bool {
		let shown = &mut self.shown[index(layer)];
		let changed = (shown.scale - scale).abs() > f32::EPSILON;
		shown.scale = scale;
		changed
	}

	/// Shows both overlays of `project` with `selection`.
	pub(crate) fn show(&mut self, canvas: &ui::Canvas<'_>, project: &Project, view: &View, selection: &Selection) {
		let selected: Vec<ItemRef> = selection.group().into_iter().filter_map(|n| item_of(project, n)).collect();
		let current = item_of(project, selection.current());
		let (w, h) = project.canvas_size();
		canvas.set_width(i32::try_from(w).unwrap_or(i32::MAX));
		canvas.set_height(i32::try_from(h).unwrap_or(i32::MAX));
		for (layer, shown) in [Layer::Field, Layer::Intensity].into_iter().zip(&mut self.shown) {
			let scale = shown.scale();
			let m = overlay::build(
				project,
				&selected,
				current,
				layer,
				scale,
				view.handles_in_preview,
				view.labels_in_preview,
			);
			let handles = m
				.handles
				.iter()
				.map(|h| ui::HandleData { x: h.at.0, y: h.at.1, glyph: glyph(h.kind), selected: h.selected })
				.collect();
			update(&shown.handles, handles);
			let labels = m
				.labels
				.iter()
				.map(|l| ui::LabelData {
					text: l.text.as_str().into(),
					x: l.at.0,
					y: l.at.1,
					below: l.below,
					selected: l.selected,
				})
				.collect();
			update(&shown.labels, labels);
			let r = 1.5 / scale;
			let (mut lines, mut picked) = (String::new(), String::new());
			for l in &m.lines {
				polyline(&[l.a, l.b], false, if l.selected { &mut picked } else { &mut lines });
			}
			for h in &m.handles {
				if let Some(from) = h.from {
					polyline(&[from, h.at], false, if h.selected { &mut picked } else { &mut lines });
				}
			}
			let mut outlines = String::new();
			for o in &m.outlines {
				polyline(&o.points, o.closed, &mut outlines);
			}
			let art = ui::OverlayArt {
				lines: lines.into(),
				selected_lines: picked.into(),
				outlines: outlines.into(),
				dots: circles(m.dots.iter().filter(|d| !d.selected).map(|d| d.at), r).into(),
				selected_dots: circles(m.dots.iter().filter(|d| d.selected).map(|d| d.at), r).into(),
			};
			match layer {
				Layer::Field => canvas.set_field_art(art),
				_ => canvas.set_intensity_art(art),
			}
			shown.model = m;
		}
	}

	/// A press at canvas pixel `at` of `layer`'s picture. On a handle (the topmost within reach): selects as a
	/// click does (Ctrl toggles, Shift adds, neither drags) and starts a drag of what the handle moves. Elsewhere:
	/// deselects unless Ctrl or Shift is held.
	pub(crate) fn press(
		&self,
		project: &Project,
		selection: &mut Selection,
		layer: ui::Layer,
		at: (f32, f32),
		(control, shift): (bool, bool),
		gesture: u64,
	) -> Option<Drag> {
		let shown = &self.shown[index(layer)];
		let Some(handle) = hit(&shown.model, at, HIT_REACH / shown.scale()) else {
			if !control && !shift {
				selection.set(Node::Project);
			}
			return None;
		};
		let node = node_of(project, handle.item)?;
		if control {
			selection.toggle(node);
			return None;
		}
		if shift {
			if !selection.contains(node) {
				selection.toggle(node);
			}
			return None;
		}
		let group = selection.group_of(node);
		let starts = if group.len() > 1 && handle.kind.moves_item() {
			selection.pick(project, node, ui::Pick::Focus);
			group.iter().filter_map(|&n| start(project, n, Which::Both)).collect()
		} else {
			selection.set(node);
			let which = match handle.kind {
				HandleKind::ProbeA => Which::A,
				HandleKind::ProbeB => Which::B,
				HandleKind::Source | HandleKind::PartCentre => Which::Both,
				_ => return None,
			};
			start(project, node, which).into_iter().collect()
		};
		Some(Drag { key: format!("drag {gesture}"), node, starts, by: (0., 0.) })
	}
}

/// Logical pixels from a handle's point that still press it.
const HIT_REACH: f32 = 10.;

/// The topmost handle of `model` within `reach` canvas pixels of `at`.
fn hit(model: &OverlayModel, at: (f32, f32), reach: f32) -> Option<&overlay::Handle> {
	model.handles.iter().rev().find(|h| (h.at.0 - at.0).hypot(h.at.1 - at.1) <= reach)
}

fn start(project: &Project, node: Node, which: Which) -> Option<(ItemRef, Start)> {
	let item = item_of(project, node)?;
	let start = match node {
		Node::Source(i) => Start::Source(project.sources[i].x, project.sources[i].y),
		Node::Part(i) => Start::Part(project.parts[i].base.x, project.parts[i].base.y),
		Node::Probe(i) => {
			let p = &project.probes[i];
			// a point probe's B is unused
			let which = if p.kind == ProbeKind::Line { which } else { Which::A };
			Start::Probe([p.ax, p.ay, p.bx, p.by], which)
		}
		_ => return None,
	};
	groupable(node).then_some((item, start))
}

impl Start {
	/// The canvas points the drag moves.
	fn points(self) -> Vec<(f64, f64)> {
		match self {
			Self::Source(x, y) => vec![(x, y)],
			Self::Part(x, y) => vec![(x.into(), y.into())],
			Self::Probe([ax, ay, bx, by], which) => {
				let (a, b) = ((ax.into(), ay.into()), (bx.into(), by.into()));
				match which {
					Which::A => vec![a],
					Which::B => vec![b],
					Which::Both => vec![a, b],
				}
			}
		}
	}
}

impl Drag {
	pub(crate) fn key(&self) -> &str {
		&self.key
	}

	pub(crate) fn node(&self) -> Node {
		self.node
	}

	/// Whether the items are away from where they started.
	pub(crate) fn moved(&self) -> bool {
		self.by != (0., 0.)
	}

	/// Moves the dragged items by `(dx, dy)` canvas pixels from where they started, whole pixels, limited so none
	/// leaves the canvas.
	pub(crate) fn apply(&mut self, project: &mut Project, (dx, dy): (f32, f32)) {
		let (w, h) = project.canvas_size();
		let (w, h) = (f64::from(w) - 1., f64::from(h) - 1.);
		let points: Vec<(f64, f64)> = self.starts.iter().flat_map(|(_, s)| s.points()).collect();
		let range = |of: fn(&(f64, f64)) -> f64, max: f64| {
			let lo = points.iter().map(|p| -of(p)).fold(f64::NEG_INFINITY, f64::max);
			let hi = points.iter().map(|p| max - of(p)).fold(f64::INFINITY, f64::min);
			(lo, hi.max(lo))
		};
		let ((xl, xh), (yl, yh)) = (range(|p| p.0, w), range(|p| p.1, h));
		let d = (f64::from(dx).round().clamp(xl, xh), f64::from(dy).round().clamp(yl, yh));
		self.by = d;
		for &(item, s) in &self.starts {
			let Some(node) = node_of(project, item) else { continue };
			match (node, s) {
				(Node::Source(i), Start::Source(x, y)) => {
					let src = &mut project.sources[i];
					(src.x, src.y) = (x + d.0, y + d.1);
				}
				(Node::Part(i), Start::Part(x, y)) => {
					let base = &mut project.parts[i].base;
					(base.x, base.y) = (x + d.0 as i32, y + d.1 as i32);
				}
				(Node::Probe(i), Start::Probe([ax, ay, bx, by], which)) => {
					let p = &mut project.probes[i];
					let (ex, ey) = (d.0 as i32, d.1 as i32);
					if which != Which::B {
						(p.ax, p.ay) = (ax + ex, ay + ey);
					}
					if which != Which::A {
						(p.bx, p.by) = (bx + ex, by + ey);
					}
				}
				_ => {}
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use waveduderust::model::Project;

	#[test]
	fn a_group_drag_stops_at_the_canvas_edge_for_all() {
		let mut p = Project::default();
		let (a, b) = (p.add_source_at(10, 10), p.add_source_at(50, 10));
		let starts = [a, b].map(|id| {
			let s = p.sources.iter().find(|s| s.id() == id).unwrap();
			(ItemRef::Source(id), Start::Source(s.x, s.y))
		});
		let mut drag = Drag { key: String::new(), node: Node::Source(0), starts: starts.to_vec(), by: (0., 0.) };
		drag.apply(&mut p, (-30.4, 2.6));
		let xs: Vec<(f64, f64)> = p.sources.iter().map(|s| (s.x, s.y)).collect();
		assert_eq!(xs, [(0., 13.), (40., 13.)]);
		assert!(drag.moved());
	}

	#[test]
	fn a_point_probe_in_a_group_is_bounded_by_its_pixel_alone() {
		let mut p = Project::default();
		p.add_probe();
		p.probes[0].kind = ProbeKind::Point;
		(p.probes[0].ax, p.probes[0].ay, p.probes[0].bx, p.probes[0].by) = (50, 50, 0, 0);
		let starts = start(&p, Node::Probe(0), Which::Both).into_iter().collect();
		let mut drag = Drag { key: String::new(), node: Node::Probe(0), starts, by: (0., 0.) };
		drag.apply(&mut p, (-20., -20.));
		assert_eq!((p.probes[0].ax, p.probes[0].ay), (30, 30));
	}

	#[test]
	fn a_probe_end_drag_moves_that_end_alone() {
		let mut p = Project::default();
		p.add_probe();
		let pr = &p.probes[0];
		let start = Start::Probe([pr.ax, pr.ay, pr.bx, pr.by], Which::B);
		let (a, b) = ((pr.ax, pr.ay), (pr.bx + 5, pr.by - 3));
		let mut drag = Drag {
			key: String::new(),
			node: Node::Probe(0),
			starts: vec![(ItemRef::Probe(pr.id()), start)],
			by: (0., 0.),
		};
		drag.apply(&mut p, (5., -3.));
		assert_eq!(((p.probes[0].ax, p.probes[0].ay), (p.probes[0].bx, p.probes[0].by)), (a, b));
	}
}
