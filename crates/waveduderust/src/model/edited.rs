//! What an edit carries along: an edited object makes its fields agree, knowing what it was before. Applied to edits
//! made in the editor (forms, handles), never on load, where [`super::normalize`] repairs a file instead.

use super::names::mirror_name;
use super::{Ellipse, FresnelLens, Part, PartKind, Probe, Rectangle, TriangleType};

/// An object's rules for an edit.
pub trait Edited {
	/// Makes the fields agree after an edit that turned `before` into `self`.
	fn edited(&mut self, before: &Self);
}

impl Edited for Probe {
	/// A graph box on point A moves with it; a detector is a line of the integrated intensity; a layer takes only its
	/// own measurements and the field a linear axis.
	fn edited(&mut self, before: &Self) {
		if before.box_attached() {
			(self.box_x, self.box_y) = (self.ax, self.ay);
		}
		self.coerce();
	}
}

impl Edited for Part {
	/// A linked rectangle or ellipse keeps both sides equal, the edited one leading; a triangle's type sets its
	/// angles, an isosceles one copies angle A to B; a lens's name follows its mirror box (Lens 1, Mirror 1).
	fn edited(&mut self, before: &Self) {
		match (&mut self.kind, &before.kind) {
			(
				PartKind::Rectangle(Rectangle { length, width, linked: true, .. })
				| PartKind::Ellipse(Ellipse { length, width, linked: true, .. }),
				PartKind::Rectangle(Rectangle { width: was, .. }) | PartKind::Ellipse(Ellipse { width: was, .. }),
			) => {
				if width == was {
					*width = *length;
				} else {
					*length = *width;
				}
			}
			(PartKind::Triangle(t), PartKind::Triangle(was)) => {
				if t.kind != was.kind {
					(t.angle_a, t.angle_b) = t.effective_angles();
				} else if t.kind == TriangleType::Isosceles {
					t.angle_b = t.angle_a;
				}
			}
			_ => {}
		}
		if let (Some(mirror), Some(was)) = (lens_mirror(&self.kind), lens_mirror(&before.kind))
			&& mirror != was
		{
			self.base.name = mirror_name(&self.base.name, mirror);
		}
	}
}

/// A lens's or Fresnel lens's mirror flag.
fn lens_mirror(kind: &PartKind) -> Option<bool> {
	match kind {
		PartKind::Lens(lens) | PartKind::FresnelLens(FresnelLens { lens, .. }) => Some(lens.material.mirror),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::{Lens, Project, Triangle};

	fn edit<T: Edited + Clone>(of: &mut T, change: impl FnOnce(&mut T)) {
		let before = of.clone();
		change(of);
		of.edited(&before);
	}

	#[test]
	fn a_lens_is_renamed_by_its_mirror_box_once() {
		let mut p = Project::default();
		p.add_part(PartKind::Lens(Lens::default()));
		let part = &mut p.parts[0];
		let mirror = |part: &mut Part, on| {
			edit(part, |p| {
				if let PartKind::Lens(l) = &mut p.kind {
					l.material.mirror = on;
				}
			});
		};
		mirror(part, true);
		assert_eq!(part.base.name, "Mirror 1");
		mirror(part, true);
		assert_eq!(part.base.name, "Mirror 1");
		mirror(part, false);
		assert_eq!(part.base.name, "Lens 1");
	}

	#[test]
	fn linked_sides_follow_the_edited_one_and_a_preset_sets_the_angles() {
		let mut part = Part::new(PartKind::Rectangle(Rectangle { linked: false, ..Rectangle::default() }), 0, 0);
		edit(&mut part, |p| {
			if let PartKind::Rectangle(r) = &mut p.kind {
				(r.length, r.linked) = (80, true);
			}
		});
		assert!(matches!(part.kind, PartKind::Rectangle(Rectangle { width: 80, .. })));
		edit(&mut part, |p| {
			if let PartKind::Rectangle(r) = &mut p.kind {
				r.width = 30;
			}
		});
		assert!(matches!(part.kind, PartKind::Rectangle(Rectangle { length: 30, .. })));
		let mut tri = Part::new(PartKind::Triangle(Triangle::default()), 0, 0);
		edit(&mut tri, |p| {
			if let PartKind::Triangle(t) = &mut p.kind {
				t.kind = TriangleType::RightIsosceles;
			}
		});
		assert!(matches!(tri.kind, PartKind::Triangle(Triangle { angle_a: 45.0, angle_b: 45.0, .. })));
	}

	#[test]
	fn a_probe_box_on_a_follows_it_and_a_detector_is_an_intensity_line() {
		use crate::model::{Layer, ProbeDetector, ProbeKind};
		let mut probe = Probe::default();
		(probe.box_x, probe.box_y) = (probe.ax, probe.ay);
		edit(&mut probe, |p| (p.ax, p.detector) = (40, ProbeDetector::Counting));
		assert_eq!((probe.box_x, probe.kind, probe.layer), (40, ProbeKind::Line, Layer::Intensity));
	}
}
