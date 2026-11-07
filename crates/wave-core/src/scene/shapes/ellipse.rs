use super::Vec2;
use crate::model::Ellipse;

/// A vertex per 3 px of circumference, 24..180. The circumference is Ramanujan's second
/// approximation, as the original computes it.
pub fn vertex_count(length: u32, width: u32) -> usize {
	let a = length as f64 / 2.0;
	let b = width as f64 / 2.0;
	let h = (a - b) * (a - b) / ((a + b) * (a + b));
	let circumference = std::f64::consts::PI * (a + b) * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt()));
	((circumference / 3.0).round_ties_even() as i64).clamp(24, 180) as usize
}

/// `(a cos(2 pi k/n), b sin(2 pi k/n))`, a = Length/2, b = Width/2.
pub fn vertices(e: &Ellipse) -> Vec<Vec2> {
	let a = e.length as f64 / 2.0;
	let b = e.width as f64 / 2.0;
	let n = vertex_count(e.length, e.width);
	(0..n)
		.map(|k| {
			let theta = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
			Vec2::new(a * theta.cos(), b * theta.sin())
		})
		.collect()
}
