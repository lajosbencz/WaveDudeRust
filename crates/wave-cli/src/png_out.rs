use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result};

/// Writes `rgba` (8 bits per channel, `width * height * 4` bytes) as a PNG.
pub fn write_rgba(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
	let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
	let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
	encoder.set_color(png::ColorType::Rgba);
	encoder.set_depth(png::BitDepth::Eight);
	finish(path, encoder, rgba)
}

/// Writes `grey` (8 bits, `width * height` bytes) as a greyscale PNG.
pub fn write_grey(path: &Path, width: u32, height: u32, grey: &[u8]) -> Result<()> {
	let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
	let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
	encoder.set_color(png::ColorType::Grayscale);
	encoder.set_depth(png::BitDepth::Eight);
	finish(path, encoder, grey)
}

/// Writes the header, the image data and IEND and flushes, so a full disk is an error.
fn finish(path: &Path, encoder: png::Encoder<BufWriter<File>>, data: &[u8]) -> Result<()> {
	let context = || format!("writing {}", path.display());
	let mut writer = encoder.write_header().with_context(context)?;
	writer.write_image_data(data).with_context(context)?;
	writer.finish().with_context(context)
}
