//! PNG files: signature, IHDR (8-bit RGB, no interlace), IDAT, IEND, through the `png` crate.

use std::cell::RefCell;
use std::io::Write;

use png::{BitDepth, ColorType, Compression, Encoder, Filter};

use crate::Result;

/// Row filter of a PNG: Paeth for sequences and stills, none for MOV samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PngFilter {
	Paeth,
	None,
}

thread_local! {
	/// The RGB rows of the picture being encoded on this thread.
	static RGB: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Writes an RGBA8 picture as a 24-bit RGB PNG to `out`, through IEND; the caller flushes.
pub fn write_png(out: impl Write, rgba: &[u8], width: u32, height: u32, filter: PngFilter) -> Result<()> {
	let mut encoder = Encoder::new(out, width, height);
	encoder.set_color(ColorType::Rgb);
	encoder.set_depth(BitDepth::Eight);
	encoder.set_compression(Compression::Balanced);
	encoder.set_filter(match filter {
		PngFilter::Paeth => Filter::Paeth,
		PngFilter::None => Filter::NoFilter,
	});
	let mut writer = encoder.write_header()?;
	RGB.with_borrow_mut(|rgb| {
		rgb.resize(rgba.len() / 4 * 3, 0);
		for (d, s) in rgb.as_chunks_mut::<3>().0.iter_mut().zip(rgba.as_chunks::<4>().0) {
			d.copy_from_slice(&s[..3]);
		}
		writer.write_image_data(rgb)
	})?;
	writer.finish()?;
	Ok(())
}

/// An RGBA8 picture as a PNG file's bytes in `out`, cleared first.
pub fn encode_png_into(rgba: &[u8], width: u32, height: u32, filter: PngFilter, out: &mut Vec<u8>) -> Result<()> {
	out.clear();
	write_png(&mut *out, rgba, width, height, filter)
}

/// An RGBA8 picture as a 24-bit RGB PNG file's bytes.
pub fn encode_png(rgba: &[u8], width: u32, height: u32, filter: PngFilter) -> Result<Vec<u8>> {
	let mut out = Vec::with_capacity(rgba.len() / 2);
	encode_png_into(rgba, width, height, filter, &mut out)?;
	Ok(out)
}
