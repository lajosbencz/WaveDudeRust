//! The burned-in overlays rasterised over a grey frame.
mod common;

use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::model::{BoxSize, Layer, Measurement, Probe, ProbeKind, Project, Source, SourceArray, YScale};
use wavedude_export::overlays::{Overlays, arm_length, label_origin, label_size};
use wavedude_view::FontMetrics;
use wavedude_view::epaint::text::{FontId, Fonts, TextOptions};
use wavedude_view::graph::{self, GraphData};

const GREY: [u8; 4] = [128, 128, 128, 255];

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

/// One character per pixel: `#` white, `.` black, ` ` the grey background, `+` lighter, `-` darker.
fn ascii(px: &[u8], w: usize) -> String {
	px.chunks_exact(w * 4)
		.map(|row| {
			row.as_chunks::<4>()
				.0
				.iter()
				.map(|p| match p[0] {
					255 => '#',
					0 => '.',
					128 => ' ',
					v if v > 128 => '+',
					_ => '-',
				})
				.collect::<String>()
		})
		.collect::<Vec<_>>()
		.join("\n")
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
	let ink = |(x, y): (i32, i32)| {
		let (lw, lh) = (24, (label_size(h as u32) * 1.4) as usize + 4);
		(y as usize..(y as usize + lh).min(h))
			.flat_map(|yy| (x as usize..(x as usize + lw).min(w)).map(move |xx| (xx, yy)))
			.filter(|&(xx, yy)| at(&px, w, xx, yy)[0] > 200)
			.count()
	};
	assert!(ink(label_origin(20, 40, h as u32)) > 10, "label 1, the first active probe");
	assert!(ink(label_origin(50, 30, h as u32)) > 15, "label 2A");
	assert!(ink(label_origin(66, 45, h as u32)) > 15, "label 2B");
	insta::assert_snapshot!("probe_markers", ascii(&px, w));
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
	let border = at(&px, w, 10, 5);
	assert!((136..=146).contains(&border[0]) && border[2] > border[0], "the border over the background {border:?}");
	let inside = at(&px, w, 12, 7);
	assert!(inside[0] < 40, "the dark background {inside:?}");
	assert_eq!(at(&px, w, 9, 5), GREY, "nothing outside the box");
	assert_eq!(at(&px, w, 210, 5), GREY);
	let curve = px.as_chunks::<4>().0.iter().filter(|p| p[0] > 200 && p[1] > 150 && p[2] < 120).count();
	assert!(curve > 100, "the curve's yellow ({curve} pixels)");
	let corner = at(&px, w, 10 + 200 - 4, 5 + 100 - 4);
	assert!(corner[0] < 40, "no resize grip in an export {corner:?}");
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
