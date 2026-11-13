//! The CPU rasteriser: shapes over a frame's pixels, the frame elsewhere unchanged, the same bytes every time and on
//! every machine; a label's halo.

use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::model::{BoxSize, Layer, Probe, ProbeKind, Project, Source};
use wavedude_export::ExportError;
use wavedude_export::overlays::Overlays;
use wavedude_export::raster::Rasteriser;
use wavedude_view::draw::{Halo, Outlines, haloed_text};
use wavedude_view::epaint::text::{FontId, FontsView};
use wavedude_view::epaint::{ClippedShape, Color32, Rect, Shape, pos2, vec2};

const W: u32 = 160;
const H: u32 = 90;

/// A frame no two neighbours of which are alike.
fn picture() -> Vec<u8> {
	(0..W * H).flat_map(|i| [(i % 251) as u8, (i * 7 % 256) as u8, (i / W * 3) as u8, 255]).collect()
}

fn at(px: &[u8], x: u32, y: u32) -> [u8; 4] {
	let i = ((y * W + x) * 4) as usize;
	[px[i], px[i + 1], px[i + 2], px[i + 3]]
}

fn shapes(fonts: &mut FontsView<'_>) -> Vec<ClippedShape> {
	let everything = Rect::from_min_size(pos2(0., 0.), vec2(W as f32, H as f32));
	let galley = fonts.layout_no_wrap("Probe 1".into(), FontId::proportional(11.), Color32::WHITE);
	vec![
		ClippedShape::new(
			everything,
			Shape::rect_filled(Rect::from_min_size(pos2(10., 10.), vec2(20., 10.)), 0, Color32::WHITE),
		),
		ClippedShape::new(
			everything,
			Shape::rect_filled(
				Rect::from_min_size(pos2(40., 10.), vec2(20., 10.)),
				0,
				Color32::from_rgba_unmultiplied(0, 0, 0, 128),
			),
		),
		ClippedShape::new(everything, Shape::galley(pos2(10., 40.), galley, Color32::WHITE)),
		ClippedShape::new(
			Rect::from_min_size(pos2(100., 0.), vec2(10., H as f32)),
			Shape::line_segment([pos2(80., 60.5), pos2(150., 60.5)], (1., Color32::RED)),
		),
	]
}

#[test]
fn shapes_cover_the_frame_which_stays_unchanged_elsewhere() {
	let mut raster = Rasteriser::new(W, H);
	let before = picture();
	let mut px = before.clone();
	raster.render(&mut px, |fonts, _| shapes(fonts)).unwrap();
	assert_eq!(at(&px, 15, 15), [255; 4], "an opaque rectangle on whole pixels");
	assert_eq!(at(&px, 9, 15), at(&before, 9, 15), "nothing beside it");
	assert_eq!(at(&px, 30, 15), at(&before, 30, 15), "its right edge ends on a pixel boundary");
	let [r, g, b, a] = at(&before, 45, 15);
	let half = |v: u8| i32::from(v) * 127 / 255;
	let got = at(&px, 45, 15);
	for (got, want) in got.iter().zip([half(r), half(g), half(b)]) {
		assert!((i32::from(*got) - want).abs() <= 1, "black at half alpha over the frame: {got} {want}");
	}
	assert_eq!((got[3], a), (255, 255), "opaque");
	let text = (38..58)
		.flat_map(|y| (8..60).map(move |x| (x, y)))
		.filter(|&(x, y)| at(&px, x, y)[..3].iter().all(|&v| v > 230));
	let n = text.count();
	assert!(n > 30, "the text's ink: {n} pixels");
	assert_eq!(at(&px, 105, 60), [255, 0, 0, 255], "the line inside its clip");
	assert_eq!(at(&px, 95, 60), at(&before, 95, 60), "and nothing of it outside");
	let changed = (0..H).flat_map(|y| (0..W).map(move |x| (x, y))).filter(|&(x, y)| at(&px, x, y) != at(&before, x, y));
	assert!(changed.clone().all(|(x, y)| y < 60 || (100..110).contains(&x)), "only under the shapes");
}

/// Probes in two corners, crossing each other and with graph boxes, sources whose names reach the frame's edges.
fn corners_and_crossings() -> Overlays {
	let probes = vec![
		Probe { ax: 0, ay: 0, ..Probe::default() },
		Probe { ax: W as i32 - 1, ay: H as i32 - 1, ..Probe::default() },
		Probe { ax: 60, ay: 40, ..Probe::default() },
		Probe { kind: ProbeKind::Line, ax: 55, ay: 35, bx: 70, by: 48, ..Probe::default() },
		Probe { name: "Graph".into(), box_x: 90, box_y: 2, box_size: BoxSize::Tiny, ..Probe::default() },
	];
	let sources = vec![
		Source { name: "Edge".into(), x: W as f64 - 12., y: 30., ..Source::default() },
		Source { name: "Low".into(), x: 20., y: H as f64 - 4., ..Source::default() },
	];
	let mut project = Project { probes, sources, seed: 1, ..Project::default() };
	project.simulation.visualize_every = 1;
	project.display.probe_markers_in_export = true;
	project.display.probe_graphs_in_export = true;
	project.display.source_markers_in_export = true;
	// a triangle wave: no maths library in the input either
	let series = (0..60).map(|i| ((i % 20) as f32 - 10.).abs() / 10. - 0.5).collect();
	let trace = ProbeTrace { series, ..ProbeTrace::default() };
	Overlays::new(&project, W, H, &vec![trace; 5], 60)
}

/// A haloed label clipped at the frame's top right, over the test shapes.
fn edge_label(fonts: &mut FontsView<'_>, outlines: &mut Outlines) -> Vec<ClippedShape> {
	let mut out = shapes(fonts);
	let galley = fonts.layout_no_wrap("Edge label".into(), FontId::proportional(11.), Color32::WHITE);
	let mut haloed = Vec::new();
	let halo = Halo { color: Color32::BLACK, width: 3. };
	haloed_text(&mut haloed, outlines, pos2(W as f32 - 30., -4.), galley, 11., Color32::WHITE, halo);
	let everything = Rect::from_min_size(pos2(0., 0.), vec2(W as f32, H as f32));
	out.extend(haloed.into_iter().map(|s| ClippedShape::new(everything, s)));
	out
}

/// FNV-1a of `bytes`: a frame's fingerprint in a snapshot.
fn fingerprint(bytes: &[u8]) -> String {
	let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
	format!("{hash:016x}")
}

#[test]
fn frames_rasterise_to_the_same_bytes_every_time_and_on_every_machine() {
	let overlays = corners_and_crossings();
	let render = |(marks, plain): (&mut Rasteriser, &mut Rasteriser)| {
		let mut frames = Vec::new();
		for layer in [Layer::Field, Layer::Intensity] {
			let mut px = picture();
			overlays.draw(marks, &mut px, layer, 30).unwrap();
			assert!(px != picture(), "the overlays of {layer:?} draw");
			frames.push(px);
		}
		let mut px = picture();
		plain.render(&mut px, edge_label).unwrap();
		frames.push(px);
		frames
	};
	let (mut marks, mut plain) = (overlays.rasteriser(), Rasteriser::new(W, H));
	let first = render((&mut marks, &mut plain));
	assert!(render((&mut marks, &mut plain)) == first, "the same rasterisers again");
	assert!(render((&mut overlays.rasteriser(), &mut Rasteriser::new(W, H))) == first, "fresh ones");
	let prints: Vec<String> = first.iter().map(|f| fingerprint(f)).collect();
	insta::assert_snapshot!("corners_crossings_graphs_labels", prints.join("\n"));
}

#[test]
fn no_shapes_or_shapes_beyond_the_frame_leave_it_untouched() {
	let mut raster = Rasteriser::new(W, H);
	let mut px = picture();
	raster.render(&mut px, |_, _| Vec::new()).unwrap();
	let off = Rect::from_min_size(pos2(W as f32 + 5., 10.), vec2(20., 10.));
	let beyond = vec![ClippedShape::new(off, Shape::rect_filled(off, 0, Color32::WHITE))];
	raster.render(&mut px, |_, _| beyond).unwrap();
	assert!(px == picture());
}

#[test]
fn a_frame_of_another_size_is_refused() {
	let mut raster = Rasteriser::new(W, H);
	let mut px = vec![0u8; 4];
	assert!(matches!(raster.render(&mut px, |_, _| Vec::new()), Err(ExportError::WrongInput(_))));
}

#[test]
fn a_label_halo_outlines_every_glyph() {
	let mut raster = Rasteriser::new(W, H);
	let mut px = [128, 128, 128, 255].repeat((W * H) as usize);
	raster
		.render(&mut px, |fonts, outlines| {
			let galley = fonts.layout_no_wrap("Probe 1".into(), FontId::proportional(11.), Color32::WHITE);
			let mut shapes = Vec::new();
			let halo = Halo { color: Color32::BLACK, width: 3. };
			haloed_text(&mut shapes, outlines, pos2(20., 30.), galley, 11., Color32::WHITE, halo);
			let everything = Rect::from_min_size(pos2(0., 0.), vec2(W as f32, H as f32));
			shapes.into_iter().map(|s| ClippedShape::new(everything, s)).collect()
		})
		.unwrap();
	let ink = |x: u32, y: u32| at(&px, x, y)[0] > 230;
	let near_ink = |x: u32, y: u32| (y - 1..=y + 1).any(|ny| (x - 1..=x + 1).any(|nx| ink(nx, ny)));
	let (mut around, mut bare) = (0, 0);
	for y in 1..H - 1 {
		for x in 1..W - 1 {
			if !ink(x, y) && near_ink(x, y) {
				around += 1;
				bare += usize::from(at(&px, x, y)[0] == 128);
			}
		}
	}
	assert!(around > 50, "{around} pixels beside the ink");
	assert_eq!(bare, 0, "no pixel beside the ink without its halo");
	let dark = (0..H).flat_map(|y| (0..W).map(move |x| (x, y))).filter(|&(x, y)| at(&px, x, y)[0] < 16).count();
	assert!(dark * 2 > around, "a solid halo: {dark} black pixels, {around} beside the ink");
	let far = |x: u32, y: u32| !(16..70).contains(&x) || !(26..50).contains(&y);
	let touched =
		(0..H).flat_map(|y| (0..W).map(move |x| (x, y))).filter(|&(x, y)| far(x, y) && at(&px, x, y)[0] != 128);
	assert_eq!(touched.count(), 0, "the halo reaches a pixel and a half past the glyphs");
}
