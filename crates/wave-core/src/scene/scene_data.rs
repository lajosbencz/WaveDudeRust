//! Composed scene pixels to the engine's medium (spec/20-scene-painting.md, SceneData).
//!
//! Pixels are RGBA8, row-major, top row first: red the refractive index times 100 (0 = mirror),
//! green an image source's frequency level, blue the absorption level. A mirror cell has c2 = 0: it
//! never moves, a Dirichlet wall (docs/physics.md, Spatial stencils).

use crate::engine::{ImageSource, PmlLayer, SceneData};
use crate::model::SceneEdges;

impl SceneData {
	/// The medium of a `w` x `h` canvas from its RGBA8 pixels and the scene's edge settings.
	/// Image sources are taken only when `scene.image_sources` is on.
	pub fn from_pixels(rgba: &[u8], w: u32, h: u32, scene: impl Into<SceneEdges>) -> SceneData {
		let scene = scene.into();
		assert_eq!(rgba.len(), w as usize * h as usize * 4, "RGBA8 buffer of {w} x {h}");
		let mut c2 = Vec::with_capacity(w as usize * h as usize);
		let mut d = Vec::with_capacity(c2.capacity());
		let mut sources = Vec::new();
		let rim = scene.rim();
		for (i, px) in rgba.as_chunks::<4>().0.iter().enumerate() {
			let (red, green, blue) = (px[0], px[1], px[2]);
			let n = (red as f64 / 100.0).max(1.0);
			c2.push(if red == 0 { 0.0 } else { (1.0 / n).powi(2) as f32 });
			d.push((1.0 - blue as f64 / 255.0) as f32);
			if green > 0 && scene.image_sources {
				let (x, y) = (i as u32 % w, i as u32 / w);
				sources.push(ImageSource { gx: x + rim, gy: y + rim, level: green });
			}
		}
		Self::with_edges(w, h, c2, d, sources, &scene)
	}

	/// Whether `other` has this grid, edges and image-source count, so an engine can swap it in
	/// (`WaveEngine::try_replace_scene`).
	pub fn same_grid(&self, other: &SceneData) -> bool {
		self.canvas_width == other.canvas_width
			&& self.canvas_height == other.canvas_height
			&& self.rim == other.rim
			&& self.edges == other.edges
			&& self.image_sources.len() == other.image_sources.len()
	}

	/// The medium of a blank `w` x `h` canvas at the scene's background index.
	pub fn blank(w: u32, h: u32, scene: impl Into<SceneEdges>) -> SceneData {
		let scene = scene.into();
		let speed = (1.0 / scene.refractive_index) as f32;
		let cells = w as usize * h as usize;
		Self::with_edges(w, h, vec![speed * speed; cells], vec![1.0; cells], Vec::new(), &scene)
	}

	/// Grows the canvas maps by the rim (the rim's PML profile) or applies the inside damping band.
	fn with_edges(
		w: u32,
		h: u32,
		c2: Vec<f32>,
		d: Vec<f32>,
		image_sources: Vec<ImageSource>,
		scene: &SceneEdges,
	) -> Self {
		let rim = scene.rim();
		let (c2, mut d) = if rim > 0 { (grow(&c2, w, h, rim), grow(&d, w, h, rim)) } else { (c2, d) };
		let (gw, gh) = (w + 2 * rim, h + 2 * rim);
		let mut pml = Vec::new();
		if rim > 0 {
			let fastest = rim_cells(gw, gh, rim).map(|i| c2[i]).fold(0f32, f32::max);
			pml = PmlParams::default().profile(rim, (fastest as f64).sqrt());
		} else if scene.damping_edge > 0 {
			let border = (scene.damping_edge as u32).min(w.min(h) / 2);
			for i in 0..border {
				set_ring(&mut d, gw, gh, i, (i as f64 / border as f64).sqrt() as f32);
			}
		}
		SceneData { canvas_width: w, canvas_height: h, rim, edges: scene.edges, c2, d, image_sources, pml }
	}
}

/// The CPML profile's parameters (Roden and Gedney 2000; docs/physics.md, Absorbing boundaries).
/// At relative depth `r = depth / thickness` (0 at the canvas, 1 at the wall):
///
/// ```text
/// sigma = sigma_max r^m,   sigma_max = -(m + 1) ln(R) c / (2 thickness)
/// kappa = 1 + (kappa_max - 1) r^m
/// alpha = alpha_max (1 - r)
/// ```
///
/// R the normal-incidence reflection of the continuum layer backed by the wall, c the fastest
/// speed in the rim (cells a step).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PmlParams {
	pub order: i32,
	pub reflection: f64,
	pub kappa_max: f64,
	/// 1/step.
	pub alpha_max: f64,
}

impl Default for PmlParams {
	fn default() -> Self {
		Self { order: 3, reflection: 1e-6, kappa_max: 2.0, alpha_max: 0.05 }
	}
}

impl PmlParams {
	/// The layers of a `thickness`-cell rim for waves of speed `c`.
	pub fn profile(&self, thickness: u32, c: f64) -> Vec<PmlLayer> {
		let t = thickness as f64;
		let m = self.order;
		let sigma_max = -(m as f64 + 1.0) * self.reflection.ln() * c / (2.0 * t);
		(1..=thickness)
			.map(|depth| {
				let r = depth as f64 / t;
				let kappa = 1.0 + (self.kappa_max - 1.0) * r.powi(m);
				let slope = -(self.kappa_max - 1.0) * m as f64 * r.powi(m - 1) / (t * kappa * kappa);
				PmlLayer {
					sigma: sigma_max * r.powi(m),
					kappa,
					alpha: self.alpha_max * (1.0 - r),
					inv_kappa_slope: slope,
				}
			})
			.collect()
	}
}

/// Indices of the rim's cells on a `gw` x `gh` grid.
fn rim_cells(gw: u32, gh: u32, rim: u32) -> impl Iterator<Item = usize> {
	(0..gh).flat_map(move |y| {
		(0..gw)
			.filter(move |&x| x < rim || y < rim || x >= gw - rim || y >= gh - rim)
			.map(move |x| (y * gw + x) as usize)
	})
}

/// `values` of a `w` x `h` canvas on a grid `rim` larger on every side, each rim cell repeating the
/// nearest canvas cell.
fn grow(values: &[f32], w: u32, h: u32, rim: u32) -> Vec<f32> {
	let (gw, gh) = (w + 2 * rim, h + 2 * rim);
	let mut out = Vec::with_capacity(gw as usize * gh as usize);
	for gy in 0..gh {
		let y = gy.saturating_sub(rim).min(h - 1) as usize;
		let row = &values[y * w as usize..(y + 1) * w as usize];
		out.extend((0..gw).map(|gx| row[gx.saturating_sub(rim).min(w - 1) as usize]));
	}
	out
}

/// Writes `value` on ring `i` (0 = the outermost) of a `gw` x `gh` grid.
fn set_ring(d: &mut [f32], gw: u32, gh: u32, i: u32, value: f32) {
	let (gw, gh, i) = (gw as usize, gh as usize, i as usize);
	if i > gw - 1 - i || i > gh - 1 - i {
		return;
	}
	for x in i..gw - i {
		d[i * gw + x] = value;
		d[(gh - 1 - i) * gw + x] = value;
	}
	for y in i..gh - i {
		d[y * gw + i] = value;
		d[y * gw + gw - 1 - i] = value;
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::model::Edges;
	use crate::model::Scene;

	fn scene(damping_edge: i32) -> Scene {
		Scene { damping_edge, ..Scene::default() }
	}

	fn pixels(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
		(0..w * h).flat_map(|_| [rgb[0], rgb[1], rgb[2], 255]).collect()
	}

	#[test]
	fn band_of_six() {
		let s = SceneData::from_pixels(&pixels(20, 16, [100, 0, 0]), 20, 16, &scene(6));
		assert_eq!((s.grid_width(), s.grid_height(), s.rim), (20, 16, 0));
		assert_eq!(s.d[0], 0.0);
		assert_eq!(s.d[8 * 20 + 8], 1.0);
		assert_eq!(s.d[20 + 1], (1.0f64 / 6.0).sqrt() as f32);
		assert_eq!(s.c2[0], 1.0);
		let small = SceneData::from_pixels(&pixels(8, 8, [100, 0, 0]), 8, 8, &scene(6));
		assert_eq!(small.d[0], 0.0, "the band is cut to half the shorter side");
		assert_eq!(small.d[3 * 8 + 3], 0.75f64.sqrt() as f32);
	}

	#[test]
	fn rim_of_32_grows_103_by_67_to_167_by_131() {
		let s = SceneData::blank(103, 67, &scene(-32));
		assert_eq!((s.grid_width(), s.grid_height()), (167, 131));
		assert_eq!(s.c2.len(), 167 * 131);
		assert!(s.d.iter().all(|&d| d == 1.0), "the rim absorbs through its PML, not by damping");
		assert_eq!(s.pml.len(), 32);
		let (inner, wall) = (s.pml[0], s.pml[31]);
		let sigma_max = 4.0 * 1e6f64.ln() / 64.0;
		assert!((wall.sigma - sigma_max).abs() < 1e-12 && (wall.kappa - 2.0).abs() < 1e-12 && wall.alpha == 0.0);
		assert!(inner.sigma < 1e-4 * sigma_max && inner.kappa < 1.001 && inner.alpha > 0.04);
		assert!(s.pml.windows(2).all(|p| p[1].sigma > p[0].sigma && p[1].inv_kappa_slope < 0.0));
		assert!(SceneData::blank(10, 10, &scene(0)).pml.is_empty());
	}

	#[test]
	fn pml_slope_is_the_derivative_of_one_over_kappa() {
		let p = PmlParams::default().profile(20, 1.0);
		let at = |r: f64| 1.0 / (1.0 + r.powi(3));
		for (i, layer) in p.iter().enumerate() {
			let r = (i + 1) as f64 / 20.0;
			let numeric = (at(r + 1e-6) - at(r - 1e-6)) / 2e-6 / 20.0;
			assert!((layer.inv_kappa_slope - numeric).abs() < 1e-7, "{i}: {} vs {numeric}", layer.inv_kappa_slope);
		}
	}

	#[test]
	fn rim_repeats_the_edge_and_moves_sources() {
		let mut px = pixels(8, 8, [100, 0, 0]);
		px[0] = 200;
		px[(3 * 8 + 2) * 4 + 1] = 120;
		let s = SceneData::from_pixels(&px, 8, 8, &Scene { image_sources: true, ..scene(-4) });
		assert_eq!((s.grid_width(), s.rim), (16, 4));
		assert_eq!(s.image_sources, vec![ImageSource { gx: 6, gy: 7, level: 120 }]);
		assert_eq!(s.c2[0], 0.25);
		assert_eq!(s.c2[4 * 16 + 4], 0.25);
		assert_eq!(s.c2[4 * 16 + 5], 1.0);
		let off = SceneData::from_pixels(&px, 8, 8, &scene(-4));
		assert!(off.image_sources.is_empty());
	}

	#[test]
	fn red_levels() {
		let mut px = Vec::new();
		for red in [0u8, 1, 50, 99, 100, 150, 255] {
			px.extend([red, 0, 51, 255]);
		}
		let s = SceneData::from_pixels(&px, 7, 1, &scene(0));
		let c: Vec<f32> = s.c2.clone();
		assert_eq!(c[0], 0.0, "red 0 is a mirror");
		assert_eq!(&c[1..5], &[1.0; 4], "red 1..100 is index 1");
		assert_eq!(c[5], (1.0f64 / 1.5).powi(2) as f32);
		assert_eq!(c[6], (1.0f64 / 2.55).powi(2) as f32);
		assert_eq!(s.d[0], 0.8);
		assert_eq!(s.edges, Edges::Open);
	}

	#[test]
	fn blank_takes_the_background_index() {
		let s = SceneData::blank(4, 4, &Scene { refractive_index: 2.0, ..scene(0) });
		assert!(s.c2.iter().all(|&c| c == 0.25) && s.d.iter().all(|&d| d == 1.0));
	}
}
