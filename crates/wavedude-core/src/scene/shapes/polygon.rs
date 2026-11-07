use super::Vec2;
use crate::model::Polygon;

/// Regular n-gon of side `Side`: circumradius `Side / (2 sin(pi/n))`, vertex k at angle
/// `pi + (2k+1) pi/n`, so a face is centred on -x'.
pub fn vertices(p: &Polygon) -> Vec<Vec2> {
	let n = p.sides;
	let radius = p.side as f64 / (2.0 * (std::f64::consts::PI / n as f64).sin());
	(0..n)
		.map(|k| {
			let theta = std::f64::consts::PI + (2 * k + 1) as f64 * std::f64::consts::PI / n as f64;
			Vec2::new(radius * theta.cos(), radius * theta.sin())
		})
		.collect()
}
