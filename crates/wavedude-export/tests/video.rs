mod common;

use std::io::Cursor;

use common::*;
use wavedude_export::sinks::FrameSink;
use wavedude_export::sinks::avi::AviWriter;
use wavedude_export::sinks::mov::{MovCodec, MovWriter};
use wavedude_export::sinks::mpeg1::{Mpeg1Writer, frame_rate_code, quantiser_scale};

#[test]
fn mpeg1_quantiser_and_frame_rate_rules() {
	assert_eq!(quantiser_scale(100), 1);
	assert_eq!(quantiser_scale(1), 31);
	assert_eq!(quantiser_scale(70), 6);
	assert_eq!(quantiser_scale(50), 12);
	assert_eq!(frame_rate_code(60.0), 8);
	assert_eq!(frame_rate_code(25.0), 3);
	assert_eq!(frame_rate_code(29.97), 4);
	assert_eq!(frame_rate_code(1.0), 1);
	assert_eq!(frame_rate_code(1000.0), 8);
}

/// The video elementary stream of an MPEG-1 program stream (the payloads of stream 0xE0).
fn demux(ps: &[u8]) -> Vec<u8> {
	let mut es = Vec::new();
	let mut i = 0;
	while i + 4 <= ps.len() {
		assert_eq!(&ps[i..i + 3], &[0, 0, 1], "a start code at {i}");
		match ps[i + 3] {
			0xBA => i += 12,
			0xBB => i += 6 + u16::from_be_bytes([ps[i + 4], ps[i + 5]]) as usize,
			0xB9 => {
				assert_eq!(i + 4, ps.len(), "the program end code ends the file");
				break;
			}
			0xE0 => {
				let len = u16::from_be_bytes([ps[i + 4], ps[i + 5]]) as usize;
				assert!(6 + len <= 65000);
				let body = &ps[i + 6..i + 6 + len];
				let skip = if body[0] == 0x0F { 1 } else { 5 };
				es.extend_from_slice(&body[skip..]);
				i += 6 + len;
			}
			other => panic!("unexpected stream id {other:#x}"),
		}
	}
	es
}

fn count_start_codes(es: &[u8], code: u8) -> usize {
	es.windows(4).filter(|w| w == &[0, 0, 1, code]).count()
}

#[test]
fn mpeg1_stream_structure() {
	let (w, h) = (40, 24);
	let mut writer = Mpeg1Writer::new(Vec::new(), w, h, 25.0, 85).unwrap();
	for t in 0..7 {
		writer.write_frame(&smooth_frame(w, h, t), w, h).unwrap();
	}
	writer.finish().unwrap();
	let ps = writer.into_inner();
	let es = demux(&ps);
	assert_eq!(count_start_codes(&ps, 0xBB), 1, "one system header");
	assert_eq!(count_start_codes(&es, 0xB3), 1, "one sequence header");
	assert_eq!(count_start_codes(&es, 0xB8), 1, "one GOP header");
	assert_eq!(count_start_codes(&es, 0x00), 7, "seven picture headers");
	assert_eq!(count_start_codes(&es, 0xB7), 1, "one sequence end code");
	assert_eq!(count_start_codes(&es, 0x01), 7, "slice 1 a picture");
	assert_eq!(count_start_codes(&es, 0x02), 7, "slice 2 a picture (24 rows padded to 32)");
	assert_eq!(count_start_codes(&es, 0x03), 0);
	assert_eq!(&es[4..7], &[(w >> 4) as u8, ((w & 15) << 4 | h >> 8) as u8, h as u8], "the real size");
	let mut long = Mpeg1Writer::new(Vec::new(), w, h, 25.0, 85).unwrap();
	for t in 0..13 {
		long.write_frame(&smooth_frame(w, h, t), w, h).unwrap();
	}
	long.finish().unwrap();
	let es = demux(&long.into_inner());
	assert_eq!(count_start_codes(&es, 0xB3), 2, "a sequence header every 12 pictures");
	assert_eq!(count_start_codes(&es, 0xB8), 2, "a GOP header every 12 pictures");
}

fn mpeg1_decoded(name: &str, w: u32, h: u32) -> Vec<(Vec<u8>, Vec<u8>)> {
	let dir = scratch(name);
	let frames: Vec<Vec<u8>> = (0..14).map(|t| wave_frame(w, h, t)).collect();
	let path = dir.join("v.mpg");
	let mut writer = Mpeg1Writer::create(&path, w, h, 60.0, 85).unwrap();
	for f in &frames {
		writer.write_frame(f, w, h).unwrap();
	}
	writer.finish().unwrap();
	drop(writer);
	assert_eq!(ffprobe(&path), format!("mpeg1video,{w},{h},14"), "the real size, padded inside");
	let decoded = ffmpeg_frames(&path, w, h);
	std::fs::remove_dir_all(&dir).unwrap();
	assert_eq!(decoded.len(), frames.len());
	decoded.into_iter().zip(frames.iter().map(|f| rgb(f))).collect()
}

#[test]
fn mpeg1_decodes_close_to_the_source() {
	if !has_ffmpeg() {
		return;
	}
	for (d, f) in mpeg1_decoded("mpeg1", 200, 120) {
		let p = psnr(&d, &f);
		assert!(p >= 40.0, "PSNR {p:.2} dB");
	}
	for (d, f) in mpeg1_decoded("mpeg1-odd", 201, 121) {
		let p = psnr(&d, &f);
		assert!(p >= 38.0, "an odd size padded by repetition: PSNR {p:.2} dB");
	}
}

/// Room for the first segment's headers (16.9 KB) and two 63 x 48 frames, four frames in an AVIX segment.
const SMALL_SEGMENTS: u64 = 40_000;

#[test]
fn avi_is_riff_with_odml_segments() {
	let (w, h) = (63, 48);
	let frames: Vec<Vec<u8>> = (0..9).map(|t| busy_frame(w, h, t)).collect();
	let mut writer = AviWriter::new(Cursor::new(Vec::new()), w, h, 30, SMALL_SEGMENTS).unwrap();
	for f in &frames {
		writer.write_frame(f, w, h).unwrap();
	}
	writer.finish().unwrap();
	let data = writer.into_inner().into_inner();
	assert_eq!(&data[..4], b"RIFF");
	assert_eq!(&data[8..12], b"AVI ");
	assert_eq!(&data[12..16], b"LIST");
	assert_eq!(&data[20..28], b"hdrlavih");
	let riffs: Vec<usize> = data.windows(4).enumerate().filter(|(_, w)| w == b"RIFF").map(|(i, _)| i).collect();
	assert!(riffs.len() >= 3, "a small segment limit splits the frames over AVIX segments ({})", riffs.len());
	let mut i = 0;
	let mut segments = 0;
	while i < data.len() {
		assert_eq!(&data[i..i + 4], b"RIFF");
		assert_eq!(&data[i + 8..i + 12], if segments == 0 { b"AVI " } else { b"AVIX" });
		let size = u32::from_le_bytes(data[i + 4..i + 8].try_into().unwrap()) as u64;
		assert!(size + 8 <= SMALL_SEGMENTS);
		i += 8 + size as usize;
		segments += 1;
	}
	assert_eq!(i, data.len(), "the segments tile the file");
	let dmlh = data.windows(4).position(|w| w == b"dmlh").unwrap();
	assert_eq!(u32::from_le_bytes(data[dmlh + 8..dmlh + 12].try_into().unwrap()), 9, "dmlh counts every frame");
	assert_eq!(data.windows(4).filter(|w| w == b"ix00").count(), segments, "one standard index a segment");
	assert_eq!(data.windows(4).filter(|w| w == b"idx1").count(), 1);
}

#[test]
fn avi_decodes_to_identical_frames() {
	if !has_ffmpeg() {
		return;
	}
	let dir = scratch("avi");
	let (w, h) = (63, 48);
	let frames: Vec<Vec<u8>> = (0..9).map(|t| busy_frame(w, h, t)).collect();
	for (name, limit) in [("one.avi", wavedude_export::sinks::avi::SEGMENT_LIMIT), ("split.avi", SMALL_SEGMENTS)] {
		let path = dir.join(name);
		let file = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
		let mut writer = AviWriter::new(file, w, h, 30, limit).unwrap();
		for f in &frames {
			writer.write_frame(f, w, h).unwrap();
		}
		writer.finish().unwrap();
		drop(writer);
		let decoded = ffmpeg_frames(&path, w, h);
		assert_eq!(decoded.len(), frames.len(), "{name}");
		for (d, f) in decoded.iter().zip(&frames) {
			assert_eq!(d, &rgb(f), "{name}");
		}
	}
	std::fs::remove_dir_all(&dir).unwrap();
}

fn mov(codec: MovCodec, frames: &[Vec<u8>], w: u32, h: u32) -> Vec<u8> {
	let mut writer = MovWriter::new(Cursor::new(Vec::new()), w, h, 24, codec, Some(frames.len())).unwrap();
	for f in frames {
		writer.write_frame(f, w, h).unwrap();
	}
	writer.finish().unwrap();
	writer.into_inner().into_inner()
}

#[test]
fn mov_structure_animation_smaller_than_raw_png_first_sample_exact() {
	let (w, h) = (33, 17);
	let frames: Vec<Vec<u8>> = (0..4).map(|t| busy_frame(w, h, t)).collect();
	let rle = mov(MovCodec::Animation, &frames, w, h);
	let top: Vec<[u8; 4]> = atoms(&rle, 0, rle.len()).into_iter().map(|a| a.0).collect();
	assert_eq!(top, [*b"ftyp", *b"moov", *b"free", *b"mdat"], "moov leading, in the reserved room");
	let (mb, me) = find_atom(&rle, &[b"mdat"]).unwrap();
	assert!(me - mb < (w * h * 3) as usize * frames.len(), "Animation is smaller than raw");
	let (sb, _) = find_atom(&rle, &[b"moov", b"trak", b"mdia", b"minf", b"stbl", b"stsd"]).unwrap();
	assert_eq!(&rle[sb + 12..sb + 16], b"rle ");

	let png = mov(MovCodec::Png, &frames, w, h);
	let top: Vec<[u8; 4]> = atoms(&png, 0, png.len()).into_iter().map(|a| a.0).collect();
	assert_eq!(&top[..2], &[*b"ftyp", *b"moov"]);
	let stbl = [b"moov", b"trak", b"mdia", b"minf", b"stbl"];
	let (zb, _) = find_atom(&png, &[stbl.as_slice(), &[b"stsz"]].concat()).unwrap();
	let (cb, _) = find_atom(&png, &[stbl.as_slice(), &[b"co64"]].concat()).unwrap();
	assert_eq!(u32::from_be_bytes(png[zb + 8..zb + 12].try_into().unwrap()), 4);
	let size = u32::from_be_bytes(png[zb + 12..zb + 16].try_into().unwrap()) as usize;
	let offset = u64::from_be_bytes(png[cb + 8..cb + 16].try_into().unwrap()) as usize;
	let first = image::load_from_memory_with_format(&png[offset..offset + size], image::ImageFormat::Png).unwrap();
	assert_eq!(first.into_rgb8().into_raw(), rgb(&frames[0]), "the first sample is the first frame");
}

#[test]
fn mov_without_a_known_count_puts_moov_last() {
	let (w, h) = (8, 8);
	let mut writer = MovWriter::new(Cursor::new(Vec::new()), w, h, 24, MovCodec::Animation, None).unwrap();
	writer.write_frame(&busy_frame(w, h, 0), w, h).unwrap();
	writer.finish().unwrap();
	let data = writer.into_inner().into_inner();
	let top: Vec<[u8; 4]> = atoms(&data, 0, data.len()).into_iter().map(|a| a.0).collect();
	assert_eq!(top, [*b"ftyp", *b"mdat", *b"moov"]);
}

#[test]
fn mov_decodes_to_identical_frames() {
	if !has_ffmpeg() {
		return;
	}
	let dir = scratch("mov");
	let (w, h) = (33, 17);
	let frames: Vec<Vec<u8>> = (0..4).map(|t| busy_frame(w, h, t)).collect();
	for (codec, name, probe) in [(MovCodec::Animation, "rle.mov", "qtrle"), (MovCodec::Png, "png.mov", "png")] {
		let path = dir.join(name);
		std::fs::write(&path, mov(codec, &frames, w, h)).unwrap();
		assert_eq!(ffprobe(&path), format!("{probe},33,17,4"));
		let decoded = ffmpeg_frames(&path, w, h);
		assert_eq!(decoded.len(), frames.len(), "{name}");
		for (d, f) in decoded.iter().zip(&frames) {
			assert_eq!(d, &rgb(f), "{name}");
		}
	}
	std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(feature = "openh264-source")]
fn mp4(path: &std::path::Path, frames: &[Vec<u8>], w: u32, h: u32) {
	use wavedude_export::sinks::mp4::Mp4Writer;
	let spec = wavedude_export::SinkSpec {
		width: w,
		height: h,
		fps: 60,
		quality: 85,
		first_frame: 0,
		frame_count: frames.len(),
	};
	let mut writer = Mp4Writer::create(path, spec, None).unwrap();
	for f in frames {
		writer.write_frame(f, w, h).unwrap();
	}
	writer.finish().unwrap();
}

#[cfg(feature = "openh264-source")]
#[test]
fn mp4_decodes_close_to_the_source_at_the_default_quality() {
	use wavedude_export::sinks::mp4::bitrate;
	assert_eq!(bitrate(1920, 1080, 60, 85), (1920.0 * 1080.0 * 60.0 * 0.01 * 50f64.powf(0.85)).round() as u32);
	assert_eq!(bitrate(16, 16, 1, 1), 300_000);
	assert_eq!(bitrate(4096, 2304, 240, 100), 200_000_000);
	if !has_ffmpeg() {
		return;
	}
	let dir = scratch("mp4");
	let (w, h) = (640, 360);
	let frames: Vec<Vec<u8>> = (0..30).map(|t| wave_frame(w, h, t)).collect();
	let path = dir.join("v.mp4");
	mp4(&path, &frames, w, h);
	assert_eq!(ffprobe(&path), "h264,640,360,30");
	let decoded = ffmpeg_frames(&path, w, h);
	assert_eq!(decoded.len(), frames.len());
	let worst = decoded.iter().zip(&frames).map(|(d, f)| psnr(d, &rgb(f))).fold(f64::INFINITY, f64::min);
	assert!(worst >= 40.0, "worst PSNR {worst:.2} dB");
	std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(feature = "openh264-source")]
#[test]
fn mp4_pads_an_odd_size_to_even() {
	if !has_ffmpeg() {
		return;
	}
	let dir = scratch("mp4-odd");
	let (w, h) = (101, 61);
	let frames: Vec<Vec<u8>> = (0..10).map(|t| smooth_frame(w, h, t)).collect();
	let path = dir.join("v.mp4");
	mp4(&path, &frames, w, h);
	assert_eq!(ffprobe(&path), "h264,102,62,10", "an odd size padded to even");
	let decoded = ffmpeg_frames(&path, w + 1, h + 1);
	let padded = |f: &[u8]| -> Vec<u8> {
		let px = |x: u32, y: u32| {
			let i = ((y.min(h - 1) * w + x.min(w - 1)) * 4) as usize;
			[f[i], f[i + 1], f[i + 2]]
		};
		(0..h + 1).flat_map(|y| (0..w + 1).flat_map(move |x| px(x, y))).collect()
	};
	for (d, f) in decoded.iter().zip(&frames) {
		let p = psnr(d, &padded(f));
		assert!(p >= 35.0, "the last column and row repeated: PSNR {p:.2} dB");
	}
	std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_parallel_mov_writes_the_serial_bytes() {
	use wavedude_export::EncoderPool;
	use wavedude_export::sinks::mov::ParallelMov;
	let (w, h) = (33, 17);
	let frames: Vec<Vec<u8>> = (0..9).map(|t| busy_frame(w, h, t)).collect();
	for codec in [MovCodec::Animation, MovCodec::Png] {
		let writer = MovWriter::new(Cursor::new(Vec::new()), w, h, 24, codec, Some(frames.len())).unwrap();
		let mut parallel = ParallelMov::new(writer, &EncoderPool::default()).unwrap();
		for f in &frames {
			parallel.write_frame(f, w, h).unwrap();
		}
		parallel.finish().unwrap();
		assert_eq!(parallel.frames_written(), frames.len());
		assert!(parallel.into_inner().into_inner() == mov(codec, &frames, w, h), "{codec:?}");
	}
}

#[test]
fn mpeg1_pts_leads_the_scr_by_a_tenth_of_a_second() {
	let (w, h) = (32, 16);
	let mut writer = Mpeg1Writer::new(Vec::new(), w, h, 25.0, 85).unwrap();
	for t in 0..3 {
		writer.write_frame(&smooth_frame(w, h, t), w, h).unwrap();
	}
	writer.finish().unwrap();
	let ps = writer.into_inner();
	let timestamp = |b: &[u8]| {
		((b[0] as u64 >> 1) & 7) << 30
			| (u16::from_be_bytes([b[1], b[2]]) as u64 >> 1) << 15
			| u16::from_be_bytes([b[3], b[4]]) as u64 >> 1
	};
	let mut scr = None;
	let mut pairs = Vec::new();
	let mut i = 0;
	while i + 4 <= ps.len() {
		match &ps[i..i + 4] {
			[0, 0, 1, 0xBA] => scr = Some(timestamp(&ps[i + 4..i + 9])),
			[0, 0, 1, 0xE0] if ps[i + 6] >> 4 == 0b0010 => pairs.push((scr.unwrap(), timestamp(&ps[i + 6..i + 11]))),
			_ => {}
		}
		i += 1;
	}
	assert_eq!(pairs, [(0, 9000), (3600, 12600), (7200, 16200)]);
}

#[test]
fn avi_refuses_a_frame_of_4_gib() {
	assert!(AviWriter::new(Cursor::new(Vec::new()), 40_000, 40_000, 30, 1 << 30).is_err());
	assert!(AviWriter::new(Cursor::new(Vec::new()), 1920, 1080, 30, 1 << 30).is_ok());
}

/// Cisco's binary downloaded, refused when missing, and encoding a video that decodes like the compiled-in one.
#[cfg(all(feature = "mp4", not(feature = "openh264-source")))]
#[test]
#[ignore = "downloads Cisco's OpenH264; run with --ignored"]
fn mp4_through_ciscos_downloaded_binary() {
	use wavedude_export::h264;
	use wavedude_export::sinks::mp4::Mp4Writer;
	let dir = scratch("mp4-cisco");
	let spec =
		wavedude_export::SinkSpec { width: 640, height: 360, fps: 30, quality: 85, first_frame: 0, frame_count: 10 };
	let path = dir.join("v.mp4");
	assert!(matches!(Mp4Writer::create(&path, spec, None), Err(wavedude_export::ExportError::H264Missing)));
	let library = h264::download(&dir.join("codec")).unwrap();
	assert_eq!(h264::installed(&dir.join("codec")), Some(library.clone()));
	let mut writer = Mp4Writer::create(&path, spec, Some(&library)).unwrap();
	let frames: Vec<Vec<u8>> = (0..10).map(|t| wave_frame(640, 360, t)).collect();
	for f in &frames {
		writer.write_frame(f, 640, 360).unwrap();
	}
	writer.finish().unwrap();
	if has_ffmpeg() {
		assert_eq!(ffprobe(&path), "h264,640,360,10");
		let decoded = ffmpeg_frames(&path, 640, 360);
		let worst = decoded.iter().zip(&frames).map(|(d, f)| psnr(d, &rgb(f))).fold(f64::INFINITY, f64::min);
		assert!(worst >= 40.0, "worst PSNR {worst:.2} dB");
	}
	h264::delete(&dir.join("codec")).unwrap();
	assert_eq!(h264::installed(&dir.join("codec")), None);
	std::fs::remove_dir_all(&dir).unwrap();
}
