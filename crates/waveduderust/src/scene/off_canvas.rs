//! The objects a run leaves out or clamps because they lie off the canvas.

use crate::engine::emitters::{drives_canvas, elements_of};
use crate::model::{ProbeKind, Project};
use crate::scene::painter::part_on_canvas;

/// An object of the project off the canvas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OffCanvas {
	/// An enabled source with `outside` of its `of` elements driving no cell; they are left out.
	Source { name: String, outside: usize, of: usize },
	/// An enabled part with no shape on the canvas.
	Part(String),
	/// An active probe with an end off the canvas: it reads at the edge.
	Probe(String),
}

/// The objects of the normalised `project` off its canvas, sources then parts then probes, each in project order.
pub fn off_canvas(project: &Project) -> Vec<OffCanvas> {
	let (w, h) = project.canvas_size();
	let sources = project.sources.iter().filter(|s| s.enabled && project.scene.object_sources).filter_map(|s| {
		let elements = elements_of(s, project.seed);
		let outside = elements.iter().filter(|e| !drives_canvas(e, w, h)).count();
		(outside > 0).then(|| OffCanvas::Source { name: s.name.clone(), outside, of: elements.len() })
	});
	let parts = project
		.parts
		.iter()
		.filter(|p| p.base.enabled && !part_on_canvas(p, &project.simulation, project.seed, w, h))
		.map(|p| OffCanvas::Part(p.base.name.clone()));
	let inside = |x: i32, y: i32| (0..w as i64).contains(&i64::from(x)) && (0..h as i64).contains(&i64::from(y));
	let probes = project
		.probes
		.iter()
		.filter(|p| p.active && !(inside(p.ax, p.ay) && (p.kind != ProbeKind::Line || inside(p.bx, p.by))))
		.map(|p| OffCanvas::Probe(p.name.clone()));
	sources.chain(parts).chain(probes).collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::normalize::Normalize;
	use crate::model::{PartKind, Probe, ProbeDetector, Rectangle, SourceArray, SourceId};

	#[test]
	fn objects_off_the_canvas_are_named_and_the_rest_not() {
		let mut p = Project::default();
		let (w, h) = p.canvas_size();
		let (w, h) = (w as i32, h as i32);
		p.add_source();
		let out = p.add_source_at(w + 50, 10);
		let edge = p.add_source_at(w - 30, 10);
		p.source_mut(edge).unwrap().array = Some(SourceArray { size_x: 4, spacing_x: 20, ..SourceArray::default() });
		let disabled = p.add_source_at(-50, 10);
		p.source_mut(disabled).unwrap().enabled = false;
		p.add_part(PartKind::Rectangle(Rectangle::default()));
		let far = p.add_part_at(PartKind::Rectangle(Rectangle::default()), w + 500, 10);
		let hidden = p.add_part_at(PartKind::Rectangle(Rectangle::default()), w + 500, 10);
		p.part_mut(hidden).unwrap().base.enabled = false;
		let probe = |name: &str, kind, a: (i32, i32), b: (i32, i32), active| Probe {
			name: name.into(),
			kind,
			ax: a.0,
			ay: a.1,
			bx: b.0,
			by: b.1,
			active,
			..Probe::default()
		};
		p.probes = vec![
			probe("Inside", ProbeKind::Line, (1, 1), (w - 1, h - 1), true),
			probe("Line end", ProbeKind::Line, (1, 1), (w, 1), true),
			probe("Point", ProbeKind::Point, (-1, 3), (5, 5), true),
			probe("Inactive", ProbeKind::Point, (-1, 3), (5, 5), false),
			Probe { detector: ProbeDetector::Counting, ..probe("Detector", ProbeKind::Point, (1, 1), (-4, 1), true) },
		];
		p.normalize();
		let found = off_canvas(&p);
		let source = |i: SourceId| p.source(i).unwrap().name.clone();
		assert_eq!(found[0], OffCanvas::Source { name: source(out), outside: 1, of: 1 });
		let OffCanvas::Source { name, outside, of } = &found[1] else { panic!("{found:?}") };
		assert_eq!((name, *of), (&source(edge), 4));
		assert!((1..4).contains(outside), "{outside} of the array's elements outside");
		assert_eq!(
			found[2..],
			[
				OffCanvas::Part(p.part_mut(far).unwrap().base.name.clone()),
				OffCanvas::Probe("Line end".into()),
				OffCanvas::Probe("Point".into()),
				OffCanvas::Probe("Detector".into()),
			]
		);
		p.scene.object_sources = false;
		assert!(!off_canvas(&p).iter().any(|o| matches!(o, OffCanvas::Source { .. })), "no object sources run");
	}
}
