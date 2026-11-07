//! Baseline JFIF JPEG, 4:4:4, Annex K tables at the IJG quality scaling, through `jpeg-encoder`.

use std::io::Write;

use jpeg_encoder::{ColorType, Encoder, SamplingFactor};

use crate::{ExportError, Result};

/// Writes an RGBA8 picture as a JPEG at quality 1..100 to `out`, a restart marker every two MCU rows; the caller
/// flushes.
pub fn write_jpeg(out: impl Write, rgba: &[u8], width: u32, height: u32, quality: u32) -> Result<()> {
	let (Ok(w), Ok(h)) = (u16::try_from(width), u16::try_from(height)) else {
		return Err(ExportError::Size { format: "JPEG", width, height, limit: "up to 65535 x 65535" });
	};
	let mut encoder = Encoder::new(out, quality as u8);
	encoder.set_sampling_factor(SamplingFactor::R_4_4_4);
	encoder.set_restart_interval((2 * width.div_ceil(8)) as u16);
	encoder.encode(rgba, w, h, ColorType::Rgba)?;
	Ok(())
}

/// An RGBA8 picture as a JPEG file's bytes.
pub fn encode_jpeg(rgba: &[u8], width: u32, height: u32, quality: u32) -> Result<Vec<u8>> {
	let mut out = Vec::with_capacity(rgba.len() / 8);
	write_jpeg(&mut out, rgba, width, height, quality)?;
	Ok(out)
}
