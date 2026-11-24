//! Auto brightness: the gain that brings a frame's 99.5th percentile magnitude to the palette's top.

use half::f16;

use crate::engine::types::{FrameShape, SampleFormat, SampleKind};
use crate::model::Layer;

/// Percentile of the magnitudes put at the palette's top.
pub const PERCENTILE: f64 = 0.995;
/// Bounds of a fitted brightness, the model's range.
pub const MIN_BRIGHTNESS: f64 = crate::model::ranges::LAYER_BRIGHTNESS.min;
pub const MAX_BRIGHTNESS: f64 = crate::model::ranges::LAYER_BRIGHTNESS.max;

/// The fitted brightness of an f16 plane, three significant digits; `None` when no finite sample is above zero
/// in magnitude.
pub fn fit_brightness(samples: &[f16], kind: SampleKind) -> Option<f64> {
	fit(samples.iter().map(|s| s.to_f32()), kind)
}

/// The fitted brightness of `layer`'s stored plane of a frame of `shape`; `None` for palette indices, which need
/// the frame in 16-bit, or when there is nothing to fit.
pub fn fit_plane(bytes: &[u8], shape: &FrameShape, layer: Layer) -> Option<f64> {
	if !matches!(shape.format, SampleFormat::Half) {
		return None;
	}
	let kind = match layer {
		Layer::Field => SampleKind::Field,
		Layer::Intensity if shape.intensity_root => SampleKind::IntensityRoot,
		Layer::Intensity => SampleKind::Intensity,
	};
	fit(bytes.as_chunks::<2>().0.iter().map(|&b| f16::from_le_bytes(b).to_f32()), kind)
}

fn fit(samples: impl Iterator<Item = f32>, kind: SampleKind) -> Option<f64> {
	let mut magnitudes: Vec<f32> = samples
		.filter(|v| v.is_finite())
		.map(|v| match kind {
			SampleKind::Field => v.abs(),
			SampleKind::Intensity => v.max(0.0).sqrt(),
			SampleKind::IntensityRoot => v.max(0.0),
		})
		.collect();
	let n = magnitudes.len();
	if n == 0 {
		return None;
	}
	let rank = (PERCENTILE * (n - 1) as f64).floor() as usize;
	let top = *magnitudes.select_nth_unstable_by(rank, f32::total_cmp).1 as f64;
	if top <= 0.0 {
		return None;
	}
	Some(three_digits((1.0 / top).clamp(MIN_BRIGHTNESS, MAX_BRIGHTNESS)))
}

/// Rounded half to even at three significant digits, by decimal places so the result is the nearest double.
fn three_digits(b: f64) -> f64 {
	let digits = 2 - b.log10().floor() as i32;
	if digits >= 0 {
		let scale = 10f64.powi(digits);
		(b * scale).round_ties_even() / scale
	} else {
		let unit = 10f64.powi(-digits);
		(b / unit).round_ties_even() * unit
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn rounds_to_three_significant_digits() {
		assert_eq!(three_digits(33.3347), 33.3);
		assert_eq!(three_digits(1.0025), 1.0);
		assert_eq!(three_digits(0.0012345), 0.00123);
		assert_eq!(three_digits(12345.0), 12300.0);
		assert_eq!(three_digits(12350.0), 12400.0);
		assert_eq!(three_digits(1e5), 1e5);
	}

	#[test]
	fn field_fit_ignores_a_single_source_cell() {
		let mut field: Vec<f16> =
			(0..10_000).map(|i| f16::from_f32(if i % 2 == 0 { 1.0 } else { -1.0 } * 0.001 * i as f32)).collect();
		field[1234] = f16::from_f32(50.0);
		let fit = fit_brightness(&field, SampleKind::Field).unwrap();
		assert!((0.099..=0.102).contains(&fit), "{fit}");
	}
}
