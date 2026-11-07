//! Scene pixels: the RGBA8 picture the engines read the scene from.

/// RGBA8 scene pixels, row-major, `width * 4` bytes a row, origin top-left.
///
/// Channels: red is the refractive index times 100 (red 0 is a mirror), green an image source's
/// frequency level, blue an absorber (damping `1 - blue/255` per step); alpha is 255 and ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenePixels {
	pub width: u32,
	pub height: u32,
	pub rgba: Vec<u8>,
}

/// Red level of a blank canvas of a normalised index (1..2.55): `round_even(index * 100)`.
pub fn background_red(index: f64) -> u8 {
	debug_assert!(crate::model::ranges::INDEX.contains(index), "background index {index}");
	(index * 100.0).round_ties_even() as u8
}

impl ScenePixels {
	/// A blank canvas: red from `background_index`, green and blue 0, alpha 255.
	pub fn blank(width: u32, height: u32, background_index: f64) -> Self {
		let red = background_red(background_index);
		let rgba = [red, 0, 0, 255].repeat(width as usize * height as usize);
		Self { width, height, rgba }
	}

	/// Decodes a PNG, JPEG or BMP file into RGBA8.
	pub fn from_image_bytes(bytes: &[u8]) -> Result<Self, image::ImageError> {
		let decoded = image::load_from_memory(bytes)?.into_rgba8();
		let (width, height) = decoded.dimensions();
		Ok(Self { width, height, rgba: decoded.into_raw() })
	}

	/// Bytes per row.
	pub fn stride(&self) -> usize {
		self.width as usize * 4
	}

	fn offset(&self, x: u32, y: u32) -> usize {
		debug_assert!(x < self.width && y < self.height);
		y as usize * self.stride() + x as usize * 4
	}

	/// The pixel's `[r, g, b, a]`.
	pub fn rgba_at(&self, x: u32, y: u32) -> [u8; 4] {
		let i = self.offset(x, y);
		[self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
	}

	pub fn red(&self, x: u32, y: u32) -> u8 {
		self.rgba[self.offset(x, y)]
	}

	pub fn green(&self, x: u32, y: u32) -> u8 {
		self.rgba[self.offset(x, y) + 1]
	}

	pub fn blue(&self, x: u32, y: u32) -> u8 {
		self.rgba[self.offset(x, y) + 2]
	}

	pub fn set_rgba(&mut self, x: u32, y: u32, value: [u8; 4]) {
		let i = self.offset(x, y);
		self.rgba[i..i + 4].copy_from_slice(&value);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn blank_canvas_is_opaque_background() {
		let p = ScenePixels::blank(3, 2, 1.5);
		assert_eq!(p.rgba.len(), 24);
		assert_eq!(p.rgba_at(2, 1), [150, 0, 0, 255]);
		assert_eq!(background_red(1.005), 100);
		assert_eq!(background_red(2.55), 255);
	}

	#[test]
	fn decodes_png() {
		let mut img = image::RgbaImage::new(2, 1);
		img.put_pixel(1, 0, image::Rgba([120, 7, 9, 255]));
		let mut bytes = Vec::new();
		img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
		let p = ScenePixels::from_image_bytes(&bytes).unwrap();
		assert_eq!((p.width, p.height), (2, 1));
		assert_eq!(p.rgba_at(1, 0), [120, 7, 9, 255]);
	}
}
