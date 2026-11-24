//! The tree rows' icons (docs/ui/20, Tree structure): WaveDude Next's drawings and, for its Segoe Fluent glyphs,
//! Material Symbols (docs/divergences.md, Icons). Icons the `.slint` files name are theirs to load.

use std::cell::RefCell;
use std::collections::HashMap;

use slint::Image;
use waveduderust::model::lens_geometry::sag;
use waveduderust::model::{GateKind, MetaDesign, PartKind, ProbeDetector, Project};

use crate::tree::Node;

macro_rules! svgs {
	($($name:literal => $path:literal),* $(,)?) => {
		fn svg(name: &str) -> &'static [u8] {
			match name {
				$($name => include_bytes!(concat!("../ui/icons/", $path)),)*
				_ => unreachable!("{name} is not an icon"),
			}
		}
	};
}

svgs! {
	"project" => "next/project.svg",
	"source" => "next/source.svg",
	"probe" => "next/probe.svg",
	"part" => "next/part.svg",
	"counting-detector" => "next/counting-detector.svg",
	"imaging-detector" => "next/imaging-detector.svg",
	"lens-convex-convex" => "next/lens-convex-convex.svg",
	"lens-convex-flat" => "next/lens-convex-flat.svg",
	"lens-convex-concave" => "next/lens-convex-concave.svg",
	"lens-flat-convex" => "next/lens-flat-convex.svg",
	"lens-flat-flat" => "next/lens-flat-flat.svg",
	"lens-flat-concave" => "next/lens-flat-concave.svg",
	"lens-concave-convex" => "next/lens-concave-convex.svg",
	"lens-concave-flat" => "next/lens-concave-flat.svg",
	"lens-concave-concave" => "next/lens-concave-concave.svg",
	"fresnel-lens" => "next/fresnel-lens.svg",
	"polygon" => "next/polygon.svg",
	"rectangle" => "next/rectangle.svg",
	"ellipse" => "next/ellipse.svg",
	"triangle" => "next/triangle.svg",
	"outline" => "next/outline.svg",
	"diffuser" => "next/diffuser.svg",
	"interference-filter" => "next/interference-filter.svg",
	"simple-grating" => "next/simple-grating.svg",
	"facetted-grating" => "next/facetted-grating.svg",
	"meta-lens" => "next/meta-lens.svg",
	"meta-prism" => "next/meta-prism.svg",
	"meta-axicon" => "next/meta-axicon.svg",
	"meta-surfaces" => "next/meta-surfaces.svg",
	"optical-logic" => "next/optical-logic.svg",
	"fiber" => "next/fiber.svg",
	"bragg-reflector" => "next/bragg-reflector.svg",
	"grin-lens" => "next/grin-lens.svg",
	"xor-gate" => "next/xor-gate.svg",
	"and-gate" => "next/and-gate.svg",
	"or-gate" => "next/or-gate.svg",
	"zone-plate" => "next/zone-plate.svg",
	"scene" => "symbols/image.svg",
	"simulation" => "symbols/timer.svg",
	"display" => "symbols/monitor.svg",
	"palette" => "symbols/palette.svg",
}

thread_local! {
	static LOADED: RefCell<HashMap<&'static str, Image>> = RefCell::new(HashMap::new());
}

/// The icon `name`, parsed once per thread.
fn image(name: &'static str) -> Image {
	LOADED.with_borrow_mut(|loaded| {
		loaded
			.entry(name)
			.or_insert_with(|| Image::load_from_svg_data(svg(name)).expect("a bundled icon is valid SVG"))
			.clone()
	})
}

/// A lens surface as its row draws it: flat when it bows less than half a pixel at the rim (Next's
/// `LensPart.ShapeOf`).
fn surface(outward: f64, diameter: u32) -> Surface {
	if !outward.is_finite() || sag(outward, f64::from(diameter.max(2)) / 2.0) < 0.5 {
		Surface::Flat
	} else if outward > 0.0 {
		Surface::Convex
	} else {
		Surface::Concave
	}
}

#[derive(Debug, PartialEq)]
enum Surface {
	Convex,
	Flat,
	Concave,
}

fn part(kind: &PartKind) -> &'static str {
	match kind {
		PartKind::Lens(l) => match (surface(l.r1, l.diameter), surface(-l.r2, l.diameter)) {
			(Surface::Convex, Surface::Convex) => "lens-convex-convex",
			(Surface::Convex, Surface::Flat) => "lens-convex-flat",
			(Surface::Convex, Surface::Concave) => "lens-convex-concave",
			(Surface::Flat, Surface::Convex) => "lens-flat-convex",
			(Surface::Flat, Surface::Flat) => "lens-flat-flat",
			(Surface::Flat, Surface::Concave) => "lens-flat-concave",
			(Surface::Concave, Surface::Convex) => "lens-concave-convex",
			(Surface::Concave, Surface::Flat) => "lens-concave-flat",
			(Surface::Concave, Surface::Concave) => "lens-concave-concave",
		},
		PartKind::FresnelLens(_) => "fresnel-lens",
		PartKind::Polygon(_) => "polygon",
		PartKind::Rectangle(_) => "rectangle",
		PartKind::Ellipse(_) => "ellipse",
		PartKind::Triangle(_) => "triangle",
		PartKind::Outline(_) => "outline",
		PartKind::Diffuser(_) => "diffuser",
		PartKind::InterferenceFilter(_) => "interference-filter",
		PartKind::SimpleGrating(_) => "simple-grating",
		PartKind::FacettedGrating(_) => "facetted-grating",
		PartKind::MetaSurface(m) => match m.design {
			MetaDesign::Lens(_) => "meta-lens",
			MetaDesign::Prism(_) => "meta-prism",
			MetaDesign::Axicon(_) => "meta-axicon",
			_ => "meta-surfaces",
		},
		PartKind::Fiber(_) => "fiber",
		PartKind::BraggReflector(_) => "bragg-reflector",
		PartKind::GrinLens(_) => "grin-lens",
		PartKind::LogicGate(g) => match g.kind {
			GateKind::Xor => "xor-gate",
			GateKind::And => "and-gate",
			GateKind::Or => "or-gate",
			_ => "optical-logic",
		},
		PartKind::ZonePlate(_) => "zone-plate",
		_ => "part",
	}
}

/// The icon of `node`'s row; a colour scheme's row shows its gradient instead.
pub(crate) fn tree(project: &Project, node: Node) -> Image {
	image(match node {
		Node::Project => "project",
		Node::Scene => "scene",
		Node::Simulation => "simulation",
		Node::Display => "display",
		Node::Sources | Node::Source(_) => "source",
		Node::Parts => "part",
		Node::Probes => "probe",
		Node::Schemes => "palette",
		Node::Part(i) => part(&project.parts[i].kind),
		Node::Probe(i) => match project.probes[i].detector {
			ProbeDetector::Counting => "counting-detector",
			ProbeDetector::Imaging => "imaging-detector",
			_ => "probe",
		},
		Node::Scheme(_) => return Image::default(),
	})
}

/// Width and height of a colour scheme's strip in the tree (Next's 36 x 10 rectangle).
const STRIP: (u32, u32) = (36, 10);

/// The gradient of `scheme` as the tree row's strip.
pub(crate) fn strip(scheme: &waveduderust::model::ColorScheme) -> Image {
	use waveduderust::unstable::render::gradient::Gradient;
	let colours = Gradient::new(&scheme.stops).interpolate(STRIP.0 as usize);
	let mut buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(STRIP.0, STRIP.1);
	for (i, pixel) in buffer.make_mut_slice().iter_mut().enumerate() {
		let c = colours[i % STRIP.0 as usize];
		*pixel = slint::Rgb8Pixel { r: c.r, g: c.g, b: c.b };
	}
	Image::from_rgb8(buffer)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_lens_row_follows_its_surfaces() {
		assert_eq!(surface(300.0, 200), Surface::Convex);
		assert_eq!(surface(-300.0, 200), Surface::Concave);
		assert_eq!(surface(0.0, 200), Surface::Flat);
		// Next's Example 1 mirror: radius 14401 on 104 px bows under half a pixel
		assert_eq!(surface(14401.0, 104), Surface::Flat);
	}
}
