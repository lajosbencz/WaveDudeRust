mod common;

use common::*;
use exr::prelude::{FlatSamples, ReadChannels, ReadLayers, read};
use half::f16;
use wavedude_export::sinks::exr::write_exr;
use wavedude_export::sinks::jpeg::encode_jpeg;
use wavedude_export::sinks::png::{PngFilter, encode_png};
use wavedude_export::sinks::sequence::{ImageKind, ImageSink, Placement};
use wavedude_export::sinks::{EncoderPool, FrameSink, SinkSpec};

#[test]
fn png_decodes_pixel_for_pixel_with_either_filter() {
	let (w, h) = (37, 23);
	let frame = busy_frame(w, h, 1);
	for filter in [PngFilter::Paeth, PngFilter::None] {
		let bytes = encode_png(&frame, w, h, filter).unwrap();
		assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
		let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).unwrap();
		assert!(matches!(decoded, image::DynamicImage::ImageRgb8(_)), "24-bit RGB");
		assert_eq!(decoded.into_rgb8().into_raw(), rgb(&frame), "{filter:?}");
	}
}

/// The components' sampling factors of the first SOF marker.
fn sof_sampling(jpeg: &[u8]) -> Vec<u8> {
	let mut i = 2;
	while i + 4 < jpeg.len() {
		assert_eq!(jpeg[i], 0xFF);
		let marker = jpeg[i + 1];
		let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
		if marker == 0xC0 {
			let count = jpeg[i + 9] as usize;
			return (0..count).map(|c| jpeg[i + 10 + c * 3 + 1]).collect();
		}
		assert_ne!(marker, 0xC2, "baseline, not progressive");
		i += 2 + len;
	}
	panic!("no SOF0");
}

#[test]
fn jpeg_is_baseline_444_and_close_to_the_source() {
	let (w, h) = (96, 64);
	let frame = smooth_frame(w, h, 3);
	let bytes = encode_jpeg(&frame, w, h, 90).unwrap();
	assert_eq!(&bytes[..2], &[0xFF, 0xD8], "SOI");
	assert_eq!(&bytes[bytes.len() - 2..], &[0xFF, 0xD9], "EOI");
	assert_eq!(sof_sampling(&bytes), vec![0x11, 0x11, 0x11], "4:4:4");
	let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Jpeg).unwrap().into_rgb8();
	let p = psnr(&decoded.into_raw(), &rgb(&frame));
	assert!(p >= 40.0, "PSNR {p:.2} dB");
}

#[test]
fn exr_is_a_version_2_scanline_file_of_one_uncompressed_half_channel() {
	let dir = scratch("exr");
	let (w, h) = (13, 7);
	let halves: Vec<f16> = (0..w * h).map(|i| f16::from_bits((i * 709 + 1) as u16 & 0x7BFF)).collect();
	let path = dir.join("name_0001.exr");
	let mut out = file(&path);
	write_exr(&mut out, &halves, w, h).unwrap();
	drop(out);
	let bytes = std::fs::read(&path).unwrap();
	assert_eq!(&bytes[..4], &[0x76, 0x2f, 0x31, 0x01]);
	assert_eq!(&bytes[4..8], &[2, 0, 0, 0], "version 2, single-part scanline");
	let image = read()
		.no_deep_data()
		.largest_resolution_level()
		.all_channels()
		.all_layers()
		.all_attributes()
		.from_file(&path)
		.unwrap();
	assert_eq!(image.layer_data.len(), 1);
	let layer = &image.layer_data[0];
	assert_eq!(layer.encoding.compression, exr::compression::Compression::Uncompressed);
	assert_eq!(layer.channel_data.list.len(), 1);
	let channel = &layer.channel_data.list[0];
	assert!(channel.name == *"Y");
	match &channel.sample_data {
		FlatSamples::F16(read) => {
			assert_eq!(
				read.iter().map(|h| h.to_bits()).collect::<Vec<_>>(),
				halves.iter().map(|h| h.to_bits()).collect::<Vec<_>>()
			);
		}
		other => panic!("HALF expected, got {other:?}"),
	}
	std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_sequence_goes_into_a_folder_of_its_name_and_a_still_is_one_file() {
	let dir = scratch("sequence");
	let (w, h) = (16, 8);
	let spec = SinkSpec { width: w, height: h, fps: 30, quality: 90, first_frame: 5, frame_count: 3 };
	let chosen = dir.join("chosenname.png");
	let mut sink = ImageSink::new(
		ImageKind::Png,
		std_fs(),
		Placement::Sequence { chosen, first: 5, planned: 3 },
		spec,
		&EncoderPool::default(),
	)
	.unwrap();
	let frames: Vec<Vec<u8>> = (0..3).map(|t| busy_frame(w, h, t)).collect();
	for f in &frames {
		sink.write_frame(f, w, h).unwrap();
	}
	sink.finish().unwrap();
	let folder = dir.join("chosenname");
	let mut names: Vec<String> =
		std::fs::read_dir(&folder).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
	names.sort();
	assert_eq!(names, ["chosenname_0005.png", "chosenname_0006.png", "chosenname_0007.png"]);
	let second = image::open(folder.join("chosenname_0006.png")).unwrap().into_rgb8().into_raw();
	assert_eq!(second, rgb(&frames[1]), "the second file holds the second frame");

	let still = dir.join("ball_field_0655.jpg");
	let mut sink = ImageSink::new(
		ImageKind::Jpeg { quality: 95 },
		std_fs(),
		Placement::Still(still.clone()),
		spec,
		&EncoderPool::default(),
	)
	.unwrap();
	sink.write_frame(&frames[0], w, h).unwrap();
	assert!(sink.write_frame(&frames[1], w, h).is_err(), "a still holds one frame");
	sink.finish().unwrap();
	assert!(still.is_file());
	std::fs::remove_dir_all(&dir).unwrap();
}
