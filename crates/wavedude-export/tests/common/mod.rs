#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use wavedude_export::fs::{OutputFs, StdFs};

/// The platform's file system, as an export takes it.
pub(crate) fn std_fs() -> Arc<dyn OutputFs> {
	Arc::new(StdFs)
}

/// A buffered file created at `path`, for a writer.
pub(crate) fn file(path: &Path) -> std::io::BufWriter<std::fs::File> {
	std::io::BufWriter::new(std::fs::File::create(path).unwrap())
}

/// A fresh, empty scratch folder under cargo's target tmp dir.
pub(crate) fn scratch(name: &str) -> PathBuf {
	let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("wavedude-export").join(name);
	let _ = std::fs::remove_dir_all(&dir);
	std::fs::create_dir_all(&dir).unwrap();
	dir
}

/// A smooth RGBA8 test picture that moves with `t`: gradients and a soft blob, no hard edges.
pub(crate) fn smooth_frame(w: u32, h: u32, t: u32) -> Vec<u8> {
	let mut out = Vec::with_capacity((w * h * 4) as usize);
	for y in 0..h {
		for x in 0..w {
			let fx = x as f32 / w as f32;
			let fy = y as f32 / h as f32;
			let d = ((fx - 0.3 - t as f32 * 0.02).powi(2) + (fy - 0.5).powi(2)).sqrt();
			let blob = (-d * d * 12.0).exp();
			out.extend_from_slice(&[
				(40.0 + 180.0 * fx) as u8,
				(30.0 + 200.0 * blob) as u8,
				(60.0 + 150.0 * fy) as u8,
				255,
			]);
		}
	}
	out
}

/// A picture with hard edges and noise-like detail: exercises the lossless paths.
pub(crate) fn busy_frame(w: u32, h: u32, t: u32) -> Vec<u8> {
	let mut out = Vec::with_capacity((w * h * 4) as usize);
	let mut s = 0x9E37_79B9u32 ^ t.wrapping_mul(0x85EB_CA6B);
	for y in 0..h {
		for x in 0..w {
			s ^= s << 13;
			s ^= s >> 17;
			s ^= s << 5;
			let flat = (x / 7 + y / 5 + t).is_multiple_of(3);
			let v = if flat { [10, 200, 30] } else { [(s & 0xFF) as u8, (x * 3) as u8, (y * 5 + t) as u8] };
			out.extend_from_slice(&[v[0], v[1], v[2], 255]);
		}
	}
	out
}

/// A rendered field picture as an export makes it: two point sources' circular waves with a 1/sqrt(r) fall-off,
/// coloured by the default field scheme at the default brightness and black level.
pub(crate) fn wave_frame(w: u32, h: u32, t: u32) -> Vec<u8> {
	use wavedude_core::engine::types::SampleKind;
	use wavedude_core::model::DisplayLayer;
	use wavedude_core::render::{HalfTable, render_half};
	let display = DisplayLayer::field_default();
	let scheme =
		wavedude_core::io::schemes::find(wavedude_core::io::schemes::built_in(), &display.color_scheme).unwrap();
	let table = HalfTable::cached(scheme, display.black_level, display.brightness as f32, SampleKind::Field);
	let sources = [(0.3, 0.4), (0.7, 0.6)];
	let plane: Vec<half::f16> = (0..h)
		.flat_map(|y| (0..w).map(move |x| (x as f32, y as f32)))
		.map(|(x, y)| {
			let v: f32 = sources
				.iter()
				.map(|(sx, sy)| {
					let r = (x - sx * w as f32).hypot(y - sy * h as f32) + 1.0;
					(r * 0.35 - t as f32 * 0.4).sin() / r.sqrt()
				})
				.sum();
			half::f16::from_f32(v * 0.01)
		})
		.collect();
	render_half(&plane, &table)
}

pub(crate) fn rgb(rgba: &[u8]) -> Vec<u8> {
	rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect()
}

/// PSNR in dB of two RGB8 pictures; infinity when equal.
pub(crate) fn psnr(a: &[u8], b: &[u8]) -> f64 {
	assert_eq!(a.len(), b.len());
	let mse = a.iter().zip(b).map(|(x, y)| (*x as f64 - *y as f64).powi(2)).sum::<f64>() / a.len() as f64;
	if mse == 0.0 { f64::INFINITY } else { 10.0 * (255.0f64 * 255.0 / mse).log10() }
}

pub(crate) fn has_ffmpeg() -> bool {
	let found = Command::new("which").arg("ffmpeg").output().is_ok_and(|o| o.status.success());
	if !found {
		eprintln!("ffmpeg is not on the PATH: skipping the decode check");
	}
	found
}

/// Every frame ffmpeg decodes from `path`, as RGB8 of `w x h`.
pub(crate) fn ffmpeg_frames(path: &Path, w: u32, h: u32) -> Vec<Vec<u8>> {
	let out = Command::new("ffmpeg")
		.args(["-v", "error", "-i"])
		.arg(path)
		.args(["-sws_flags", "bicubic+accurate_rnd+full_chroma_int", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
		.output()
		.expect("ffmpeg runs");
	assert!(out.status.success(), "ffmpeg failed: {}", String::from_utf8_lossy(&out.stderr));
	assert!(out.stderr.is_empty(), "ffmpeg complained: {}", String::from_utf8_lossy(&out.stderr));
	let size = (w * h * 3) as usize;
	assert_eq!(out.stdout.len() % size, 0);
	out.stdout.chunks_exact(size).map(<[u8]>::to_vec).collect()
}

/// The codec name and frame count ffprobe reports.
pub(crate) fn ffprobe(path: &Path) -> String {
	let out = Command::new("ffprobe")
		.args(["-v", "error", "-select_streams", "v:0", "-count_frames", "-show_entries"])
		.arg("stream=codec_name,width,height,nb_read_frames")
		.args(["-of", "csv=p=0"])
		.arg(path)
		.output()
		.expect("ffprobe runs");
	String::from_utf8_lossy(&out.stdout).trim().trim_end_matches(',').to_owned()
}

/// Atoms of a `QuickTime` file at one level: (kind, start of body, end).
pub(crate) fn atoms(data: &[u8], from: usize, to: usize) -> Vec<([u8; 4], usize, usize)> {
	let mut out = Vec::new();
	let mut i = from;
	while i + 8 <= to {
		let size = u32::from_be_bytes(data[i..i + 4].try_into().unwrap()) as usize;
		let kind: [u8; 4] = data[i + 4..i + 8].try_into().unwrap();
		let (body, end) = if size == 1 {
			(i + 16, i + u64::from_be_bytes(data[i + 8..i + 16].try_into().unwrap()) as usize)
		} else {
			(i + 8, i + size)
		};
		out.push((kind, body, end));
		i = end;
	}
	out
}

pub(crate) fn find_atom(data: &[u8], path: &[&[u8; 4]]) -> Option<(usize, usize)> {
	let (mut from, mut to) = (0, data.len());
	for kind in path {
		let (_, b, e) = atoms(data, from, to).into_iter().find(|a| &a.0 == *kind)?;
		(from, to) = (b, e);
	}
	Some((from, to))
}
