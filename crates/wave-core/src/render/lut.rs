//! Colour tables: the 8-bit palette, the half tables and the palette indices an 8-bit frame stores.

use std::sync::{Arc, Mutex};

use half::f16;
use rayon::prelude::*;

use super::gradient::{Gradient, level};
use crate::engine::types::SampleKind;
use crate::model::{ColorScheme, Rgb};

/// Entries of the 8-bit palette, indices 0..=254.
pub const PALETTE_SIZE: usize = 255;
/// Values a 16-bit sample can hold: the length of a half table.
pub const HALF_VALUES: usize = 1 << 16;
/// The 16-bit channel scale of a [`DeepHalfTable`]: the 8-bit level times 256.
pub const DEEP_SCALE: f64 = 65280.0;

const CACHE_CAPACITY: usize = 16;

/// The colours of an 8-bit frame: entry i is the gradient at i / 254, each channel rounded half away from zero,
/// then through the black level and truncated to 8 bits. RGBA8, alpha 255.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
	entries: [[u8; 4]; PALETTE_SIZE],
}

impl Palette {
	pub fn new(scheme: &ColorScheme, black_level: f64) -> Self {
		let raw = Gradient::new(&scheme.stops).interpolate(PALETTE_SIZE);
		let mut entries = [[0, 0, 0, 255]; PALETTE_SIZE];
		for (entry, Rgb { r, g, b }) in entries.iter_mut().zip(raw) {
			let truncate = |c: u8| (level(c as f64, black_level) * 255.0) as u8;
			*entry = [truncate(r), truncate(g), truncate(b), 255];
		}
		Self { entries }
	}

	/// [`Palette::new`] through a process-wide cache keyed by the inputs.
	pub fn cached(scheme: &ColorScheme, black_level: f64) -> Arc<Self> {
		PALETTES.get_or_build(Key::new(scheme, black_level, None), || Self::new(scheme, black_level))
	}

	/// The colour of a stored index; 255 reads as 254.
	pub fn color(&self, index: u8) -> [u8; 4] {
		self.entries[(index as usize).min(PALETTE_SIZE - 1)]
	}

	pub fn entries(&self) -> &[[u8; 4]; PALETTE_SIZE] {
		&self.entries
	}
}

/// The colour of every value a 16-bit sample can hold, indexed by the f16 bit pattern: the gradient at the value's
/// position for its kind and brightness, through the black level, each channel rounded half away from zero.
/// RGBA8, alpha 255.
#[derive(Clone, Debug, PartialEq)]
pub struct HalfTable {
	kind: SampleKind,
	entries: Box<[[u8; 4]]>,
}

impl HalfTable {
	/// Built over the rayon pool (65,536 gradient evaluations).
	pub fn new(scheme: &ColorScheme, black_level: f64, brightness: f64, kind: SampleKind) -> Self {
		let gradient = Gradient::new(&scheme.stops);
		let entries = build(|bits| {
			let [r, g, b] = levels(&gradient, black_level, position(kind, bits, brightness));
			let byte = |l: f64| (l * 255.0).round() as u8;
			[byte(r), byte(g), byte(b), 255]
		});
		Self { kind, entries }
	}

	/// [`HalfTable::new`] through a process-wide cache keyed by the inputs.
	pub fn cached(scheme: &ColorScheme, black_level: f64, brightness: f64, kind: SampleKind) -> Arc<Self> {
		HALF_TABLES.get_or_build(Key::new(scheme, black_level, Some((brightness, kind))), || {
			Self::new(scheme, black_level, brightness, kind)
		})
	}

	pub fn kind(&self) -> SampleKind {
		self.kind
	}

	pub fn color(&self, value: f16) -> [u8; 4] {
		self.entries[value.to_bits() as usize]
	}

	/// All 65,536 entries, indexed by f16 bits.
	pub fn entries(&self) -> &[[u8; 4]] {
		&self.entries
	}
}

/// [`HalfTable`] at 16 bits per channel for dithering: each channel `round_away(level * 65280)` (0..=65280, the 8-bit
/// level in the high byte), packed `R << 32 | G << 16 | B`.
#[derive(Clone, Debug, PartialEq)]
pub struct DeepHalfTable {
	kind: SampleKind,
	entries: Box<[u64]>,
}

impl DeepHalfTable {
	pub fn new(scheme: &ColorScheme, black_level: f64, brightness: f64, kind: SampleKind) -> Self {
		let gradient = Gradient::new(&scheme.stops);
		let entries = build(|bits| {
			let [r, g, b] = levels(&gradient, black_level, position(kind, bits, brightness));
			let deep = |l: f64| (l * DEEP_SCALE).round() as u64;
			deep(r) << 32 | deep(g) << 16 | deep(b)
		});
		Self { kind, entries }
	}

	/// [`DeepHalfTable::new`] through a process-wide cache keyed by the inputs.
	pub fn cached(scheme: &ColorScheme, black_level: f64, brightness: f64, kind: SampleKind) -> Arc<Self> {
		DEEP_TABLES.get_or_build(Key::new(scheme, black_level, Some((brightness, kind))), || {
			Self::new(scheme, black_level, brightness, kind)
		})
	}

	pub fn kind(&self) -> SampleKind {
		self.kind
	}

	/// The packed 16-bit channels of a value: `[r, g, b]`, each 0..=65280.
	pub fn channels(&self, value: f16) -> [u16; 3] {
		unpack(self.entries[value.to_bits() as usize])
	}

	/// All 65,536 packed entries, indexed by f16 bits.
	pub fn entries(&self) -> &[u64] {
		&self.entries
	}
}

pub(crate) fn unpack(deep: u64) -> [u16; 3] {
	[(deep >> 32) as u16, (deep >> 16) as u16, deep as u16]
}

/// A sample's position on the gradient (0..1) by its kind at `brightness`.
fn position(kind: SampleKind, bits: u16, brightness: f64) -> f64 {
	let v = f16::from_bits(bits).to_f64();
	match kind {
		SampleKind::Field => {
			let scaled = v * brightness;
			if scaled.is_nan() { 0.5 } else { (scaled.clamp(-1.0, 1.0) + 1.0) / 2.0 }
		}
		SampleKind::Intensity => root_position(if v > 0.0 { v.sqrt() } else { 0.0 }, brightness),
		SampleKind::IntensityRoot => root_position(v, brightness),
	}
}

fn root_position(root: f64, brightness: f64) -> f64 {
	if root > 0.0 { (root * brightness).clamp(0.0, 1.0) } else { 0.0 }
}

fn levels(gradient: &Gradient, black_level: f64, pos: f64) -> [f64; 3] {
	gradient.evaluate(pos).map(|c| level(c, black_level))
}

fn build<T: Copy + Default + Send>(entry: impl Fn(u16) -> T + Sync) -> Box<[T]> {
	const CHUNK: usize = 4096;
	let mut entries = vec![T::default(); HALF_VALUES];
	entries.par_chunks_mut(CHUNK).enumerate().for_each(|(c, chunk)| {
		for (i, e) in chunk.iter_mut().enumerate() {
			*e = entry((c * CHUNK + i) as u16);
		}
	});
	entries.into_boxed_slice()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
	stops: Vec<(u64, Rgb)>,
	black_level: u64,
	table: Option<(u64, SampleKind)>,
}

impl Key {
	fn new(scheme: &ColorScheme, black_level: f64, table: Option<(f64, SampleKind)>) -> Self {
		Self {
			stops: scheme.stops.iter().map(|s| (s.at.to_bits(), s.color)).collect(),
			black_level: black_level.to_bits(),
			table: table.map(|(b, kind)| (b.to_bits(), kind)),
		}
	}
}

/// Most recently used last; two threads missing at once may both build, to the same table.
struct Cache<T> {
	entries: Mutex<Vec<(Key, Arc<T>)>>,
}

impl<T> Cache<T> {
	const fn new() -> Self {
		Self { entries: Mutex::new(Vec::new()) }
	}

	fn get_or_build(&self, key: Key, build: impl FnOnce() -> T) -> Arc<T> {
		{
			let mut entries = self.entries.lock().unwrap();
			if let Some(i) = entries.iter().position(|(k, _)| *k == key) {
				let hit = entries.remove(i);
				let table = hit.1.clone();
				entries.push(hit);
				return table;
			}
		}
		let table = Arc::new(build());
		let mut entries = self.entries.lock().unwrap();
		if entries.len() >= CACHE_CAPACITY {
			entries.remove(0);
		}
		entries.push((key, table.clone()));
		table
	}
}

static PALETTES: Cache<Palette> = Cache::new();
static HALF_TABLES: Cache<HalfTable> = Cache::new();
static DEEP_TABLES: Cache<DeepHalfTable> = Cache::new();

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn cache_returns_the_same_table_until_an_input_changes() {
		let grey = ColorScheme::grey();
		let a = HalfTable::cached(&grey, 0.0, 1.0, SampleKind::IntensityRoot);
		assert!(Arc::ptr_eq(&a, &HalfTable::cached(&grey, 0.0, 1.0, SampleKind::IntensityRoot)));
		assert!(!Arc::ptr_eq(&a, &HalfTable::cached(&grey, 0.0, 2.0, SampleKind::IntensityRoot)));
		assert!(!Arc::ptr_eq(&a, &HalfTable::cached(&grey, 0.0, 1.0, SampleKind::Field)));
		assert!(!Arc::ptr_eq(&a, &HalfTable::cached(&grey, 0.1, 1.0, SampleKind::IntensityRoot)));
		assert!(Arc::ptr_eq(&Palette::cached(&grey, 0.0), &Palette::cached(&grey, 0.0)));
	}

	#[test]
	fn ramp_half_tables_take_the_nearest_level() {
		let ramp = ColorScheme {
			stops: vec![
				crate::model::Stop { at: 0.0, color: Rgb::new(0, 0, 0) },
				crate::model::Stop { at: 1.0, color: Rgb::new(0, 0, 250) },
			],
			..ColorScheme::default()
		};
		for kind in [SampleKind::Field, SampleKind::Intensity, SampleKind::IntensityRoot] {
			let table = HalfTable::new(&ramp, 0.0, 3.5, kind);
			let mut used = [false; 256];
			for bits in 0..=u16::MAX {
				let v = f16::from_bits(bits).to_f64();
				let at = match kind {
					SampleKind::Field if v.is_nan() => 0.5,
					SampleKind::Field => ((v * 3.5).clamp(-1.0, 1.0) + 1.0) / 2.0,
					SampleKind::Intensity if v > 0.0 => (v.sqrt() * 3.5).min(1.0),
					SampleKind::IntensityRoot if v > 0.0 => (v * 3.5).min(1.0),
					_ => 0.0,
				};
				let [r, g, b, a] = table.entries()[bits as usize];
				assert_eq!([r, g, a], [0, 0, 255]);
				assert!((b as f64 - 250.0 * at).abs() <= 0.5 + 1e-9, "{kind:?} bits {bits:#06x}");
				used[b as usize] = true;
			}
			assert_eq!(used.iter().filter(|u| **u).count(), 251);
		}
	}
}
