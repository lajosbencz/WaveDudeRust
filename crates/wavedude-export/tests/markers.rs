use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::model::{Layer, Measurement, Probe, ProbeKind, Project, Source, SourceArray, YScale};
use wavedude_export::graph::{self, GraphData};
use wavedude_export::overlays::{
	FaceMetrics, GRAPH_TEXT_SIZE, Overlays, arm_length, draw_graph_box, draw_probe_markers, label_origin, label_size,
	source_marker_layer,
};
use wavedude_export::raster::{Canvas, typeface};

const GREY: [u8; 4] = [128, 128, 128, 255];

fn grey(w: usize, h: usize) -> Vec<u8> {
	GREY.repeat(w * h)
}

fn at(px: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
	let i = (y * w + x) * 4;
	[px[i], px[i + 1], px[i + 2], px[i + 3]]
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

fn probes() -> (Probe, Probe) {
	let point = Probe { ax: 20, ay: 40, ..Probe::default() };
	let line = Probe { kind: ProbeKind::Line, ax: 50, ay: 30, bx: 66, by: 45, ..Probe::default() };
	(point, line)
}

#[test]
fn probe_markers_black_around_white_clipped_at_the_corner_labelled() {
	let (w, h) = (96usize, 64usize);
	let mut px = grey(w, h);
	let (point, line) = probes();
	let corner = Probe { ax: 0, ay: 0, ..Probe::default() };
	{
		let mut canvas = Canvas::new(&mut px, w, h);
		draw_probe_markers(&mut canvas, [(2, &point), (3, &line), (4, &corner)].into_iter());
	}
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
	let ink = |text: &str, (x, y): (i32, i32)| {
		let size = label_size(h as u32);
		let lw = typeface().measure(text, size, true).ceil() as usize + 4;
		let lh = typeface().line_height(size).ceil() as usize + 4;
		let mut white = 0;
		for yy in y as usize..(y as usize + lh).min(h) {
			for xx in x as usize..(x as usize + lw).min(w) {
				white += (at(&px, w, xx, yy)[0] > 200) as usize;
			}
		}
		white
	};
	assert!(ink("2", label_origin(20, 40, h as u32)) > 10, "label 2");
	assert!(ink("3A", label_origin(50, 30, h as u32)) > 15, "label 3A");
	assert!(ink("3B", label_origin(66, 45, h as u32)) > 15, "label 3B");
	insta::assert_snapshot!("probe_markers", ascii(&px, w));
}

#[test]
fn graph_boxes_show_background_border_axes_and_the_curve() {
	let (w, h) = (220usize, 120usize);
	let mut px = grey(w, h);
	let probe = Probe {
		name: "Probe 1".into(),
		box_x: 10,
		box_y: 5,
		box_size: wavedude_core::model::BoxSize::Tiny,
		..Probe::default()
	};
	let trace = ProbeTrace {
		series: (0..300).map(|i| ((i as f32) * 0.1).sin() * (i as f32 / 300.0)).collect(),
		..ProbeTrace::default()
	};
	let metrics = FaceMetrics { face: typeface(), size: GRAPH_TEXT_SIZE };
	let data = GraphData::at(&trace, 299, 0);
	let layout = graph::layout(&probe, &data, 300, 200.0, 100.0, &metrics);
	assert!(layout.has_plot && layout.show_labels);
	assert!(layout.y_ticks.iter().any(|t| t.zero), "a symmetric amplitude axis has its zero line");
	assert_eq!(layout.readout, format!("= {}", graph::format_g3(trace.series[299] as f64)));
	{
		let mut canvas = Canvas::new(&mut px, w, h);
		draw_graph_box(&mut canvas, &layout, 10, 5);
	}
	let border = at(&px, w, 10, 5);
	assert!((136..=146).contains(&border[0]) && border[2] > border[0], "the border over the background {border:?}");
	let inside = at(&px, w, 12, 7);
	assert!(inside[0] < 40, "the dark background {inside:?}");
	assert_eq!(at(&px, w, 9, 5), GREY, "nothing outside the box");
	assert_eq!(at(&px, w, 210, 5), GREY);
	let curve = px.as_chunks::<4>().0.iter().filter(|p| p[0] > 200 && p[1] > 150 && p[2] < 120).count();
	assert!(curve > 100, "the curve's yellow ({curve} pixels)");

	let log = Probe {
		measurement: Measurement::Value,
		layer: Layer::Intensity,
		y_scale: YScale::Logarithmic,
		decades: 4,
		..probe.clone()
	};
	let positive =
		ProbeTrace { series: (1..300).map(|i| (i as f32 * 1e-4).powi(2)).collect(), ..ProbeTrace::default() };
	let layout = graph::layout(&log, &GraphData::at(&positive, 298, 0), 300, 400.0, 200.0, &metrics);
	assert!(
		layout.y_ticks.iter().all(|t| t.label.starts_with("1e") || t.label.starts_with("0.") || t.label == "1"),
		"{:?}",
		layout.y_ticks
	);
}

#[test]
fn source_markers_are_one_premultiplied_layer() {
	let (w, h) = (120u32, 80u32);
	let sources = [
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
	let (layer, ink) = source_marker_layer(&sources, 1, w, h);
	let alpha = |x: usize, y: usize| layer[(y * w as usize + x) * 4 + 3];
	assert_eq!(&layer[(40 * w as usize + 30) * 4..][..4], &[255; 4], "the white centre dot");
	assert!(alpha(30 + 6, 40) > 0, "the ring");
	assert!(alpha(72, 30) > 0 && alpha(88, 30) > 0, "the emitter dots");
	assert_eq!(alpha(100, 70), 0, "a disabled source has no marker");
	assert!(!ink.is_empty() && ink.x <= 20 && ink.right() >= 92);
	for p in layer.as_chunks::<4>().0 {
		assert!(p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3], "premultiplied");
	}
	let mut project = Project { sources: sources.to_vec(), ..Project::default() };
	project.display.source_markers_in_export = true;
	project.display.probe_markers_in_export = false;
	let overlays = Overlays::new(&project, w, h, &[], 10);
	let mut px = grey(w as usize, h as usize);
	overlays.draw(&mut px, Layer::Intensity, 0);
	assert_eq!(at(&px, w as usize, 30, 40), [255; 4], "on both layers");
}
