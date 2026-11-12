//! The overlay of a picture (docs/ui/40-preview-pane.md, Handles and overlays): which handles, dots, lines,
//! outlines and labels it shows, built from the project and the selection, and the shapes that draw them.
//!
//! Coordinates: canvas pixels as continuous values; a pixel (x, y) spans [x, x + 1), so a source or probe at
//! pixel (x, y) sits at (x + 0.5, y + 0.5) and a part's centre at (x, y), the corner of its pixel. Angles in
//! degrees, counter-clockwise on screen. `scale` is logical pixels per canvas pixel; the shapes are in the
//! logical pixels of the picture's cell.

use wavedude_core::engine::emitters::elements_of;
use wavedude_core::model::{Edges, ItemKind, ItemRef, Layer, Part, PartKind, ProbeKind, Project, Scene};
use wavedude_core::scene::shapes::{self, Frame, Vec2};

use crate::draw::{Baseline, DeviceGrid, Disc, Halo, Ink, Pen, Rect, Rgba, Shape, TextMetrics};

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

/// The handles' fixed colours: light grey over a dark halo, the selection bright yellow.
pub const HANDLE: Rgba = Rgba::rgb(0xD8, 0xD8, 0xD8);
pub const SELECTED: Rgba = Rgba::rgb(0xFF, 0xE6, 0x00);
pub const HALO: Rgba = Rgba::new(0x00, 0x00, 0x00, 0xB0);
/// The labels' text size, logical pixels.
pub const LABEL_SIZE: f32 = 11.;

fn handle_color(h: &Handle, focused: bool, accent: Rgba) -> Rgba {
	if focused {
		accent
	} else if h.selected {
		SELECTED
	} else {
		HANDLE
	}
}

fn line(out: &mut Vec<Shape>, from: (f32, f32), to: (f32, f32), pen: Pen) {
	out.push(Shape::Line { from, to, pen });
}

fn circle(out: &mut Vec<Shape>, centre: (f32, f32), radius: f32, ink: Ink) {
	out.push(Shape::Circle { centre, radius, ink });
}

fn square(out: &mut Vec<Shape>, (x, y): (f32, f32), half: f32, ink: Ink) {
	out.push(Shape::Rect { rect: Rect::new(x - half, y - half, x + half, y + half), ink });
}

fn glyph(out: &mut Vec<Shape>, kind: HandleKind, (x, y): (f32, f32), color: Rgba) {
	let pens = [Pen::new(HALO, 3.5), Pen::new(color, 1.5)];
	let ticks = |out: &mut Vec<Shape>, inner: f32, outer: f32, pen: Pen| {
		line(out, (x - outer, y), (x - inner, y), pen);
		line(out, (x + inner, y), (x + outer, y), pen);
		line(out, (x, y - outer), (x, y - inner), pen);
		line(out, (x, y + inner), (x, y + outer), pen);
	};
	match kind {
		HandleKind::Source => {
			for pen in pens {
				circle(out, (x, y), 6., Ink::Stroke(pen));
				ticks(out, 7., 10., pen);
			}
		}
		HandleKind::PartCentre => {
			for pen in pens {
				circle(out, (x, y), 7., Ink::Stroke(pen));
				circle(out, (x, y), 3., Ink::Stroke(pen));
				ticks(out, 8., 10., pen);
			}
		}
		HandleKind::ProbeA | HandleKind::ProbeB => {
			let r = if kind == HandleKind::ProbeA { 5. } else { 4. };
			for pen in pens {
				square(out, (x, y), r, Ink::Stroke(pen));
				ticks(out, r + 1., 7., pen);
			}
		}
		HandleKind::SourceLever | HandleKind::PartLever => {
			circle(out, (x, y), 6., Ink::fill(HALO));
			circle(out, (x, y), 4.5, Ink::fill(color));
		}
		HandleKind::Grip => {
			square(out, (x, y), 5.5, Ink::fill(HALO));
			square(out, (x, y), 4., Ink::fill(color));
		}
		HandleKind::Point(_) | HandleKind::Node(_) => {
			for pen in pens {
				circle(out, (x, y), 4.5, Ink::Stroke(pen));
			}
		}
		HandleKind::InHandle(_) | HandleKind::OutHandle(_) => {
			circle(out, (x, y), 4.5, Ink::fill(HALO));
			circle(out, (x, y), 3., Ink::fill(color));
		}
	}
}

/// The shapes of `model` over a picture at `picture` (x, y, width, height in the cell's logical pixels) and
/// `scale`: probe lines, emitter dots, outlines, handles with the `focused` one in `accent`, then the labels,
/// measured by `text` at [`LABEL_SIZE`] and held inside the picture.
pub fn shapes(
	model: &OverlayModel,
	scale: f32,
	picture: (f32, f32, f32, f32),
	focused: Option<usize>,
	accent: Rgba,
	text: &dyn TextMetrics,
) -> Vec<Shape> {
	let (px, py, pw, ph) = picture;
	let s = |p: (f32, f32)| (px + p.0 * scale, py + p.1 * scale);
	let mut out = Vec::new();
	for l in &model.lines {
		let (a, b) = (s(l.a), s(l.b));
		line(&mut out, a, b, Pen::new(HALO, 3.));
		line(&mut out, a, b, Pen::new(if l.selected { SELECTED } else { HANDLE }, 1.5).dashed(5., 4.));
	}
	if !model.dots.is_empty() {
		out.push(Shape::Dots {
			origin: (px, py),
			size: (pw, ph),
			points: model.dots.iter().map(|&(x, y)| (x * scale, y * scale)).collect(),
			halo: Disc { radius: 2.5, color: HALO },
			dot: Disc { radius: 1.5, color: HANDLE },
		});
	}
	for outline in &model.outlines {
		let points: Vec<(f32, f32)> = outline.points.iter().map(|&p| s(p)).collect();
		out.push(Shape::Path { points: points.clone(), closed: outline.closed, pen: Pen::new(HALO, 3.) });
		out.push(Shape::Path { points, closed: outline.closed, pen: Pen::new(SELECTED, 1.) });
	}
	for (i, h) in model.handles.iter().enumerate() {
		let at = s(h.at);
		if let Some(from) = h.from {
			let from = s(from);
			line(&mut out, from, at, Pen::new(HALO, 3.));
			let pen = Pen::new(handle_color(h, false, accent), 1.);
			line(&mut out, from, at, if h.kind.is_lever() { pen } else { pen.dashed(2., 2.) });
		}
		glyph(&mut out, h.kind, at, handle_color(h, focused == Some(i), accent));
	}
	for label in &model.labels {
		let (x, y) = s(label.at);
		let width = text.width(&label.text);
		let (mut lx, mut ly) = if label.below { (x - width / 2., y + 24.) } else { (x + 13., y + 4.) };
		lx = lx.clamp(px + 2., (px + pw - width - 2.).max(px + 2.));
		ly = ly.clamp(py + 12., (py + ph - 3.).max(py + 12.));
		out.push(Shape::Text {
			text: label.text.clone(),
			at: (lx, ly),
			baseline: Baseline::Alphabetic,
			size: LABEL_SIZE,
			color: if label.selected { SELECTED } else { HANDLE },
			halo: Some(Halo { color: HALO, width: 3. }),
		});
	}
	out
}

/// The edge line's colour: pure blue for a damping rim outside the canvas (it wins over a wall), pure red for a
/// reflective wall, none for open edges or a damping band inside the canvas.
pub fn edge_color(scene: &Scene) -> Option<Rgba> {
	if scene.has_rim() {
		Some(Rgba::rgb(0, 0, 255))
	} else if scene.edges == Edges::Reflective {
		Some(Rgba::rgb(255, 0, 0))
	} else {
		None
	}
}

/// The edge line around `picture` (x, y, width, height in the element's logical pixels): an aliased hairline of
/// `color` just outside the picture's device pixels, in device pixels of `grid`.
pub fn edge_line(grid: DeviceGrid, picture: (f32, f32, f32, f32), color: Rgba) -> Shape {
	let hl = grid.hairline();
	let (x, y, w, h) = picture;
	let r = grid.rect((x, y), (x + w, y + h));
	let half = hl / 2.;
	Shape::Rect {
		rect: Rect::new(r.left - half, r.top - half, r.right + half, r.bottom + half),
		ink: Ink::Stroke(Pen::new(color, hl).aliased()),
	}
}

#[cfg(test)]
mod tests {
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

	struct Fixed;
	impl TextMetrics for Fixed {
		fn line_height(&self) -> f32 {
			14.
		}
		fn width(&self, text: &str) -> f32 {
			text.len() as f32 * 6.
		}
	}

	fn count(shapes: &[Shape], f: impl Fn(&Shape) -> bool) -> usize {
		shapes.iter().filter(|s| f(s)).count()
	}

	#[test]
	fn a_model_draws_its_lines_handles_and_labels_in_order() {
		let m = build(&project(), &[], None, Layer::Field, 2., true, true);
		let shapes = shapes(&m, 2., (10., 20., 400., 200.), Some(0), Rgba::rgb(1, 2, 3), &Fixed);
		assert_eq!(
			shapes[..2],
			[
				Shape::Line { from: (111., 121.), to: (171., 121.), pen: Pen::new(HALO, 3.) },
				Shape::Line { from: (111., 121.), to: (171., 121.), pen: Pen::new(HANDLE, 1.5).dashed(5., 4.) },
			],
			"the probe line, canvas pixels to the cell's"
		);
		assert_eq!(
			shapes[2],
			Shape::Circle { centre: (31., 61.), radius: 6., ink: Ink::Stroke(Pen::new(HALO, 3.5)) },
			"the source's halo first"
		);
		assert_eq!(
			shapes[7],
			Shape::Circle { centre: (31., 61.), radius: 6., ink: Ink::Stroke(Pen::new(Rgba::rgb(1, 2, 3), 1.5)) },
			"the focused handle in the accent"
		);
		let texts: Vec<(&str, (f32, f32))> = shapes
			.iter()
			.filter_map(|s| match s {
				Shape::Text { text, at, .. } => Some((text.as_str(), *at)),
				_ => None,
			})
			.collect();
		assert_eq!(texts, vec![("S", (44., 65.)), ("R", (207., 144.)), ("P", (124., 125.))]);
		assert!(matches!(shapes.last(), Some(Shape::Text { .. })), "labels over everything");
		assert_eq!(count(&shapes, |s| matches!(s, Shape::Dots { .. } | Shape::Path { .. })), 0);
	}

	#[test]
	fn labels_stay_inside_the_picture() {
		let mut p = project();
		p.sources[0].x = 199.;
		p.sources[0].name = "Long name".into();
		let m = build(&p, &[], None, Layer::Intensity, 1., false, true);
		let shapes = shapes(&m, 1., (0., 0., 200., 100.), None, HANDLE, &Fixed);
		let source = shapes.iter().find_map(|s| match s {
			Shape::Text { text, at, .. } if text == "Long name" => Some(*at),
			_ => None,
		});
		assert_eq!(source, Some((200. - 54. - 2., 24.5)));
	}

	#[test]
	fn a_selected_composite_draws_dots_an_outline_and_a_lever() {
		let mut p = project();
		p.sources[0].array =
			Some(SourceArray { size_x: 3, size_y: 2, spacing_x: 10, spacing_y: 10, ..SourceArray::default() });
		let m = build(&p, &[S, R], Some(R), Layer::Field, 1., true, false);
		let shapes = shapes(&m, 1., (5., 5., 200., 100.), None, HANDLE, &Fixed);
		let dots = shapes.iter().find_map(|s| match s {
			Shape::Dots { origin, points, dot, .. } => Some((*origin, points.len(), dot.color)),
			_ => None,
		});
		assert_eq!(dots, Some(((5., 5.), 6, HANDLE)));
		let outlines: Vec<(usize, bool, Rgba)> = shapes
			.iter()
			.filter_map(|s| match s {
				Shape::Path { points, closed, pen } => Some((points.len(), *closed, pen.color)),
				_ => None,
			})
			.collect();
		assert_eq!(outlines, vec![(4, true, HALO), (4, true, SELECTED)]);
		let lever = shapes.iter().any(|s| {
			matches!(s, Shape::Line { from: (105., 55.), to, pen } if to.0 > 105. + LEVER_GAP && *pen == Pen::new(SELECTED, 1.))
		});
		assert!(lever, "a lever's line is solid");
		assert_eq!(count(&shapes, |s| matches!(s, Shape::Text { .. })), 2, "the selected items' labels");
	}

	#[test]
	fn the_edge_line_hugs_the_picture_on_whole_device_pixels() {
		let red = Rgba::rgb(255, 0, 0);
		let line = edge_line(DeviceGrid::new(1.5, 0.5, 0.), (10., 4., 100., 50.), red);
		assert_eq!(
			line,
			Shape::Rect { rect: Rect::new(15., 5., 167., 82.), ink: Ink::Stroke(Pen::new(red, 2.).aliased()) },
			"a two-pixel hairline around the picture's device rectangle 16, 6 to 166, 81"
		);
	}

	#[test]
	fn the_edge_line_names_what_a_wave_meets() {
		let mut s = Scene { damping_edge: -32, edges: Edges::Reflective, ..Scene::default() };
		assert_eq!(edge_color(&s), Some(Rgba::rgb(0, 0, 255)), "a rim wins over a wall");
		s.damping_edge = 0;
		assert_eq!(edge_color(&s), Some(Rgba::rgb(255, 0, 0)));
		s.damping_edge = 20;
		assert_eq!(edge_color(&s), Some(Rgba::rgb(255, 0, 0)), "a band inside keeps the wall");
		s.edges = Edges::Open;
		assert_eq!(edge_color(&s), None);
	}
}
