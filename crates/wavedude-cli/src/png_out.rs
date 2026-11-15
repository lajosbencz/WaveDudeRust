use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result};
use wavedude_export::sinks::png::{PngFilter, encode_png};

/// Writes an opaque RGBA8 picture (`width * height * 4` bytes) as an RGB PNG.
pub(crate) fn write_rgba(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
	let bytes =
		encode_png(rgba, width, height, PngFilter::Paeth).with_context(|| format!("encoding {}", path.display()))?;
	fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

/// Writes `grey` (8 bits, `width * height` bytes) as a greyscale PNG, a third of an RGB one.
pub(crate) fn write_grey(path: &Path, width: u32, height: u32, grey: &[u8]) -> Result<()> {
	let context = || format!("writing {}", path.display());
	let file = File::create(path).with_context(context)?;
	let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
	encoder.set_color(png::ColorType::Grayscale);
	encoder.set_depth(png::BitDepth::Eight);
	let mut writer = encoder.write_header().with_context(context)?;
	writer.write_image_data(grey).with_context(context)?;
	writer.finish().with_context(context)
}
