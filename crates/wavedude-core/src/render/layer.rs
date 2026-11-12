//! One layer's picture: the colours the project asks for and the choice of renderer for a stored plane.

use crate::engine::{FrameShape, SampleFormat, as_halves};
use crate::io::schemes::SchemeStore;
use crate::model::{ColorScheme, Layer, Project};
use crate::render::screen::{ScreenColors, Window};
use crate::render::{DeepHalfTable, DitherDepth, HalfTable, Palette, ScreenRenderer, render_indexed_into};

/// How the project colours a layer.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerColours {
	pub scheme: ColorScheme,
	pub black_level: f64,
	/// Applies to 16-bit planes; an 8-bit plane carries its own.
	pub brightness: f64,
}

impl LayerColours {
	pub fn of(project: &Project, layer: Layer, schemes: &SchemeStore) -> Self {
		let d = project.display.layer(layer);
		Self {
			scheme: schemes.resolve(&project.color_schemes, &d.color_scheme),
			black_level: d.black_level,
			brightness: d.brightness,
		}
	}

	/// The gain of the scene outline blended over the layer's picture; `None` draws none.
	pub fn overlay_gain(project: &Project, layer: Layer) -> Option<f64> {
		let d = project.display.layer(layer);
		d.overlay.then_some(d.overlay_gain)
	}
}

/// The size and part of the canvas of a picture of a plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
	pub width: u32,
	pub height: u32,
	/// The part of the canvas the picture shows; `None` is all of it.
	pub window: Option<Window>,
	/// 10, 12, 14 or 16 dithers a 16-bit plane; anything else is plain.
	pub dither_bits: u32,
}

impl View {
	/// The view of a plane of `shape`: an 8-bit plane is coloured at its stored size; a 16-bit one at `device`
	/// pixels (the stored size when `device` is empty) and `window`.
	pub fn of(shape: &FrameShape, device: (u32, u32), window: Option<Window>, dither_bits: u32) -> Self {
		let stored = (shape.width(), shape.height());
		match shape.format {
			SampleFormat::Index => Self { width: stored.0, height: stored.1, window: None, dither_bits },
			SampleFormat::Half => {
				let (width, height) = if device.0 > 0 && device.1 > 0 { device } else { stored };
				Self { width, height, window, dither_bits }
			}
		}
	}

	/// Bytes of the RGBA8 picture.
	pub fn bytes(&self) -> usize {
		self.width as usize * self.height as usize * 4
	}
}

/// Colours the plane `bytes` of `layer` into `out` ([`View::bytes`] long): an index plane through the palette,
/// a half plane through `screen` at the view's size, plain or dithered.
pub fn paint(
	bytes: &[u8],
	shape: &FrameShape,
	layer: Layer,
	colours: &LayerColours,
	view: &View,
	screen: &mut ScreenRenderer,
	out: &mut [u8],
) {
	let LayerColours { scheme, black_level, brightness } = colours;
	match shape.format {
		SampleFormat::Index => render_indexed_into(bytes, &Palette::cached(scheme, *black_level), out),
		SampleFormat::Half => {
			let halves = as_halves(bytes);
			let kind = shape.sample_kind(layer);
			let (sw, sh) = (shape.width() as usize, shape.height() as usize);
			screen.set_view(sw, sh, view.window, view.width as usize, view.height as usize);
			match DitherDepth::from_bits(view.dither_bits) {
				Some(depth) => {
					let deep = DeepHalfTable::cached(scheme, *black_level, *brightness as f32, kind);
					screen.render(&halves, ScreenColors::Dithered(&deep, depth), out);
				}
				None => {
					let table = HalfTable::cached(scheme, *black_level, *brightness as f32, kind);
					screen.render(&halves, ScreenColors::Plain(&table), out);
				}
			}
		}
	}
}

/// RGBA8 `src` of `from` pixels, or the `window` of it, resampled to `to` pixels by nearest neighbour (pixel centres).
pub fn scale_nearest(src: &[u8], from: (u32, u32), window: Option<Window>, to: (u32, u32)) -> Vec<u8> {
	if from == to && window.is_none() {
		return src.to_vec();
	}
	let (fw, fh) = (from.0 as usize, from.1 as usize);
	let (tw, th) = (to.0 as usize, to.1 as usize);
	let (x0, y0, w, h) = window.unwrap_or((0.0, 0.0, fw as f64, fh as f64));
	let column = |x: usize| ((x0 + (x as f64 + 0.5) * w / tw as f64).floor().max(0.0) as usize).min(fw - 1);
	let columns: Vec<usize> = (0..tw).map(column).collect();
	let mut out = Vec::with_capacity(tw * th * 4);
	for y in 0..th {
		let sy = ((y0 + (y as f64 + 0.5) * h / th as f64).floor().max(0.0) as usize).min(fh - 1);
		let row = &src[sy * fw * 4..(sy + 1) * fw * 4];
		for &sx in &columns {
			out.extend_from_slice(&row[sx * 4..sx * 4 + 4]);
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::f16;

	fn shape(format: SampleFormat) -> FrameShape {
		FrameShape {
			canvas_width: 4,
			canvas_height: 2,
			divisor: 1,
			format,
			field_brightness: 1.,
			intensity_brightness: 1.,
			intensity_root: false,
		}
	}

	fn colours() -> LayerColours {
		LayerColours { scheme: ColorScheme::grey(), black_level: 0., brightness: 2. }
	}

	#[test]
	fn an_index_plane_goes_through_the_palette_at_its_stored_size() {
		let shape = shape(SampleFormat::Index);
		let view = View::of(&shape, (64, 64), Some((1., 0., 2., 2.)), 12);
		assert_eq!((view.width, view.height, view.window), (4, 2, None));
		let plane: Vec<u8> = (0..8u8).map(|i| i * 30).collect();
		let mut out = vec![0; view.bytes()];
		paint(&plane, &shape, Layer::Field, &colours(), &view, &mut ScreenRenderer::new(1, 1, 1, 1), &mut out);
		let mut expected = vec![0; 32];
		render_indexed_into(&plane, &Palette::cached(&colours().scheme, 0.), &mut expected);
		assert_eq!(out, expected);
	}

	#[test]
	fn a_half_plane_goes_through_the_screen_renderer_plain_or_dithered() {
		let shape = shape(SampleFormat::Half);
		let halves: Vec<f16> = (0..8).map(|i| f16::from_f32(i as f32 / 4. - 1.)).collect();
		let plane: &[u8] = bytemuck::cast_slice(&halves);
		let table = HalfTable::cached(&colours().scheme, 0., 2., shape.sample_kind(Layer::Field));
		let view = View::of(&shape, (0, 0), None, 0);
		assert_eq!((view.width, view.height), (4, 2));
		let mut out = vec![0; view.bytes()];
		paint(plane, &shape, Layer::Field, &colours(), &view, &mut ScreenRenderer::new(1, 1, 1, 1), &mut out);
		assert_eq!(out, crate::render::render_half(&halves, &table));
		let view = View::of(&shape, (8, 4), None, 16);
		let mut dithered = vec![0; view.bytes()];
		paint(plane, &shape, Layer::Field, &colours(), &view, &mut ScreenRenderer::new(1, 1, 1, 1), &mut dithered);
		let mut screen = ScreenRenderer::new(1, 1, 1, 1);
		screen.set_view(4, 2, None, 8, 4);
		let deep = DeepHalfTable::cached(&colours().scheme, 0., 2., shape.sample_kind(Layer::Field));
		let mut expected = vec![0; view.bytes()];
		screen.render(&halves, ScreenColors::Dithered(&deep, DitherDepth::Bits16), &mut expected);
		assert_eq!(dithered, expected);
		let mut plain = vec![0; view.bytes()];
		let plain_view = View { dither_bits: 0, ..view };
		paint(plane, &shape, Layer::Field, &colours(), &plain_view, &mut ScreenRenderer::new(1, 1, 1, 1), &mut plain);
		assert_ne!(dithered, plain);
	}

	#[test]
	fn the_colours_follow_the_displays_layer() {
		let mut project = Project::default();
		project.display.field.overlay = false;
		project.display.intensity.overlay = true;
		project.display.intensity.overlay_gain = 3.;
		let schemes = SchemeStore::default();
		assert_eq!(LayerColours::overlay_gain(&project, Layer::Field), None);
		assert_eq!(LayerColours::overlay_gain(&project, Layer::Intensity), Some(3.));
		let c = LayerColours::of(&project, Layer::Intensity, &schemes);
		assert_eq!(c.brightness, project.display.intensity.brightness);
	}

	#[test]
	fn nearest_scaling_samples_pixel_centres() {
		let src: Vec<u8> = (0..4u8).flat_map(|v| [v, v, v, 255]).collect();
		let up = scale_nearest(&src, (2, 2), None, (4, 4));
		let reds: Vec<u8> = up.chunks(4).map(|p| p[0]).collect();
		assert_eq!(reds, vec![0, 0, 1, 1, 0, 0, 1, 1, 2, 2, 3, 3, 2, 2, 3, 3]);
		let down = scale_nearest(&up, (4, 4), None, (2, 2));
		assert_eq!(down, src);
		assert_eq!(scale_nearest(&src, (2, 2), None, (2, 2)), src);
		let corner = scale_nearest(&src, (2, 2), Some((1.0, 1.0, 1.0, 1.0)), (2, 2));
		assert_eq!(corner, [&src[12..16], &src[12..16], &src[12..16], &src[12..16]].concat());
	}
}
