//! Probe cells and the collection of probe readings during a run (spec/30-engine.md, Probes).
//!
//! Probe positions are canvas pixels, clamped into the canvas. The collector lives on the run's driver
//! thread.

use super::poisson::PoissonProcess;
use super::types::{ProbeCells, ProbeReadings, SceneData};
use crate::model::{Layer, Measurement, Probe, ProbeDetector, ProbeKind};
use crate::rng::stable_hash;

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

/// What one probe recorded: a point probe's or a counting detector's series (one value per step, value i after
/// step i + 1) or a line probe's or an imaging detector's profiles (one per timeline frame).
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
	detector: ProbeDetector,
	sensitivity: f64,
	bin_size: usize,
}

/// A detector's Poisson processes: a counting detector's one, an imaging detector's one per pixel of its line.
#[derive(Clone, Debug)]
struct Detections {
	processes: Vec<PoissonProcess>,
	/// A counting detector's count at its last step.
	counted: u32,
}

/// The random stream of a detector's process: the probe's name hash, with the pixel's index along the line for
/// an imaging detector's processes.
fn stream(name: &str, pixel: usize) -> u64 {
	(u64::from(stable_hash(name)) << 24) ^ pixel as u64
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
	/// Per probe; empty processes for a standard probe.
	detections: Vec<Detections>,
}

impl ProbeCollector {
	/// A collector for `probes` (the active ones, in marker order) on `scene`; `seed` is the project's, which a
	/// detector's detections are drawn from.
	pub fn new(probes: &[Probe], scene: &SceneData, seed: i32) -> Self {
		let cells: Vec<ProbeCells> = probes.iter().map(|p| probe_cells(p, scene)).collect();
		let specs = probes
			.iter()
			.map(|p| Spec {
				line: p.kind == ProbeKind::Line,
				integrated: p.measurement == Measurement::IntegratedValue,
				start_after: p.start_after as u64,
				detector: p.detector,
				sensitivity: p.sensitivity,
				bin_size: p.bin_size.max(1) as usize,
			})
			.collect();
		let detections = probes
			.iter()
			.zip(&cells)
			.map(|(p, c)| {
				let pixels = match p.detector {
					ProbeDetector::Standard => 0,
					ProbeDetector::Counting => 1,
					ProbeDetector::Imaging => c.cells.len(),
				};
				let seed = seed as u32 as u64;
				Detections {
					processes: (0..pixels).map(|k| PoissonProcess::new(seed, stream(&p.name, k))).collect(),
					counted: 0,
				}
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
		Self { specs, point_totals: vec![0.0; probes.len()], line_totals, cells, traces, detections }
	}

	/// The cells to hand to the engine's `set_probes`.
	pub fn cells(&self) -> &[ProbeCells] {
		&self.cells
	}

	/// Records the readings after a step; `steps_done` counts that step.
	/// Integrated values stay 0 while `steps_done < start_after`.
	pub fn record_step(&mut self, readings: &ProbeReadings, steps_done: u64) {
		for p in 0..self.specs.len() {
			let spec = self.specs[p].clone();
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
					if spec.detector == ProbeDetector::Counting {
						self.count_detections(p);
					}
				}
				(true, true) if spec.detector == ProbeDetector::Counting => self.traces[p].series.push(0.0),
				(true, _) => continue,
			}
			self.traces[p].steps_sampled = steps_done;
		}
	}

	/// A counting detector's detections in this step: the line's integrated intensity, summed over its pixels and
	/// times the sensitivity, is the cumulative expectation; the events of the probe's Poisson process up to it,
	/// less those counted before, go into the series, one value a step.
	fn count_detections(&mut self, p: usize) {
		let expectation = self.line_totals[p].iter().sum::<f64>() * self.specs[p].sensitivity;
		let d = &mut self.detections[p];
		let up_to = d.processes[0].count_up_to(expectation);
		self.traces[p].series.push((up_to - d.counted) as f32);
		d.counted = up_to;
	}

	/// An imaging detector's image at this frame: at every pixel of the line the events of that pixel's process up
	/// to its integrated intensity times the sensitivity, which only grows, so the image builds up over the run.
	/// With a bin of more than a pixel the detections of a bin's pixels are added up and every pixel of the bin
	/// shows the total, a histogram of flat bars over the line; the last bin may be shorter.
	fn image_detections(&mut self, p: usize) -> Vec<f32> {
		let spec = &self.specs[p];
		let processes = &mut self.detections[p].processes;
		let mut counts = vec![0f32; self.line_totals[p].len()];
		for start in (0..counts.len()).step_by(spec.bin_size) {
			let end = (start + spec.bin_size).min(counts.len());
			let total: u32 =
				(start..end).map(|k| processes[k].count_up_to(self.line_totals[p][k] * spec.sensitivity)).sum();
			counts[start..end].fill(total as f32);
		}
		counts
	}

	/// Records the line profiles of a timeline frame: the live values, or the totals of an integrated probe.
	pub fn record_frame(&mut self, readings: &ProbeReadings, steps_done: u64) {
		for p in 0..self.specs.len() {
			let spec = self.specs[p].clone();
			if !spec.line || spec.detector == ProbeDetector::Counting {
				continue;
			}
			if spec.detector == ProbeDetector::Imaging {
				let image = self.image_detections(p);
				self.traces[p].profiles.push(image.into_iter());
			} else if spec.integrated {
				self.traces[p].profiles.push(self.line_totals[p].iter().map(|&t| t as f32));
			} else {
				self.traces[p].profiles.push(readings.probe(p).iter().copied());
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
		let mut c = ProbeCollector::new(&probes, &s, 0);
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

	/// A counting detector gives one count a step, the sum over the run the process's count at the line's
	/// integrated intensity times the sensitivity; an imaging detector a profile per frame that only grows, in
	/// flat bins; both repeat for the same seed and name and differ for another seed.
	#[test]
	fn detectors_draw_detections() {
		let s = scene();
		let line = Probe { kind: ProbeKind::Line, bx: 5, sensitivity: 3.0, ..Probe::default() };
		let probes = [
			Probe { name: "c".into(), detector: ProbeDetector::Counting, ..line.clone() },
			Probe { name: "i".into(), detector: ProbeDetector::Imaging, bin_size: 4, ..line },
		];
		let run = |seed: i32| {
			let mut probes = probes.clone();
			probes.iter_mut().for_each(crate::model::normalize::Normalize::normalize);
			let mut c = ProbeCollector::new(&probes, &s, seed);
			for step in 1..=40u64 {
				let r = ProbeReadings::from_probes([&[0.5f32; 6][..], &[0.5f32; 6]]);
				c.record_step(&r, step);
				if (step - 1) % 10 == 0 {
					c.record_frame(&r, step);
				}
			}
			c
		};
		let c = run(1);
		assert_eq!(c.traces[0].series.len(), 40);
		let total: f32 = c.traces[0].series.iter().sum();
		let expectation = 40.0 * 6.0 * 0.5 * 3.0;
		assert_eq!(total, PoissonProcess::new(1, stream("c", 0)).count_up_to(expectation) as f32);
		assert!(c.traces[0].profiles.is_empty(), "a counting detector is a series");
		assert_eq!(c.traces[1].profiles.len(), 4);
		let last = c.traces[1].profiles.last().unwrap().to_vec();
		assert!(last[..4].iter().all(|&v| v == last[0]) && last[4] == last[5], "bins of 4 and 2: {last:?}");
		for (a, b) in c.traces[1].profiles.iter().zip(c.traces[1].profiles.iter().skip(1)) {
			assert!(a.iter().zip(b).all(|(a, b)| a <= b), "the image only grows");
		}
		assert!(last[0] > 0.0);
		assert_eq!(run(1).traces[0].series, c.traces[0].series);
		assert_ne!(run(2).traces[0].series, c.traces[0].series);
	}
}
