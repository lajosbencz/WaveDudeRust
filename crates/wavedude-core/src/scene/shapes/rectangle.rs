use super::Vec2;
use crate::model::Rectangle;

/// `Length` along x', `Width` across, centred.
pub(super) fn vertices(r: &Rectangle) -> Vec<Vec2> {
	let l = r.length as f64 / 2.0;
	let w = r.width as f64 / 2.0;
	vec![Vec2::new(-l, -w), Vec2::new(l, -w), Vec2::new(l, w), Vec2::new(-l, w)]
}
