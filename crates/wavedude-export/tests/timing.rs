//! Throughput figures, run with `cargo test -p wavedude-export --release --test timing -- --ignored --nocapture`.
mod common;

use std::time::{Duration, Instant};

use common::*;
use wavedude_export::sinks::{self, SinkSpec, jpeg::encode_jpeg};
use wavedude_export::{EncoderPool, ExportFormat};

const W: u32 = 1920;
const H: u32 = 1080;

fn frames(n: u32) -> Vec<Vec<u8>> {
	(0..n).map(|t| wave_frame(W, H, t)).collect()
}

fn per_frame(elapsed: Duration, n: usize) -> f64 {
	elapsed.as_secs_f64() * 1000.0 / n as f64
}

fn spec(n: usize) -> SinkSpec {
	SinkSpec { width: W, height: H, fps: 30, quality: 90, first_frame: 0, frame_count: n }
}

/// C#: in-house 9.8 ms a frame on ten cores, 15 in a sequence job.
#[test]
#[ignore = "timing for the user: cargo test -p wavedude-export --release --test timing -- --ignored --nocapture"]
fn jpeg_1080p_444() {
	let frames = frames(8);
	encode_jpeg(&frames[0], W, H, 90).unwrap();
	let start = Instant::now();
	for f in &frames {
		encode_jpeg(f, W, H, 90).unwrap();
	}
	eprintln!("JPEG 1080p 4:4:4 q90, one encode: {:.1} ms", per_frame(start.elapsed(), frames.len()));

	let n = 48;
	let dir = scratch("timing_jpeg");
	let mut sink =
		sinks::open(ExportFormat::Jpeg, &dir.join("seq.jpg"), spec(n), false, &EncoderPool::default(), None).unwrap();
	let start = Instant::now();
	for i in 0..n {
		sink.write_frame(&frames[i % frames.len()], W, H).unwrap();
	}
	sink.finish().unwrap();
	eprintln!("JPEG 1080p sequence: {:.1} ms a frame", per_frame(start.elapsed(), n));
}

/// C#: an MP4 job 16 ms a frame.
#[cfg(feature = "openh264-source")]
#[test]
#[ignore = "timing for the user: cargo test -p wavedude-export --release --test timing -- --ignored --nocapture"]
fn mp4_1080p_per_frame() {
	let frames = frames(8);
	let n = 60;
	let path = scratch("timing_mp4").join("run.mp4");
	let mut sink = sinks::open(ExportFormat::Mp4, &path, spec(n), false, &EncoderPool::default(), None).unwrap();
	sink.write_frame(&frames[0], W, H).unwrap();
	let start = Instant::now();
	for i in 1..n {
		sink.write_frame(&frames[i % frames.len()], W, H).unwrap();
	}
	sink.finish().unwrap();
	eprintln!("MP4 1080p: {:.1} ms a frame", per_frame(start.elapsed(), n - 1));
}

#[test]
#[ignore = "timing for the user: cargo test -p wavedude-export --release --test timing -- --ignored --nocapture"]
fn mpeg1_1080p_per_frame() {
	let frames = frames(8);
	let n = 36;
	let path = scratch("timing_mpeg1").join("run.mpg");
	let mut sink = sinks::open(ExportFormat::Mpeg1, &path, spec(n), false, &EncoderPool::default(), None).unwrap();
	let start = Instant::now();
	for i in 0..n {
		sink.write_frame(&frames[i % frames.len()], W, H).unwrap();
	}
	sink.finish().unwrap();
	eprintln!("MPEG-1 1080p: {:.1} ms a frame", per_frame(start.elapsed(), n));
}
