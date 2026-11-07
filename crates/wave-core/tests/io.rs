//! Project files against the shipped examples and the Wave Dude 2D fixtures.

use std::path::{Path, PathBuf};

use wave_core::io::{self, IoError, legacy, wdproj, wdr};
use wave_core::model::*;

fn fixtures() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn examples() -> Vec<PathBuf> {
	let mut v: Vec<PathBuf> = std::fs::read_dir(fixtures().join("examples"))
		.unwrap()
		.map(|e| e.unwrap().path())
		.filter(|p| p.extension().is_some_and(|e| e == "wdproj"))
		.collect();
	v.sort();
	assert_eq!(v.len(), 24);
	v
}

/// `b` equals `a` except for source frequencies, which may differ by rounding of the 9-point rescale.
fn assert_same(a: &Project, b: &Project, what: &str) {
	assert_eq!(a.sources.len(), b.sources.len(), "{what}");
	let mut b = b.clone();
	for (x, y) in a.sources.iter().zip(&mut b.sources) {
		assert!(
			(x.frequency - y.frequency).abs() <= 1e-9 * x.frequency.abs(),
			"{what}: {} vs {}",
			x.frequency,
			y.frequency
		);
		y.frequency = x.frequency;
	}
	for (x, y) in format!("{a:#?}").lines().zip(format!("{b:#?}").lines()) {
		assert_eq!(x, y, "{what}");
	}
	assert_eq!(*a, b, "{what}");
}

#[test]
fn every_example_loads_round_trips_and_validates() {
	for path in examples() {
		let name = path.file_name().unwrap().to_string_lossy().into_owned();
		let p = io::load_project(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
		assert!(!p.description.is_empty(), "{name}");
		let text = wdproj::write_string(&p, None);
		assert_eq!(wdproj::validate(&text), Vec::<String>::new(), "{name}");
		let back = wdproj::read_str(&text).unwrap();
		assert_same(&p, &back, &name);
		let native = wdr::read_bytes(&wdr::write_bytes(&p, None).unwrap()).unwrap().project;
		assert_eq!(native, p, "{name} through .wdr");
	}
}

/// Every example comes back byte for byte after its DOCTYPE, except where a file was edited by hand:
/// Example 6 omits a probe's `BoxX` equal to its `AX` (read back the same, written out here).
#[test]
fn examples_are_rewritten_as_shipped() {
	let mut differing = Vec::new();
	for path in examples() {
		let name = path.file_name().unwrap().to_string_lossy().into_owned();
		let original = std::fs::read_to_string(&path).unwrap();
		let p = wdproj::read_str(&original).unwrap();
		let text = wdproj::write_string(&p, None);
		let (head, body) = text.split_once("]>\n").unwrap();
		assert!(head.starts_with("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE Project ["));
		let original_body = original.split_once('\n').unwrap().1;
		for (a, b) in body.lines().zip(original_body.lines()) {
			if a != b {
				differing.push(format!("{name}:\n  {a}\n  {b}"));
			}
		}
	}
	assert_eq!(differing.len(), 1, "{}", differing.join("\n"));
	assert!(differing[0].starts_with("Example06"), "{}", differing[0]);
}

#[test]
fn nine_point_example_is_rescaled() {
	let p = io::load_project(&fixtures().join("examples/Example09_Laplacian_dispersion.wdproj")).unwrap();
	assert_eq!(p.simulation.stencil, Stencil::NinePoint);
	assert_eq!(p.sources[0].frequency, 30.0);
	assert!((p.simulation.source_frequency_scale - 2.0 / io::nine_point_speed()).abs() < 1e-12);
}

#[test]
fn saved_files_load_back() {
	let dir = tempfile::tempdir().unwrap();
	let p = io::load_project(&fixtures().join("examples/Example23_Fibres.wdproj")).unwrap();
	for ext in ["wdproj", "wdr"] {
		let path = dir.path().join(format!("p.{ext}"));
		io::save_project(&p, &path).unwrap();
		assert_eq!(io::load_project(&path).unwrap(), p, "{ext}");
	}
	assert!(matches!(io::save_project(&p, &dir.path().join("p.txt")), Err(IoError::UnknownType(_))));
}

#[test]
fn malformed_project_names_the_problem_and_quotes_the_file() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("bad.wdproj");
	let text = "<?xml version=\"1.0\"?>\n<Project Version=\"3\" Seed=\"1\">\n\t<Source Name=\"a\" X=\"1\" X=\"2\" Y=\"0\" />\n</Project>";
	std::fs::write(&path, text).unwrap();
	let Err(IoError::Unreadable(e)) = io::load_project(&path) else { panic!("expected an unreadable file") };
	assert!(e.error.message.contains("attribute 'X' is repeated"), "{}", e.error.message);
	assert_eq!(e.error.at.map(|a| a.0), Some(3));
	assert_eq!(e.text, text);
	assert!(e.report().ends_with(text));
	let missing = io::load_project(&dir.path().join("missing.wdproj")).unwrap_err();
	assert!(matches!(missing, IoError::Io { .. }), "{missing}");
}

#[test]
fn undecodable_image_names_the_file() {
	let text = "<Project Seed=\"1\"><Scene Mode=\"Image\"><ImageData>AAAA</ImageData></Scene></Project>";
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("img.wdproj");
	std::fs::write(&path, text).unwrap();
	let err = io::load_project(&path).unwrap_err();
	assert!(matches!(&err, IoError::Image { path: p, .. } if p == &path), "{err}");
}

fn legacy_fixture(name: &str) -> PathBuf {
	fixtures().join("legacy").join(name).join("configuration.json")
}

#[test]
fn legacy_example_12() {
	let path = legacy_fixture("Example12_Dispersion");
	let i = legacy::import(&path).unwrap();
	assert_eq!(i.name, "Example12_Dispersion");
	assert!(!i.project.description.is_empty());
	assert_eq!(i.notes, Vec::<String>::new());
	let p = &i.project;
	assert!(Path::new(&p.scene.image_path).is_absolute());
	assert_eq!(p.scene.image, "frequency_race_track (2).png");
	assert_eq!(p.scene.mode, SceneMode::Image);
	assert_ne!(p.scene.image_size, (0, 0));
	assert_eq!((p.simulation.steps, p.simulation.visualize_every), (15000, 2));
	assert_eq!(p.simulation.stencil, Stencil::NinePoint);
	assert_eq!((p.simulation.wave_speed, p.simulation.time_decay), (1.0, 10.0));
	assert!(p.display.field.preview && p.display.intensity.preview);
	assert_eq!((p.display.video.fps, p.display.video.quality), (60, 70));
	let probe = &p.probes[0];
	assert_eq!(probe.kind, ProbeKind::Line);
	assert_eq!((probe.ax, probe.ay, probe.bx, probe.by), (1713, 2, 1719, 1073));
	assert_eq!((probe.box_x, probe.box_y), (1, 426));
	let mut back = wdproj::read_str(&wdproj::write_string(p, None)).unwrap();
	back.scene.image_size = p.scene.image_size;
	let mut expected = p.clone();
	for q in expected.probes.iter_mut().filter(|q| q.kind == ProbeKind::Point) {
		(q.bx, q.by) = (0, 0);
	}
	assert_same(&expected, &back, "Example 12 through XML");
}

#[test]
fn legacy_example_5() {
	let i = legacy::import(&legacy_fixture("Example5_Small_incoherent_source")).unwrap();
	let p = &i.project;
	assert_eq!(p.scene.mode, SceneMode::Canvas);
	assert!(p.scene.image_data.is_none());
	assert!(i.notes.iter().any(|n| n.contains("was not found")), "{:?}", i.notes);
	let s = &p.sources[0];
	assert_eq!((s.mode, s.pulse_width, s.off_width, s.detach_after), (SourceMode::Pulsed, 5, 20, Some(2000)));
	assert!(p.probes.iter().all(|q| q.active));
}

#[test]
fn legacy_example_11() {
	let i = legacy::import(&legacy_fixture("Example11_Fluorescent_emitter")).unwrap();
	let s = &i.project.sources[0];
	assert!(s.enabled);
	assert_eq!((s.x, s.y), (100.0, 100.0));
	assert!(s.array.is_some());
	assert!(i.notes.iter().any(|n| n.contains("no picked position")), "{:?}", i.notes);
}

#[test]
fn legacy_example_3_and_newer_format() {
	let i = legacy::import(&legacy_fixture("Example3_Spherical_mirror")).unwrap();
	assert_eq!(i.project.sources.len(), 1);
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("configuration.json");
	std::fs::write(&path, "{\"format_version\": 2}").unwrap();
	let Err(IoError::Unreadable(e)) = io::load_project(&path) else { panic!("a newer format is refused") };
	assert!(e.error.message.contains("newer"));
}

/// The `.wdr` JSON's keys, and its string values, of one object of every kind: a renamed model field or enum
/// member changes the file format and this snapshot with it.
#[test]
fn wdr_json_names_are_stable() {
	fn walk(v: &serde_json::Value, path: &str, out: &mut std::collections::BTreeSet<String>) {
		match v {
			serde_json::Value::Object(m) => m.iter().for_each(|(k, v)| walk(v, &format!("{path}.{k}"), out)),
			serde_json::Value::Array(a) => a.iter().for_each(|v| walk(v, &format!("{path}[]"), out)),
			serde_json::Value::String(s) => _ = out.insert(format!("{path} = {s:?}")),
			_ => _ = out.insert(path.to_owned()),
		}
	}
	let mut p = Project::default();
	let s = p.add_source();
	p.sources[s].detach_after = Some(5);
	p.sources[s].array = Some(SourceArray::default());
	for kind in [
		PartKind::Lens(Lens::default()),
		PartKind::Polygon(Polygon::default()),
		PartKind::Rectangle(Rectangle::default()),
		PartKind::Ellipse(Ellipse::default()),
		PartKind::Triangle(Triangle::default()),
		PartKind::Outline(Outline { points: vec![Point::new(1, 2)], ..Outline::default() }),
		PartKind::Fiber(Fiber { nodes: vec![FiberNode::default()], ..Fiber::default() }),
		PartKind::BraggReflector(BraggReflector::default()),
		PartKind::GrinLens(GrinLens::default()),
		PartKind::LogicGate(LogicGate::default()),
		PartKind::ZonePlate(ZonePlate::default()),
	] {
		p.add_part(kind);
	}
	p.add_probe();
	p.add_new_color_scheme();
	let mut names = std::collections::BTreeSet::new();
	walk(&serde_json::to_value(&p).unwrap(), "", &mut names);
	insta::assert_snapshot!(names.into_iter().collect::<Vec<_>>().join("\n"));
}
