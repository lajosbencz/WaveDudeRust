//! The handle model of a picture: which handles, dots, lines, outlines and labels it shows, their hit test, the
//! marquee, and the drag arithmetic of levers, grips and point handles (docs/ui/40-preview-pane.md, Handles and
//! overlays). Pure: built from the project and the selection, tested without a window.
//!
//! Coordinates: canvas pixels as continuous values; a pixel (x, y) spans [x, x + 1), so a source or probe at
//! pixel (x, y) sits at (x + 0.5, y + 0.5) and a part's centre at (x, y), the corner of its pixel. Angles in
//! degrees, counter-clockwise on screen. `scale` is logical pixels per canvas pixel.

use wavedude_core::engine::emitters::elements_of;
use wavedude_core::model::{
	DiffuserEdit, EllipseEdit, FacettedGratingEdit, Fiber, FiberNode, FresnelLensEdit, GrinLensEdit,
	InterferenceFilterEdit, Layer, LensEdit, MetaSurfaceEdit, Part, PartEdit, PartKind, PartKindEdit, Point,
	PolygonEdit, ProbeKind, Project, RectangleEdit,
};
use wavedude_core::scene::shapes::{self, Frame, Vec2};

use wavedude_app_state::selection::{ItemKind, ItemRef};

/// Screen radius in logical pixels within which a press hits a handle.
pub const HIT_RADIUS: f32 = 10.;
/// Screen distance of a lever beyond the item's reach, logical pixels.
pub const LEVER_GAP: f32 = 24.;
/// Screen distance a press must travel to become a drag, logical pixels.
pub const DRAG_THRESHOLD: f32 = 3.;

/// What a handle edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleKind {
	Source,
	SourceLever,
	PartCentre,
	PartLever,
	/// The size grip of a rectangle, ellipse, polygon, lens or GRIN lens.
	Grip,
	/// Point `i` of an outline.
	Point(usize),
	/// Node `i` of a fibre, and its Bezier handles.
	Node(usize),
	InHandle(usize),
	OutHandle(usize),
	ProbeA,
	ProbeB,
}

impl HandleKind {
	/// A handle that moves its whole item (and a group with it).
	pub fn moves_item(self) -> bool {
		matches!(self, Self::Source | Self::PartCentre | Self::ProbeA | Self::ProbeB)
	}

	pub fn is_lever(self) -> bool {
		matches!(self, Self::SourceLever | Self::PartLever)
	}
}

/// One handle over a picture.
#[derive(Clone, Debug, PartialEq)]
pub struct Handle {
	pub item: ItemRef,
	pub kind: HandleKind,
	/// Canvas position.
	pub at: (f32, f32),
	/// A lever's pivot or a Bezier handle's node: a line is drawn from it.
	pub from: Option<(f32, f32)>,
	pub selected: bool,
}

/// A name beside an item's main handle.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
	pub text: String,
	pub at: (f32, f32),
	/// Below the handle (composites and parts) rather than to its right.
	pub below: bool,
	pub selected: bool,
}

/// A line probe's segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbeLine {
	pub a: (f32, f32),
	pub b: (f32, f32),
	pub selected: bool,
}

/// A selected part's painted outline (closed) or a fibre's centre line (open).
#[derive(Clone, Debug, PartialEq)]
pub struct PartOutline {
	pub points: Vec<(f32, f32)>,
	pub closed: bool,
}

/// Everything a picture's overlay draws, in drawing order within each list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OverlayModel {
	pub lines: Vec<ProbeLine>,
	/// Emitter dots of composite sources; not focusable.
	pub dots: Vec<(f32, f32)>,
	pub outlines: Vec<PartOutline>,
	pub handles: Vec<Handle>,
	pub labels: Vec<Label>,
}

fn centre_of(x: f64, y: f64) -> (f32, f32) {
	((x + 0.5) as f32, (y + 0.5) as f32)
}

fn v(p: Vec2) -> (f32, f32) {
	(p.x as f32, p.y as f32)
}

/// Unit vector of `angle` degrees counter-clockwise on screen.
fn direction(angle: f64) -> (f64, f64) {
	let r = angle.to_radians();
	(r.cos(), -r.sin())
}

/// Items in drawing order: the unselected ones first, the selected ones last.
fn order(len: usize, selected: impl Fn(usize) -> bool) -> Vec<usize> {
	let (mut on, off): (Vec<usize>, Vec<usize>) = (0..len).partition(|&i| !selected(i));
	on.extend(off);
	on
}

/// The overlay of the picture of `layer`: sources and parts on both pictures, probes on their own layer's.
/// `handles_shown` and `labels_shown` are the Display settings; a selected item shows both either way.
pub fn build(
	project: &Project,
	selected: &[ItemRef],
	current: Option<ItemRef>,
	layer: Layer,
	scale: f32,
	handles_shown: bool,
	labels_shown: bool,
) -> OverlayModel {
	let mut m = OverlayModel::default();
	let is_selected = |i: ItemRef| selected.contains(&i);
	let gap = f64::from(LEVER_GAP / scale.max(1e-3));

	for i in order(project.sources.len(), |i| is_selected(ItemRef::new(ItemKind::Source, i))) {
		let s = &project.sources[i];
		let item = ItemRef::new(ItemKind::Source, i);
		let sel = is_selected(item);
		let at = centre_of(s.x, s.y);
		if handles_shown || sel {
			if let Some(array) = &s.array {
				m.dots.extend(elements_of(s, project.seed).iter().map(|e| centre_of(e.x, e.y)));
				if current == Some(item) {
					let half_row =
						f64::from(array.size_x.saturating_sub(1)) * f64::from(array.spacing_x.unsigned_abs()) / 2.;
					let (dx, dy) = direction(array.angle);
					let reach = half_row + gap;
					let tip = (at.0 + (dx * reach) as f32, at.1 + (dy * reach) as f32);
					m.handles.push(Handle {
						item,
						kind: HandleKind::SourceLever,
						at: tip,
						from: Some(at),
						selected: sel,
					});
				}
			}
			m.handles.push(Handle { item, kind: HandleKind::Source, at, from: None, selected: sel });
		}
		if labels_shown || sel {
			m.labels.push(Label { text: s.name.clone(), at, below: s.array.is_some(), selected: sel });
		}
	}

	for i in order(project.parts.len(), |i| is_selected(ItemRef::new(ItemKind::Part, i))) {
		let p = &project.parts[i];
		let item = ItemRef::new(ItemKind::Part, i);
		let sel = is_selected(item);
		let at = (p.base.x as f32, p.base.y as f32);
		if sel && let Some(outline) = part_outline(p, project.seed) {
			m.outlines.push(outline);
		}
		if handles_shown || sel {
			if current == Some(item) {
				part_handles(project, p, item, gap, &mut m.handles);
			}
			m.handles.push(Handle { item, kind: HandleKind::PartCentre, at, from: None, selected: sel });
		}
		if labels_shown || sel {
			m.labels.push(Label { text: p.base.name.clone(), at, below: true, selected: sel });
		}
	}

	for i in order(project.probes.len(), |i| is_selected(ItemRef::new(ItemKind::Probe, i))) {
		let p = &project.probes[i];
		if p.layer != layer {
			continue;
		}
		let item = ItemRef::new(ItemKind::Probe, i);
		let sel = is_selected(item);
		let a = centre_of(p.ax.into(), p.ay.into());
		if handles_shown || sel {
			if p.kind == ProbeKind::Line {
				let b = centre_of(p.bx.into(), p.by.into());
				m.lines.push(ProbeLine { a, b, selected: sel });
				m.handles.push(Handle { item, kind: HandleKind::ProbeB, at: b, from: None, selected: sel });
			}
			m.handles.push(Handle { item, kind: HandleKind::ProbeA, at: a, from: None, selected: sel });
		}
		if labels_shown || sel {
			m.labels.push(Label { text: p.name.clone(), at: a, below: false, selected: sel });
		}
	}
	m
}

/// The outline drawn for a selected part: the vertices of a polygon-type kind or a profiled plate, a fibre's
/// centre line.
fn part_outline(p: &Part, seed: i32) -> Option<PartOutline> {
	let frame = Frame::of(&p.base);
	if let Some(vertices) = shapes::vertices(&p.kind).or_else(|| shapes::outline_of(p, seed)) {
		return Some(PartOutline {
			points: vertices.into_iter().map(|q| v(frame.to_canvas(q))).collect(),
			closed: true,
		});
	}
	if let PartKind::Fiber(f) = &p.kind {
		let path = shapes::flatten(&f.nodes);
		return (!path.is_empty())
			.then(|| PartOutline { points: path.into_iter().map(|q| v(frame.to_canvas(q))).collect(), closed: false });
	}
	None
}

/// The grip's part-frame position of the kinds that have one.
pub fn grip_of(kind: &PartKind) -> Option<Vec2> {
	match kind {
		PartKind::Rectangle(r) => Some(Vec2::new(f64::from(r.length) / 2., f64::from(r.width) / 2.)),
		PartKind::Ellipse(e) => Some(Vec2::new(f64::from(e.length) / 2., f64::from(e.width) / 2.)),
		PartKind::Polygon(_) => shapes::vertices(kind).and_then(|v| v.first().copied()),
		PartKind::Lens(l) => Some(Vec2::new(0., f64::from(l.diameter) / 2.)),
		PartKind::FresnelLens(f) => Some(Vec2::new(0., f64::from(f.lens.diameter) / 2.)),
		PartKind::Diffuser(d) => Some(Vec2::new(f64::from(d.length) / 2., f64::from(d.width) / 2.)),
		PartKind::InterferenceFilter(f) => Some(Vec2::new(f64::from(f.length) / 2., f64::from(f.width) / 2.)),
		PartKind::MetaSurface(m) => Some(Vec2::new(f64::from(m.length) / 2., f64::from(m.width) / 2.)),
		PartKind::FacettedGrating(g) => Some(Vec2::new(f64::from(g.length) / 2., f64::from(g.width) / 2.)),
		PartKind::GrinLens(g) => Some(Vec2::new(f64::from(g.length) / 2., f64::from(g.diameter) / 2.)),
		_ => None,
	}
}

fn part_handles(project: &Project, p: &Part, item: ItemRef, gap: f64, out: &mut Vec<Handle>) {
	let frame = Frame::of(&p.base);
	let centre = (p.base.x as f32, p.base.y as f32);
	let handle =
		|kind, at: Vec2, from: Option<Vec2>| Handle { item, kind, at: v(at), from: from.map(v), selected: true };
	let shapes = shapes::shapes_for(p, &project.simulation, project.seed);
	let extent = shapes.iter().map(|s| s.extent()).reduce(f64::max).unwrap_or(20.);
	let (dx, dy) = direction(p.base.angle);
	let reach = extent + gap;
	out.push(Handle {
		item,
		kind: HandleKind::PartLever,
		at: (centre.0 + (dx * reach) as f32, centre.1 + (dy * reach) as f32),
		from: Some(centre),
		selected: true,
	});
	if let Some(g) = grip_of(&p.kind) {
		out.push(handle(HandleKind::Grip, frame.to_canvas(g), None));
	}
	match &p.kind {
		PartKind::Outline(o) => {
			for (i, q) in o.points.iter().enumerate() {
				out.push(handle(HandleKind::Point(i), frame.to_canvas(Vec2::new(q.x.into(), q.y.into())), None));
			}
		}
		PartKind::Fiber(f) => {
			let last = f.nodes.len().saturating_sub(1);
			for (i, n) in f.nodes.iter().enumerate() {
				let node = Vec2::new(n.x.into(), n.y.into());
				if i > 0 {
					let h = Vec2::new(f64::from(n.x + n.in_x), f64::from(n.y + n.in_y));
					out.push(handle(HandleKind::InHandle(i), frame.to_canvas(h), Some(frame.to_canvas(node))));
				}
				if i < last {
					let h = Vec2::new(f64::from(n.x + n.out_x), f64::from(n.y + n.out_y));
					out.push(handle(HandleKind::OutHandle(i), frame.to_canvas(h), Some(frame.to_canvas(node))));
				}
				out.push(handle(HandleKind::Node(i), frame.to_canvas(node), None));
			}
		}
		_ => {}
	}
}

/// The topmost handle within [`HIT_RADIUS`] screen pixels of the canvas point `p` (the last drawn wins).
pub fn hit(model: &OverlayModel, p: (f32, f32), scale: f32) -> Option<usize> {
	let r = HIT_RADIUS / scale.max(1e-3);
	model.handles.iter().enumerate().rev().find(|(_, h)| (h.at.0 - p.0).hypot(h.at.1 - p.1) <= r).map(|(i, _)| i)
}

/// How a marquee combines with the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarqueeMode {
	Replace,
	/// Shift.
	Add,
	/// Ctrl.
	Toggle,
}

/// Items whose whole-item handles lie inside the canvas rectangle spanned by `a` and `b`, in drawing order.
pub fn marquee_hits(model: &OverlayModel, a: (f32, f32), b: (f32, f32)) -> Vec<ItemRef> {
	let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
	let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
	let mut hits = Vec::new();
	for h in model.handles.iter().filter(|h| h.kind.moves_item()) {
		let inside = h.at.0 >= x0 && h.at.0 <= x1 && h.at.1 >= y0 && h.at.1 <= y1;
		if inside && !hits.contains(&h.item) {
			hits.push(h.item);
		}
	}
	hits
}

/// The selection a marquee leaves; the last item becomes current.
pub fn marquee_select(current: &[ItemRef], hits: &[ItemRef], mode: MarqueeMode) -> Vec<ItemRef> {
	match mode {
		MarqueeMode::Replace => hits.to_vec(),
		MarqueeMode::Add => {
			let mut out = current.to_vec();
			out.extend(hits.iter().filter(|h| !current.contains(h)));
			out
		}
		MarqueeMode::Toggle => {
			let mut out: Vec<ItemRef> = current.iter().filter(|c| !hits.contains(c)).copied().collect();
			out.extend(hits.iter().filter(|h| !current.contains(h)));
			out
		}
	}
}

/// The whole-pixel move still wanted when a drag that started at `press` is at `now` and `applied` pixels of it
/// have been applied: measured against the absolute pointer, so a clamped move does not drift.
pub fn drag_delta(press: (f32, f32), now: (f32, f32), applied: (i32, i32)) -> (i32, i32) {
	let total = ((now.0 - press.0).round() as i32, (now.1 - press.1).round() as i32);
	(total.0 - applied.0, total.1 - applied.1)
}

/// The angle a lever pivoting at `pivot` points at `pointer`: whole degrees, 15-degree steps with `snap`, in
/// (-180, 180].
pub fn lever_angle(pivot: (f32, f32), pointer: (f32, f32), snap: bool) -> f64 {
	let dx = f64::from(pointer.0 - pivot.0);
	let dy = f64::from(pivot.1 - pointer.1);
	let step = if snap { 15. } else { 1. };
	let mut a = (dy.atan2(dx).to_degrees() / step).round() * step;
	if a <= -180. {
		a += 360.;
	}
	a
}

/// `angle` turned by `delta` degrees, kept in (-180, 180].
pub fn turn(angle: f64, delta: f64) -> f64 {
	let mut a = (angle + delta).rem_euclid(360.);
	if a > 180. {
		a -= 360.;
	}
	a
}

/// The edits of dragging a part's grip to the part-frame point `q`: whole pixels, at least the kind's
/// minimum; a linked rectangle or ellipse takes the farther coordinate for both.
pub fn grip_edits(kind: &PartKind, q: Vec2) -> Vec<PartEdit> {
	let span = |c: f64, min: u32| ((2. * c.abs()).round() as u32).max(min);
	let k = PartEdit::Kind;
	match kind {
		PartKind::Rectangle(r) => {
			let (length, width) = linked_extents(r.linked, span(q.x, 1), span(q.y, 1));
			let rect = |e| k(PartKindEdit::Rectangle(e));
			length.map(RectangleEdit::Length).into_iter().chain([RectangleEdit::Width(width)]).map(rect).collect()
		}
		PartKind::Ellipse(e) => {
			let (length, width) = linked_extents(e.linked, span(q.x, 1), span(q.y, 1));
			let ellipse = |e| k(PartKindEdit::Ellipse(e));
			length.map(EllipseEdit::Length).into_iter().chain([EllipseEdit::Width(width)]).map(ellipse).collect()
		}
		PartKind::Polygon(p) => {
			let n = f64::from(p.sides.max(3));
			let side = (2. * q.length() * (std::f64::consts::PI / n).sin()).round().max(1.) as u32;
			vec![k(PartKindEdit::Polygon(PolygonEdit::Side(side)))]
		}
		PartKind::Lens(_) => vec![k(PartKindEdit::Lens(LensEdit::Diameter(span(q.y, 2))))],
		PartKind::FresnelLens(_) => {
			vec![k(PartKindEdit::FresnelLens(FresnelLensEdit::Lens(LensEdit::Diameter(span(q.y, 2)))))]
		}
		PartKind::Diffuser(_) => vec![
			k(PartKindEdit::Diffuser(DiffuserEdit::Length(span(q.x, 1)))),
			k(PartKindEdit::Diffuser(DiffuserEdit::Width(span(q.y, 2)))),
		],
		PartKind::FacettedGrating(_) => vec![
			k(PartKindEdit::FacettedGrating(FacettedGratingEdit::Length(span(q.x, 1)))),
			k(PartKindEdit::FacettedGrating(FacettedGratingEdit::Width(span(q.y, 2)))),
		],
		PartKind::MetaSurface(_) => vec![
			k(PartKindEdit::MetaSurface(MetaSurfaceEdit::Length(span(q.x, 1)))),
			k(PartKindEdit::MetaSurface(MetaSurfaceEdit::Width(span(q.y, 2)))),
		],
		PartKind::InterferenceFilter(_) => vec![
			k(PartKindEdit::InterferenceFilter(InterferenceFilterEdit::Length(span(q.x, 1)))),
			k(PartKindEdit::InterferenceFilter(InterferenceFilterEdit::Width(span(q.y, 1)))),
		],
		PartKind::GrinLens(_) => vec![
			k(PartKindEdit::GrinLens(GrinLensEdit::Length(span(q.x, 1)))),
			k(PartKindEdit::GrinLens(GrinLensEdit::Diameter(span(q.y, 2)))),
		],
		_ => Vec::new(),
	}
}

/// The length to set (`None` for a linked shape, whose width sets it) and the width.
fn linked_extents(linked: bool, length: u32, width: u32) -> (Option<u32>, u32) {
	if linked { (None, length.max(width)) } else { (Some(length), width) }
}

/// An outline's points with point `i` at the part-frame point `q`.
pub fn moved_point(points: &[Point], i: usize, q: Vec2) -> Vec<Point> {
	let mut out = points.to_vec();
	if let Some(p) = out.get_mut(i) {
		*p = Point::new(q.x.round() as i32, q.y.round() as i32);
	}
	out
}

/// A fibre's nodes after dragging `kind` (a node or one of its Bezier handles) to the part-frame point `q`. A node
/// carries its handles; a smooth node turns the opposite handle to stay opposite, keeping its length.
pub fn moved_node(fiber: &Fiber, kind: HandleKind, q: Vec2) -> Vec<FiberNode> {
	let mut nodes = fiber.nodes.clone();
	let (qx, qy) = (q.x.round() as i32, q.y.round() as i32);
	match kind {
		HandleKind::Node(i) => {
			if let Some(n) = nodes.get_mut(i) {
				(n.x, n.y) = (qx, qy);
			}
		}
		HandleKind::InHandle(i) => {
			if let Some(n) = nodes.get_mut(i) {
				let h = (qx - n.x, qy - n.y);
				if n.smooth {
					(n.out_x, n.out_y) = opposite(h, (n.out_x, n.out_y));
				}
				(n.in_x, n.in_y) = h;
			}
		}
		HandleKind::OutHandle(i) => {
			if let Some(n) = nodes.get_mut(i) {
				let h = (qx - n.x, qy - n.y);
				if n.smooth {
					(n.in_x, n.in_y) = opposite(h, (n.in_x, n.in_y));
				}
				(n.out_x, n.out_y) = h;
			}
		}
		kind => unreachable!("{kind:?} is no fibre handle"),
	}
	nodes
}

/// `keep` turned to point away from `h`, its length kept.
fn opposite(h: (i32, i32), keep: (i32, i32)) -> (i32, i32) {
	let len = f64::from(keep.0).hypot(f64::from(keep.1));
	let d = f64::from(h.0).hypot(f64::from(h.1));
	if d == 0. {
		return keep;
	}
	((-f64::from(h.0) / d * len).round() as i32, (-f64::from(h.1) / d * len).round() as i32)
}

/// The canvas point `p` in the part frame of `part`.
pub fn to_part_frame(part: &Part, p: (f32, f32)) -> Vec2 {
	Frame::of(&part.base).to_frame(Vec2::new(f64::from(p.0), f64::from(p.1)))
}

#[cfg(test)]
mod tests {
	use wavedude_core::model::{self as m, Edit as _, Probe, Rectangle, Source, SourceArray};

	use super::*;

	fn project() -> Project {
		let mut p = Project::default();
		p.scene.width = 200;
		p.scene.height = 100;
		p.sources.push(Source { name: "S".into(), x: 10.0, y: 20.0, ..Source::default() });
		p.probes.push(Probe {
			name: "P".into(),
			ax: 50,
			ay: 50,
			bx: 80,
			by: 50,
			kind: ProbeKind::Line,
			..Probe::default()
		});
		let mut part =
			Part::new(PartKind::Rectangle(Rectangle { length: 40, width: 20, ..Rectangle::default() }), 100, 50);
		part.base.name = "R".into();
		p.parts.push(part);
		p
	}

	const S: ItemRef = ItemRef::new(ItemKind::Source, 0);
	const R: ItemRef = ItemRef::new(ItemKind::Part, 0);
	const P: ItemRef = ItemRef::new(ItemKind::Probe, 0);

	#[test]
	fn handles_sit_on_pixel_centres_and_part_corners() {
		let m = build(&project(), &[], None, Layer::Field, 1., true, true);
		let at = |kind| m.handles.iter().find(|h| h.kind == kind).map(|h| h.at);
		assert_eq!(at(HandleKind::Source), Some((10.5, 20.5)));
		assert_eq!(at(HandleKind::PartCentre), Some((100., 50.)));
		assert_eq!(at(HandleKind::ProbeA), Some((50.5, 50.5)));
		assert_eq!(at(HandleKind::ProbeB), Some((80.5, 50.5)));
		assert_eq!(m.lines.len(), 1);
		assert_eq!(m.labels.len(), 3);
		assert!(m.outlines.is_empty());
	}

	#[test]
	fn probes_show_on_their_layer_only() {
		let m = build(&project(), &[], None, Layer::Intensity, 1., true, true);
		assert!(m.handles.iter().all(|h| h.item.kind != ItemKind::Probe));
		assert!(m.lines.is_empty());
	}

	#[test]
	fn hidden_handles_and_labels_keep_the_selected_item() {
		let m = build(&project(), &[S], Some(S), Layer::Field, 1., false, false);
		assert!(m.handles.iter().all(|h| h.item == S));
		assert_eq!(m.labels.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), vec!["S"]);
	}

	#[test]
	fn the_current_part_gets_its_lever_grip_and_outline() {
		let m = build(&project(), &[R], Some(R), Layer::Field, 1., true, true);
		let kinds: Vec<HandleKind> = m.handles.iter().filter(|h| h.item == R).map(|h| h.kind).collect();
		assert_eq!(kinds, vec![HandleKind::PartLever, HandleKind::Grip, HandleKind::PartCentre]);
		let grip = m.handles.iter().find(|h| h.kind == HandleKind::Grip).unwrap();
		assert_eq!(grip.at, (120., 60.));
		let lever = m.handles.iter().find(|h| h.kind == HandleKind::PartLever).unwrap();
		assert_eq!(lever.from, Some((100., 50.)));
		assert!(lever.at.0 > 120. && (lever.at.1 - 50.).abs() < 1e-4);
		assert_eq!(m.outlines.len(), 1);
		assert_eq!(m.outlines[0].points.len(), 4);
	}

	#[test]
	fn selected_items_are_drawn_last_among_their_kind() {
		let mut p = project();
		p.sources.push(Source { name: "T".into(), x: 30.0, y: 30.0, ..Source::default() });
		let m = build(&p, &[S], None, Layer::Field, 1., true, true);
		let sources: Vec<usize> =
			m.handles.iter().filter(|h| h.item.kind == ItemKind::Source).map(|h| h.item.index).collect();
		assert_eq!(sources, vec![1, 0]);
	}

	#[test]
	fn composite_sources_show_their_emitters() {
		let mut p = project();
		p.sources[0].array =
			Some(SourceArray { size_x: 3, size_y: 2, spacing_x: 10, spacing_y: 10, ..SourceArray::default() });
		let m = build(&p, &[S], Some(S), Layer::Field, 1., true, true);
		assert_eq!(m.dots.len(), 6);
		let lever = m.handles.iter().find(|h| h.kind == HandleKind::SourceLever).unwrap();
		assert_eq!(lever.at, (10.5 + 10. + LEVER_GAP, 20.5));
		assert!(m.labels[0].below);
	}

	#[test]
	fn hit_takes_the_topmost_handle_within_the_screen_radius() {
		let m = build(&project(), &[], None, Layer::Field, 2., true, true);
		assert_eq!(m.handles[hit(&m, (10., 20.), 2.).unwrap()].item, S);
		assert_eq!(hit(&m, (10.5 + 4.9, 20.5), 2.).map(|i| m.handles[i].item), Some(S));
		assert_eq!(hit(&m, (10.5 + 5.1, 20.5), 2.), None);
		let mut overlapping = m.clone();
		overlapping.handles[2].at = overlapping.handles[0].at;
		assert_eq!(hit(&overlapping, (10.5, 20.5), 2.), Some(2));
	}

	#[test]
	fn marquee_selects_by_whole_item_handles() {
		let m = build(&project(), &[R], Some(R), Layer::Field, 1., true, true);
		assert_eq!(marquee_hits(&m, (0., 0.), (60., 60.)), vec![S, P]);
		assert_eq!(
			marquee_hits(&m, (115., 55.), (130., 65.)),
			Vec::<ItemRef>::new(),
			"a grip is not a whole-item handle"
		);
		assert_eq!(marquee_hits(&m, (70., 60.), (101., 40.)), vec![R, P], "the rectangle may be dragged either way");
		assert_eq!(marquee_select(&[R], &[S, P], MarqueeMode::Replace), vec![S, P]);
		assert_eq!(marquee_select(&[R, S], &[S, P], MarqueeMode::Add), vec![R, S, P]);
		assert_eq!(marquee_select(&[R, S], &[S, P], MarqueeMode::Toggle), vec![R, P]);
	}

	#[test]
	fn drags_follow_the_absolute_pointer_and_hold_the_formation() {
		assert_eq!(drag_delta((10., 10.), (13.4, 7.6), (0, 0)), (3, -2));
		assert_eq!(drag_delta((10., 10.), (13.4, 7.6), (3, -2)), (0, 0));
		assert_eq!(drag_delta((10., 10.), (15., 10.), (1, 0)), (4, 0), "a clamped move catches up once room is back");
		let mut p = project();
		let applied = wavedude_app_state::selection::move_group(&mut p, &[S, P], -20, 0);
		assert_eq!(applied, (-10, 0), "the source at x 10 stops the group at the left edge");
		assert_eq!((p.sources[0].x, p.probes[0].ax, p.probes[0].bx), (0.0, 40, 70));
	}

	#[test]
	fn levers_snap_and_stay_in_range() {
		assert_eq!(lever_angle((0., 0.), (10., 0.), false), 0.);
		assert_eq!(lever_angle((0., 0.), (0., -10.), false), 90.);
		assert_eq!(lever_angle((0., 0.), (-10., 0.01), false), 180.);
		assert_eq!(lever_angle((0., 0.), (10., -3.), true), 15.);
		assert_eq!(turn(179., 3.), -178.);
		assert_eq!(turn(-179., -1.), 180.);
	}

	#[test]
	fn grips_set_whole_extents() {
		let rect = |e| PartEdit::Kind(PartKindEdit::Rectangle(e));
		let r = PartKind::Rectangle(Rectangle::default());
		let edits = grip_edits(&r, Vec2::new(-20.4, 7.));
		assert_eq!(edits, vec![rect(RectangleEdit::Length(41)), rect(RectangleEdit::Width(14))]);
		let linked = PartKind::Rectangle(Rectangle { linked: true, ..Rectangle::default() });
		assert_eq!(grip_edits(&linked, Vec2::new(5., 30.)), vec![rect(RectangleEdit::Width(60))]);
		assert_eq!(grip_edits(&r, Vec2::new(0., 0.))[0], rect(RectangleEdit::Length(1)));
	}

	/// Every kind with a grip: dropping the grip where it stands leaves the part as it is, so a drag edits
	/// what the grip shows.
	#[test]
	fn grips_round_trip_through_their_edits() {
		let kinds = [
			PartKind::Rectangle(Rectangle::default()),
			PartKind::Ellipse(m::Ellipse::default()),
			PartKind::Lens(m::Lens::default()),
			PartKind::FresnelLens(m::FresnelLens::new_plate()),
			PartKind::Diffuser(m::Diffuser::default()),
			PartKind::FacettedGrating(m::FacettedGrating::default()),
			PartKind::InterferenceFilter(m::InterferenceFilter::new_part()),
			PartKind::MetaSurface(m::MetaSurface::new_part(m::MetaDesign::default())),
			PartKind::GrinLens(m::GrinLens::default()),
		];
		for kind in kinds {
			let grip = grip_of(&kind).expect("a grip");
			let part = Part::new(kind, 0, 0);
			let mut edited = part.clone();
			let edits = grip_edits(&part.kind, grip);
			assert!(!edits.is_empty(), "{:?}", part.kind);
			for edit in &edits {
				edit.apply(&mut edited).expect("the edit fits its kind");
			}
			assert_eq!(edited, part, "{:?}", part.kind);
		}
	}

	#[test]
	fn fibre_handles_mirror_on_smooth_nodes() {
		let fiber = Fiber {
			nodes: vec![
				FiberNode { x: 0, y: 0, out_x: 10, ..FiberNode::default() },
				FiberNode { x: 100, in_x: -20, out_x: 20, ..FiberNode::default() },
				FiberNode { x: 200, in_x: -10, ..FiberNode::default() },
			],
			..Fiber::default()
		};
		let n = moved_node(&fiber, HandleKind::InHandle(1), Vec2::new(100., -20.));
		assert_eq!((n[1].in_x, n[1].in_y, n[1].out_x, n[1].out_y), (0, -20, 0, 20));
		let n = moved_node(&fiber, HandleKind::OutHandle(1), Vec2::new(100., 10.));
		assert_eq!((n[1].in_x, n[1].in_y, n[1].out_x, n[1].out_y), (0, -20, 0, 10));
		let n = moved_node(&fiber, HandleKind::Node(2), Vec2::new(210.4, 5.6));
		assert_eq!((n[2].x, n[2].y, n[2].in_x), (210, 6, -10));
		assert_eq!(
			moved_point(&[Point::new(1, 1), Point::new(2, 2)], 1, Vec2::new(4.6, -1.2)),
			vec![Point::new(1, 1), Point::new(5, -1)]
		);
	}
}
