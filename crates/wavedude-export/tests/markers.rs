//! The burned-in overlays rasterised over a grey frame.
mod common;

use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::model::{BoxSize, Layer, Measurement, Probe, ProbeKind, Project, Source, SourceArray, YScale};
use wavedude_export::overlays::{Overlays, arm_length, label_origin, label_size};
use wavedude_view::FontMetrics;
use wavedude_view::epaint::Color32;
use wavedude_view::epaint::text::{FontId, Fonts, TextOptions};
use wavedude_view::graph::{self, GraphData};

const GREY: [u8; 4] = [128, 128, 128, 255];

/// `c` painted over the opaque `under`, straight alpha in sRGB.
fn over(under: [u8; 3], c: Color32) -> [u8; 3] {
	let [r, g, b, a] = c.to_srgba_unmultiplied();
	let a = f32::from(a) / 255.;
	[r, g, b]
		.map(f32::from)
		.iter()
		.zip(under)
		.map(|(c, u)| (c * a + f32::from(u) * (1. - a)).round() as u8)
		.collect::<Vec<_>>()
		.try_into()
		.unwrap()
}

/// Whether `px` is `want` within the rounding of the blends and the rasteriser.
fn near(px: [u8; 4], want: [u8; 3]) -> bool {
	px[..3].iter().zip(want).all(|(&p, w)| p.abs_diff(w) <= 3)
}

fn at(px: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
	let i = (y * w + x) * 4;
	[px[i], px[i + 1], px[i + 2], px[i + 3]]
}

/// `project`'s overlays of `layer` at frame 0 over a grey frame.
fn burned(project: &Project, (w, h): (u32, u32), traces: &[ProbeTrace], layer: Layer) -> Vec<u8> {
	let mut px = GREY.repeat((w * h) as usize);
	let overlays = Overlays::new(project, w, h, traces, 300);
	overlays.draw(&mut overlays.rasteriser(), &mut px, layer, 0).unwrap();
	px
}

/// The spec's ThickLine over `px`: a `(2r + 1)^2` square of `c` at each of the `max(|dx|, |dy|) + 1` points, rounded
/// half to even, clipped to the frame.
fn thick_line(px: &mut [u8], (w, h): (usize, usize), (x0, y0): (i64, i64), (x1, y1): (i64, i64), c: [u8; 4], r: i64) {
	let steps = (x1 - x0).abs().max((y1 - y0).abs());
	for k in 0..=steps {
		let at = |a: i64, b: i64| {
			if steps == 0 { a } else { a + ((b - a) as f64 * k as f64 / steps as f64).round_ties_even() as i64 }
		};
		let (cx, cy) = (at(x0, x1), at(y0, y1));
		for y in (cy - r).max(0)..=(cy + r).min(h as i64 - 1) {
			for x in (cx - r).max(0)..=(cx + r).min(w as i64 - 1) {
				let i = (y as usize * w + x as usize) * 4;
				px[i..i + 4].copy_from_slice(&c);
			}
		}
	}
}

/// The spec's strokes of `probes` over a grey frame: per probe the line A-B and the crosses, black at radius 2, then
/// white at 1.
fn strokes<'a>(probes: impl Iterator<Item = &'a Probe>, (w, h): (usize, usize)) -> Vec<u8> {
	let mut px = GREY.repeat(w * h);
	let arm = i64::from(arm_length(h as u32));
	for p in probes {
		let line = p.kind == ProbeKind::Line;
		let (a, b) = ((i64::from(p.ax), i64::from(p.ay)), (i64::from(p.bx), i64::from(p.by)));
		for (c, r) in [([0, 0, 0, 255], 2), ([255; 4], 1)] {
			if line {
				thick_line(&mut px, (w, h), a, b, c, r);
			}
			let ends: &[(i64, i64)] = if line { &[a, b] } else { &[a] };
			for &(x, y) in ends {
				thick_line(&mut px, (w, h), (x - arm, y), (x + arm, y), c, r);
				thick_line(&mut px, (w, h), (x, y - arm), (x, y + arm), c, r);
			}
		}
	}
	px
}

/// The rectangle of the probe label `text` at `(x, y)`: `ceil(text width) + 4 x ceil(text height) + 4`.
fn label_rect(text: &str, (x, y): (i32, i32), h: u32) -> (std::ops::Range<i32>, std::ops::Range<i32>) {
	let mut fonts = Fonts::new(TextOptions::default(), wavedude_view::fonts::definitions());
	let mut view = fonts.with_pixels_per_point(1.);
	let font = FontId::new(label_size(h), wavedude_view::fonts::bold());
	let width = view.layout_no_wrap(text.into(), font.clone(), Color32::WHITE).size().x.ceil() as i32 + 4;
	let height = view.row_height(&font).ceil() as i32 + 4;
	(x..x + width, y..y + height)
}

fn markers_only(project: &mut Project) {
	project.display.probe_markers_in_export = true;
	project.display.probe_graphs_in_export = false;
	project.display.source_markers_in_export = false;
}

#[test]
fn probe_markers_black_around_white_clipped_at_the_corner_labelled() {
	let (w, h) = (96usize, 64usize);
	let point = Probe { ax: 20, ay: 40, ..Probe::default() };
	let line = Probe { kind: ProbeKind::Line, ax: 50, ay: 30, bx: 66, by: 45, ..Probe::default() };
	let corner = Probe { ax: 0, ay: 0, ..Probe::default() };
	let inactive = Probe { active: false, ..Probe::default() };
	let mut project = Project { probes: vec![inactive, point, line, corner], ..Project::default() };
	markers_only(&mut project);
	let px = burned(&project, (w as u32, h as u32), &[], Layer::Field);
	let arm = arm_length(h as u32) as usize;
	assert_eq!(arm, 6);
	assert_eq!(at(&px, w, 20, 40), [255; 4], "the centre is white");
	assert_eq!(at(&px, w, 20 + arm + 1, 40), [255; 4], "white to the arm's end plus its radius");
	assert_eq!(at(&px, w, 20 + arm + 2, 40), [0, 0, 0, 255], "the black stroke ends the arm");
	assert_eq!(at(&px, w, 20 + arm + 3, 40), GREY);
	assert_eq!(at(&px, w, 20 - 3, 40 + 2), [0, 0, 0, 255], "black beside the white arm");
	assert_eq!(at(&px, w, 0, 0), [255; 4], "a cross at the corner is drawn clipped");
	assert_eq!(at(&px, w, arm + 2, 0), [0, 0, 0, 255]);
	assert_eq!(at(&px, w, 58, 37), [255; 4], "the line A-B is white");
	let labels = [("1", (20, 40)), ("2A", (50, 30)), ("2B", (66, 45)), ("3", (0, 0))]
		.map(|(text, (x, y))| label_rect(text, label_origin(x, y, h as u32), h as u32));
	let ink = |(xs, ys): &(std::ops::Range<i32>, std::ops::Range<i32>)| {
		ys.clone()
			.flat_map(|y| xs.clone().map(move |x| (x, y)))
			.filter(|&(x, y)| (0..w as i32).contains(&x) && (0..h as i32).contains(&y))
			.filter(|&(x, y)| at(&px, w, x as usize, y as usize)[0] > 200)
			.count()
	};
	assert!(ink(&labels[0]) > 0, "label 1, the first active probe");
	assert!(ink(&labels[1]) > 0, "label 2A");
	assert!(ink(&labels[2]) > 0, "label 2B");
	let want = strokes(project.probes.iter().filter(|p| p.active), (w, h));
	let labelled =
		|x: usize, y: usize| labels.iter().any(|(xs, ys)| xs.contains(&(x as i32)) && ys.contains(&(y as i32)));
	for (x, y) in (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, y)| !labelled(x, y)) {
		assert_eq!(at(&px, w, x, y), at(&want, w, x, y), "({x}, {y}): the spec's strokes outside the labels");
	}
}

#[test]
fn graph_boxes_show_background_border_axes_and_the_curve() {
	let (w, h) = (220usize, 120usize);
	let probe = Probe { name: "Probe 1".into(), box_x: 10, box_y: 5, box_size: BoxSize::Tiny, ..Probe::default() };
	let trace = ProbeTrace {
		series: (0..300).map(|i| ((i as f32) * 0.1).sin() * (i as f32 / 300.0)).collect(),
		..ProbeTrace::default()
	};
	let mut fonts = Fonts::new(TextOptions::default(), wavedude_view::fonts::definitions());
	let mut metrics =
		FontMetrics { fonts: &mut fonts.with_pixels_per_point(1.), font: FontId::proportional(graph::FONT_SIZE) };
	let layout = graph::layout(&probe, &GraphData::at(&trace, 299, 0), 300, 200.0, 100.0, &mut metrics);
	assert!(layout.has_plot && layout.show_labels);
	assert!(layout.y_ticks.iter().any(|t| t.zero), "a symmetric amplitude axis has its zero line");
	assert_eq!(layout.readout, format!("= {}", graph::format_g3(trace.series[299] as f64)));

	let log = Probe {
		measurement: Measurement::Value,
		layer: Layer::Intensity,
		y_scale: YScale::Logarithmic,
		decades: 4,
		..probe.clone()
	};
	let positive =
		ProbeTrace { series: (1..300).map(|i| (i as f32 * 1e-4).powi(2)).collect(), ..ProbeTrace::default() };
	let layout = graph::layout(&log, &GraphData::at(&positive, 298, 0), 300, 400.0, 200.0, &mut metrics);
	assert!(
		layout.y_ticks.iter().all(|t| t.label.starts_with("1e") || t.label.starts_with("0.") || t.label == "1"),
		"{:?}",
		layout.y_ticks
	);

	let mut project = Project { probes: vec![probe], ..Project::default() };
	project.simulation.visualize_every = 299;
	project.display.probe_markers_in_export = false;
	project.display.probe_graphs_in_export = true;
	project.display.source_markers_in_export = false;
	let mut px = GREY.repeat(w * h);
	let overlays = Overlays::new(&project, w as u32, h as u32, &[trace], 300);
	overlays.draw(&mut overlays.rasteriser(), &mut px, Layer::Field, 1).unwrap();
	let grey = [GREY[0], GREY[1], GREY[2]];
	let background = over(grey, graph::BACKGROUND);
	let border = at(&px, w, 10, 5);
	assert!(near(border, over(background, graph::BORDER)), "the border over the background {border:?}");
	let inside = at(&px, w, 12, 7);
	assert!(near(inside, background), "the background {inside:?}");
	assert_eq!(at(&px, w, 9, 5), GREY, "nothing outside the box");
	assert_eq!(at(&px, w, 210, 5), GREY);
	let [r, g, b, _] = graph::CURVE.to_srgba_unmultiplied();
	let columns: std::collections::BTreeSet<usize> =
		(0..w * h).filter(|&i| near(at(&px, w, i % w, i / w), [r, g, b])).map(|i| i % w).collect();
	let span = columns.last().zip(columns.first()).map_or(0, |(l, f)| l - f);
	assert!(span > 200 / 2, "the curve runs along the plot: {span} of the box's 200 px");
	let corner = at(&px, w, 10 + 200 - 4, 5 + 100 - 4);
	assert!(near(corner, background), "no resize grip in an export {corner:?}");
}

#[test]
fn source_markers_on_both_layers() {
	let (w, h) = (120u32, 80u32);
	let sources = vec![
		Source { name: "S".into(), x: 30.0, y: 40.0, ..Source::default() },
		Source {
			name: "Array".into(),
			x: 80.0,
			y: 30.0,
			array: Some(SourceArray { size_x: 3, size_y: 1, spacing_x: 8, ..SourceArray::default() }),
			..Source::default()
		},
		Source { enabled: false, x: 100.0, y: 70.0, ..Source::default() },
	];
	let mut project = Project { sources, seed: 1, ..Project::default() };
	project.display.source_markers_in_export = true;
	project.display.probe_markers_in_export = false;
	for layer in [Layer::Field, Layer::Intensity] {
		let px = burned(&project, (w, h), &[], layer);
		let px_at = |x: u32, y: u32| at(&px, w as usize, x as usize, y as usize);
		assert_eq!(px_at(30, 40), [255; 4], "the white centre dot");
		assert_ne!(px_at(30 + 6, 40), GREY, "the ring");
		assert_ne!(px_at(72, 30), GREY, "an emitter dot");
		assert_ne!(px_at(88, 30), GREY, "another");
		assert_eq!(px_at(100, 70), GREY, "a disabled source has no marker");
		let name = (36..60).flat_map(|x| (34..48).map(move |y| (x, y))).filter(|&(x, y)| px_at(x, y)[0] > 200);
		assert!(name.count() > 3, "the name to the right of the marker");
	}
}
