use super::Vec2;
use crate::model::Triangle;

/// Base of `Base` across x' toward -x', apex along +x': `(-h/2, -B/2), (-h/2, B/2), (h/2, apexY)`. The base
/// angles are the ones normalisation wrote back for the type.
pub fn vertices(t: &Triangle) -> Vec<Vec2> {
	let (alpha, beta) = (t.angle_a.to_radians(), t.angle_b.to_radians());
	let half = t.base as f64 / 2.0;
	let height = 2.0 * half * alpha.sin() * beta.sin() / (alpha + beta).sin();
	let apex_y = -half + height * alpha.cos() / alpha.sin();
	vec![Vec2::new(-height / 2.0, -half), Vec2::new(-height / 2.0, half), Vec2::new(height / 2.0, apex_y)]
}
