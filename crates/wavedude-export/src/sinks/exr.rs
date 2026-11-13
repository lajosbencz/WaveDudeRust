//! `OpenEXR` files of a stored plane: single-part scanline, uncompressed, one HALF channel `Y`, the halves as stored.

use std::io::{Seek, Write};

use exr::prelude::{AnyChannel, AnyChannels, Encoding, FlatSamples, Image, LayerAttributes, Text, WritableImage};
use half::f16;

use crate::Result;

/// Writes `halves` (`width * height`, row-major, top row first) as an EXR file to `out`.
pub fn write_exr(out: impl Write + Seek, halves: &[f16], width: u32, height: u32) -> Result<()> {
	write_exr_owned(out, halves.to_vec(), width, height).0
}

/// [`write_exr`] of an owned plane, handed back for reuse whatever the outcome.
pub fn write_exr_owned(out: impl Write + Seek, halves: Vec<f16>, width: u32, height: u32) -> (Result<()>, Vec<f16>) {
	let channel = AnyChannel::new("Y", FlatSamples::F16(halves));
	let attributes = LayerAttributes { comments: Some(Text::from("WaveDude Next")), ..LayerAttributes::default() };
	let layer = exr::prelude::Layer::new(
		(width as usize, height as usize),
		attributes,
		Encoding::UNCOMPRESSED,
		AnyChannels::sort(vec![channel].into()),
	);
	let image = Image::from_layer(layer);
	let written = image.write().to_buffered(out).map_err(Into::into);
	let Some(FlatSamples::F16(halves)) = image.layer_data.channel_data.list.into_iter().next().map(|c| c.sample_data)
	else {
		unreachable!("the image was built from one F16 channel")
	};
	(written, halves)
}
