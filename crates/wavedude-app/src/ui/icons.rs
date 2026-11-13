//! The drawn icons of the Project pane and its menus (docs/ui/20, Row icons): SVG path data on a 16 x 16 grid with
//! the ink in rows and columns 0..14, stroked 1 px on half pixels or filled, in the text colour. A drawing is
//! flattened once; its mesh at a place and colour is kept in egui's frame cache while it is shown there.

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::cache::{ComputerMut, FrameCache};
use eframe::egui::epaint::{TessellationOptions, Tessellator};
use eframe::egui::{Color32, Mesh, Pos2, Rect, Shape, Stroke, Ui, pos2, vec2};
use kurbo::{BezPath, PathEl};
use wavedude_app_state::commands::NewPart;
use wavedude_core::model::{GateKind, Lens, Part, PartKind};

/// A drawing: a stroked path, a filled one, or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Icon {
	stroke: &'static str,
	fill: &'static str,
}

const fn path(stroke: &'static str) -> Icon {
	Icon { stroke, fill: "" }
}

const fn filled(fill: &'static str) -> Icon {
	Icon { stroke: "", fill }
}

pub(super) const CHEVRON_RIGHT: Icon = path("M5.5 3.5 L9.5 7.5 L5.5 11.5");
pub(super) const CHEVRON_DOWN: Icon = path("M3.5 5.5 L7.5 9.5 L11.5 5.5");
pub(super) const CHEVRON_UP: Icon = path("M3.5 9.5 L7.5 5.5 L11.5 9.5");
pub(super) const PLUS: Icon = path("M7.5 2.5 L7.5 12.5 M2.5 7.5 L12.5 7.5");
pub(super) const MINUS: Icon = path("M2.5 7.5 L12.5 7.5");
pub(super) const CHECK: Icon = path("M2.5 7.5 L5.5 10.5 L12.5 3.5");
pub(super) const DROPDOWN: Icon = filled("M4 6 L11 6 L7.5 9.5 Z");
pub(super) const CLOSE: Icon = path("M3.5 3.5 L11.5 11.5 M11.5 3.5 L3.5 11.5");
pub(super) const GRIP: Icon = path("M5 5.5 L10 5.5 M5 7.5 L10 7.5 M5 9.5 L10 9.5");
pub(super) const SAVE: Icon =
	path("M1.5 1.5 L11.5 1.5 L13.5 3.5 L13.5 13.5 L1.5 13.5 Z M4.5 1.5 L4.5 5.5 L10.5 5.5 L10.5 1.5");
/// Two rings, overlapping: a rectangle's or ellipse's sizes linked.
pub(super) const CHAIN_LINKED: Icon = path(
	"M6.5 0.5 h2 a3 3 0 0 1 3 3 v2 a3 3 0 0 1 -3 3 h-2 a3 3 0 0 1 -3 -3 v-2 a3 3 0 0 1 3 -3 z \
	 M6.5 6.5 h2 a3 3 0 0 1 3 3 v2 a3 3 0 0 1 -3 3 h-2 a3 3 0 0 1 -3 -3 v-2 a3 3 0 0 1 3 -3 z",
);
/// Two rings apart: the sizes parted.
pub(super) const CHAIN_PARTED: Icon = path(
	"M6.5 0.5 h2 a3 3 0 0 1 3 3 a3 3 0 0 1 -3 3 h-2 a3 3 0 0 1 -3 -3 a3 3 0 0 1 3 -3 z \
	 M6.5 8.5 h2 a3 3 0 0 1 3 3 a3 3 0 0 1 -3 3 h-2 a3 3 0 0 1 -3 -3 a3 3 0 0 1 3 -3 z",
);
pub(super) const SOURCE: Icon =
	path("M7.5 3.5 A4 4 0 1 1 7.49 3.5 Z M7.5 0.5 L7.5 2.5 M7.5 12.5 L7.5 14.5 M0.5 7.5 L2.5 7.5 M12.5 7.5 L14.5 7.5");
pub(super) const PROBE: Icon = path("M3.5 3.5 L11.5 3.5 L11.5 11.5 L3.5 11.5 Z M7.5 0.5 L7.5 3.5 M7.5 11.5 L7.5 14.5");
pub(super) const PROJECT: Icon =
	path("M1.5 1.5 L13.5 1.5 L13.5 13.5 L1.5 13.5 Z M1.5 9.5 Q4.5 5.5 7.5 9.5 Q10.5 13.5 13.5 9.5");
pub(super) const SCENE: Icon =
	path("M1.5 2.5 L13.5 2.5 L13.5 12.5 L1.5 12.5 Z M1.5 10.5 L5.5 6.5 L8.5 9.5 L10.5 7.5 L13.5 10.5");
pub(super) const SIMULATION: Icon = path("M7.5 1.5 A6 6 0 1 1 7.49 1.5 Z M7.5 4.5 L7.5 7.5 L10.5 7.5");
pub(super) const DISPLAY: Icon =
	path("M0.5 1.5 L14.5 1.5 L14.5 10.5 L0.5 10.5 Z M4.5 13.5 L10.5 13.5 M7.5 10.5 L7.5 13.5");
pub(super) const PART: Icon = path("M7.5 1.5 Q3.5 7.5 7.5 13.5 Q11.5 7.5 7.5 1.5 Z");
pub(super) const SCHEME: Icon = path("M7.5 1.5 A6 6 0 1 1 7.49 1.5 Z M7.5 1.5 L7.5 13.5");

/// The drawing of a part kind (the Add part menu and the rows of parts other than lenses).
pub(super) fn kind_icon(kind: NewPart) -> Icon {
	match kind {
		NewPart::Lens => {
			path("M3.5 1.5 Q0.5 7.5 3.5 13.5 Q6.5 7.5 3.5 1.5 Z M9.5 1.5 Q12.5 7.5 9.5 13.5 M13.5 1.5 L13.5 13.5")
		}
		NewPart::FresnelLens => path(
			"M5.5 1.5 L9.5 1.5 L9.5 13.5 L5.5 13.5 Z M5.5 1.5 L3.5 3.5 L5.5 4.5 L3.5 6.5 L5.5 7.5 L3.5 8.5 L5.5 10.5 L3.5 11.5 L5.5 13.5 M9.5 1.5 L11.5 3.5 L9.5 4.5 L11.5 6.5 L9.5 7.5 L11.5 8.5 L9.5 10.5 L11.5 11.5 L9.5 13.5",
		),
		NewPart::Diffuser => {
			path("M11.5 1.5 L11.5 13.5 L6.5 13.5 Q4.5 12 6.5 10.5 Q3.5 9 5.5 7.5 Q3.5 6 6 4.5 Q4.5 3 6.5 1.5 Z")
		}
		NewPart::InterferenceFilter => {
			path("M9.5 1.5 L13.5 1.5 L13.5 13.5 L9.5 13.5 Z M7.5 1.5 L7.5 13.5 M5.5 1.5 L5.5 13.5 M3.5 1.5 L3.5 13.5")
		}
		NewPart::SimpleGrating => filled(
			"M7.5 1.5 A1.5 1.5 0 1 1 7.49 1.5 Z M7.5 5.5 A1.5 1.5 0 1 1 7.49 5.5 Z M7.5 9.5 A1.5 1.5 0 1 1 7.49 9.5 Z M7.5 13.5 A1.5 1.5 0 1 1 7.49 13.5 Z",
		),
		NewPart::FacettedGrating => {
			path("M11.5 1.5 L11.5 13.5 L7.5 13.5 L4.5 11.5 L7.5 9.5 L4.5 7.5 L7.5 5.5 L4.5 3.5 L7.5 1.5 Z")
		}
		NewPart::MetaLens => path(
			"M10.5 1.5 L13.5 1.5 L13.5 13.5 L10.5 13.5 Z M10.5 6 L4.5 6 L4.5 9 L10.5 9 M10.5 3 L6.5 3 L6.5 4 L10.5 4 M10.5 11 L6.5 11 L6.5 12 L10.5 12",
		),
		NewPart::MetaPrism => path(
			"M10.5 1.5 L13.5 1.5 L13.5 13.5 L10.5 13.5 Z M10.5 2 L4.5 2 L4.5 3 L10.5 3 M10.5 5 L6 5 L6 7 L10.5 7 M10.5 9 L8 9 L8 10 L10.5 10 M10.5 12 L9.5 12 L9.5 13 L10.5 13",
		),
		NewPart::MetaAxicon => path(
			"M10.5 1.5 L13.5 1.5 L13.5 13.5 L10.5 13.5 Z M10.5 7 L4.5 7 L4.5 8 L10.5 8 M10.5 4 L7 4 L7 5 L10.5 5 M10.5 10 L7 10 L7 11 L10.5 11 M10.5 2 L9 2 L9 2.5 L10.5 2.5 M10.5 12.5 L9 12.5 L9 13 L10.5 13",
		),
		NewPart::Fiber => path("M0.5 4.5 C6.5 4.5 7.5 10.5 14.5 10.5 M0.5 7.5 C6.5 7.5 7.5 13.5 14.5 13.5"),
		NewPart::BraggReflector => {
			path("M2.5 1.5 L2.5 13.5 M5.5 1.5 L5.5 13.5 M8.5 1.5 L8.5 13.5 M11.5 1.5 L11.5 13.5")
		}
		NewPart::GrinLens => {
			path("M1.5 1.5 L13.5 1.5 L13.5 13.5 L1.5 13.5 Z M1.5 4.5 L13.5 4.5 M1.5 7.5 L13.5 7.5 M1.5 10.5 L13.5 10.5")
		}
		NewPart::ZonePlate => path("M7.5 0.5 L7.5 2.5 M7.5 4.5 L7.5 5.5 M7.5 9.5 L7.5 10.5 M7.5 12.5 L7.5 14.5"),
		NewPart::Polygon => path("M4.5 1.5 L10.5 1.5 L13.5 7.5 L10.5 13.5 L4.5 13.5 L1.5 7.5 Z"),
		NewPart::Rectangle => path("M1.5 3.5 L13.5 3.5 L13.5 11.5 L1.5 11.5 Z"),
		NewPart::Ellipse => path("M7.5 3.5 A6 4 0 1 1 7.49 3.5 Z"),
		NewPart::Triangle => path("M7.5 1.5 L13.5 13.5 L1.5 13.5 Z"),
		NewPart::Outline => path("M2.5 3.5 L8.5 1.5 L13.5 6.5 L10.5 13.5 L4.5 11.5 L1.5 7.5 Z"),
		NewPart::XorGate => {
			path("M1.5 2.5 Q4.5 7.5 1.5 12.5 M3.5 2.5 Q6.5 7.5 3.5 12.5 Q11.5 12.5 14.5 7.5 Q11.5 2.5 3.5 2.5")
		}
		NewPart::AndGate => path("M2.5 2.5 L8.5 2.5 A5 5 0 0 1 8.5 12.5 L2.5 12.5 Z"),
		NewPart::OrGate => path("M2.5 2.5 Q5.5 7.5 2.5 12.5 Q10.5 12.5 14.5 7.5 Q10.5 2.5 2.5 2.5"),
	}
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Surface {
	Convex,
	Flat,
	Concave,
}

/// A surface by its sag at the rim: flat under half a pixel; `bulge` is true when the radius's sign bulges
/// outward on that face.
fn surface(radius: f64, diameter: f64, bulge: bool) -> Surface {
	let r = radius.abs();
	let h = diameter / 2.;
	let sag = if radius == 0. {
		0.
	} else if r <= h {
		r
	} else {
		r - (r * r - h * h).sqrt()
	};
	match (sag < 0.5, bulge) {
		(true, _) => Surface::Flat,
		(false, true) => Surface::Convex,
		(false, false) => Surface::Concave,
	}
}

/// The lens profiles by front then back face, each convex, flat or concave.
const LENSES: [[Icon; 3]; 3] = [
	[
		path("M6.5 1.5 L8.5 1.5 Q13.5 7.5 8.5 13.5 L6.5 13.5 Q1.5 7.5 6.5 1.5 Z"),
		path("M6.5 1.5 L10.5 1.5 Q10.5 7.5 10.5 13.5 L6.5 13.5 Q1.5 7.5 6.5 1.5 Z"),
		path("M6.5 1.5 L11.5 1.5 Q8.5 7.5 11.5 13.5 L6.5 13.5 Q1.5 7.5 6.5 1.5 Z"),
	],
	[
		path("M4.5 1.5 L8.5 1.5 Q13.5 7.5 8.5 13.5 L4.5 13.5 Q4.5 7.5 4.5 1.5 Z"),
		path("M4.5 1.5 L10.5 1.5 Q10.5 7.5 10.5 13.5 L4.5 13.5 Q4.5 7.5 4.5 1.5 Z"),
		path("M4.5 1.5 L11.5 1.5 Q8.5 7.5 11.5 13.5 L4.5 13.5 Q4.5 7.5 4.5 1.5 Z"),
	],
	[
		path("M3.5 1.5 L8.5 1.5 Q13.5 7.5 8.5 13.5 L3.5 13.5 Q6.5 7.5 3.5 1.5 Z"),
		path("M3.5 1.5 L10.5 1.5 Q10.5 7.5 10.5 13.5 L3.5 13.5 Q6.5 7.5 3.5 1.5 Z"),
		path("M3.5 1.5 L11.5 1.5 Q8.5 7.5 11.5 13.5 L3.5 13.5 Q6.5 7.5 3.5 1.5 Z"),
	],
];

/// A lens row's icon: its own profile from R1, R2 and the diameter.
pub(super) fn lens_icon(lens: &Lens) -> Icon {
	let d = f64::from(lens.diameter);
	let front = surface(lens.r1, d, lens.r1 > 0.);
	let back = surface(lens.r2, d, lens.r2 < 0.);
	LENSES[front as usize][back as usize]
}

/// A part row's icon.
pub(super) fn part_icon(part: &Part) -> Icon {
	match &part.kind {
		PartKind::Lens(l) => lens_icon(l),
		PartKind::LogicGate(g) => kind_icon(match g.kind {
			GateKind::Xor => NewPart::XorGate,
			GateKind::And => NewPart::AndGate,
			GateKind::Or => NewPart::OrGate,
		}),
		kind => kind_icon(NewPart::of(kind)),
	}
}

/// A drawing flattened to polylines on its grid: each with whether it is closed, stroked ones then filled ones.
struct Flat {
	strokes: Vec<(Vec<Pos2>, bool)>,
	fills: Vec<Vec<Pos2>>,
}

impl Flat {
	fn of(icon: Icon) -> Self {
		let fills = flatten(icon.fill).into_iter().map(|(points, _)| points).collect();
		Self { strokes: flatten(icon.stroke), fills }
	}
}

/// Grid units a flattened curve may stray from the true one.
const TOLERANCE: f64 = 0.05;

fn flatten(data: &str) -> Vec<(Vec<Pos2>, bool)> {
	let path = BezPath::from_svg(data).unwrap_or_else(|e| panic!("the icon path '{data}' is not SVG: {e}"));
	let mut out: Vec<(Vec<Pos2>, bool)> = Vec::new();
	kurbo::flatten(path, TOLERANCE, |el| match el {
		PathEl::MoveTo(p) => out.push((vec![pos2(p.x as f32, p.y as f32)], false)),
		PathEl::LineTo(p) => {
			if let Some((points, _)) = out.last_mut() {
				points.push(pos2(p.x as f32, p.y as f32));
			}
		}
		PathEl::ClosePath => {
			if let Some((_, closed)) = out.last_mut() {
				*closed = true;
			}
		}
		PathEl::QuadTo(..) | PathEl::CurveTo(..) => unreachable!("flattening yields lines"),
	});
	out
}

/// An icon drawn in a rect, in a colour, at a pixel density: the key of its mesh.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Placed {
	icon: Icon,
	rect: [u32; 4],
	color: Color32,
	pixels_per_point: u32,
}

/// Tessellates placed icons, flattening each drawing once.
#[derive(Default)]
struct Placer {
	flats: HashMap<Icon, Flat>,
}

impl ComputerMut<Placed, Arc<Mesh>> for Placer {
	fn compute(&mut self, placed: Placed) -> Arc<Mesh> {
		let flat = self.flats.entry(placed.icon).or_insert_with(|| Flat::of(placed.icon));
		let [x0, y0, x1, y1] = placed.rect.map(f32::from_bits);
		let rect = Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
		let scale = rect.width().min(rect.height()) / 16.;
		let origin = rect.center() - vec2(8., 8.) * scale;
		let place = |points: &[Pos2]| points.iter().map(|p| origin + p.to_vec2() * scale).collect::<Vec<_>>();
		let ppp = f32::from_bits(placed.pixels_per_point);
		let mut tessellator = Tessellator::new(ppp, TessellationOptions::default(), [1, 1], Vec::new());
		let mut mesh = Mesh::default();
		for points in &flat.fills {
			tessellator.tessellate_shape(Shape::convex_polygon(place(points), placed.color, Stroke::NONE), &mut mesh);
		}
		let stroke = Stroke::new(scale, placed.color);
		for (points, closed) in &flat.strokes {
			let points = place(points);
			let shape = if *closed { Shape::closed_line(points, stroke) } else { Shape::line(points, stroke) };
			tessellator.tessellate_shape(shape, &mut mesh);
		}
		Arc::new(mesh)
	}
}

/// Draws `icon` in `rect`, scaled from its 16 px grid.
pub(super) fn paint(ui: &Ui, rect: Rect, icon: Icon, color: Color32) {
	let rect = [rect.min.x, rect.min.y, rect.max.x, rect.max.y].map(f32::to_bits);
	let pixels_per_point = ui.ctx().pixels_per_point().to_bits();
	let placed = Placed { icon, rect, color, pixels_per_point };
	let mesh = ui.memory_mut(|m| Arc::clone(m.caches.cache::<FrameCache<Arc<Mesh>, Placer>>().get(placed)));
	ui.painter().add(Shape::Mesh(mesh));
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn every_drawing_parses() {
		let fixed = [CHEVRON_RIGHT, CHEVRON_DOWN, CHEVRON_UP, PLUS, MINUS, CHECK, DROPDOWN, SOURCE, PROBE, PROJECT];
		let more = [SCENE, SIMULATION, DISPLAY, PART, SCHEME];
		let kinds = crate::menus::PART_KINDS.map(|(kind, ..)| kind_icon(kind));
		for icon in fixed.iter().chain(&more).chain(&kinds).chain(LENSES.as_flattened()) {
			let flat = Flat::of(*icon);
			assert!(!flat.strokes.is_empty() || !flat.fills.is_empty(), "{icon:?} draws nothing");
		}
	}

	#[test]
	fn a_lens_icon_follows_its_faces() {
		let lens = |r1, r2| Lens { r1, r2, diameter: 100, ..Lens::default() };
		assert_eq!(lens_icon(&lens(80., -80.)), LENSES[0][0], "biconvex");
		assert_eq!(lens_icon(&lens(0., 1e9)), LENSES[1][1], "flat faces");
		assert_eq!(lens_icon(&lens(-80., 80.)), LENSES[2][2], "biconcave");
		assert_eq!(lens_icon(&lens(80., 0.)), LENSES[0][1], "plano-convex");
	}
}
