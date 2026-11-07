//! Probe cells and the collection of probe readings during a run (spec/30-engine.md, Probes).
//!
//! Probe positions are canvas pixels, clamped into the canvas. The collector lives on the run's driver
//! thread.

use super::types::{ProbeCells, ProbeReadings, SceneData};
use crate::model::{Layer, Measurement, Probe, ProbeKind};

/// Canvas pixels of a line from A to B, both inclusive, A first (Bresenham).
pub fn line_pixels(ax: i32, ay: i32, bx: i32, by: i32) -> Vec<(i32, i32)> {
	let (mut x, mut y) = (ax, ay);
	let dx = (bx - ax).abs();
	let dy = -(by - ay).abs();
	let sx = if ax < bx { 1 } else { -1 };
	let sy = if ay < by { 1 } else { -1 };
	let mut err = dx + dy;
	let mut out = Vec::with_capacity(dx.max(-dy) as usize + 1);
	loop {
		out.push((x, y));
		if x == bx && y == by {
			return out;
		}
		let e2 = 2 * err;
		if e2 >= dy {
			err += dy;
			x += sx;
		}
		if e2 <= dx {
			err += dx;
			y += sy;
		}
	}
}

/// The probe's ends clamped into a `w` x `h` canvas; B equals A for a point probe.
pub fn clamped_ends(probe: &Probe, w: u32, h: u32) -> ((i32, i32), (i32, i32)) {
	let clamp = |x: i32, y: i32| (x.clamp(0, w as i32 - 1), y.clamp(0, h as i32 - 1));
	let a = clamp(probe.ax, probe.ay);
	let b = if probe.kind == ProbeKind::Line { clamp(probe.bx, probe.by) } else { a };
	(a, b)
}

/// Euclidean length in canvas pixels of the clamped A-B line; 0 for a point probe.
pub fn line_length(probe: &Probe, w: u32, h: u32) -> f64 {
	let ((ax, ay), (bx, by)) = clamped_ends(probe, w, h);
	((bx - ax) as f64).hypot((by - ay) as f64)
}

/// Distance in pixels of each of `n` samples along a line of `length` (the CSV header).
pub fn sample_distances(n: usize, length: f64) -> Vec<f64> {
	(0..n).map(|k| if n > 1 { k as f64 * length / (n - 1) as f64 } else { 0.0 }).collect()
}

/// The canvas pixels `probe` reads in `scene`, and the layer: the intensity for any
/// measurement but Amplitude.
pub fn probe_cells(probe: &Probe, scene: &SceneData) -> ProbeCells {
	let ((ax, ay), (bx, by)) = clamped_ends(probe, scene.canvas_width, scene.canvas_height);
	let index = |(x, y): (i32, i32)| (x as u32, y as u32);
	let cells = match probe.kind {
		ProbeKind::Point => vec![index((ax, ay))],
		ProbeKind::Line => line_pixels(ax, ay, bx, by).into_iter().map(index).collect(),
	};
	ProbeCells { layer: if probe.uses_intensity() { Layer::Intensity } else { Layer::Field }, cells }
}

/// Equal-length profiles of a line probe in one buffer, the oldest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profiles {
	values: Vec<f32>,
	/// Samples per profile; 0 while empty.
	width: usize,
}

impl Profiles {
	pub fn len(&self) -> usize {
		self.values.len().checked_div(self.width).unwrap_or(0)
	}

	pub fn is_empty(&self) -> bool {
		self.values.is_empty()
	}

	pub fn get(&self, k: usize) -> Option<&[f32]> {
		(k < self.len()).then(|| &self.values[k * self.width..(k + 1) * self.width])
	}

	pub fn first(&self) -> Option<&[f32]> {
		self.get(0)
	}

	pub fn last(&self) -> Option<&[f32]> {
		self.len().checked_sub(1).and_then(|k| self.get(k))
	}

	pub fn iter(&self) -> impl ExactSizeIterator<Item = &[f32]> {
		(0..self.len()).map(|k| &self.values[k * self.width..(k + 1) * self.width])
	}

	/// Appends a profile of the same length as the others.
	pub fn push(&mut self, profile: impl ExactSizeIterator<Item = f32>) {
		if self.is_empty() {
			self.width = profile.len();
		}
		assert_eq!(profile.len(), self.width, "every profile of a probe has its samples");
		self.values.extend(profile);
	}

	/// Moves `other`'s profiles behind these.
	pub fn append(&mut self, other: &mut Profiles) {
		if self.is_empty() {
			self.width = other.width;
		}
		assert!(other.is_empty() || other.width == self.width, "every profile of a probe has its samples");
		self.values.append(&mut other.values);
	}
}

impl From<Vec<Vec<f32>>> for Profiles {
	fn from(rows: Vec<Vec<f32>>) -> Self {
		let mut p = Self::default();
		rows.into_iter().for_each(|r| p.push(r.into_iter()));
		p
	}
}

/// What one probe recorded: a point probe's series (one value per step, value i after step i + 1)
/// or a line probe's profiles (one per timeline frame).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProbeTrace {
	pub series: Vec<f32>,
	pub profiles: Profiles,
	/// Line length in canvas pixels; 0 for a point probe.
	pub length: f64,
	/// Steps done at the last recording.
	pub steps_sampled: u64,
}

#[derive(Clone, Debug)]
struct Spec {
	line: bool,
	integrated: bool,
	start_after: u64,
}

/// Turns per-step engine readings into probe traces. Call [`record_step`](Self::record_step)
/// after every step and [`record_frame`](Self::record_frame) after the steps that close a timeline
/// frame (step index `i` with `i % VisualizeEvery == 0`), with the same readings.
#[derive(Clone, Debug)]
pub struct ProbeCollector {
	specs: Vec<Spec>,
	cells: Vec<ProbeCells>,
	pub traces: Vec<ProbeTrace>,
	point_totals: Vec<f64>,
	line_totals: Vec<Vec<f64>>,
}

impl ProbeCollector {
	/// A collector for `probes` (the active ones, in marker order) on `scene`.
	pub fn new(probes: &[Probe], scene: &SceneData) -> Self {
		let cells: Vec<ProbeCells> = probes.iter().map(|p| probe_cells(p, scene)).collect();
		let specs = probes
			.iter()
			.map(|p| Spec {
				line: p.kind == ProbeKind::Line,
				integrated: p.measurement == Measurement::IntegratedValue,
				start_after: p.start_after as u64,
			})
			.collect();
		let traces = probes
			.iter()
			.map(|p| ProbeTrace {
				length: line_length(p, scene.canvas_width, scene.canvas_height),
				..ProbeTrace::default()
			})
			.collect();
		let line_totals = cells
			.iter()
			.zip(probes)
			.map(|(c, p)| if p.kind == ProbeKind::Line { vec![0.0; c.cells.len()] } else { Vec::new() })
			.collect();
		Self { specs, point_totals: vec![0.0; probes.len()], line_totals, cells, traces }
	}

	/// The cells to hand to the engine's `set_probes`.
	pub fn cells(&self) -> &[ProbeCells] {
		&self.cells
	}

	/// Records the readings after a step; `steps_done` counts that step.
	/// Integrated values stay 0 while `steps_done < start_after`.
	pub fn record_step(&mut self, readings: &ProbeReadings, steps_done: u64) {
		for (p, spec) in self.specs.iter().enumerate() {
			let values = readings.probe(p);
			let started = steps_done >= spec.start_after;
			match (spec.line, spec.integrated) {
				(false, false) => self.traces[p].series.push(values[0]),
				(false, true) => {
					self.point_totals[p] = if started { self.point_totals[p] + values[0] as f64 } else { 0.0 };
					self.traces[p].series.push(self.point_totals[p] as f32);
				}
				(true, true) if started => {
					for (t, v) in self.line_totals[p].iter_mut().zip(values) {
						*t += *v as f64;
					}
				}
				(true, _) => continue,
			}
			self.traces[p].steps_sampled = steps_done;
		}
	}

	/// Records the line profiles of a timeline frame: the live values, or the totals of an integrated probe.
	pub fn record_frame(&mut self, readings: &ProbeReadings, steps_done: u64) {
		for (p, spec) in self.specs.iter().enumerate() {
			if !spec.line {
				continue;
			}
			let profiles = &mut self.traces[p].profiles;
			if spec.integrated {
				profiles.push(self.line_totals[p].iter().map(|&t| t as f32));
			} else {
				profiles.push(readings.probe(p).iter().copied());
			}
			self.traces[p].steps_sampled = steps_done;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::Scene;

	fn scene() -> SceneData {
		SceneData::blank(20, 10, &Scene { damping_edge: -3, ..Scene::default() })
	}

	#[test]
	fn bresenham_is_inclusive_and_starts_at_a() {
		assert_eq!(line_pixels(0, 0, 3, 1), vec![(0, 0), (1, 0), (2, 1), (3, 1)]);
		assert_eq!(line_pixels(3, 1, 0, 0).first(), Some(&(3, 1)));
		assert_eq!(line_pixels(2, 2, 2, 2), vec![(2, 2)]);
		assert_eq!(line_pixels(1, 5, 1, 1).len(), 5);
	}

	#[test]
	fn cells_are_clamped() {
		let s = scene();
		let p = Probe { ax: -5, ay: 50, ..Probe::default() };
		let c = probe_cells(&p, &s);
		assert_eq!(c.cells, vec![(0, 9)]);
		assert_eq!(c.layer, Layer::Field);
		let line = Probe {
			kind: ProbeKind::Line,
			ax: 0,
			ay: 0,
			bx: 30,
			by: 0,
			measurement: Measurement::Value,
			..Probe::default()
		};
		let c = probe_cells(&line, &s);
		assert_eq!(c.cells.len(), 20);
		assert_eq!(c.layer, Layer::Intensity);
		assert_eq!(line_length(&line, 20, 10), 19.0);
		assert_eq!(sample_distances(3, 19.0), vec![0.0, 9.5, 19.0]);
		assert_eq!(sample_distances(1, 0.0), vec![0.0]);
	}

	#[test]
	fn collection_rules() {
		let s = scene();
		let probes = [
			Probe { measurement: Measurement::Amplitude, ..Probe::default() },
			Probe { measurement: Measurement::IntegratedValue, start_after: 3, ..Probe::default() },
			Probe { kind: ProbeKind::Line, bx: 2, measurement: Measurement::Value, ..Probe::default() },
			Probe {
				kind: ProbeKind::Line,
				bx: 2,
				measurement: Measurement::IntegratedValue,
				start_after: 2,
				..Probe::default()
			},
		];
		let mut c = ProbeCollector::new(&probes, &s);
		for step in 1..=4u64 {
			let v = step as f32;
			let r = ProbeReadings::from_probes([&[v][..], &[v], &[v; 3], &[v; 3]]);
			c.record_step(&r, step);
			if (step - 1) % 2 == 0 {
				c.record_frame(&r, step);
			}
		}
		assert_eq!(c.traces[0].series, vec![1.0, 2.0, 3.0, 4.0]);
		assert_eq!(c.traces[1].series, vec![0.0, 0.0, 3.0, 7.0]);
		assert_eq!(c.traces[2].profiles, vec![vec![1.0; 3], vec![3.0; 3]].into());
		assert_eq!(c.traces[3].profiles, vec![vec![0.0; 3], vec![5.0; 3]].into());
		assert_eq!(c.traces[3].length, 2.0);
	}
}
