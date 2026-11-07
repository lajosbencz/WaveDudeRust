use super::Vec2;
use crate::model::Outline;

/// The points as given (part frame).
pub fn vertices(o: &Outline) -> Vec<Vec2> {
	o.points.iter().map(|p| Vec2::new(p.x as f64, p.y as f64)).collect()
}
