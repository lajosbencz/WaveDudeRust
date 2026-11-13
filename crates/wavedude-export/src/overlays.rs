//! What colour exports burn into a rendered frame: source markers, probe markers and probe graph boxes
//! (docs/spec/50-export.md, Burned-in overlays), as epaint shapes the [`Rasteriser`] draws over the frame.
//! Canvas pixels are points; a source marker centred on the pixel centre `(x + 0.5, y + 0.5)`, probe strokes on whole
//! pixels; everything clipped to the frame.

use wavedude_core::engine::emitters::elements_of;
use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::model::normalize::box_in_canvas;
use wavedude_core::model::{Layer, Probe, ProbeKind, Project, Source};
use wavedude_view::draw::{Halo, Outlines, haloed_text};
use wavedude_view::epaint::text::{FontId, FontsView};
use wavedude_view::epaint::{ClippedShape, Color32, Mesh, Pos2, Rect, Shape, Stroke, pos2, vec2};
use wavedude_view::graph::{self, GraphData};
use wavedude_view::{DeviceGrid, FontMetrics, fonts};

use crate::Result;
use crate::raster::Rasteriser;

/// The probe cross's arm: `max(6, h / 120)`.
pub fn arm_length(height: u32) -> i32 {
	(height as i32 / 120).max(6)
}

/// The probe label's size in pixels: `max(12, h / 60)`.
pub fn label_size(height: u32) -> f32 {
	(height as f32 / 60.0).max(12.0)
}

/// The top-left of a probe label's rectangle: `(x + arm + 2, y - arm - round_even(size * 1.4))`.
pub fn label_origin(x: i32, y: i32, height: u32) -> (i32, i32) {
	let arm = arm_length(height);
	(x + arm + 2, y - arm - (label_size(height) * 1.4).round_ties_even() as i32)
}

/// A probe's label: its number, with "A" or "B" for a line probe's end.
pub fn probe_label(number: usize, line: bool, end_b: bool) -> String {
	match (line, end_b) {
		(false, _) => number.to_string(),
		(true, false) => format!("{number}A"),
		(true, true) => format!("{number}B"),
	}
}

/// The frame of `canvas` pixels as points.
fn frame_rect((w, h): (u32, u32)) -> Rect {
	Rect::from_min_size(Pos2::ZERO, vec2(w as f32, h as f32))
}

/// A line of opaque squares of `(2r + 1)^2` pixels at `steps = max(|dx|, |dy|) + 1` points, point k at
/// `(x0 + round_even(dx * k / steps), y0 + round_even(dy * k / steps))`, as two triangles each: on whole pixels they
/// need no antialiasing. Only the points whose square can reach the `canvas` are added, so a far-off end costs nothing.
pub fn thick_line(
	out: &mut Mesh,
	(x0, y0): (i64, i64),
	(x1, y1): (i64, i64),
	color: Color32,
	radius: i32,
	canvas: (u32, u32),
) {
	let steps = (x1 - x0).abs().max((y1 - y0).abs());
	let r = i64::from(radius);
	let (w, h) = (i64::from(canvas.0), i64::from(canvas.1));
	let (xs, xe) = steps_within(x0, x1, steps, -r, w - 1 + r);
	let (ys, ye) = steps_within(y0, y1, steps, -r, h - 1 + r);
	let side = (2 * radius + 1) as f32;
	for k in xs.max(ys)..=xe.min(ye) {
		let at = |a: i64, b: i64| {
			if steps == 0 { a } else { a + ((b - a) as f64 * k as f64 / steps as f64).round_ties_even() as i64 }
		};
		let (x, y) = (at(x0, x1) - r, at(y0, y1) - r);
		if x + 2 * r < 0 || y + 2 * r < 0 || x >= w || y >= h {
			continue;
		}
		out.add_colored_rect(Rect::from_min_size(pos2(x as f32, y as f32), vec2(side, side)), color);
	}
}

/// The steps `k` of `0..=steps` whose point along `a..b` rounds into `lo..=hi`, with a step of slack for float
/// error; empty as `(1, 0)`.
fn steps_within(a: i64, b: i64, steps: i64, lo: i64, hi: i64) -> (i64, i64) {
	if a == b {
		return if (lo..=hi).contains(&a) { (0, steps) } else { (1, 0) };
	}
	let t = |v: f64| (v - a as f64) * steps as f64 / (b - a) as f64;
	let (tl, th) = (t(lo as f64 - 0.5), t(hi as f64 + 0.5));
	let (t0, t1) = (tl.min(th), tl.max(th));
	((t0.floor() as i64 - 1).max(0), (t1.ceil() as i64 + 1).min(steps))
}

/// A probe's strokes: the line A-B (a line probe) and a cross at each end, black at radius 2, then white at 1.
pub fn probe_strokes(out: &mut Mesh, probe: &Probe, canvas: (u32, u32)) {
	let arm = i64::from(arm_length(canvas.1));
	let line = probe.kind == ProbeKind::Line;
	let (a, b) = ((i64::from(probe.ax), i64::from(probe.ay)), (i64::from(probe.bx), i64::from(probe.by)));
	for (color, radius) in [(Color32::BLACK, 2), (Color32::WHITE, 1)] {
		if line {
			thick_line(out, a, b, color, radius, canvas);
		}
		let ends: &[(i64, i64)] = if line { &[a, b] } else { &[a] };
		for &(x, y) in ends {
			thick_line(out, (x - arm, y), (x + arm, y), color, radius, canvas);
			thick_line(out, (x, y - arm), (x, y + arm), color, radius, canvas);
		}
	}
}

const LABEL_SHADOW: Color32 = Color32::from_rgba_unmultiplied_const(0, 0, 0, 220);

/// A probe label in Selawik Bold: a rectangle of `ceil(width) + 4 x ceil(line height) + 4` at its origin clipping it,
/// the text's shadow at (3, 3) inside it, the white text at (2, 2).
pub fn probe_label_shapes(
	fonts: &mut FontsView<'_>,
	text: &str,
	(x, y): (i32, i32),
	canvas: (u32, u32),
) -> [ClippedShape; 2] {
	let font = FontId::new(label_size(canvas.1), fonts::bold());
	let galley = fonts.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE);
	let size = vec2(galley.size().x.ceil() + 4., fonts.row_height(&font).ceil() + 4.);
	let clip = Rect::from_min_size(pos2(x as f32, y as f32), size).intersect(frame_rect(canvas));
	let text = |offset: f32, color: Color32| {
		let at = pos2(x as f32 + offset, y as f32 + offset);
		ClippedShape::new(clip, Shape::galley_with_override_text_color(at, galley.clone(), color))
	};
	[text(3., LABEL_SHADOW), text(2., Color32::WHITE)]
}

/// Every probe's strokes, then every label; `probes` are (number, probe).
pub fn probe_marker_shapes<'a>(
	fonts: &mut FontsView<'_>,
	probes: impl Iterator<Item = (usize, &'a Probe)> + Clone,
	canvas: (u32, u32),
) -> Vec<ClippedShape> {
	let mut strokes = Mesh::default();
	for (_, p) in probes.clone() {
		probe_strokes(&mut strokes, p, canvas);
	}
	let mut out = vec![ClippedShape::new(frame_rect(canvas), Shape::mesh(strokes))];
	for (n, p) in probes {
		let line = p.kind == ProbeKind::Line;
		out.extend(probe_label_shapes(fonts, &probe_label(n, line, false), label_origin(p.ax, p.ay, canvas.1), canvas));
		if line {
			out.extend(probe_label_shapes(
				fonts,
				&probe_label(n, true, true),
				label_origin(p.bx, p.by, canvas.1),
				canvas,
			));
		}
	}
	out
}

const HALO: Color32 = Color32::from_rgba_unmultiplied_const(0, 0, 0, 191);
const SOURCE_DISC: Color32 = Color32::from_rgba_unmultiplied_const(0, 0, 0, 0x60);

/// The source marker scale: `k = max(12, h / 60) / 11`.
pub fn source_scale(height: u32) -> f32 {
	label_size(height) / 11.0
}

/// The source markers of the enabled sources, all scaled by [`source_scale`]: every composite source's emitter dots,
/// then per source the disc, the glyph, the centre dot and the name over its outline halo.
pub fn source_marker_shapes(
	fonts: &mut FontsView<'_>,
	outlines: &mut Outlines,
	sources: &[Source],
	seed: i32,
	canvas: (u32, u32),
) -> Vec<Shape> {
	let k = source_scale(canvas.1);
	let enabled: Vec<&Source> = sources.iter().filter(|s| s.enabled).collect();
	let mut out = Vec::new();
	for source in enabled.iter().filter(|s| s.array.is_some()) {
		let dots: Vec<Pos2> =
			elements_of(source, seed).iter().map(|e| pos2(e.x as f32 + 0.5, e.y as f32 + 0.5)).collect();
		out.extend(dots.iter().map(|&c| Shape::circle_filled(c, 2.5 * k, Color32::WHITE)));
		out.extend(dots.iter().map(|&c| Shape::circle_stroke(c, 2.5 * k, Stroke::new(1.5 * k, HALO))));
	}
	for source in enabled {
		let c = pos2((source.x + 0.5) as f32, (source.y + 0.5) as f32);
		out.push(Shape::circle_filled(c, 6. * k, SOURCE_DISC));
		for stroke in [Stroke::new(3.5 * k, HALO), Stroke::new(1.5 * k, Color32::WHITE)] {
			out.push(Shape::circle_stroke(c, 6. * k, stroke));
			let p = |gx: f32, gy: f32| c + vec2(gx - 10., gy - 10.) * k;
			for (a, b) in
				[((10., 0.), (10., 2.5)), ((10., 17.5), (10., 20.)), ((0., 10.), (2.5, 10.)), ((17.5, 10.), (20., 10.))]
			{
				out.push(Shape::line_segment([p(a.0, a.1), p(b.0, b.1)], stroke));
			}
		}
		out.push(Shape::circle_filled(c, 2.5 * k, HALO));
		out.push(Shape::circle_filled(c, 1.5 * k, Color32::WHITE));
		if source.name.is_empty() {
			continue;
		}
		let size = 11. * k;
		let galley = fonts.layout_no_wrap(source.name.clone(), FontId::proportional(size), Color32::WHITE);
		let (tw, th) = (galley.size().x, galley.size().y);
		let (x, y) =
			if source.array.is_some() { (c.x - tw / 2., c.y + 13. * k) } else { (c.x + 13. * k, c.y - th / 2.) };
		let at = pos2(x.clamp(0., (canvas.0 as f32 - tw).max(0.)), y.clamp(0., (canvas.1 as f32 - th).max(0.)));
		haloed_text(&mut out, outlines, at, galley, size, Color32::WHITE, Halo { color: HALO, width: 3. * k });
	}
	out
}

/// One active probe of an export: its 1-based number among the active probes, a copy, its recording.
#[derive(Clone, Debug)]
pub struct OverlayProbe {
	pub number: usize,
	pub probe: Probe,
	pub trace: Option<ProbeTrace>,
	/// Smallest top of the graph's fitted range.
	pub floor: f64,
}

/// Everything burned into a project's exported frames, copied when the export starts.
#[derive(Clone, Debug)]
pub struct Overlays {
	pub width: u32,
	pub height: u32,
	/// The enabled sources when source markers are on.
	pub sources: Vec<Source>,
	pub seed: i32,
	pub probe_markers: bool,
	pub probe_graphs: bool,
	pub probes: Vec<OverlayProbe>,
	pub visualize_every: u32,
	/// Steps of the run, the x span of a full-scale graph.
	pub total_steps: u64,
}

impl Overlays {
	/// From a project's Display settings for a canvas of `width x height`; `traces` are the run's recordings of the
	/// active probes in project order.
	pub fn new(project: &Project, width: u32, height: u32, traces: &[ProbeTrace], total_steps: u64) -> Self {
		let display = &project.display;
		let sources = if display.source_markers_in_export {
			project.sources.iter().filter(|s| s.enabled).cloned().collect()
		} else {
			Vec::new()
		};
		let probes = project
			.probes
			.iter()
			.filter(|p| p.active)
			.enumerate()
			.map(|(i, p)| OverlayProbe {
				number: i + 1,
				probe: p.clone(),
				trace: traces.get(i).cloned(),
				floor: graph::full_scale(p, display),
			})
			.collect();
		Self {
			width,
			height,
			sources,
			seed: project.seed,
			probe_markers: display.probe_markers_in_export,
			probe_graphs: display.probe_graphs_in_export,
			probes,
			visualize_every: project.simulation.visualize_every,
			total_steps,
		}
	}

	/// Whether [`Overlays::draw`] changes anything.
	pub fn is_empty(&self) -> bool {
		self.sources.is_empty() && (self.probes.is_empty() || !(self.probe_markers || self.probe_graphs))
	}

	/// A rasteriser of the overlays' frames with the source markers, which do not move, under every frame's shapes.
	pub fn rasteriser(&self) -> Rasteriser {
		let (sources, seed, canvas) = (self.sources.clone(), self.seed, (self.width, self.height));
		Rasteriser::new(self.width, self.height).with_fixed(move |fonts, outlines| {
			let whole = frame_rect(canvas);
			let markers = source_marker_shapes(fonts, outlines, &sources, seed, canvas);
			markers.into_iter().map(|s| ClippedShape::new(whole, s)).collect()
		})
	}

	/// The shapes of `layer` at timeline `frame` over the source markers: the probe markers of the layer, then its
	/// graph boxes.
	pub fn shapes(&self, fonts: &mut FontsView<'_>, layer: Layer, frame: usize) -> Vec<ClippedShape> {
		let canvas = (self.width, self.height);
		let whole = frame_rect(canvas);
		let mut out = Vec::new();
		let of_layer = || self.probes.iter().filter(move |p| p.probe.layer == layer);
		if self.probe_markers {
			out.extend(probe_marker_shapes(fonts, of_layer().map(|p| (p.number, &p.probe)), canvas));
		}
		if self.probe_graphs {
			let step = frame as u64 * self.visualize_every as u64;
			for p in of_layer() {
				let trace = p.trace.as_ref().expect("a trace per active probe when graphs are exported");
				let data = GraphData::at(trace, step, frame).with_floor(p.floor);
				let r = box_in_canvas(&p.probe, self.width, self.height);
				let mut metrics = FontMetrics { fonts: &mut *fonts, font: FontId::proportional(graph::FONT_SIZE) };
				let layout =
					graph::layout(&p.probe, &data, self.total_steps, r.width as f32, r.height as f32, &mut metrics);
				if layout.visible {
					let grid = DeviceGrid::new(1., r.x as f32, r.y as f32);
					out.extend(graph::box_shapes(&layout, grid, whole, false, None, fonts));
				}
			}
		}
		out
	}

	/// Burns the overlays of `layer` at timeline `frame` into an RGBA8 frame of the overlays' size, with `raster`
	/// from [`Overlays::rasteriser`].
	pub fn draw(&self, raster: &mut Rasteriser, rgba: &mut [u8], layer: Layer, frame: usize) -> Result<()> {
		raster.render(rgba, |fonts, _| self.shapes(fonts, layer, frame))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn marker_geometry() {
		assert_eq!(arm_length(600), 6);
		assert_eq!(arm_length(1080), 9);
		assert_eq!(label_size(1080), 18.0);
		assert_eq!(label_origin(100, 100, 1080), (111, 100 - 9 - 25));
		assert_eq!(probe_label(3, true, true), "3B");
		assert_eq!(probe_label(2, false, false), "2");
	}

	/// The top-left corners of the squares of `side` pixels in `mesh`.
	fn squares(mesh: &Mesh, side: f32) -> Vec<(f32, f32)> {
		mesh.vertices
			.as_chunks::<4>()
			.0
			.iter()
			.map(|[a, .., d]| {
				assert_eq!(d.pos - a.pos, vec2(side, side));
				(a.pos.x, a.pos.y)
			})
			.collect()
	}

	#[test]
	fn thick_line_stamps_squares_that_reach_the_canvas() {
		let mut out = Mesh::default();
		thick_line(&mut out, (0, 0), (3, 0), Color32::WHITE, 1, (8, 8));
		assert_eq!(squares(&out, 3.), [(-1., -1.), (0., -1.), (1., -1.), (2., -1.)]);
		out.clear();
		thick_line(&mut out, (-300, 5), (500, 5), Color32::WHITE, 1, (8, 8));
		assert_eq!(squares(&out, 3.).len(), 10, "x from -1 to 8 only");
		out.clear();
		thick_line(&mut out, (-50, -50), (-3, -3), Color32::WHITE, 1, (8, 8));
		assert!(out.is_empty(), "never within reach");
	}

	#[test]
	fn probe_ends_at_the_integer_limits_draw_at_once() {
		let mut out = Mesh::default();
		let probe = Probe { kind: ProbeKind::Line, ax: i32::MIN, ay: 3, bx: i32::MAX, by: 20, ..Probe::default() };
		probe_strokes(&mut out, &probe, (32, 24));
		let n = out.vertices.len() / 4;
		assert!(n > 0 && n < 200, "the line crosses the canvas: {n} squares");
	}
}
