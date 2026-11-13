//! The overlay of a picture (docs/ui/40-preview-pane.md, Handles and overlays): which handles, dots, lines,
//! outlines and labels it shows, built from the project and the selection, and the shapes that draw them.
//!
//! Coordinates: canvas pixels as continuous values; a pixel (x, y) spans [x, x + 1), so a source or probe at
//! pixel (x, y) sits at (x + 0.5, y + 0.5) and a part's centre at (x, y), the corner of its pixel. Angles in
//! degrees, counter-clockwise on screen. `scale` is logical pixels per canvas pixel; the shapes are epaint shapes in
//! the window's points.

use epaint::text::{FontId, FontsView};
use epaint::{Color32, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};
use wavedude_core::engine::emitters::elements_of;
use wavedude_core::model::{Edges, ItemKind, ItemRef, Layer, Part, PartKind, ProbeKind, Project, Scene};
use wavedude_core::scene::shapes::{self, Frame, Vec2};

use crate::draw::{DeviceGrid, Halo, Outlines, baseline, haloed_text};

/// Screen distance of a lever beyond the item's reach, logical pixels.
pub const LEVER_GAP: f32 = 24.;

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
	(libm::cos(r), -libm::sin(r))
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

/// The handles' fixed colours: light grey over a dark halo, the selection bright yellow.
pub const HANDLE: Color32 = Color32::from_rgb(0xD8, 0xD8, 0xD8);
pub const SELECTED: Color32 = Color32::from_rgb(0xFF, 0xE6, 0x00);
pub const HALO: Color32 = Color32::from_rgba_unmultiplied_const(0x00, 0x00, 0x00, 0xB0);
/// The labels' text size, logical pixels.
pub const LABEL_SIZE: f32 = 11.;
/// The halo under a label: its outline stroked 3 logical pixels wide.
pub const LABEL_HALO: Halo = Halo { color: HALO, width: 3. };

fn handle_color(h: &Handle, focused: bool, accent: Color32) -> Color32 {
	if focused {
		accent
	} else if h.selected {
		SELECTED
	} else {
		HANDLE
	}
}

fn line(out: &mut Vec<Shape>, from: Pos2, to: Pos2, stroke: Stroke) {
	out.push(Shape::line_segment([from, to], stroke));
}

fn ring(out: &mut Vec<Shape>, centre: Pos2, radius: f32, stroke: Stroke) {
	out.push(Shape::circle_stroke(centre, radius, stroke));
}

fn square(centre: Pos2, half: f32) -> Rect {
	Rect::from_center_size(centre, vec2(2. * half, 2. * half))
}

fn glyph(out: &mut Vec<Shape>, kind: HandleKind, at: Pos2, color: Color32) {
	let strokes = [Stroke::new(3.5, HALO), Stroke::new(1.5, color)];
	let ticks = |out: &mut Vec<Shape>, inner: f32, outer: f32, stroke: Stroke| {
		line(out, at - vec2(outer, 0.), at - vec2(inner, 0.), stroke);
		line(out, at + vec2(inner, 0.), at + vec2(outer, 0.), stroke);
		line(out, at - vec2(0., outer), at - vec2(0., inner), stroke);
		line(out, at + vec2(0., inner), at + vec2(0., outer), stroke);
	};
	match kind {
		HandleKind::Source => {
			for stroke in strokes {
				ring(out, at, 6., stroke);
				ticks(out, 7., 10., stroke);
			}
		}
		HandleKind::PartCentre => {
			for stroke in strokes {
				ring(out, at, 7., stroke);
				ring(out, at, 3., stroke);
				ticks(out, 8., 10., stroke);
			}
		}
		HandleKind::ProbeA | HandleKind::ProbeB => {
			let r = if kind == HandleKind::ProbeA { 5. } else { 4. };
			for stroke in strokes {
				out.push(Shape::rect_stroke(square(at, r), 0, stroke, StrokeKind::Middle));
				ticks(out, r + 1., 7., stroke);
			}
		}
		HandleKind::SourceLever | HandleKind::PartLever => {
			out.push(Shape::circle_filled(at, 6., HALO));
			out.push(Shape::circle_filled(at, 4.5, color));
		}
		HandleKind::Grip => {
			out.push(Shape::rect_filled(square(at, 5.5), 0, HALO));
			out.push(Shape::rect_filled(square(at, 4.), 0, color));
		}
		HandleKind::Point(_) | HandleKind::Node(_) => {
			for stroke in strokes {
				ring(out, at, 4.5, stroke);
			}
		}
		HandleKind::InHandle(_) | HandleKind::OutHandle(_) => {
			out.push(Shape::circle_filled(at, 4.5, HALO));
			out.push(Shape::circle_filled(at, 3., color));
		}
	}
}

/// The shapes of `model` over `picture` (points of the window) at `scale` points per canvas pixel: probe lines,
/// emitter dots, outlines, handles with the `focused` one in `accent`, then the labels in Selawik at [`LABEL_SIZE`]
/// over their halo ([`LABEL_HALO`], from `outlines`), on the alphabetic baseline and held inside the picture. The
/// caller clips them to the picture.
pub fn shapes(
	model: &OverlayModel,
	scale: f32,
	picture: Rect,
	focused: Option<usize>,
	accent: Color32,
	fonts: &mut FontsView<'_>,
	outlines: &mut Outlines,
) -> Vec<Shape> {
	let s = |p: (f32, f32)| picture.min + vec2(p.0 * scale, p.1 * scale);
	let mut out = Vec::new();
	for l in &model.lines {
		let (a, b) = (s(l.a), s(l.b));
		line(&mut out, a, b, Stroke::new(3., HALO));
		out.extend(Shape::dashed_line(&[a, b], Stroke::new(1.5, if l.selected { SELECTED } else { HANDLE }), 5., 4.));
	}
	out.extend(model.dots.iter().map(|&p| Shape::circle_filled(s(p), 2.5, HALO)));
	out.extend(model.dots.iter().map(|&p| Shape::circle_filled(s(p), 1.5, HANDLE)));
	for outline in &model.outlines {
		let points: Vec<Pos2> = outline.points.iter().map(|&p| s(p)).collect();
		let path = |stroke: Stroke| {
			if outline.closed {
				Shape::closed_line(points.clone(), stroke)
			} else {
				Shape::line(points.clone(), stroke)
			}
		};
		out.push(path(Stroke::new(3., HALO)));
		out.push(path(Stroke::new(1., SELECTED)));
	}
	for (i, h) in model.handles.iter().enumerate() {
		let at = s(h.at);
		if let Some(from) = h.from {
			let from = s(from);
			line(&mut out, from, at, Stroke::new(3., HALO));
			let stroke = Stroke::new(1., handle_color(h, false, accent));
			if h.kind.is_lever() {
				line(&mut out, from, at, stroke);
			} else {
				out.extend(Shape::dashed_line(&[from, at], stroke, 2., 2.));
			}
		}
		glyph(&mut out, h.kind, at, handle_color(h, focused == Some(i), accent));
	}
	for label in model.labels.iter().filter(|l| !l.text.is_empty()) {
		let at = s(label.at);
		let color = if label.selected { SELECTED } else { HANDLE };
		let galley = fonts.layout_no_wrap(label.text.clone(), FontId::proportional(LABEL_SIZE), color);
		let width = galley.size().x;
		let (mut lx, mut ly) = if label.below { (at.x - width / 2., at.y + 24.) } else { (at.x + 13., at.y + 4.) };
		lx = lx.clamp(picture.left() + 2., (picture.right() - width - 2.).max(picture.left() + 2.));
		ly = ly.clamp(picture.top() + 12., (picture.bottom() - 3.).max(picture.top() + 12.));
		let top = pos2(lx, ly - baseline(&galley));
		haloed_text(&mut out, outlines, top, galley, LABEL_SIZE, color, LABEL_HALO);
	}
	out
}

/// The edge line's colour: pure blue for a damping rim outside the canvas (it wins over a wall), pure red for a
/// reflective wall, none for open edges or a damping band inside the canvas.
pub fn edge_color(scene: &Scene) -> Option<Color32> {
	if scene.has_rim() {
		Some(Color32::from_rgb(0, 0, 255))
	} else if scene.edges == Edges::Reflective {
		Some(Color32::from_rgb(255, 0, 0))
	} else {
		None
	}
}

/// The edge line around `picture` (x, y, width, height in the element's logical pixels, the element's origin that of
/// `grid`): a hairline of `color` just outside the picture's device pixels.
pub fn edge_line(grid: DeviceGrid, picture: (f32, f32, f32, f32), color: Color32) -> Shape {
	let hl = grid.hairline();
	let (x, y, w, h) = picture;
	let [left, top, right, bottom] = grid.snapped((x, y), (x + w, y + h));
	let half = hl / 2.;
	let rect = grid.rect((left - half, top - half), (right + half, bottom + half));
	Shape::rect_stroke(rect, 0, Stroke::new(hl / grid.scale, color), StrokeKind::Middle)
}

#[cfg(test)]
mod tests {
	use epaint::text::{Fonts, TextOptions};
	use epaint::{ColorMode, PathStroke};
	use wavedude_core::model::{Probe, Rectangle, Source, SourceArray};

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

	fn fonts() -> Fonts {
		Fonts::new(TextOptions::default(), crate::fonts::definitions())
	}

	fn count(shapes: &[Shape], f: impl Fn(&Shape) -> bool) -> usize {
		shapes.iter().filter(|s| f(s)).count()
	}

	/// The top-left and colour of every text drawn in its own colour, not a halo copy.
	fn texts(shapes: &[Shape]) -> Vec<(String, Pos2, Color32)> {
		shapes
			.iter()
			.filter_map(|s| match s {
				Shape::Text(t) if t.override_text_color != Some(HALO) => {
					Some((t.galley.text().to_owned(), t.pos, t.override_text_color?))
				}
				_ => None,
			})
			.collect()
	}

	#[test]
	fn a_model_draws_its_lines_handles_and_labels_in_order() {
		let mut fonts = fonts();
		let mut view = fonts.with_pixels_per_point(2.);
		let m = build(&project(), &[], None, Layer::Field, 2., true, true);
		let accent = Color32::from_rgb(1, 2, 3);
		let picture = Rect::from_min_size(pos2(10., 20.), vec2(400., 200.));
		let shapes = shapes(&m, 2., picture, Some(0), accent, &mut view, &mut Outlines::new(1.));
		let (a, b) = (pos2(111., 121.), pos2(171., 121.));
		assert_eq!(shapes[0], Shape::line_segment([a, b], Stroke::new(3., HALO)), "the probe line, canvas to window");
		let dash = Stroke::new(1.5, HANDLE);
		assert_eq!(shapes[1], Shape::line_segment([a, a + vec2(5., 0.)], dash), "dashed over its halo");
		let dashes =
			shapes[1..].iter().take_while(|s| matches!(s, Shape::LineSegment { stroke, .. } if *stroke == dash));
		assert_eq!(dashes.count(), 7, "60 points of 5 on, 4 off");
		let source = pos2(31., 61.);
		let rings: Vec<Stroke> = shapes
			.iter()
			.filter_map(|s| match s {
				Shape::Circle(c) if c.center == source => Some(c.stroke),
				_ => None,
			})
			.collect();
		assert_eq!(
			rings,
			[Stroke::new(3.5, HALO), Stroke::new(1.5, accent)],
			"the halo first, the focused one in the accent"
		);
		let texts = texts(&shapes);
		let names: Vec<&str> = texts.iter().map(|t| t.0.as_str()).collect();
		assert_eq!(names, ["S", "R", "P"]);
		assert_eq!(texts[0].1.x, source.x + 13., "a source's name to its right");
		assert!(matches!(shapes.last(), Some(Shape::Text(_))), "labels over everything");
		let first_label = shapes.iter().position(|s| matches!(s, Shape::Path(_) | Shape::Text(_)));
		let halo = first_label.map(|i| &shapes[i]);
		assert!(
			matches!(halo, Some(Shape::Path(p)) if p.closed && p.stroke == PathStroke::new(3., HALO)),
			"no outline unselected: the first path is a label's halo"
		);
	}

	#[test]
	fn labels_stay_inside_the_picture_on_their_baseline() {
		let mut fonts = fonts();
		let mut view = fonts.with_pixels_per_point(1.);
		let mut p = project();
		p.sources[0].x = 199.;
		p.sources[0].name = "Long name".into();
		let m = build(&p, &[], None, Layer::Intensity, 1., false, true);
		let picture = Rect::from_min_size(Pos2::ZERO, vec2(200., 100.));
		let shapes = shapes(&m, 1., picture, None, HANDLE, &mut view, &mut Outlines::new(1.));
		let galley = view.layout_no_wrap("Long name".into(), FontId::proportional(LABEL_SIZE), HANDLE);
		let at = texts(&shapes).into_iter().find(|t| t.0 == "Long name").map(|t| t.1);
		assert_eq!(at, Some(pos2(200. - galley.size().x - 2., 24.5 - baseline(&galley))));
		let halos = count(&shapes, |s| matches!(s, Shape::Path(p) if p.stroke == PathStroke::new(3., HALO)));
		assert!(halos >= 2, "each label over its halo, a path per contour: {halos}");
		assert_eq!(count(&shapes, |s| matches!(s, Shape::Text(_))), 2, "and no copies of the text");
	}

	#[test]
	fn a_selected_composite_draws_dots_an_outline_and_a_lever() {
		let mut fonts = fonts();
		let mut view = fonts.with_pixels_per_point(1.);
		let mut p = project();
		p.sources[0].array =
			Some(SourceArray { size_x: 3, size_y: 2, spacing_x: 10, spacing_y: 10, ..SourceArray::default() });
		let m = build(&p, &[S, R], Some(R), Layer::Field, 1., true, false);
		let shapes = shapes(
			&m,
			1.,
			Rect::from_min_size(pos2(5., 5.), vec2(200., 100.)),
			None,
			HANDLE,
			&mut view,
			&mut Outlines::new(1.),
		);
		let dots = |radius: f32, color: Color32| {
			count(&shapes, |s| matches!(s, Shape::Circle(c) if c.radius == radius && c.fill == color))
		};
		assert_eq!((dots(2.5, HALO), dots(1.5, HANDLE)), (6, 6));
		let outlines: Vec<(usize, bool, ColorMode)> = shapes
			.iter()
			.filter_map(|s| match s {
				Shape::Path(p) => Some((p.points.len(), p.closed, p.stroke.color.clone())),
				_ => None,
			})
			.take(2)
			.collect();
		assert_eq!(outlines, vec![(4, true, ColorMode::Solid(HALO)), (4, true, ColorMode::Solid(SELECTED))]);
		let lever = shapes.iter().any(|s| match s {
			Shape::LineSegment { points: [from, to], stroke } => {
				*from == pos2(105., 55.) && to.x > 105. + LEVER_GAP && *stroke == Stroke::new(1., SELECTED)
			}
			_ => false,
		});
		assert!(lever, "a lever's line is solid");
		assert_eq!(texts(&shapes).len(), 2, "the selected items' labels");
	}

	#[test]
	fn the_edge_line_hugs_the_picture_on_whole_device_pixels() {
		let red = Color32::from_rgb(255, 0, 0);
		let line = edge_line(DeviceGrid::new(1.5, 0.5, 0.), (10., 4., 100., 50.), red);
		let Shape::Rect(r) = line else { panic!("a rectangle") };
		assert_eq!(r.stroke, Stroke::new(2. / 1.5, red), "a two-pixel hairline");
		assert_eq!(r.stroke_kind, StrokeKind::Middle);
		let device = |p: Pos2| (p.x * 1.5, p.y * 1.5);
		assert_eq!(
			(device(r.rect.min), device(r.rect.max)),
			((15., 5.), (167., 82.)),
			"around the picture's device rectangle 16, 6 to 166, 81"
		);
	}

	#[test]
	fn the_edge_line_names_what_a_wave_meets() {
		let mut s = Scene { damping_edge: -32, edges: Edges::Reflective, ..Scene::default() };
		assert_eq!(edge_color(&s), Some(Color32::from_rgb(0, 0, 255)), "a rim wins over a wall");
		s.damping_edge = 0;
		assert_eq!(edge_color(&s), Some(Color32::from_rgb(255, 0, 0)));
		s.damping_edge = 20;
		assert_eq!(edge_color(&s), Some(Color32::from_rgb(255, 0, 0)), "a band inside keeps the wall");
		s.edges = Edges::Open;
		assert_eq!(edge_color(&s), None);
	}
}
