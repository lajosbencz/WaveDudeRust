//! The gallery's card pictures: each example's scene, composed from its project and shrunk to a thumbnail, cached as
//! a texture under the example's stem. Natively one worker thread paints; in the browser one card is painted per frame.

use std::collections::{HashMap, HashSet};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::Sender;
use std::sync::mpsc::{Receiver, channel};

use eframe::egui::{ColorImage, Context, TextureHandle, TextureOptions};
use waveduderust::unstable::scene::compose::{compose_scene, decode_image};
use waveduderust::unstable::scene::glowing_edges::{EdgeImage, blend_additive};
use waveduderust::unstable::scene::pixels::ScenePixels;
use waveduderust_examples::all;

/// The thumbnail's bounds in pixels.
const MAX_W: u32 = 320;
const MAX_H: u32 = 180;

type Painted = (&'static str, Option<ColorImage>);

pub(super) struct Thumbs {
	textures: HashMap<&'static str, TextureHandle>,
	/// Stems asked for, painted or not.
	asked: HashSet<&'static str>,
	done: Receiver<Painted>,
	#[cfg(not(target_arch = "wasm32"))]
	work: Option<Sender<usize>>,
	#[cfg(not(target_arch = "wasm32"))]
	finished: Sender<Painted>,
	#[cfg(target_arch = "wasm32")]
	queue: Vec<usize>,
}

impl Default for Thumbs {
	fn default() -> Self {
		let (finished, done) = channel();
		#[cfg(target_arch = "wasm32")]
		drop(finished);
		Self {
			textures: HashMap::new(),
			asked: HashSet::new(),
			done,
			#[cfg(not(target_arch = "wasm32"))]
			work: None,
			#[cfg(not(target_arch = "wasm32"))]
			finished,
			#[cfg(target_arch = "wasm32")]
			queue: Vec::new(),
		}
	}
}

impl Thumbs {
	/// Example `i`'s texture, asking for its painting the first time; None until it is ready (or when it has no scene).
	pub(super) fn get(&mut self, ctx: &Context, i: usize) -> Option<&TextureHandle> {
		let stem = all()[i].stem();
		if self.asked.insert(stem) {
			self.ask(ctx, i);
		}
		self.textures.get(stem)
	}

	/// Takes in the finished paintings; in the browser paints the next queued card.
	pub(super) fn poll(&mut self, ctx: &Context) {
		#[cfg(target_arch = "wasm32")]
		if let Some(i) = self.queue.pop() {
			let stem = all()[i].stem();
			self.take(ctx, (stem, paint(i)));
			ctx.request_repaint();
		}
		while let Ok(painted) = self.done.try_recv() {
			self.take(ctx, painted);
		}
	}

	fn take(&mut self, ctx: &Context, (stem, image): Painted) {
		if let Some(image) = image {
			self.textures.insert(stem, ctx.load_texture(format!("thumb-{stem}"), image, TextureOptions::LINEAR));
		}
	}

	#[cfg(target_arch = "wasm32")]
	fn ask(&mut self, ctx: &Context, i: usize) {
		self.queue.insert(0, i);
		ctx.request_repaint();
	}

	#[cfg(not(target_arch = "wasm32"))]
	fn ask(&mut self, ctx: &Context, i: usize) {
		let work = self.work.get_or_insert_with(|| {
			let (work, requests) = channel::<usize>();
			let (ctx, finished) = (ctx.clone(), self.finished.clone());
			std::thread::Builder::new()
				.name("thumbs".into())
				.spawn(move || {
					// lives as long as the app: Pages owns the sender; a panicking paint costs only that card
					for i in requests {
						let image = std::panic::catch_unwind(|| paint(i)).unwrap_or(None);
						if finished.send((all()[i].stem(), image)).is_err() {
							break;
						}
						ctx.request_repaint();
					}
				})
				.expect("the OS starts a thread");
			work
		});
		let _ = work.send(i);
	}
}

/// Example `i`'s scene as the scene view shows it: an image scene's picture, else the glowing edges of the composed
/// scene over black at the field layer's overlay gain. None when the project does not load or has no scene.
fn paint(i: usize) -> Option<ColorImage> {
	let (project, view) = all()[i].load().ok()?;
	if let Some(image) = decode_image(&project) {
		return Some(shrink(&image));
	}
	let pixels = compose_scene(&project)?;
	let (tw, th) = fit(pixels.width, pixels.height);
	let mut rgba = vec![0u8; (tw * th * 4) as usize];
	rgba.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p[3] = 255);
	blend_additive(&mut rgba, &EdgeImage::of(&pixels).scaled(None, (tw, th)), view.field.overlay_gain);
	Some(ColorImage::from_rgba_unmultiplied([tw as usize, th as usize], &rgba))
}

/// The size of a `w` x `h` picture fitted into the bounds, never enlarged.
fn fit(w: u32, h: u32) -> (u32, u32) {
	let (w, h) = (w.max(1), h.max(1));
	let factor = (w as f32 / MAX_W as f32).max(h as f32 / MAX_H as f32).max(1.);
	(((w as f32 / factor) as u32).max(1), ((h as f32 / factor) as u32).max(1))
}

/// `pixels` averaged down to fit the bounds.
fn shrink(pixels: &ScenePixels) -> ColorImage {
	let (w, h) = (pixels.width.max(1), pixels.height.max(1));
	let (tw, th) = fit(w, h);
	let mut rgba = Vec::with_capacity((tw * th * 4) as usize);
	for ty in 0..th {
		let (y0, y1) = (ty * h / th, ((ty + 1) * h / th).max(ty * h / th + 1));
		for tx in 0..tw {
			let (x0, x1) = (tx * w / tw, ((tx + 1) * w / tw).max(tx * w / tw + 1));
			let mut sum = [0u32; 3];
			for y in y0..y1 {
				for x in x0..x1 {
					let p = pixels.rgba_at(x, y);
					sum.iter_mut().zip(p).for_each(|(s, c)| *s += c as u32);
				}
			}
			let n = (y1 - y0) * (x1 - x0);
			rgba.extend(sum.iter().map(|s| (s / n) as u8));
			rgba.push(255);
		}
	}
	ColorImage::from_rgba_unmultiplied([tw as usize, th as usize], &rgba)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_large_scene_shrinks_into_the_bounds_and_a_small_one_stays() {
		let big = shrink(&ScenePixels::blank(1280, 720, 1.));
		assert_eq!(big.size, [320, 180]);
		let small = shrink(&ScenePixels::blank(40, 30, 1.));
		assert_eq!(small.size, [40, 30]);
	}
}
