//! Engine input and output types shared by the CPU and GPU engines.

use std::borrow::Cow;
use std::num::NonZeroU32;

use half::f16;

use crate::model::{Edges, Layer, PulseShape, Stencil};

/// The simulated medium on the grid (canvas plus rim), row-major, no padding.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneData {
	pub canvas_width: u32,
	pub canvas_height: u32,
	/// Cells of rim per side.
	pub rim: u32,
	pub edges: Edges,
	/// Squared wave speed per grid cell, (1/n)^2.
	pub c2: Vec<f32>,
	/// Damping factor per grid cell per step, 1 = none (the inside band and blue absorbers; the rim
	/// repeats the canvas edge, its absorption is the PML's).
	pub d: Vec<f32>,
	/// Green pixels that emit, in grid coordinates, row-major order.
	pub image_sources: Vec<ImageSource>,
	/// The CPML profile of the rim, entry i for the cells i + 1 deep (rim entries, empty without a rim).
	pub pml: Vec<PmlLayer>,
}

/// The CPML's coordinate stretch `s = kappa + sigma / (alpha + i omega)` at one depth of the rim,
/// in units of steps (dt = 1), the same on every side.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PmlLayer {
	/// Conductivity, 1/step.
	pub sigma: f64,
	pub kappa: f64,
	/// Complex frequency shift, 1/step.
	pub alpha: f64,
	/// `d(1/kappa)/d(depth)`, 1/cell, depth growing outward.
	pub inv_kappa_slope: f64,
}

/// Threshold below which a cell counts as a mirror: a mirror has c2 = 0, glass at most index 10 has c2 0.01.
pub const MIRROR_C2: f32 = 1e-4;

impl SceneData {
	pub fn grid_width(&self) -> u32 {
		self.canvas_width + 2 * self.rim
	}

	pub fn grid_height(&self) -> u32 {
		self.canvas_height + 2 * self.rim
	}

	/// Row length of the engines' padded buffers: [`PAD`](super::stencil::PAD) cells each side.
	pub fn stride(&self) -> usize {
		self.grid_width() as usize + 2 * super::stencil::PAD
	}

	/// Cells of the engines' padded buffers.
	pub fn padded_cells(&self) -> usize {
		self.stride() * (self.grid_height() as usize + 2 * super::stencil::PAD)
	}

	/// Index of grid cell (gx, gy) in the engines' padded buffers.
	pub fn padded_index(&self, gx: usize, gy: usize) -> usize {
		(gy + super::stencil::PAD) * self.stride() + gx + super::stencil::PAD
	}

	/// Writes the grid cells of the padded `c2` and `d` at the scale of `n` sub-steps: `c2 / n^2`, `d^(1/n)`.
	pub(crate) fn fill_sub_step_medium(&self, n: u32, c2: &mut [f32], d: &mut [f32]) {
		let gw = self.grid_width() as usize;
		let mult = 1.0 / (n as f32 * n as f32);
		let root = 1.0 / n as f64;
		// A scene is runs of equal damping: the root is taken once per run.
		let mut last: Option<(u32, f32)> = None;
		for (gy, (rc2, rd)) in self.c2.chunks_exact(gw).zip(self.d.chunks_exact(gw)).enumerate() {
			let at = self.padded_index(0, gy);
			for (k, (&c, &dd)) in rc2.iter().zip(rd).enumerate() {
				let dv = match last {
					Some((bits, v)) if bits == dd.to_bits() => v,
					_ => {
						let v = if n > 1 { (dd as f64).powf(root) as f32 } else { dd };
						last = Some((dd.to_bits(), v));
						v
					}
				};
				c2[at + k] = c * mult;
				d[at + k] = dv;
			}
		}
	}

	/// Whether padded cell `i` is a grid cell painted as a mirror ([`MIRROR_C2`]).
	pub fn is_mirror_at(&self, i: usize) -> bool {
		let (s, pad) = (self.stride(), super::stencil::PAD);
		let (x, y) = ((i % s).wrapping_sub(pad), (i / s).wrapping_sub(pad));
		x < self.grid_width() as usize
			&& y < self.grid_height() as usize
			&& self.c2[y * self.grid_width() as usize + x] <= MIRROR_C2
	}

	pub fn max_c2(&self) -> f32 {
		self.c2.iter().copied().fold(0.0, f32::max)
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageSource {
	pub gx: u32,
	pub gy: u32,
	/// Frequency level 1..255.
	pub level: u8,
}

/// Solver options taken from the project at engine build.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineOptions {
	pub stencil: Stencil,
	pub source_frequency_scale: f64,
	pub image_source_scale: f64,
	pub time_decay: f64,
	pub intensity_scale: f64,
	/// Intensity maintained (a cost); off when nothing reads it.
	pub track_intensity: bool,
}

/// One emitting cell from an object source.
#[derive(Clone, Debug, PartialEq)]
pub struct Emitter {
	/// Canvas position in cells: a whole number is a cell's centre, a fraction spreads the drive over
	/// the cells around it (emitters.rs, `taps`).
	pub x: f64,
	pub y: f64,
	pub level: f64,
	/// 0..1.
	pub amplitude: f64,
	pub phase_radians: f64,
	pub pulsed: bool,
	pub shape: PulseShape,
	pub on_width_cycles: u32,
	pub off_width_cycles: u32,
	pub off_width_base: u32,
	/// Random gap spread in cycles; 0 = fixed gap.
	pub randomize_off_width: u32,
	pub detach_after_step: Option<u32>,
	/// Seed of the per-pulse gap sequence (xorshift32, whose state must not be 0).
	pub rng_seed: NonZeroU32,
}

/// Which cells a probe reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProbeCells {
	pub layer: Layer,
	/// Canvas pixels (x, y) on the canvas, A first.
	pub cells: Vec<(u32, u32)>,
}

/// Sample format of a stored plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SampleFormat {
	/// f16 per sample.
	Half,
	/// u8 palette index per sample, brightness baked in.
	Index,
}

/// Geometry and format of captured planes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameShape {
	pub canvas_width: u32,
	pub canvas_height: u32,
	/// 1, 2 or 4.
	pub divisor: u32,
	pub format: SampleFormat,
	pub field_brightness: f32,
	pub intensity_brightness: f32,
	/// An f16 intensity plane holds sqrt(I) rather than I.
	pub intensity_root: bool,
}

impl FrameShape {
	pub fn width(&self) -> u32 {
		self.canvas_width.div_ceil(self.divisor)
	}

	pub fn height(&self) -> u32 {
		self.canvas_height.div_ceil(self.divisor)
	}

	pub fn plane_samples(&self) -> usize {
		self.width() as usize * self.height() as usize
	}

	/// How the values of a Half plane of `layer` are meant.
	pub fn sample_kind(&self, layer: Layer) -> SampleKind {
		match layer {
			Layer::Field => SampleKind::Field,
			Layer::Intensity if self.intensity_root => SampleKind::IntensityRoot,
			Layer::Intensity => SampleKind::Intensity,
		}
	}

	pub fn plane_bytes(&self) -> usize {
		self.plane_samples()
			* match self.format {
				SampleFormat::Half => 2,
				SampleFormat::Index => 1,
			}
	}
}

/// How the values in a plane are meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SampleKind {
	Field,
	Intensity,
	IntensityRoot,
}

/// One captured frame: raw plane bytes per layer in the frame's shape.
#[derive(Clone, Debug)]
pub struct Snapshot {
	pub shape: FrameShape,
	/// Step index the frame holds the state after.
	pub step: u32,
	pub field: Vec<u8>,
	/// Empty when intensity is not tracked.
	pub intensity: Vec<u8>,
}

impl Snapshot {
	/// A frame of `shape` without planes, to capture into.
	pub fn new(shape: FrameShape) -> Self {
		Self { shape, step: 0, field: Vec::new(), intensity: Vec::new() }
	}

	pub fn plane(&self, layer: Layer) -> &[u8] {
		match layer {
			Layer::Field => &self.field,
			Layer::Intensity => &self.intensity,
		}
	}

	/// The plane as halves (Half format only). A plane is an owned allocation, which every
	/// supported allocator aligns to at least 2 bytes; a plane built from an unaligned slice is a bug.
	pub fn halves(&self, layer: Layer) -> &[f16] {
		bytemuck::try_cast_slice(self.plane(layer)).expect("a Half plane is a 2-byte aligned owned buffer")
	}
}

/// A capture begun by [`WaveEngine::begin_capture`](super::WaveEngine::begin_capture), for the same
/// engine's `finish_capture`.
#[must_use]
pub struct PendingCapture {
	pub(crate) snapshot: Snapshot,
	/// The engine's staging set.
	#[cfg_attr(not(feature = "gpu"), allow(dead_code))]
	pub(crate) slot: usize,
}

/// A Half plane's bytes as halves, borrowed when aligned.
pub fn as_halves(bytes: &[u8]) -> Cow<'_, [f16]> {
	match bytemuck::try_cast_slice(bytes) {
		Ok(halves) => Cow::Borrowed(halves),
		Err(_) => Cow::Owned(bytes.as_chunks::<2>().0.iter().map(|&c| f16::from_bits(u16::from_ne_bytes(c))).collect()),
	}
}

/// Probe readings after one step: one value per cell of each probe, probe order kept, in one buffer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProbeReadings {
	values: Vec<f32>,
	/// End of each probe's values in `values`.
	ends: Vec<usize>,
}

impl ProbeReadings {
	/// Readings from one list of values per probe.
	pub fn from_probes<'a>(probes: impl IntoIterator<Item = &'a [f32]>) -> Self {
		let mut r = Self::default();
		probes.into_iter().for_each(|p| r.push(p.iter().copied()));
		r
	}

	pub fn clear(&mut self) {
		self.values.clear();
		self.ends.clear();
	}

	/// Appends the next probe's values.
	pub fn push(&mut self, values: impl IntoIterator<Item = f32>) {
		self.values.extend(values);
		self.ends.push(self.values.len());
	}

	/// Replaces the readings by `values`, the probes' values one after another, `lengths` of them each.
	pub fn fill(&mut self, lengths: &[usize], values: &[f32]) {
		self.clear();
		let mut at = 0;
		for &n in lengths {
			self.push(values[at..at + n].iter().copied());
			at += n;
		}
	}

	/// Number of probes.
	pub fn len(&self) -> usize {
		self.ends.len()
	}

	pub fn is_empty(&self) -> bool {
		self.ends.is_empty()
	}

	/// The values of probe `p`.
	pub fn probe(&self, p: usize) -> &[f32] {
		let start = if p == 0 { 0 } else { self.ends[p - 1] };
		&self.values[start..self.ends[p]]
	}

	/// Every probe's values in probe order.
	pub fn iter(&self) -> impl ExactSizeIterator<Item = &[f32]> {
		(0..self.len()).map(|p| self.probe(p))
	}

	/// Every probe's values one after another.
	pub fn values(&self) -> &[f32] {
		&self.values
	}
}

/// Probe readings of consecutive steps, kept by an engine until [`WaveEngine`](super::WaveEngine::drain_probes)
/// hands them out.
#[derive(Clone, Debug, Default)]
pub struct ProbeQueue {
	lengths: Vec<usize>,
	/// Values of one step's readings: the sum of `lengths`.
	width: usize,
	values: Vec<f32>,
	count: usize,
	reading: ProbeReadings,
}

impl ProbeQueue {
	/// A queue of readings of probes with `lengths` cells.
	pub fn new(lengths: Vec<usize>) -> Self {
		let width = lengths.iter().sum();
		Self { lengths, width, ..Self::default() }
	}

	/// Values of one step's readings.
	pub fn width(&self) -> usize {
		self.width
	}

	/// Steps queued.
	pub fn len(&self) -> usize {
		self.count
	}

	pub fn is_empty(&self) -> bool {
		self.count == 0
	}

	pub fn clear(&mut self) {
		self.values.clear();
		self.count = 0;
	}

	/// Appends the next step's readings, [`width`](Self::width) values.
	pub fn push(&mut self, values: &[f32]) {
		debug_assert_eq!(values.len(), self.width);
		self.values.extend_from_slice(values);
		self.count += 1;
	}

	/// Calls `f` with each step's readings, oldest first, and empties the queue.
	pub fn drain(&mut self, f: &mut dyn FnMut(&ProbeReadings)) {
		for i in 0..self.count {
			let at = i * self.width;
			self.reading.fill(&self.lengths, &self.values[at..at + self.width]);
			f(&self.reading);
		}
		self.clear();
	}
}
