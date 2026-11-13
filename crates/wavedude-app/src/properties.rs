//! What the property forms of docs/ui/30 show beside the edited object: the read-outs computed from the project and
//! a number's bounds.

use wavedude_app_state::selection::Row;
use wavedude_core::io::{resolve_image_path, schemes};
use wavedude_core::model::{
	self as m, ColorScheme, Field as _, Fields, ItemKind, Layer, Lens, PartKind, Point, Project, Prop, SchemeKind,
	Simulation, Stencil, ZonePlate,
};
use wavedude_core::scene::gate_design::{self, GateDesign};

use crate::App;

/// A number's bounds from the field's declared range; open when it has none or depends on another value.
pub fn bounds_of<T: Fields, V>(p: Prop<T, V>) -> (f64, f64) {
	p.field.range().map_or((-1e9, 1e9), |r| (r.min, r.max))
}

/// A meta surface's read-outs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MetaReadouts {
	pub full_turn: Option<f64>,
	pub zones: u32,
	/// A prism's or an axicon's.
	pub zone_width: Option<f64>,
	/// An axicon's line of focus.
	pub line: Option<f64>,
}

/// A scheme a layer's picker lists.
#[derive(Clone, Debug, PartialEq)]
pub struct SchemeEntry {
	pub name: String,
	/// The project's own.
	pub own: bool,
	pub stops: Vec<m::Stop>,
}

/// A logic gate's design as its form shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GateReadouts {
	/// Core width, coupler gap, coupler length, total length.
	pub layout: [i64; 4],
	pub ports: [Point; 4],
	pub quarter_wave: f64,
	/// The port spacing the design uses: the stored one raised to the design's minimum.
	pub spacing: i64,
}

/// Values the forms show that the edited object alone does not hold.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Readouts {
	pub wavelength: Option<f64>,
	pub nominal: bool,
	pub lens_edge: Option<f64>,
	pub apex: Option<f64>,
	pub gate: Option<GateReadouts>,
	pub zones: Option<(i64, f64)>,
	/// A diffuser's roughness: the face's height from crest to trough and its number of ripples.
	pub roughness: Option<(f64, usize)>,
	/// An interference filter's stack thickness.
	pub stack: Option<f64>,
	/// A simple grating's largest dot diameter.
	pub max_diameter: Option<u32>,
	pub meta: Option<MetaReadouts>,
	pub image_found: bool,
	/// Width, height and size in KB of the embedded image.
	pub image_size: Option<(u32, u32, usize)>,
	/// Scheme names for each layer's picker: (name, project's own).
	pub schemes: [Vec<SchemeEntry>; 2],
	/// The user's scheme each layer's scheme resolves to, when no project scheme takes its name: its name.
	pub mine: [Option<String>; 2],
	/// For the scheme form's Based on picker.
	pub based_on: Vec<String>,
	pub can_remove_stop: bool,
	/// The scheme form's scheme is one of the user's by name.
	pub user_scheme: bool,
}

fn zones(z: &ZonePlate, sim: &Simulation) -> Option<(i64, f64)> {
	let lambda = m::wavelength(z.design_frequency, sim.wave_speed, sim.source_frequency_scale);
	if !lambda.is_finite() || lambda <= 0. {
		return None;
	}
	let f = f64::from(z.focal_length);
	let radius = |n: f64| (n * lambda * f + (n * lambda / 2.).powi(2)).sqrt();
	let r = f64::from(z.diameter) / 2.;
	let a = lambda * lambda / 4.;
	let b = lambda * f;
	// the zone index at the rim
	let n = ((-b + (b * b + 4. * a * r * r).sqrt()) / (2. * a)).floor() as i64;
	let outer = if n >= 1 { radius(n as f64) - radius(n as f64 - 1.) } else { r };
	Some((n + 1, outer))
}

fn edge_thickness(l: &Lens, zones: u32, division: m::ZoneDivision) -> f64 {
	m::lens_geometry::edge_thickness(l.diameter, l.thickness, l.r1, l.r2, zones, division)
}

/// The schemes a layer's picker lists: the project's, the user's, the built-in ones.
fn scheme_names(project: &Project, user: &[ColorScheme], kind: SchemeKind) -> Vec<SchemeEntry> {
	let fits = |k: SchemeKind| k == kind || k == SchemeKind::Unspecified;
	let entry = |s: &ColorScheme, own| SchemeEntry { name: s.name.clone(), own, stops: s.stops.clone() };
	let mut v: Vec<SchemeEntry> =
		project.color_schemes.iter().filter(|s| fits(s.kind)).map(|s| entry(s, true)).collect();
	for s in user.iter().chain(schemes::built_in()).filter(|s| fits(s.kind)) {
		if !v.iter().any(|e| e.name.eq_ignore_ascii_case(&s.name)) {
			v.push(entry(s, false));
		}
	}
	v
}

/// The read-outs of a row.
pub fn readouts(app: &App, row: Row) -> Readouts {
	let p = &app.document.project;
	let sim = &p.simulation;
	let mut r = Readouts::default();
	let nominal = sim.stencil == Stencil::NinePoint;
	match row {
		Row::Scene => {
			r.image_found = !p.scene.image_path.is_empty()
				&& resolve_image_path(app.document.folder(), &p.scene.image_path).is_file();
			r.image_size =
				p.scene.image_data.as_ref().map(|b| (p.scene.width, p.scene.height, (b.len() / 1024).max(1)));
		}
		Row::Display => {
			let user = &app.schemes.user;
			r.schemes = [scheme_names(p, user, SchemeKind::Field), scheme_names(p, user, SchemeKind::Intensity)];
			r.mine = [Layer::Field, Layer::Intensity].map(|l| {
				let current = &p.display.layer(l).color_scheme;
				let own = schemes::find(&p.color_schemes, current).is_some();
				if own { None } else { schemes::find(user, current).map(|s| s.name.clone()) }
			});
		}
		Row::Item(i) => match i.kind {
			ItemKind::Source => {
				let s = &p.sources[i.index];
				r.wavelength = Some(m::wavelength(s.frequency, sim.wave_speed, sim.source_frequency_scale));
				r.nominal = nominal;
			}
			ItemKind::Part => match &p.parts[i.index].kind {
				PartKind::Lens(l) => r.lens_edge = Some(edge_thickness(l, 1, m::ZoneDivision::EqualWidth)),
				PartKind::FresnelLens(f) => r.lens_edge = Some(edge_thickness(&f.lens, f.zones, f.division)),
				PartKind::Diffuser(d) => {
					let profile = d.profile(&p.parts[i.index].base.name, p.seed);
					let (lo, hi) =
						profile.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v)));
					r.roughness = Some((hi - lo, d.components().len()));
				}
				PartKind::Triangle(t) => r.apex = Some(180. - t.angle_a - t.angle_b),
				PartKind::LogicGate(g) => {
					r.wavelength = gate_design::gate_wavelength(g.design_frequency, sim.source_frequency_scale);
					if let Some(d) = GateDesign::for_gate(g, sim) {
						r.gate = Some(GateReadouts {
							layout: [d.core_width, d.coupled, d.coupler_len, d.length],
							ports: gate_design::ports_of(&d, &p.parts[i.index].base),
							quarter_wave: d.wavelength / (4. * d.core_index.sqrt()),
							spacing: d.port_spacing,
						});
					}
				}
				PartKind::ZonePlate(z) => {
					r.wavelength = Some(m::wavelength(z.design_frequency, sim.wave_speed, sim.source_frequency_scale));
					r.zones = zones(z, sim);
				}
				PartKind::InterferenceFilter(f) => r.stack = Some(f.stack_thickness()),
				PartKind::SimpleGrating(g) => r.max_diameter = Some(g.max_diameter()),
				PartKind::MetaSurface(m) => {
					let scale = sim.source_frequency_scale;
					r.wavelength = Some(m::wavelength(m.design_frequency, 1.0, scale));
					r.meta = Some(MetaReadouts {
						full_turn: m.full_turn_height(scale),
						zones: m.zone_count(scale),
						zone_width: m.zone_width(scale),
						line: m.line_length(),
					});
				}
				PartKind::Polygon(_)
				| PartKind::Rectangle(_)
				| PartKind::Ellipse(_)
				| PartKind::Outline(_)
				| PartKind::FacettedGrating(_)
				| PartKind::Fiber(_)
				| PartKind::BraggReflector(_)
				| PartKind::GrinLens(_) => {}
			},
			ItemKind::Scheme => {
				let own = &p.color_schemes[i.index];
				r.based_on = schemes::built_in()
					.iter()
					.chain(&app.schemes.user)
					.chain(p.color_schemes.iter().filter(|s| s.name != own.name))
					.map(|s| s.name.clone())
					.collect();
				r.can_remove_stop = own.stops.len() > 1;
				r.user_scheme = app.is_user_scheme(&own.name);
			}
			ItemKind::Probe => {}
		},
		Row::Project | Row::Simulation | Row::Folder(_) => {}
	}
	r
}

#[cfg(test)]
mod tests {
	use std::time::Instant;

	use wavedude_app_state::AppState;
	use wavedude_app_state::commands::{Command, NewPart};
	use wavedude_app_state::settings::AppDirs;
	use wavedude_core::model::ItemRef;

	use super::*;

	#[test]
	fn gate_and_zone_plate_readouts_match_the_design() {
		let dir = tempfile::tempdir().unwrap();
		let mut app = AppState::start(&AppDirs::under(dir.path()), []).expect("state starts");
		let now = Instant::now();
		app.apply(Command::AddPart { kind: NewPart::XorGate, at: None }, now).expect("the gate is added");
		app.apply(Command::AddPart { kind: NewPart::ZonePlate, at: None }, now).expect("the plate is added");
		let readouts = |i| readouts(&app, Row::Item(ItemRef::new(ItemKind::Part, i)));
		let gate = readouts(0).gate.expect("the default gate has a design");
		assert_eq!(gate.spacing, 60, "the spacing the design uses, not the stored 2");
		assert!((gate.quarter_wave - 5.0).abs() < 0.05, "lambda / (4 sqrt(core index)): {}", gate.quarter_wave);
		assert_eq!(readouts(1).zones.expect("the plate has a wavelength").0, 8, "the zone index at the rim plus one");
	}
}
