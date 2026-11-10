//! A lens's surfaces as the model and the painter reckon with them: sags, the radii fitted to the diameter
//! and the centre thickness, and the zones of a Fresnel lens (spec 10 "Lens radius fit", spec 20 "Lens").
//!
//! A surface of signed radius `R` has its circle's centre at `vertex + R`; `R1 > 0` and `R2 < 0` bulge outward,
//! 0 is flat. A Fresnel lens cuts a surface into `zones` zones from the axis to the rim, divided by
//! [`ZoneDivision`]; at each zone's inner edge the arc starts again at the vertex plane, so the deepest zone's
//! sag is what the centre thickness must hold.

use super::ZoneDivision;

/// How far a surface of `radius` bows beyond its vertex at `half` pixels from the axis; 0 when flat.
pub fn sag(radius: f64, half: f64) -> f64 {
	if radius == 0.0 {
		return 0.0;
	}
	let a = radius.abs();
	let h = a.min(half);
	a - (a * a - h * h).sqrt()
}

/// Where a surface of signed `radius` lies along x' at `y` from the axis, relative to its vertex: 0 for flat,
/// positive toward +x'.
pub fn surface_sag(radius: f64, y: f64) -> f64 {
	if radius == 0.0 {
		return 0.0;
	}
	let inside = (radius * radius - y * y).max(0.0);
	radius - inside.sqrt().copysign(radius)
}

/// How far the deepest of `zones` zones of a surface of `radius` bows out beyond its own start at the vertex
/// plane: the whole sag for one zone; for zones of equal width the outer zone's (the sag grows ever faster
/// outward); for zones of equal depth the whole sag over the count.
pub fn zone_sag(radius: f64, half: f64, zones: u32, division: ZoneDivision) -> f64 {
	if zones <= 1 {
		sag(radius, half)
	} else if division == ZoneDivision::EqualDepth {
		sag(radius, half) / zones as f64
	} else {
		sag(radius, half) - sag(radius, half * (zones - 1) as f64 / zones as f64)
	}
}

/// Where zone boundary `k` (0 the axis, `zones` the rim) of a surface of `radius` lies from the axis: `k / zones`
/// of `half` for zones of equal width; for zones of equal depth where the sag reaches `k / zones` of the rim's,
/// `sqrt(R^2 - (R - k S / N)^2)`, the zones narrowing outward (a flat surface is divided by width).
pub fn zone_bound(radius: f64, half: f64, zones: u32, division: ZoneDivision, k: u32) -> f64 {
	let zones = zones.max(1);
	let k = k.min(zones);
	if division == ZoneDivision::EqualWidth || radius == 0.0 || k == zones {
		return half * k as f64 / zones as f64;
	}
	let a = radius.abs();
	let along = a - sag(radius, half) * k as f64 / zones as f64;
	(a * a - along * along).max(0.0).sqrt()
}

/// The zone, 0 to `zones - 1`, that `across` pixels from the axis lies in, by the division's boundaries.
pub fn zone_index(radius: f64, half: f64, zones: u32, division: ZoneDivision, across: f64) -> u32 {
	let zones = zones.max(1);
	let share = if division == ZoneDivision::EqualWidth || radius == 0.0 || sag(radius, half) <= 0.0 {
		across / half
	} else {
		sag(radius, across.min(half)) / sag(radius, half)
	};
	((share * zones as f64) as i64).clamp(0, zones as i64 - 1) as u32
}

fn guard_radius(radius: f64, half: f64) -> f64 {
	if !radius.is_finite() || radius == 0.0 {
		0.0
	} else if radius.abs() < half {
		half.copysign(radius)
	} else {
		radius
	}
}

/// The radius, at least `half`, whose deepest zone's sag is `sag`: the sagitta `(h^2 + s^2) / 2s` of the whole
/// sag for one zone and for zones of equal depth; for zones of equal width found by bisection, the outer zone's
/// sag falling steadily from the hemisphere's at a radius of `half` toward 0 as the radius grows.
fn radius_of_zone_sag(half: f64, zones: u32, division: ZoneDivision, sag: f64) -> f64 {
	if zones <= 1 || division == ZoneDivision::EqualDepth {
		let whole = half.min(sag * zones.max(1) as f64);
		return (half * half + whole * whole) / (2.0 * whole);
	}
	let outer = |radius: f64| zone_sag(radius, half, zones, division);
	if sag >= outer(half) {
		return half;
	}
	let (mut low, mut high) = (half, half * 2.0);
	while outer(high) > sag {
		high *= 2.0;
	}
	for _ in 0..100 {
		if high - low <= 1e-9 * high {
			break;
		}
		let mid = (low + high) / 2.0;
		if outer(mid) > sag {
			low = mid;
		} else {
			high = mid;
		}
	}
	high
}

/// [`fit_radii_zoned`] for a plain lens.
pub fn fit_radii(diameter: u32, thickness: u32, r1: f64, r2: f64) -> (f64, f64) {
	fit_radii_zoned(diameter, thickness, r1, r2, 1, ZoneDivision::EqualWidth)
}

/// The radii (pixels; R1 > 0 and R2 < 0 convex, 0 flat) as a lens of `diameter` (at least 2) and centre
/// `thickness` (at least 1) uses them: non-finite or zero is flat, a magnitude under half the diameter becomes
/// half the diameter, and convex surfaces whose deepest zone would bow out by more than the thickness plus the
/// concave sags allow are flattened in proportion, each radius rounded up to a whole pixel.
pub fn fit_radii_zoned(
	diameter: u32,
	thickness: u32,
	r1: f64,
	r2: f64,
	zones: u32,
	division: ZoneDivision,
) -> (f64, f64) {
	debug_assert!(diameter >= 2 && thickness >= 1, "lens size not normalised");
	let half = diameter as f64 / 2.0;
	let zones = zones.max(1);
	let mut r1 = guard_radius(r1, half);
	let mut r2 = guard_radius(r2, half);
	let sag1 = zone_sag(r1, half, zones, division);
	let sag2 = zone_sag(r2, half, zones, division);
	let convex = if r1 > 0.0 { sag1 } else { 0.0 } + if r2 < 0.0 { sag2 } else { 0.0 };
	let budget = thickness as f64 + if r1 < 0.0 { sag1 } else { 0.0 } + if r2 > 0.0 { sag2 } else { 0.0 };
	// a hair over the budget (the floating point of a stretched lens's sags) is no case for a refit
	if convex > budget + 1e-9 {
		let scale = budget / convex;
		let fit = |sag: f64| radius_of_zone_sag(half, zones, division, sag * scale).ceil();
		if r1 > 0.0 {
			r1 = fit(sag1);
		}
		if r2 < 0.0 {
			r2 = -fit(sag2);
		}
	}
	(r1, r2)
}

/// Thickness at the rim: the centre thickness less what the convex surfaces' deepest zones take, plus what the
/// concave ones add; never negative for fitted radii.
pub fn edge_thickness(diameter: u32, thickness: u32, r1: f64, r2: f64, zones: u32, division: ZoneDivision) -> f64 {
	let half = diameter.max(2) as f64 / 2.0;
	let zoned = |r: f64| zone_sag(r, half, zones, division);
	thickness.max(1) as f64 - zoned(r1).copysign(r1) + zoned(r2).copysign(r2)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn lens_fit_cases() {
		assert_eq!(fit_radii(100, 20, 50.0, -50.0), (130.0, -130.0));
		assert_eq!(fit_radii(2000, 40, 1000.0, -1000.0), (25010.0, -25010.0));
		assert_eq!(fit_radii(200, 40, 300.0, -300.0), (300.0, -300.0));
		assert_eq!(fit_radii(100, 80, 10.0, f64::NAN), (50.0, 0.0));
		assert_eq!(fit_radii(100, 40, 10.0, f64::NAN), (52.0, 0.0));
	}

	/// Four zones of equal width on a 500 px plate hold radii of 1500 (an outer sag of 9.2 px a surface) that a
	/// whole lens of 20 px could not; equal depths divide the rim's sag and narrow outward.
	#[test]
	fn zones_fit_and_divide() {
		let (r1, r2) = fit_radii_zoned(500, 20, 1500.0, -1500.0, 4, ZoneDivision::EqualWidth);
		assert_eq!((r1, r2), (1500.0, -1500.0));
		assert!((zone_sag(1500.0, 250.0, 4, ZoneDivision::EqualWidth) - 9.2).abs() < 0.1);
		assert_ne!(fit_radii(500, 20, 1500.0, -1500.0), (1500.0, -1500.0), "a whole lens is refit");
		let depth = ZoneDivision::EqualDepth;
		assert_eq!(zone_sag(1500.0, 250.0, 4, depth), sag(1500.0, 250.0) / 4.0);
		let bounds: Vec<f64> = (0..=4).map(|k| zone_bound(1500.0, 250.0, 4, depth, k)).collect();
		assert_eq!((bounds[0], bounds[4]), (0.0, 250.0));
		assert!(bounds[1] > 125.0 && bounds[2] - bounds[1] > bounds[3] - bounds[2]);
		assert_eq!(zone_index(1500.0, 250.0, 4, depth, bounds[2] + 0.5), 2);
		assert_eq!(zone_index(1500.0, 250.0, 4, ZoneDivision::EqualWidth, 130.0), 2);
		assert_eq!(zone_index(0.0, 250.0, 4, depth, 300.0), 3);
		assert_eq!(edge_thickness(500, 20, 1500.0, -1500.0, 1, depth), 20.0 - 2.0 * sag(1500.0, 250.0));
		// the refit of a stretched Fresnel lens lands on a radius whose outer zone just fits
		let (f1, _) = fit_radii_zoned(500, 4, 300.0, 0.0, 4, ZoneDivision::EqualWidth);
		assert!(zone_sag(f1, 250.0, 4, ZoneDivision::EqualWidth) <= 4.0 + 1e-9 && f1 > 300.0);
	}
}
