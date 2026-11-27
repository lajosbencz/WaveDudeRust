//! The pictures' overlays (docs/ui/40, Handles and overlays): the core's overlay model shown as Slint rows, and the
//! selection and drags its handles make.

use std::fmt::Write as _;
use std::rc::Rc;

use slint::{ModelRc, VecModel};
use waveduderust::model::edited::Edited;
use waveduderust::model::{BoxSize, ItemRef, Layer, Part, PartKind, ProbeKind, Project, View, ranges};
use waveduderust::unstable::view::overlay::{self, HandleKind, OverlayModel};
use waveduderust::unstable::view::{graph, handles};

use crate::document::MergeKey;
use crate::graphs::Placed;
use crate::models;
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
	/// What has the keyboard focus, and on which picture.
	focus: Option<(ui::Layer, ItemRef, Stop)>,
	/// Counts focus changes: a run of key moves is one undo step.
	run: u64,
	/// What a run of keys on a lens surface holds still, with the part as the last key left it: any other edit
	/// (undo, the form) takes them anew.
	anchors: std::cell::RefCell<Option<(Part, handles::Anchors)>>,
	/// The view's handles and labels switches the overlays were built with.
	shows: (bool, bool),
}

/// What a run of arrow keys on a picture moves: the focus run, the focused stop and the group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Nudge {
	run: u64,
	item: ItemRef,
	stop: Stop,
	group: Vec<Node>,
}

/// What Tab stops on in a picture: the handles, then each graph box and its corner grip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
	Handle(HandleKind),
	Box,
	Grip,
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

/// What a drag edits.
enum Target {
	/// Whole items (a source, a part's centre, a probe end, or a group of them) moved from where they started.
	Items(Vec<(ItemRef, Start)>),
	/// A source's or a part's rotation lever.
	Lever(ItemRef),
	/// A part's size grip, an outline point or a fibre node or Bezier handle.
	Shape(ItemRef, HandleKind),
	/// A lens surface's vertex, its rim and the other vertex as the press found them (canvas pixels).
	Surface { item: ItemRef, surface: usize, anchors: handles::Anchors },
	/// A probe's graph box from its corner; whether it sits on A (attached), screen pixels per canvas pixel.
	GraphBox { item: ItemRef, start: (i32, i32), attached: bool, scale: f64 },
	/// A probe's A alone, screen pixels per canvas pixel: it picks up its detached graph box when it comes near.
	ProbeA { start: (ItemRef, Start), scale: f64 },
	/// A graph box's corner grip from the box's size.
	GraphSize { item: ItemRef, start: (u32, u32), scale: f64 },
}

/// Screen pixels within which a graph box's corner lands on its probe's A, and beyond which it leaves it.
const BOX_SNAP: f64 = 14.;
const BOX_RELEASE: f64 = 28.;
/// Screen pixels within which a resized box takes a fixed size.
const SIZE_SNAP: f64 = 10.;
/// A graph box's resize grip: the side of its bottom-right cell, logical pixels.
const BOX_GRIP: f32 = 12.;

/// A handle drag under way, one undo step.
pub(crate) struct Drag {
	/// The press count, 0 for a drag no undo step merges.
	gesture: u64,
	node: Node,
	target: Target,
	/// Where the drag starts, canvas pixels: the press, or the shape handle's own point, so the first move does not
	/// jump it to the pointer.
	from: (f64, f64),
	/// The whole-pixel move applied last.
	by: (f64, f64),
}

pub(crate) fn model_layer(layer: ui::Layer) -> Layer {
	match layer {
		ui::Layer::Field => Layer::Field,
		ui::Layer::Intensity => Layer::Intensity,
	}
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
		HandleKind::Point(_) | HandleKind::Node(_) | HandleKind::Surface(_) => ui::HandleGlyph::Point,
		HandleKind::InHandle(_) | HandleKind::OutHandle(_) => ui::HandleGlyph::Bezier,
		HandleKind::ProbeA | HandleKind::ProbeB => ui::HandleGlyph::Probe,
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

	/// Logical pixels per canvas pixel of `layer`'s picture.
	pub(crate) fn scale(&self, layer: ui::Layer) -> f32 {
		self.shown[index(layer)].scale()
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
		self.shows = (view.handles_in_preview, view.labels_in_preview);
		let selected: Vec<ItemRef> = selection.group().into_iter().filter_map(|n| item_of(project, n)).collect();
		let current = item_of(project, selection.current());
		let (w, h) = project.canvas_size();
		canvas.set_width(models::count_to_ui(w));
		canvas.set_height(models::count_to_ui(h));
		let layers = [(Layer::Field, ui::Layer::Field), (Layer::Intensity, ui::Layer::Intensity)];
		for ((layer, ui_layer), shown) in layers.into_iter().zip(&mut self.shown) {
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
			let focus = self.focus;
			let focused = |h: &overlay::Handle| focus == Some((ui_layer, h.item, Stop::Handle(h.kind)));
			if focus.is_some_and(|f| f.0 == ui_layer && matches!(f.2, Stop::Handle(_)))
				&& !m.handles.iter().any(focused)
			{
				self.focus = None;
			}
			let handles = m.handles.iter().map(|h| ui::HandleData {
				x: h.at.0,
				y: h.at.1,
				glyph: glyph(h.kind),
				selected: h.selected,
				focused: focused(h),
			});
			models::update(&shown.handles, handles);
			let labels = m.labels.iter().map(|l| ui::LabelData {
				text: l.text.as_str().into(),
				x: l.at.0,
				y: l.at.1,
				below: l.below,
				selected: l.selected,
			});
			models::update(&shown.labels, labels);
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

	/// A press on a picture. On a graph box (they lie over the handles): selects its probe and drags the box, or
	/// from its corner grip sizes it. On a handle (the topmost within reach): selects as a click does (Ctrl toggles,
	/// Shift adds, neither drags) and starts a drag of what the handle moves. Elsewhere: a marquee.
	pub(crate) fn press(
		&mut self,
		project: &Project,
		selection: &mut Selection,
		Press { layer, at, control, shift }: Press,
		gesture: u64,
		boxes: &[Placed],
	) -> Option<Gesture> {
		let shown = &self.shown[index(layer)];
		let scale = shown.scale();
		// the boxes lie over every handle
		if let Some(b) = boxes.iter().rev().find(|b| b.contains(at)) {
			let node = Node::Probe(b.probe);
			let probe = project.probes.get(b.probe)?;
			let item = ItemRef::Probe(probe.id());
			let grip = BOX_GRIP / scale;
			let corner = at.0 >= b.at.0 + b.size.0 - grip && at.1 >= b.at.1 + b.size.1 - grip;
			if control {
				selection.toggle(node);
				self.set_focus(None);
				return None;
			}
			selection.set(node);
			self.set_focus(Some((layer, item, if corner { Stop::Grip } else { Stop::Box })));
			let scale = f64::from(scale);
			// from the box as drawn: placed in the canvas, shrunk to fit it
			let start = (b.at.0.round() as i32, b.at.1.round() as i32);
			let target = if corner {
				Target::GraphSize { item, start: (b.size.0.round() as u32, b.size.1.round() as u32), scale }
			} else {
				Target::GraphBox { item, start, attached: probe.box_attached(), scale }
			};
			let from = (f64::from(at.0), f64::from(at.1));
			return Some(Gesture::Handle(Drag { gesture, node, target, from, by: (0., 0.) }));
		}
		let Some(handle) = hit(&shown.model, at, HIT_REACH / shown.scale()).cloned() else {
			self.set_focus(None);
			return Some(Gesture::Marquee(Marquee { layer, from: at, to: at, control, shift }));
		};
		let node = node_of(project, handle.item)?;
		let focus = (layer, handle.item, Stop::Handle(handle.kind));
		let drag = |target: Target| Drag {
			gesture,
			node,
			from: match target {
				Target::Shape(..) | Target::Surface { .. } => (f64::from(handle.at.0), f64::from(handle.at.1)),
				_ => (f64::from(at.0), f64::from(at.1)),
			},
			target,
			by: (0., 0.),
		};
		if control {
			selection.toggle(node);
			self.set_focus(selection.contains(node).then_some(focus));
			return None;
		}
		self.set_focus(Some(focus));
		// a lever or a shape handle belongs to the current item alone: Shift on it snaps a lever, it adds nothing
		if shift && handle.kind.moves_item() {
			if !selection.contains(node) {
				selection.toggle(node);
			}
			return None;
		}
		let group = selection.group_of(node);
		let target = if group.len() > 1 && handle.kind.moves_item() {
			selection.pick(project, node, ui::Pick::Focus);
			Target::Items(group.iter().filter_map(|&n| start(project, n, Which::Both)).collect())
		} else {
			selection.set(node);
			let which = match handle.kind {
				HandleKind::ProbeA => Which::A,
				HandleKind::ProbeB => Which::B,
				HandleKind::Source | HandleKind::PartCentre => Which::Both,
				HandleKind::SourceLever | HandleKind::PartLever => {
					return Some(Gesture::Handle(drag(Target::Lever(handle.item))));
				}
				HandleKind::Surface(surface) => {
					let Node::Part(i) = node else { return None };
					let anchors = handles::surface_anchors(&project.parts[i], surface)?;
					return Some(Gesture::Handle(drag(Target::Surface { item: handle.item, surface, anchors })));
				}
				kind => return Some(Gesture::Handle(drag(Target::Shape(handle.item, kind)))),
			};
			let start = start(project, node, which)?;
			if which == Which::A {
				Target::ProbeA { start, scale: f64::from(scale) }
			} else {
				Target::Items(vec![start])
			}
		};
		Some(Gesture::Handle(drag(target)))
	}
}

impl Overlays {
	fn set_focus(&mut self, focus: Option<(ui::Layer, ItemRef, Stop)>) {
		self.focus = focus;
		self.run += 1;
	}

	/// The handle focused on `layer`, as shown.
	fn focused(&self, layer: ui::Layer) -> Option<&overlay::Handle> {
		let (at, item, Stop::Handle(kind)) = self.focus? else { return None };
		let handles = &self.shown[index(layer)].model.handles;
		handles.iter().find(|h| at == layer && h.item == item && h.kind == kind)
	}

	/// The Tab stops of `layer`'s picture with `boxes`: its handles, then each box and its grip.
	fn stops(&self, project: &Project, layer: ui::Layer, boxes: &[Placed]) -> Vec<(ItemRef, Stop)> {
		let handles = self.shown[index(layer)].model.handles.iter().map(|h| (h.item, Stop::Handle(h.kind)));
		// in probe order: the draw order puts the selected boxes last, and a stop selects its probe
		let mut probes: Vec<usize> = boxes.iter().map(|b| b.probe).collect();
		probes.sort_unstable();
		let boxes = probes
			.into_iter()
			.filter_map(|i| project.probes.get(i).map(|p| ItemRef::Probe(p.id())))
			.flat_map(|item| [(item, Stop::Box), (item, Stop::Grip)]);
		handles.chain(boxes).collect()
	}

	/// The focused stop of `layer` among `stops`.
	fn focused_stop(&self, layer: ui::Layer, stops: &[(ItemRef, Stop)]) -> Option<usize> {
		let (_, item, stop) = self.focus.filter(|f| f.0 == layer)?;
		stops.iter().position(|&s| s == (item, stop))
	}

	/// Takes the keyboard focus into `layer`'s picture: the stop it had, else the first.
	pub(crate) fn enter(
		&mut self,
		project: &Project,
		selection: &mut Selection,
		layer: ui::Layer,
		boxes: &[Placed],
	) -> bool {
		let stops = self.stops(project, layer, boxes);
		self.focused_stop(layer, &stops).is_some() || self.tab(project, selection, layer, false, boxes)
	}

	/// Moves the keyboard focus to the next stop of `layer`, or the previous one with `back`, and selects its item
	/// (kept in its group); false past the ends, where the focus leaves the picture. Back onto another item's handle
	/// lands on its last handle as shown once it is selected (its lever and grips).
	pub(crate) fn tab(
		&mut self,
		project: &Project,
		selection: &mut Selection,
		layer: ui::Layer,
		back: bool,
		boxes: &[Placed],
	) -> bool {
		let scale = self.shown[index(layer)].scale();
		let stops = self.stops(project, layer, boxes);
		let at = self.focused_stop(layer, &stops);
		let next = match (at, back) {
			(None, false) => Some(0),
			(None, true) => stops.len().checked_sub(1),
			(Some(i), false) => Some(i + 1),
			(Some(i), true) => i.checked_sub(1),
		};
		let Some(&(mut item, mut stop)) = next.and_then(|i| stops.get(i)) else {
			self.set_focus(None);
			return false;
		};
		let Some(node) = node_of(project, item) else { return false };
		let moved_off = self.focus.is_none_or(|f| f.1 != item);
		if selection.contains(node) {
			selection.pick(project, node, ui::Pick::Focus);
		} else {
			selection.set(node);
		}
		if back && moved_off && matches!(stop, Stop::Handle(_)) {
			let selected: Vec<ItemRef> = selection.group().into_iter().filter_map(|n| item_of(project, n)).collect();
			let m =
				overlay::build(project, &selected, Some(item), model_layer(layer), scale, self.shows.0, self.shows.1);
			if let Some(last) = m.handles.iter().rev().find(|h| h.item == item) {
				(item, stop) = (last.item, Stop::Handle(last.kind));
			}
		}
		self.set_focus(Some((layer, item, stop)));
		true
	}

	/// The undo key of a run of key moves on `layer`'s focused stop; a new focus or group starts another.
	pub(crate) fn nudge_key(&self, layer: ui::Layer, selection: &Selection) -> Option<MergeKey> {
		let (_, item, stop) = self.focus.filter(|f| f.0 == layer)?;
		Some(MergeKey::Nudge(Nudge { run: self.run, item, stop, group: selection.group() }))
	}

	/// The tooltip of what lies under canvas pixel `at` of `layer`'s picture with `boxes`: a box (its corner grip),
	/// else a handle.
	pub(crate) fn tip(&self, project: &Project, layer: ui::Layer, at: (f32, f32), boxes: &[Placed]) -> ui::HoverTip {
		let shown = &self.shown[index(layer)];
		let scale = shown.scale();
		let named = |kind, node: Node, index: usize| ui::HoverTip {
			kind,
			name: crate::tree::name(project, node).into(),
			index: i32::try_from(index).unwrap_or(0),
		};
		if let Some(b) = boxes.iter().rev().find(|b| b.contains(at)) {
			let grip = BOX_GRIP / scale;
			let corner = at.0 >= b.at.0 + b.size.0 - grip && at.1 >= b.at.1 + b.size.1 - grip;
			let kind = if corner { ui::TipKind::GraphCorner } else { ui::TipKind::Graph };
			return named(kind, Node::Probe(b.probe), 0);
		}
		let Some(h) = hit(&shown.model, at, HIT_REACH / scale) else { return ui::HoverTip::default() };
		let Some(node) = node_of(project, h.item) else { return ui::HoverTip::default() };
		let (kind, i) = match h.kind {
			HandleKind::SourceLever | HandleKind::PartLever => (ui::TipKind::Lever, 0),
			HandleKind::Grip => (ui::TipKind::Grip, 0),
			HandleKind::Point(i) => (ui::TipKind::Point, i),
			HandleKind::Surface(s) => (ui::TipKind::Surface, s),
			HandleKind::Node(i) => (ui::TipKind::Node, i),
			HandleKind::InHandle(i) => (ui::TipKind::InHandle, i),
			HandleKind::OutHandle(i) => (ui::TipKind::OutHandle, i),
			HandleKind::ProbeB => {
				let name = format!("{} B", crate::tree::name(project, node));
				return ui::HoverTip { kind: ui::TipKind::Marker, name: name.into(), index: 0 };
			}
			_ => (ui::TipKind::Marker, 0),
		};
		named(kind, node, i)
	}

	/// Drops the keyboard focus of `layer`'s picture, which is gone.
	pub(crate) fn forget(&mut self, layer: ui::Layer) {
		if self.focus.is_some_and(|f| f.0 == layer) {
			self.set_focus(None);
		}
	}

	/// Drops a graph box focus whose box `layer`'s picture no longer draws among `boxes`.
	pub(crate) fn drop_lost_box(&mut self, layer: ui::Layer, project: &Project, boxes: &[Placed]) {
		let on_box = self.focus.is_some_and(|f| f.0 == layer && matches!(f.2, Stop::Box | Stop::Grip));
		if on_box && self.focused_box(layer, project, boxes).is_none() {
			self.focus = None;
		}
	}

	/// The graph box focused on `layer` and whether its grip is, as drawn among `boxes`.
	pub(crate) fn focused_box(&self, layer: ui::Layer, project: &Project, boxes: &[Placed]) -> Option<(Placed, bool)> {
		let (at, ItemRef::Probe(id), stop @ (Stop::Box | Stop::Grip)) = self.focus? else { return None };
		let b = boxes.iter().find(|b| at == layer && project.probes.get(b.probe).is_some_and(|p| p.id() == id))?;
		Some((*b, stop == Stop::Grip))
	}

	/// Moves the focused graph box by `d` canvas pixels, or sizes it from its focused grip, kept in the canvas.
	pub(crate) fn nudge_box(&self, project: &mut Project, layer: ui::Layer, d: (f64, f64), boxes: &[Placed]) {
		let Some((b, grip)) = self.focused_box(layer, project, boxes) else { return };
		let (cw, ch) = project.canvas_size();
		let p = &mut project.probes[b.probe];
		if grip {
			// never below what the picture still draws, so keys can grow it back
			let scale = f64::from(self.shown[index(layer)].scale());
			let least = |min: f32| (f64::from(min) / scale).ceil();
			let size = |now: f32, by: f64, range: ranges::Range, min: f32, room: u32| {
				let now = f64::from(now).round();
				range.clamp_or(now + by, now).min(f64::from(room)).max(least(min).min(now)) as u32
			};
			let w = size(b.size.0, d.0, ranges::PROBE_BOX_WIDTH, graph::MIN_VISIBLE.0, cw);
			let h = size(b.size.1, d.1, ranges::PROBE_BOX_HEIGHT, graph::MIN_VISIBLE.1, ch);
			if (w, h) != (b.size.0.round() as u32, b.size.1.round() as u32) {
				(p.box_size, p.box_width, p.box_height) = (BoxSize::Custom, w, h);
			}
		} else {
			let x = (f64::from(b.at.0).round() + d.0).clamp(0., (f64::from(cw) - f64::from(b.size.0)).max(0.));
			let y = (f64::from(b.at.1).round() + d.1).clamp(0., (f64::from(ch) - f64::from(b.size.1)).max(0.));
			(p.box_x, p.box_y) = (x as i32, y as i32);
		}
	}

	/// Moves what the focused handle moves by `d` canvas pixels (its item's group with it), or turns its lever by
	/// `turn` degrees.
	pub(crate) fn nudge(
		&self,
		project: &mut Project,
		selection: &Selection,
		layer: ui::Layer,
		d: (f64, f64),
		turn: f64,
	) {
		let Some(h) = self.focused(layer) else { return };
		let (item, kind) = (h.item, h.kind);
		let Some(node) = node_of(project, item) else { return };
		match kind {
			HandleKind::SourceLever | HandleKind::PartLever => match node {
				Node::Source(i) => {
					if let Some(array) = &mut project.sources[i].array {
						array.angle = handles::turn_lever(array.angle, turn);
					}
				}
				Node::Part(i) => {
					edit_part(&mut project.parts[i], |p| p.base.angle = handles::turn_lever(p.base.angle, turn));
				}
				_ => {}
			},
			kind if kind.moves_item() => {
				let group = selection.group_of(node);
				let starts: Vec<_> = if group.len() > 1 {
					group.iter().filter_map(|&n| start(project, n, Which::Both)).collect()
				} else {
					let which = match kind {
						HandleKind::ProbeA => Which::A,
						HandleKind::ProbeB => Which::B,
						_ => Which::Both,
					};
					start(project, node, which).into_iter().collect()
				};
				move_items(project, &starts, d);
			}
			HandleKind::Surface(surface) => {
				let Node::Part(i) = node else { return };
				let part = &project.parts[i];
				// held from the first key while the lens is as the keys left it, as a drag holds them
				let held = self.anchors.borrow().as_ref().filter(|(p, _)| p == part).map(|(_, a)| *a);
				let Some(anchors) = held.or_else(|| handles::surface_anchors(part, surface)) else { return };
				let Some(vertex) = handles::surface_vertex(part, surface, anchors) else { return };
				let at = (vertex.0 + d.0, vertex.1 + d.1);
				// a key across the axis moves nothing: it would only round the radius anew
				if handles::along_axis(part, vertex, at).abs() < 1e-9 {
					return;
				}
				edit_part(&mut project.parts[i], |p| handles::drag_surface(p, surface, at, anchors));
				*self.anchors.borrow_mut() = Some((project.parts[i].clone(), anchors));
			}
			HandleKind::Grip => {
				let Node::Part(i) = node else { return };
				let at = (f64::from(h.at.0), f64::from(h.at.1));
				edit_part(&mut project.parts[i], |p| {
					let grip = handles::to_part_frame(p, at);
					let d = handles::to_part_frame(p, (at.0 + d.0, at.1 + d.1)) - grip;
					handles::nudge_grip(&mut p.kind, grip, d);
				});
			}
			kind => {
				let from = (f64::from(h.at.0), f64::from(h.at.1));
				let mut drag = Drag { gesture: 0, node, target: Target::Shape(item, kind), from, by: (0., 0.) };
				drag.apply(project, (d.0 as f32, d.1 as f32), false);
			}
		}
	}
}

/// A marquee drag on a picture: where it started, the corner now, and the keys held at the press.
pub(crate) struct Marquee {
	layer: ui::Layer,
	from: (f32, f32),
	to: (f32, f32),
	control: bool,
	shift: bool,
}

impl Marquee {
	pub(crate) fn stretch(&mut self, (dx, dy): (f32, f32)) {
		self.to = (self.from.0 + dx, self.from.1 + dy);
	}

	/// The marquee as the picture draws it.
	pub(crate) fn rect(&self) -> ui::Rect {
		let (x, y) = (self.from.0.min(self.to.0), self.from.1.min(self.to.1));
		ui::Rect { x, y, width: (self.from.0 - self.to.0).abs(), height: (self.from.1 - self.to.1).abs() }
	}

	pub(crate) fn layer(&self) -> ui::Layer {
		self.layer
	}

	/// Selects the items whose whole-item handles lie inside: Shift adds them, Ctrl toggles them, else they replace
	/// the selection, the last one current. A marquee that never opened is a click: it deselects unless a key is held.
	pub(crate) fn select(&self, overlays: &Overlays, project: &Project, selection: &mut Selection) {
		let r = self.rect();
		if r.width < 1. && r.height < 1. {
			if !self.control && !self.shift {
				selection.set(Node::Project);
			}
			return;
		}
		let mut hits = Vec::new();
		for h in overlays.shown[index(self.layer)].model.handles.iter().filter(|h| h.kind.moves_item()) {
			let inside = (r.x..=r.x + r.width).contains(&h.at.0) && (r.y..=r.y + r.height).contains(&h.at.1);
			if let Some(node) = node_of(project, h.item).filter(|n| inside && !hits.contains(n)) {
				hits.push(node);
			}
		}
		if self.shift {
			for &n in &hits {
				if !selection.contains(n) {
					selection.toggle(n);
				}
			}
			if let Some(&last) = hits.last() {
				selection.pick(project, last, ui::Pick::Focus);
			}
		} else if self.control {
			hits.into_iter().for_each(|n| selection.toggle(n));
		} else if hits.is_empty() {
			selection.set(Node::Project);
		} else {
			selection.set_group(hits);
		}
	}
}

/// A left press on `layer`'s picture at canvas pixel `at`, the keys held.
pub(crate) struct Press {
	pub(crate) layer: ui::Layer,
	pub(crate) at: (f32, f32),
	pub(crate) control: bool,
	pub(crate) shift: bool,
}

/// What a press on a picture starts.
pub(crate) enum Gesture {
	Handle(Drag),
	Marquee(Marquee),
}

/// Logical pixels from a handle's point that still press it.
const HIT_REACH: f32 = 10.;

/// The topmost handle of `model` within `reach` canvas pixels of `at`.
/// The handle nearest to `at` within `reach`; of equally near ones the topmost.
fn hit(model: &OverlayModel, at: (f32, f32), reach: f32) -> Option<&overlay::Handle> {
	let distance = |h: &overlay::Handle| (h.at.0 - at.0).hypot(h.at.1 - at.1);
	model.handles.iter().rev().filter(|h| distance(h) <= reach).min_by(|a, b| distance(a).total_cmp(&distance(b)))
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
	pub(crate) fn key(&self) -> MergeKey {
		MergeKey::Drag(self.gesture)
	}

	pub(crate) fn node(&self) -> Node {
		self.node
	}

	/// Whether the pointer is a whole pixel or more away from the press (items: the move after the canvas limit).
	pub(crate) fn moved(&self) -> bool {
		self.by != (0., 0.)
	}

	/// Takes the drag to `(dx, dy)` canvas pixels from the press; `snap` turns a lever in 15-degree steps. The
	/// edited objects then make their fields agree, as a form's edit does.
	pub(crate) fn apply(&mut self, project: &mut Project, (dx, dy): (f32, f32), snap: bool) {
		let d = (f64::from(dx).round(), f64::from(dy).round());
		let at = (self.from.0 + f64::from(dx), self.from.1 + f64::from(dy));
		match &mut self.target {
			Target::Items(starts) => self.by = move_items(project, starts, d),
			Target::ProbeA { start, scale } => {
				self.by = move_items(project, std::slice::from_ref(start), d);
				if let Some(Node::Probe(i)) = node_of(project, start.0) {
					let p = &mut project.probes[i];
					let off = f64::from(p.box_x - p.ax).hypot(f64::from(p.box_y - p.ay)) * *scale;
					if !p.box_attached() && off <= BOX_SNAP {
						(p.box_x, p.box_y) = (p.ax, p.ay);
					}
				}
			}
			&mut Target::Lever(item) => {
				self.by = d;
				match node_of(project, item) {
					Some(Node::Source(i)) => {
						let s = &mut project.sources[i];
						if let Some(array) = &mut s.array {
							array.angle = handles::lever_angle((s.x + 0.5, s.y + 0.5), at, snap);
						}
					}
					Some(Node::Part(i)) => edit_part(&mut project.parts[i], |p| {
						p.base.angle = handles::lever_angle((p.base.x.into(), p.base.y.into()), at, snap);
					}),
					_ => {}
				}
			}
			&mut Target::GraphBox { item, start, ref mut attached, scale } => {
				self.by = d;
				let Some(Node::Probe(i)) = node_of(project, item) else { return };
				let canvas = project.canvas_size();
				let p = &mut project.probes[i];
				let (bw, bh) = graph::box_size(p, canvas);
				// the corner wanted, before the canvas holds it: A off the reachable area still attaches
				let (x, y) = (f64::from(start.0) + d.0, f64::from(start.1) + d.1);
				let off = (x - f64::from(p.ax)).hypot(y - f64::from(p.ay)) * scale;
				*attached = if *attached { off <= BOX_RELEASE } else { off < BOX_SNAP };
				let x = x.clamp(0., (f64::from(canvas.0) - f64::from(bw)).max(0.));
				let y = y.clamp(0., (f64::from(canvas.1) - f64::from(bh)).max(0.));
				(p.box_x, p.box_y) = if *attached { (p.ax, p.ay) } else { (x as i32, y as i32) };
			}
			&mut Target::GraphSize { item, start, scale } => {
				self.by = d;
				let Some(Node::Probe(i)) = node_of(project, item) else { return };
				let (cw, ch) = project.canvas_size();
				let p = &mut project.probes[i];
				let w =
					ranges::PROBE_BOX_WIDTH.clamp_or(f64::from(start.0) + d.0, f64::from(start.0)).min(f64::from(cw));
				let h =
					ranges::PROBE_BOX_HEIGHT.clamp_or(f64::from(start.1) + d.1, f64::from(start.1)).min(f64::from(ch));
				let near = |(pw, ph): (u32, u32)| {
					(f64::from(pw) - w).abs() * scale < SIZE_SNAP && (f64::from(ph) - h).abs() * scale < SIZE_SNAP
				};
				const FIXED: [BoxSize; 5] =
					[BoxSize::Tiny, BoxSize::Small, BoxSize::Medium, BoxSize::Large, BoxSize::ExtraLarge];
				match FIXED.into_iter().find(|s| s.nominal().is_some_and(near)) {
					Some(size) => p.box_size = size,
					None => (p.box_size, p.box_width, p.box_height) = (BoxSize::Custom, w as u32, h as u32),
				}
			}
			&mut Target::Surface { item, surface, anchors } => {
				self.by = d;
				let Some(Node::Part(i)) = node_of(project, item) else { return };
				edit_part(&mut project.parts[i], |p| handles::drag_surface(p, surface, at, anchors));
			}
			&mut Target::Shape(item, kind) => {
				self.by = d;
				let Some(Node::Part(i)) = node_of(project, item) else { return };
				edit_part(&mut project.parts[i], |p| {
					let q = handles::to_part_frame(p, at);
					match (&mut p.kind, kind) {
						(_, HandleKind::Grip) => handles::drag_grip(&mut p.kind, q),
						(PartKind::Outline(o), HandleKind::Point(k)) => handles::drag_point(&mut o.points, k, q),
						(PartKind::Fiber(f), kind) => handles::drag_fibre(&mut f.nodes, kind, q),
						_ => {}
					}
				});
			}
		}
	}
}

/// Moves `nodes` so the middle of their points lies at canvas pixel `at`, none leaving the canvas.
pub(crate) fn place(project: &mut Project, nodes: &[Node], at: (f64, f64)) {
	let starts: Vec<(ItemRef, Start)> = nodes.iter().filter_map(|&n| start(project, n, Which::Both)).collect();
	let points: Vec<(f64, f64)> = starts.iter().flat_map(|(_, s)| s.points()).collect();
	if points.is_empty() {
		return;
	}
	let n = points.len() as f64;
	let middle = points.iter().fold((0., 0.), |m, p| (m.0 + p.0 / n, m.1 + p.1 / n));
	move_items(project, &starts, ((at.0 - middle.0).round(), (at.1 - middle.1).round()));
}

/// Edits `part` by `edit`, then makes its fields agree.
fn edit_part(part: &mut Part, edit: impl FnOnce(&mut Part)) {
	let before = part.clone();
	edit(part);
	part.edited(&before);
}

/// Moves the items of `starts` by `d` canvas pixels from where they started, limited so none leaves the canvas;
/// the move made.
fn move_items(project: &mut Project, starts: &[(ItemRef, Start)], d: (f64, f64)) -> (f64, f64) {
	let (w, h) = project.canvas_size();
	let (w, h) = (f64::from(w) - 1., f64::from(h) - 1.);
	let points: Vec<(f64, f64)> = starts.iter().flat_map(|(_, s)| s.points()).collect();
	let range = |of: fn(&(f64, f64)) -> f64, max: f64| {
		let lo = points.iter().map(|p| -of(p)).fold(f64::NEG_INFINITY, f64::max);
		let hi = points.iter().map(|p| max - of(p)).fold(f64::INFINITY, f64::min);
		(lo, hi.max(lo))
	};
	let ((xl, xh), (yl, yh)) = (range(|p| p.0, w), range(|p| p.1, h));
	let d = (d.0.clamp(xl, xh), d.1.clamp(yl, yh));
	for &(item, s) in starts {
		let Some(node) = node_of(project, item) else { continue };
		match (node, s) {
			(Node::Source(i), Start::Source(x, y)) => {
				let src = &mut project.sources[i];
				(src.x, src.y) = (x + d.0, y + d.1);
			}
			(Node::Part(i), Start::Part(x, y)) => {
				edit_part(&mut project.parts[i], |p| (p.base.x, p.base.y) = (x + d.0 as i32, y + d.1 as i32));
			}
			(Node::Probe(i), Start::Probe([ax, ay, bx, by], which)) => {
				let p = &mut project.probes[i];
				let before = p.clone();
				let (ex, ey) = (d.0 as i32, d.1 as i32);
				if which != Which::B {
					(p.ax, p.ay) = (ax + ex, ay + ey);
				}
				if which != Which::A {
					(p.bx, p.by) = (bx + ex, by + ey);
				}
				p.edited(&before);
			}
			_ => {}
		}
	}
	d
}

#[cfg(test)]
mod tests {
	use super::*;
	use waveduderust::model::Project;

	#[test]
	fn a_probe_a_dragged_near_its_detached_box_picks_it_up() {
		let mut project = Project::default();
		project.probes.push(waveduderust::model::Probe::default());
		let p = &mut project.probes[0];
		(p.ax, p.ay, p.bx, p.by, p.box_x, p.box_y) = (10, 10, 20, 10, 200, 200);
		let start = (ItemRef::Probe(p.id()), Start::Probe([10, 10, 20, 10], Which::A));
		let mut drag = Drag {
			gesture: 0,
			node: Node::Probe(0),
			target: Target::ProbeA { start, scale: 2. },
			from: (0., 0.),
			by: (0., 0.),
		};
		drag.apply(&mut project, (183., 183.), false);
		assert!(!project.probes[0].box_attached(), "10 canvas px is 20 screen px");
		drag.apply(&mut project, (186., 186.), false);
		assert!(project.probes[0].box_attached());
	}

	fn items(node: Node, starts: Vec<(ItemRef, Start)>) -> Drag {
		Drag { gesture: 0, node, target: Target::Items(starts), from: (0., 0.), by: (0., 0.) }
	}

	#[test]
	fn a_group_drag_stops_at_the_canvas_edge_for_all() {
		let mut p = Project::default();
		let (a, b) = (p.add_source_at(10, 10), p.add_source_at(50, 10));
		let starts = [a, b].map(|id| {
			let s = p.sources.iter().find(|s| s.id() == id).unwrap();
			(ItemRef::Source(id), Start::Source(s.x, s.y))
		});
		let mut drag = items(Node::Source(0), starts.to_vec());
		drag.apply(&mut p, (-30.4, 2.6), false);
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
		let mut drag = items(Node::Probe(0), starts);
		drag.apply(&mut p, (-20., -20.), false);
		assert_eq!((p.probes[0].ax, p.probes[0].ay), (30, 30));
	}

	#[test]
	fn a_probe_end_drag_moves_that_end_alone() {
		let mut p = Project::default();
		p.add_probe();
		let pr = &p.probes[0];
		let start = Start::Probe([pr.ax, pr.ay, pr.bx, pr.by], Which::B);
		let (a, b) = ((pr.ax, pr.ay), (pr.bx + 5, pr.by - 3));
		let mut drag = items(Node::Probe(0), vec![(ItemRef::Probe(pr.id()), start)]);
		drag.apply(&mut p, (5., -3.), false);
		assert_eq!(((p.probes[0].ax, p.probes[0].ay), (p.probes[0].bx, p.probes[0].by)), (a, b));
	}

	#[test]
	fn a_graph_box_lands_on_a_and_leaves_it_past_the_release() {
		let mut p = Project::default();
		p.add_probe();
		let pr = &mut p.probes[0];
		(pr.ax, pr.ay, pr.box_x, pr.box_y) = (100, 100, 150, 100);
		let target = Target::GraphBox { item: ItemRef::Probe(pr.id()), start: (150, 100), attached: false, scale: 1. };
		let mut drag = Drag { gesture: 0, node: Node::Probe(0), target, from: (0., 0.), by: (0., 0.) };
		let at = |p: &Project| (p.probes[0].box_x, p.probes[0].box_y);
		drag.apply(&mut p, (-30., 0.), false);
		assert_eq!(at(&p), (120, 100), "outside the snap radius");
		drag.apply(&mut p, (-45., 5.), false);
		assert_eq!(at(&p), (100, 100), "within it: attached");
		drag.apply(&mut p, (-30., 0.), false);
		assert_eq!(at(&p), (100, 100), "held until the release distance");
		drag.apply(&mut p, (-5., 0.), false);
		assert_eq!(at(&p), (145, 100), "past it: free");
	}
}
