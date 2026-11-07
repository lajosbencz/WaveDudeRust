//! What colour exports burn into a rendered frame: source markers, probe markers and probe graph boxes
//! (docs/spec/50-export.md, Burned-in overlays). Canvas pixels; a source marker centred on the pixel centre
//! `(x + 0.5, y + 0.5)`, probe strokes on whole pixels; everything clipped to the frame.

use wave_core::engine::emitters::elements_of;
use wave_core::engine::probes::ProbeTrace;
use wave_core::model::normalize::box_in_canvas;
use wave_core::model::{Layer, Probe, ProbeKind, Project, Source};

use crate::graph::{self, EDGE_PAD, GraphData, GraphLayout, LABEL_GAP, TextMetrics};
use crate::raster::{BLACK, Canvas, Color, IRect, Mask, Typeface, WHITE, argb, typeface};

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

/// A line of squares of `(2r + 1)^2` pixels at `steps = max(|dx|, |dy|) + 1` points, point k at
/// `(x0 + round_even(dx * k / steps), y0 + round_even(dy * k / steps))`, overwriting pixels, clipped.
pub fn thick_line(canvas: &mut Canvas, (x0, y0): (i32, i32), (x1, y1): (i32, i32), color: Color, radius: i32) {
	let steps = (x1 - x0).abs().max((y1 - y0).abs());
	for k in 0..=steps {
		let at = |a: i32, b: i32| {
			if steps == 0 { a } else { a + ((b - a) as f64 * k as f64 / steps as f64).round_ties_even() as i32 }
		};
		let (x, y) = (at(x0, x1), at(y0, y1));
		let r = IRect::new(x - radius, y - radius, 2 * radius + 1, 2 * radius + 1).intersect(&canvas.bounds());
		for py in r.y..r.bottom() {
			for px in r.x..r.right() {
				canvas.set(px, py, color);
			}
		}
	}
}

/// A probe's strokes: the line A-B (a line probe) and a cross at each end, black at radius 2, then white at 1.
pub fn draw_probe_strokes(canvas: &mut Canvas, probe: &Probe) {
	let arm = arm_length(canvas.height() as u32);
	let line = probe.kind == ProbeKind::Line;
	let (a, b) = ((probe.ax, probe.ay), (probe.bx, probe.by));
	for (color, radius) in [(BLACK, 2), (WHITE, 1)] {
		if line {
			thick_line(canvas, a, b, color, radius);
		}
		let ends: &[(i32, i32)] = if line { &[a, b] } else { &[a] };
		for &(x, y) in ends {
			thick_line(canvas, (x - arm, y), (x + arm, y), color, radius);
			thick_line(canvas, (x, y - arm), (x, y + arm), color, radius);
		}
	}
}

const LABEL_SHADOW: Color = argb(220, 0, 0, 0);

/// A probe label: a rectangle of `ceil(width) + 4 x ceil(line height) + 4` at its origin, the bold text's shadow at
/// (3, 3) inside it, the white text at (2, 2).
pub fn draw_probe_label(canvas: &mut Canvas, text: &str, (x, y): (i32, i32)) {
	let size = label_size(canvas.height() as u32);
	let face = typeface();
	let rect =
		IRect::new(x, y, face.measure(text, size, true).ceil() as i32 + 4, face.line_height(size).ceil() as i32 + 4)
			.intersect(&canvas.bounds());
	for (offset, color) in [(3.0, LABEL_SHADOW), (2.0, WHITE)] {
		let mask = face.mask(text, size, x as f32 + offset, y as f32 + offset, true, rect);
		canvas.fill_mask(&mask, color);
	}
}

/// Every probe's strokes, then every label; `probes` are (number, probe).
pub fn draw_probe_markers<'a>(canvas: &mut Canvas, probes: impl Iterator<Item = (usize, &'a Probe)> + Clone) {
	for (_, p) in probes.clone() {
		draw_probe_strokes(canvas, p);
	}
	let h = canvas.height() as u32;
	for (n, p) in probes {
		let line = p.kind == ProbeKind::Line;
		draw_probe_label(canvas, &probe_label(n, line, false), label_origin(p.ax, p.ay, h));
		if line {
			draw_probe_label(canvas, &probe_label(n, true, true), label_origin(p.bx, p.by, h));
		}
	}
}

const HALO: Color = argb(191, 0, 0, 0);
const SOURCE_DISC: Color = argb(0x60, 0, 0, 0);

/// The source marker scale: `k = max(12, h / 60) / 11`.
pub fn source_scale(height: u32) -> f32 {
	label_size(height) / 11.0
}

/// The source markers of the enabled sources as a premultiplied RGBA8 layer of `width x height`, and the rectangle
/// that holds ink: every composite source's emitter dots, then per source the disc, the glyph, the centre dot and the
/// name, all scaled by [`source_scale`].
pub fn source_marker_layer(sources: &[Source], seed: i32, width: u32, height: u32) -> (Vec<u8>, IRect) {
	let mut layer = vec![0u8; width as usize * height as usize * 4];
	let mut canvas = Canvas::new(&mut layer, width as usize, height as usize);
	let full = canvas.bounds();
	let k = source_scale(height);
	let enabled: Vec<&Source> = sources.iter().filter(|s| s.enabled).collect();
	let mut ink = IRect::default();
	let mut grow = |r: IRect| {
		ink = if ink.is_empty() {
			r
		} else {
			let (x, y) = (ink.x.min(r.x), ink.y.min(r.y));
			IRect::new(x, y, ink.right().max(r.right()) - x, ink.bottom().max(r.bottom()) - y)
		}
	};
	for source in enabled.iter().filter(|s| s.array.is_some()) {
		let elements = elements_of(source, seed);
		let mut fill = Mask::new(full);
		let mut outline = Mask::new(full);
		for e in &elements {
			let (x, y) = (e.x as f32 + 0.5, e.y as f32 + 0.5);
			fill.disc(x, y, 2.5 * k);
			outline.ring(x, y, 2.5 * k, 1.5 * k);
		}
		canvas.fill_mask(&fill, WHITE);
		canvas.fill_mask(&outline, HALO);
		for e in &elements {
			let r = (4.0 * k).ceil() as i32 + 1;
			grow(IRect::new(e.x.floor() as i32 - r, e.y.floor() as i32 - r, 2 * r + 2, 2 * r + 2));
		}
	}
	let face = typeface();
	for source in enabled {
		let (cx, cy) = ((source.x + 0.5) as f32, (source.y + 0.5) as f32);
		let reach = (12.0 * k).ceil() as i32 + 2;
		let (sx, sy) = (source.x.floor() as i32, source.y.floor() as i32);
		let local = IRect::new(sx - reach, sy - reach, 2 * reach + 2, 2 * reach + 2).intersect(&full);
		let mut disc = Mask::new(local);
		disc.disc(cx, cy, 6.0 * k);
		canvas.fill_mask(&disc, SOURCE_DISC);
		let glyph = |width: f32| {
			let mut m = Mask::new(local);
			let p = |gx: f32, gy: f32| (cx + (gx - 10.0) * k, cy + (gy - 10.0) * k);
			m.ring(cx, cy, 6.0 * k, width);
			for (a, b) in [
				((10.0, 0.0), (10.0, 2.5)),
				((10.0, 17.5), (10.0, 20.0)),
				((0.0, 10.0), (2.5, 10.0)),
				((17.5, 10.0), (20.0, 10.0)),
			] {
				m.segment(p(a.0, a.1), p(b.0, b.1), width);
			}
			m
		};
		canvas.fill_mask(&glyph(3.5 * k), HALO);
		canvas.fill_mask(&glyph(1.5 * k), WHITE);
		let mut dot = Mask::new(local);
		dot.disc(cx, cy, 2.5 * k);
		canvas.fill_mask(&dot, HALO);
		let mut dot = Mask::new(local);
		dot.disc(cx, cy, 1.5 * k);
		canvas.fill_mask(&dot, WHITE);
		grow(local);
		if source.name.is_empty() {
			continue;
		}
		let size = 11.0 * k;
		let (tw, th) = (face.measure(&source.name, size, false), face.line_height(size));
		let (x, y) =
			if source.array.is_some() { (cx - tw / 2.0, cy + 13.0 * k) } else { (cx + 13.0 * k, cy - th / 2.0) };
		let x = x.clamp(0.0, (width as f32 - tw).max(0.0));
		let y = y.clamp(0.0, (height as f32 - th).max(0.0));
		let text = face.mask(&source.name, size, x, y, false, full);
		canvas.fill_mask(&text.dilate(1.5 * k), HALO);
		canvas.fill_mask(&text, WHITE);
		grow(text.bounds().intersect(&full));
		let reach = (1.5 * k + 0.5).ceil() as i32;
		let b = text.bounds();
		grow(IRect::new(b.x - reach, b.y - reach, b.width + 2 * reach, b.height + 2 * reach).intersect(&full));
	}
	(layer, ink)
}

/// The smallest rectangle holding every pixel of a premultiplied layer `width` pixels wide with alpha above zero.
fn ink_of(layer: &[u8], width: usize) -> IRect {
	let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
	for (y, row) in layer.chunks_exact(width * 4).enumerate() {
		for (x, px) in row.as_chunks::<4>().0.iter().enumerate() {
			if px[3] != 0 {
				(x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
			}
		}
	}
	if x0 > x1 {
		IRect::default()
	} else {
		IRect::new(x0 as i32, y0 as i32, (x1 - x0 + 1) as i32, (y1 - y0 + 1) as i32)
	}
}

/// The fixed colours of a graph box.
mod box_colors {
	use super::{Color, argb};
	pub const BACKGROUND: Color = argb(235, 14, 16, 22);
	pub const BORDER: Color = argb(170, 200, 200, 210);
	pub const GRID: Color = argb(46, 255, 255, 255);
	pub const ZERO: Color = argb(120, 255, 255, 255);
	pub const FRAME: Color = argb(130, 200, 200, 210);
	pub const LABEL: Color = argb(225, 225, 225, 230);
	pub const TITLE: Color = argb(240, 240, 240, 245);
	pub const CURVE: Color = argb(255, 255, 208, 64);
}

/// Graph box text size, pixels.
pub const GRAPH_TEXT_SIZE: f32 = 11.0;

/// [`TextMetrics`] of the embedded typeface at one size.
pub struct FaceMetrics {
	pub face: &'static Typeface,
	pub size: f32,
}

impl TextMetrics for FaceMetrics {
	fn line_height(&self) -> f32 {
		self.face.line_height(self.size)
	}

	fn width(&self, text: &str) -> f32 {
		self.face.measure(text, self.size, false)
	}
}

/// Draws a laid-out graph box with its top-left at `(bx, by)`.
pub fn draw_graph_box(canvas: &mut Canvas, layout: &GraphLayout, bx: i32, by: i32) {
	use box_colors::*;
	if !layout.visible {
		return;
	}
	let (w, h) = (layout.width.round() as i32, layout.height.round() as i32);
	let rect = IRect::new(bx, by, w, h);
	let clip = rect.intersect(&canvas.bounds());
	let face = typeface();
	let size = GRAPH_TEXT_SIZE;
	let line_height = face.line_height(size);
	let text = |canvas: &mut Canvas, s: &str, x: f32, y: f32, color: Color| {
		let mask = face.mask(s, size, bx as f32 + x, by as f32 + y, false, clip);
		canvas.fill_mask(&mask, color);
	};
	canvas.fill_rect(clip, BACKGROUND);
	canvas.stroke_rect(rect, BORDER);
	if layout.has_plot {
		let left = layout.plot_x.round() as i32;
		let top = layout.plot_y.round() as i32;
		let right = (layout.plot_x + layout.plot_width).round() as i32;
		let bottom = (layout.plot_y + layout.plot_height).round() as i32;
		let (pt, pb) = (layout.plot_y, layout.plot_y + layout.plot_height);
		for tick in &layout.y_ticks {
			let row = tick.position.round() as i32;
			canvas.fill_rect(
				IRect::new(bx + left, by + row, right - left, 1).intersect(&clip),
				if tick.zero { ZERO } else { GRID },
			);
			if !tick.label.is_empty() {
				let lw = face.measure(&tick.label, size, false);
				let lx = EDGE_PAD.max(layout.plot_x - LABEL_GAP - lw);
				let ly = (tick.position - line_height / 2.0).clamp(pt - line_height / 2.0, pb - line_height / 2.0);
				text(canvas, &tick.label, lx, ly, LABEL);
			}
		}
		for tick in &layout.x_ticks {
			let column = tick.position.round() as i32;
			canvas.fill_rect(IRect::new(bx + column, by + top, 1, bottom - top).intersect(&clip), GRID);
			if !tick.label.is_empty() {
				let lw = face.measure(&tick.label, size, false);
				let lx = (tick.position - lw / 2.0).clamp(EDGE_PAD, EDGE_PAD.max(layout.width - lw - EDGE_PAD));
				text(canvas, &tick.label, lx, pb + 2.0, LABEL);
			}
		}
		canvas.stroke_rect(IRect::new(bx + left, by + top, right - left + 1, bottom - top + 1).intersect(&clip), FRAME);
		let plot_clip = IRect::new(bx + left, by + top, right - left, bottom - top).intersect(&clip);
		let mut curve = Mask::new(plot_clip);
		let points: Vec<(f32, f32)> = layout.curve.iter().map(|&(x, y)| (bx as f32 + x, by as f32 + y)).collect();
		match points.as_slice() {
			[] => {}
			[p] => curve.disc(p.0, p.1, 2.0),
			_ => curve.polyline(&points, layout.line_width),
		}
		canvas.fill_mask(&curve, CURVE);
	}
	let mut readout_width = 0.0;
	if !layout.readout.is_empty() {
		readout_width = face.measure(&layout.readout, size, false);
		text(canvas, &layout.readout, layout.width - readout_width - 8.0, EDGE_PAD, CURVE);
	}
	let title = face.fit(&layout.title, size, (layout.width - readout_width - 16.0).max(10.0));
	text(canvas, &title, EDGE_PAD + 3.0, EDGE_PAD, TITLE);
}

/// One active probe of an export: its 1-based number among the active probes, a copy, its recording.
#[derive(Clone, Debug)]
pub struct OverlayProbe {
	pub number: usize,
	pub probe: Probe,
	pub trace: Option<ProbeTrace>,
}

/// Everything burned into a project's exported frames, copied when the export starts.
#[derive(Clone, Debug)]
pub struct Overlays {
	pub width: u32,
	pub height: u32,
	/// The source marker layer (premultiplied) and its inked rectangle, when source markers are on.
	pub sources: Option<(Vec<u8>, IRect)>,
	pub probe_markers: bool,
	/// The probe marker layers (premultiplied) of the field and the intensity and their inked rectangles, when
	/// probe markers are on and the layer has probes.
	pub marker_layers: [Option<(Vec<u8>, IRect)>; 2],
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
		let sources = display
			.source_markers_in_export
			.then(|| source_marker_layer(&project.sources, project.seed, width, height))
			.filter(|(_, ink)| !ink.is_empty());
		let probes: Vec<OverlayProbe> = project
			.probes
			.iter()
			.filter(|p| p.active)
			.enumerate()
			.map(|(i, p)| OverlayProbe { number: i + 1, probe: p.clone(), trace: traces.get(i).cloned() })
			.collect();
		let marker_layer = |layer: Layer| {
			let mut of_layer = probes.iter().filter(|p| p.probe.layer == layer).peekable();
			of_layer.peek()?;
			let mut pixels = vec![0u8; width as usize * height as usize * 4];
			let mut canvas = Canvas::new(&mut pixels, width as usize, height as usize);
			draw_probe_markers(&mut canvas, of_layer.map(|p| (p.number, &p.probe)));
			let ink = ink_of(&pixels, width as usize);
			(!ink.is_empty()).then_some((pixels, ink))
		};
		let marker_layers = if display.probe_markers_in_export {
			[marker_layer(Layer::Field), marker_layer(Layer::Intensity)]
		} else {
			[None, None]
		};
		Self {
			width,
			height,
			sources,
			probe_markers: display.probe_markers_in_export,
			marker_layers,
			probe_graphs: display.probe_graphs_in_export,
			probes,
			visualize_every: project.simulation.visualize_every,
			total_steps,
		}
	}

	/// Whether [`Overlays::draw`] changes anything.
	pub fn is_empty(&self) -> bool {
		self.sources.is_none() && (self.probes.is_empty() || !(self.probe_markers || self.probe_graphs))
	}

	/// Burns the overlays of `layer` at timeline `frame` into an RGBA8 frame of the overlays' size.
	pub fn draw(&self, rgba: &mut [u8], layer: Layer, frame: usize) {
		let mut canvas = Canvas::new(rgba, self.width as usize, self.height as usize);
		if let Some((pixels, ink)) = &self.sources {
			canvas.composite(pixels, *ink);
		}
		let of_layer = || self.probes.iter().filter(move |p| p.probe.layer == layer);
		let markers = match layer {
			Layer::Field => &self.marker_layers[0],
			Layer::Intensity => &self.marker_layers[1],
		};
		if let Some((pixels, ink)) = markers {
			canvas.composite(pixels, *ink);
		}
		if self.probe_graphs {
			let metrics = FaceMetrics { face: typeface(), size: GRAPH_TEXT_SIZE };
			let step = frame as u64 * self.visualize_every as u64;
			for p in of_layer() {
				let trace = p.trace.as_ref().expect("a trace per active probe when graphs are exported");
				let data = GraphData::at(trace, step, frame);
				let r = box_in_canvas(&p.probe, self.width, self.height);
				let layout =
					graph::layout(&p.probe, &data, self.total_steps, r.width as f32, r.height as f32, &metrics);
				draw_graph_box(&mut canvas, &layout, r.x, r.y);
			}
		}
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

	#[test]
	fn thick_line_stamps_squares_clipped() {
		let mut px = vec![0u8; 8 * 8 * 4];
		let mut c = Canvas::new(&mut px, 8, 8);
		thick_line(&mut c, (0, 0), (3, 0), WHITE, 1);
		assert_eq!(c.get(4, 1), Some(WHITE));
		assert_eq!(c.get(5, 0), Some([0; 4]));
		assert_eq!(c.get(0, 2), Some([0; 4]));
	}
}
