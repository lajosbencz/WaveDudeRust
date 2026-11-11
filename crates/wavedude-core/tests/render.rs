#![allow(clippy::print_stdout, reason = "the measurements are printed for the reader")]
//! The Rendering guarantees of docs/spec/70-guarantees.md.

use std::time::Instant;

use wavedude_core::engine::SampleKind;
use wavedude_core::f16;
use wavedude_core::model::{ColorScheme, Rgb, Stop};
use wavedude_core::render::screen::ScreenColors;
use wavedude_core::render::{
	DeepHalfTable, DitherDepth, HalfTable, Palette, ResampleAxis, ScreenRenderer, field_index, fit_brightness,
	fit_brightness_f32, intensity_index, intensity_root_index, render_half, render_indexed,
};

fn scheme(colors: &[(f64, u32)]) -> ColorScheme {
	ColorScheme {
		stops: colors
			.iter()
			.map(|&(at, c)| Stop { at, color: Rgb::new((c >> 16) as u8, (c >> 8) as u8, c as u8) })
			.collect(),
		..ColorScheme::default()
	}
}

/// The shipped Inferno scheme's stops.
fn inferno() -> ColorScheme {
	scheme(&[
		(0.0, 0x000003),
		(0.05905511811023622, 0x090621),
		(0.09448818897637795, 0x140B36),
		(0.16141732283464566, 0x300A5C),
		(0.20866141732283464, 0x450A69),
		(0.2637795275590551, 0x5B116E),
		(0.3858267716535433, 0x8D2369),
		(0.5354330708661418, 0xC83E4B),
		(0.6614173228346457, 0xEC6726),
		(0.7480314960629921, 0xF98C09),
		(0.7795275590551181, 0xFB9B06),
		(0.8031496062992126, 0xFBA60B),
		(0.8740157480314961, 0xF8C931),
		(0.905511811023622, 0xF5D948),
		(0.952755905511811, 0xF1EE74),
		(1.0, 0xFCFEA4),
	])
}

/// Nine stops evenly spaced, red rising, blue falling, green 0.
fn nine_stops() -> ColorScheme {
	let stops: Vec<(f64, u32)> = (0..9).map(|i| (i as f64 / 8.0, (i * 30) << 16 | (255 - i * 30))).collect();
	scheme(&stops)
}

fn grey(level: u8) -> [u8; 4] {
	[level, level, level, 255]
}

fn h(v: f32) -> f16 {
	f16::from_f32(v)
}

fn pixels(rgba: &[u8]) -> Vec<[u8; 4]> {
	bytemuck::cast_slice(rgba).to_vec()
}

fn noise(len: usize, mut state: u32) -> impl Iterator<Item = f64> {
	(0..len).map(move |_| {
		state ^= state << 13;
		state ^= state >> 17;
		state ^= state << 5;
		state as f64 / u32::MAX as f64
	})
}

#[test]
fn field_index_on_grey() {
	let palette = Palette::new(&ColorScheme::grey(), 0.0);
	assert_eq!(field_index(h(0.0), 1.0), 127);
	assert_eq!(palette.color(127), grey(127));
	assert_eq!(field_index(h(0.5), 1.0), 190);
	assert_eq!(palette.color(190), grey(190));
	assert_eq!(field_index(h(-2.0), 1.0), 0);
	assert_eq!(field_index(h(2.0), 1.0), 254);
	assert_eq!(palette.color(0), grey(0));
	assert_eq!(palette.color(254), grey(254));
	let table = HalfTable::new(&ColorScheme::grey(), 0.0, 1.0, SampleKind::Field);
	let picture = pixels(&render_half(&[h(0.0), h(0.5), h(-2.0), h(1.0)], &table));
	assert_eq!(picture, [grey(127), grey(191), grey(0), grey(254)]);
}

#[test]
fn intensity_on_grey() {
	let as_i = HalfTable::new(&ColorScheme::grey(), 0.0, 1.0, SampleKind::Intensity);
	let picture = pixels(&render_half(&[h(0.0), h(-1.0), h(0.25), h(9.0)], &as_i));
	assert_eq!(picture, [grey(0), grey(0), grey(127), grey(254)]);
	let as_root = HalfTable::new(&ColorScheme::grey(), 0.0, 1.0, SampleKind::IntensityRoot);
	let picture = pixels(&render_half(&[h(0.0), h(-1.0), h(0.5), h(3.0)], &as_root));
	assert_eq!(picture, [grey(0), grey(0), grey(127), grey(254)]);
	assert_eq!([intensity_index(0.0, 1.0), intensity_index(-1.0, 1.0), intensity_index(0.25, 1.0)], [0, 0, 127]);
	assert_eq!(intensity_root_index(h(0.5), 1.0), 127);
	assert_eq!(intensity_index(9.0, 1.0), 254);
}

#[test]
fn a_stop_schemes_palette_hits_its_stops() {
	let s = nine_stops();
	let palette = Palette::new(&s, 0.0);
	let rgba = |i: usize| {
		let c = s.stops[i].color;
		[c.r, c.g, c.b, 255]
	};
	assert_eq!(palette.color(0), rgba(0));
	assert_eq!(palette.color(127), rgba(4));
	assert_eq!(palette.color(254), rgba(8));
	let picture = pixels(&render_indexed(&[0, 127, 254, 255], &palette));
	assert_eq!(picture, [palette.color(0), palette.color(127), palette.color(254), palette.color(254)]);
}

#[test]
fn black_level_lifts_a_zero_channel() {
	let s = nine_stops();
	assert_eq!(Palette::new(&s, 0.5).color(0)[1], 127);
	assert_eq!(HalfTable::new(&s, 0.5, 1.0, SampleKind::IntensityRoot).entries()[0][1], 128);
}

#[test]
fn the_deep_table_rounds_to_the_8_bit_one() {
	let s = inferno();
	for kind in [SampleKind::Field, SampleKind::IntensityRoot] {
		let table = HalfTable::new(&s, 0.0, 1.0, kind);
		let deep = DeepHalfTable::new(&s, 0.0, 1.0, kind);
		for bits in 0..=u16::MAX {
			let v = f16::from_bits(bits);
			let eight = table.color(v);
			for (c, d) in deep.channels(v).into_iter().enumerate() {
				assert!(d <= 65280);
				assert!(
					(eight[c] as f64 - d as f64 / 256.0).abs() <= 0.5 + 1.0 / 256.0,
					"{kind:?} {bits:#06x} channel {c}"
				);
			}
		}
	}
}

#[test]
fn inferno_dark_start_has_every_blue_level() {
	let s = inferno();
	let palette = Palette::new(&s, 0.0);
	let mut palette_blues: Vec<u8> = (0..16).map(|i| palette.color(i)[2]).collect();
	palette_blues.dedup();
	assert_eq!(palette_blues.len(), 16);
	assert!(palette_blues.windows(2).any(|w| w[1] - w[0] > 1), "the palette skips blue levels");
	let table = HalfTable::new(&s, 0.0, 1.0, SampleKind::IntensityRoot);
	let mut blues: Vec<u8> = (0..=u16::MAX)
		.map(f16::from_bits)
		.filter(|v| v.to_f64() > 0.0 && v.to_f64() <= 15.0 / 254.0)
		.map(|v| table.color(v)[2])
		.collect();
	blues.sort_unstable();
	blues.dedup();
	assert_eq!(blues, (3..=33).collect::<Vec<u8>>());
}

#[test]
fn screen_at_the_samples_size_is_the_half_table_picture() {
	let s = inferno();
	let (w, ht) = (61, 45);
	let plane: Vec<f16> = noise(w * ht, 5).map(|v| f16::from_f64(v * 0.2)).collect();
	let table = HalfTable::new(&s, 0.0, 3.0, SampleKind::IntensityRoot);
	let direct = render_half(&plane, &table);
	let screen = ScreenRenderer::new(w, ht, w, ht).render_to_vec(&plane, ScreenColors::Plain(&table));
	assert_eq!(screen, direct);
}

#[test]
fn screen_halving_blends_energy() {
	let s = inferno();
	let plane: Vec<f16> = noise(60 * 44, 7).map(|v| f16::from_f64(v * 0.2)).collect();
	let table = HalfTable::new(&s, 0.0, 3.0, SampleKind::IntensityRoot);
	let shrunk = pixels(&ScreenRenderer::new(60, 44, 30, 22).render_to_vec(&plane, ScreenColors::Plain(&table)));
	for y in 0..22 {
		for x in 0..30 {
			let at = |dx: usize, dy: usize| plane[(2 * y + dy) * 60 + 2 * x + dx].to_f64();
			let squares = [at(0, 0), at(1, 0), at(0, 1), at(1, 1)].map(|v| v * v);
			let bits = f16::from_f64((squares.iter().sum::<f64>() / 4.0).sqrt()).to_bits();
			let near = [bits - 1, bits, bits + 1].map(|b| table.entries()[b as usize]);
			assert!(near.contains(&shrunk[y * 30 + x]), "({x}, {y})");
		}
	}
}

#[test]
fn a_crest_beside_a_trough_shrinks_to_zero() {
	let s = inferno();
	let table = HalfTable::new(&s, 0.0, 1.0, SampleKind::Field);
	let wave: Vec<f16> = (0..8).map(|i| h(if i % 2 == 0 { 0.5 } else { -0.5 })).collect();
	let flat = pixels(&ScreenRenderer::new(4, 2, 2, 1).render_to_vec(&wave, ScreenColors::Plain(&table)));
	assert!(flat.iter().all(|&p| p == table.color(h(0.0))));
}

#[test]
fn enlarging_blends_the_nearest_samples() {
	let s = inferno();
	let table = HalfTable::new(&s, 0.0, 1.0, SampleKind::Field);
	let small = [h(0.5), h(-0.5), h(1.0), h(0.0)];
	let large = pixels(&ScreenRenderer::new(2, 2, 4, 4).render_to_vec(&small, ScreenColors::Plain(&table)));
	let blended =
		[0.5, 0.25, -0.25, -0.5, 0.625, 0.375, -0.125, -0.375, 0.875, 0.625, 0.125, -0.125, 1.0, 0.75, 0.25, 0.0];
	assert_eq!(large, blended.map(|v| table.color(h(v))));
}

#[test]
fn dithering_mixes_the_two_neighbouring_levels() {
	let s = inferno();
	let dark = h(0.0007);
	let deep = DeepHalfTable::new(&s, 0.0, 1.0, SampleKind::IntensityRoot);
	let blue16 = deep.channels(dark)[2] as u32;
	assert_ne!(blue16 & 0xFF, 0, "the sample lies between two levels");
	let flat = vec![dark; 32 * 32];
	for depth in DitherDepth::ALL {
		let side = depth.side();
		let blues: Vec<u32> =
			pixels(&ScreenRenderer::new(32, 32, 32, 32).render_to_vec(&flat, ScreenColors::Dithered(&deep, depth)))
				.iter()
				.map(|p| p[2] as u32)
				.collect();
		assert!(blues.iter().all(|&b| b == blue16 >> 8 || b == (blue16 >> 8) + 1));
		assert!((0..32 * 32).all(|i| blues[i] == blues[(i / 32 % side) * 32 + i % 32 % side]), "one tile repeated");
		let mean =
			(0..side * side).map(|i| blues[(i / side) * 32 + i % side] as f64).sum::<f64>() / (side * side) as f64;
		let bits = depth.bits();
		let expected = (blue16 >> 8) as f64
			+ ((blue16 & 0xFF) as f64 / (1u32 << (16 - bits)) as f64 + 0.5).floor() / (1u32 << (bits - 8)) as f64;
		assert!((mean - expected).abs() < 1e-9, "{depth:?}: {mean} for {expected}");
	}
	let table = HalfTable::new(&s, 0.0, 1.0, SampleKind::IntensityRoot);
	let plain = pixels(&ScreenRenderer::new(32, 32, 32, 32).render_to_vec(&flat, ScreenColors::Plain(&table)));
	assert!(plain.iter().all(|&p| p == table.color(dark)));
}

#[test]
fn brightness_fit() {
	let root: Vec<f16> = (0..10_000).map(|i| h((0.0001 * i as f32).sqrt())).collect();
	let linear: Vec<f16> = (0..10_000).map(|i| h(0.0001 * i as f32)).collect();
	let fit = fit_brightness(&root, SampleKind::IntensityRoot);
	assert_eq!(fit, Some(1.0));
	assert_eq!(fit_brightness(&linear, SampleKind::Intensity), fit);
	assert_eq!(fit_brightness(&[h(1e-6), h(1e-6)], SampleKind::Field), Some(1e5));
	assert_eq!(fit_brightness(&[f16::ZERO; 100], SampleKind::Field), None);
	assert_eq!(fit_brightness(&[], SampleKind::Field), None);
	assert_eq!(fit_brightness(&[f16::NAN, f16::INFINITY], SampleKind::Field), None);
	assert_eq!(fit_brightness(&[h(-1.0); 4], SampleKind::IntensityRoot), None);
	assert_eq!(fit_brightness(&[h(0.03), h(0.03)], SampleKind::Field), Some(33.3));
	assert_eq!(fit_brightness_f32(&[0.25, -0.25], SampleKind::Field), Some(4.0));
}

fn hex_picture(rgba: &[u8], width: usize) -> String {
	pixels(rgba)
		.chunks(width)
		.map(|row| row.iter().map(|p| format!("{:02X}{:02X}{:02X}", p[0], p[1], p[2])).collect::<Vec<_>>().join(" "))
		.collect::<Vec<_>>()
		.join("\n")
}

#[test]
fn snapshot_pictures() {
	let s = inferno();
	let indexed: Vec<u8> = (0..24).map(|i| (i * 11) as u8).collect();
	insta::assert_snapshot!("indexed", hex_picture(&render_indexed(&indexed, &Palette::new(&s, 0.0)), 6));
	let plane: Vec<f16> = (0..24).map(|i| h((i as f32 - 12.0) / 12.0)).collect();
	let field = HalfTable::new(&s, -0.05, 1.0, SampleKind::Field);
	insta::assert_snapshot!("half", hex_picture(&render_half(&plane, &field), 6));
	let screen = ScreenRenderer::new(6, 4, 9, 3).render_to_vec(&plane, ScreenColors::Plain(&field));
	insta::assert_snapshot!("screen", hex_picture(&screen, 9));
	let deep = DeepHalfTable::new(&s, -0.05, 1.0, SampleKind::Field);
	let dithered =
		ScreenRenderer::new(6, 4, 9, 3).render_to_vec(&plane, ScreenColors::Dithered(&deep, DitherDepth::Bits12));
	insta::assert_snapshot!("screen_dithered", hex_picture(&dithered, 9));
}

#[test]
#[ignore = "timing for the user: cargo test --release -p wavedude-core --no-default-features --test render -- --ignored --nocapture"]
fn timing() {
	const BUILDS: u32 = 20;
	let s = inferno();
	let _ = HalfTable::new(&s, 0.0, 1.0, SampleKind::Field);
	let clock = Instant::now();
	for i in 0..BUILDS {
		std::hint::black_box(HalfTable::new(&s, 0.0, 1.0 + i as f64, SampleKind::Field));
	}
	let table_ms = clock.elapsed().as_secs_f64() * 1e3 / BUILDS as f64;
	let (w, ht) = (2880, 1080);
	let plane: Vec<f16> = noise(w * ht, 3).map(|v| f16::from_f64(v * 2.0 - 1.0)).collect();
	let table = HalfTable::new(&s, 0.0, 1.0, SampleKind::Field);
	let mut out = vec![0u8; w * ht * 4];
	wavedude_core::render::render_half_into(&plane, &table, &mut out);
	const RENDERS: u32 = 50;
	let clock = Instant::now();
	for _ in 0..RENDERS {
		wavedude_core::render::render_half_into(&plane, &table, &mut out);
	}
	let render_ms = clock.elapsed().as_secs_f64() * 1e3 / RENDERS as f64;
	let indices: Vec<u8> = plane.iter().map(|&v| field_index(v, 1.0)).collect();
	let palette = Palette::new(&s, 0.0);
	let clock = Instant::now();
	for _ in 0..RENDERS {
		wavedude_core::render::render_indexed_into(&indices, &palette, &mut out);
	}
	let indexed_ms = clock.elapsed().as_secs_f64() * 1e3 / RENDERS as f64;
	let mut screen = ScreenRenderer::new(1920, 1080, 914, 514);
	let mut small = vec![0u8; 914 * 514 * 4];
	let full_hd = &plane[..1920 * 1080];
	let deep = DeepHalfTable::new(&s, 0.0, 1.0, SampleKind::Field);
	screen.render(full_hd, ScreenColors::Plain(&table), &mut small);
	let clock = Instant::now();
	for _ in 0..RENDERS {
		screen.render(full_hd, ScreenColors::Plain(&table), &mut small);
	}
	let screen_ms = clock.elapsed().as_secs_f64() * 1e3 / RENDERS as f64;
	let clock = Instant::now();
	for _ in 0..RENDERS {
		screen.render(full_hd, ScreenColors::Dithered(&deep, DitherDepth::Bits16), &mut small);
	}
	let dither_ms = clock.elapsed().as_secs_f64() * 1e3 / RENDERS as f64;
	println!(
		"threads {}: half table {table_ms:.3} ms; 2880x1080 half {render_ms:.3} ms, indexed {indexed_ms:.3} ms; \
		 screen 1920x1080 -> 914x514 {screen_ms:.3} ms, dithered {dither_ms:.3} ms",
		rayon::current_num_threads()
	);
}

#[test]
fn a_zoom_window_reads_its_samples_alone_and_survives_pans() {
	let s = inferno();
	let (w, h) = (80, 50);
	let plane: Vec<f16> = noise(w * h, 11).map(|v| f16::from_f64(v * 0.2)).collect();
	let table = HalfTable::new(&s, 0.0, 3.0, SampleKind::Field);
	let reference = |window: (f64, f64, f64, f64), tw: usize, th: usize| -> Vec<u8> {
		let (x, y, ww, wh) = window;
		let (ax, ay) = (ResampleAxis::window(w, (x, x + ww), tw), ResampleAxis::window(h, (y, y + wh), th));
		let mut out = Vec::new();
		for ty in 0..th {
			let (fy, wys) = ay.weights_of(ty);
			for tx in 0..tw {
				let (fx, wxs) = ax.weights_of(tx);
				let mut v = 0f32;
				for (k, wy) in wys.iter().enumerate() {
					let row: f32 =
						wxs.iter().enumerate().map(|(j, wx)| wx * plane[(fy + k) * w + fx + j].to_f32()).sum();
					v += wy * row;
				}
				out.extend(table.color(f16::from_f32(v)));
			}
		}
		out
	};
	let zoom = (20.5, 10.25, 20.0, 12.5);
	let pan = (40.0, 30.0, 30.0, 18.0);
	let mut screen = ScreenRenderer::windowed(w, h, Some(zoom), 64, 40);
	assert_eq!(screen.render_to_vec(&plane, ScreenColors::Plain(&table)), reference(zoom, 64, 40));
	screen.set_view(w, h, Some(pan), 30, 9);
	assert_eq!(screen.render_to_vec(&plane, ScreenColors::Plain(&table)), reference(pan, 30, 9), "a shrinking pan");
	screen.set_view(w, h, Some(zoom), 64, 40);
	assert_eq!(screen.render_to_vec(&plane, ScreenColors::Plain(&table)), reference(zoom, 64, 40), "and back");
}
