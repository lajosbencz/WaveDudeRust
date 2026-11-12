//! The probe graph box's layout: axis ranges, ticks, the curve on the pixel grid, the title and the readout; the
//! preview's boxes: which a picture shows, where, and the shapes of a laid-out box.
//!
//! Pure arithmetic over a probe's settings and its recording, in pixels relative to the box's top-left corner, so a
//! preview and an export draw the same graph. Numbers are formatted with a decimal point.

use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::model::{Layer, Measurement, Probe, ScaleMode, YScale};

use crate::draw::{Baseline, DeviceGrid, Ink, Pen, Rect, Rgba, Shape, TextMetrics};

/// Smallest box drawn at all, pixels.
pub const MIN_VISIBLE: (f32, f32) = (40.0, 30.0);
/// Space between the box edge and its text, pixels.
pub const EDGE_PAD: f32 = 3.0;
/// Space between a label and the plot frame, pixels.
pub const LABEL_GAP: f32 = 4.0;
const RIGHT_MARGIN: f32 = 8.0;
const BARE_MARGIN: f32 = 6.0;
const X_LABEL_SPACING: f32 = 8.0;
const MIN_PLOT: (f32, f32) = (20.0, 15.0);
/// Room above the largest value as a fraction of the range (of a log axis: of its decades).
const MARGIN: f64 = 0.08;

/// What a graph shows at one instant: the first `count` samples of a point probe's series (value i after step
/// i + 1), or the profile of a line probe recorded with the frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct GraphData<'a> {
	pub series: &'a [f32],
	pub profile: Option<&'a [f32]>,
	/// Line length in canvas pixels, the x span of a profile.
	pub profile_length: f64,
	/// Steps done at the instant.
	pub steps_sampled: u64,
}

impl<'a> GraphData<'a> {
	/// The trace as it stood at frame `frame`, after 0-based step `step`: the series cut after that step, the
	/// frame's profile (the newest when the frame's has not arrived).
	pub fn at(trace: &'a ProbeTrace, step: u64, frame: usize) -> Self {
		let count = (step as usize + 1).min(trace.series.len());
		let profile = trace.profiles.get(frame.min(trace.profiles.len().saturating_sub(1)));
		Self {
			series: &trace.series[..count],
			profile,
			profile_length: trace.length,
			steps_sampled: trace.steps_sampled.min(step + 1),
		}
	}
}

/// One tick: a pixel position along its axis (relative to the box), its label, whether it is the zero line.
#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
	pub position: f32,
	pub label: String,
	pub zero: bool,
	/// Top-left of the label relative to the box, (0, 0) without a label.
	pub label_at: (f32, f32),
}

/// The geometry of one graph box.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphLayout {
	/// The box's size, as laid out; the box is drawn at this size.
	pub width: f32,
	pub height: f32,
	/// False under 40 x 30: nothing is drawn.
	pub visible: bool,
	/// False when only the box and its title fit.
	pub has_plot: bool,
	/// Curve stroke width: `max(1.2, height / 110)`.
	pub line_width: f32,
	pub plot_x: f32,
	pub plot_y: f32,
	pub plot_width: f32,
	pub plot_height: f32,
	/// Horizontal grid lines, labels right-aligned in the left margin, vertically centred on their line and kept
	/// beside the plot.
	pub y_ticks: Vec<Tick>,
	/// Vertical grid lines, labels centred below the plot and kept inside the box.
	pub x_ticks: Vec<Tick>,
	pub show_labels: bool,
	/// The curve; one point is a dot. Clip to the plot when stroking.
	pub curve: Vec<(f32, f32)>,
	pub title: String,
	/// "= value" or "max value", or empty.
	pub readout: String,
}

fn log_axis(probe: &Probe) -> bool {
	probe.y_scale == YScale::Logarithmic && probe.measurement != Measurement::Amplitude
}

fn fixed_range(probe: &Probe) -> (f64, f64) {
	(probe.y_min, probe.y_max)
}

fn extent(values: &[f32]) -> Option<(f32, f32)> {
	values.iter().copied().filter(|v| v.is_finite()).fold(None, |acc, v| match acc {
		None => Some((v, v)),
		Some((lo, hi)) => Some((lo.min(v), hi.max(v))),
	})
}

/// Fitted y range: an amplitude (or data below zero) symmetric about zero, else 0 up; all zeros keep the range.
fn auto_range(probe: &Probe, lo: f64, hi: f64, range: &mut (f64, f64)) {
	let magnitude = lo.abs().max(hi.abs());
	if magnitude <= 0.0 {
		return;
	}
	*range = if probe.measurement == Measurement::Amplitude || lo < 0.0 {
		let top = magnitude * (1.0 + MARGIN);
		(-top, top)
	} else {
		(0.0, hi * (1.0 + MARGIN))
	};
}

/// A log axis: `decades` below the fitted top, or below the fixed maximum.
fn log_range(probe: &Probe, fitted_hi: Option<f64>) -> (f64, f64) {
	let decades = probe.decades as f64;
	if let Some(hi) = fitted_hi.filter(|h| *h > 0.0) {
		let top = hi.log10() + MARGIN * decades;
		return (10f64.powf(top - decades), 10f64.powf(top));
	}
	let (mut lo, mut hi) = fixed_range(probe);
	if hi.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
		hi = 1.0;
	}
	if probe.auto_scale_y || lo.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) || lo >= hi {
		lo = hi / 10f64.powf(decades);
	}
	(lo, hi)
}

/// `(x_min, x_max, y_min, y_max)` of a probe's graph; x in steps (a series) or line pixels (a profile).
pub fn ranges(probe: &Probe, data: &GraphData<'_>, total_steps: u64) -> (f64, f64, f64, f64) {
	let mut y = fixed_range(probe);
	let mut fitted_hi = None;
	let (x_min, x_max) = if probe.shows_profile() {
		let x_max = if data.profile.is_some() && data.profile_length > 0.0 { data.profile_length } else { 1.0 };
		if let Some((lo, hi)) = data.profile.and_then(extent).filter(|_| probe.auto_scale_y) {
			fitted_hi = Some(hi as f64);
			auto_range(probe, lo as f64, hi as f64, &mut y);
		}
		(0.0, x_max)
	} else {
		let start = probe.start_after as f64;
		let samples = data.series.len() as f64;
		let (lo, mut hi) = match probe.scale_mode {
			ScaleMode::Full if probe.auto_scale_x => (start, samples.max(start + 1.0)),
			ScaleMode::Full => (start, (total_steps as f64).max(start + 1.0)),
			ScaleMode::Running => {
				let window = probe.window as f64;
				let hi = if probe.auto_scale_x { samples.max(start + 1.0) } else { samples.max(start + window) };
				(start.max(hi - window), hi)
			}
		};
		if hi <= lo {
			hi = lo + 1.0;
		}
		let fit = visible_indices(data.series.len(), lo, hi).and_then(|(a, b)| extent(&data.series[a..=b]));
		if let Some((l, h)) = fit.filter(|_| probe.auto_scale_y) {
			fitted_hi = Some(h as f64);
			auto_range(probe, l as f64, h as f64, &mut y);
		}
		(lo, hi)
	};
	if log_axis(probe) {
		y = log_range(probe, fitted_hi);
	}
	(x_min, x_max, y.0, y.1)
}

/// Indices of the series samples within `[x_min, x_max]` (sample i at step i + 1), with one either side.
fn visible_indices(count: usize, x_min: f64, x_max: f64) -> Option<(usize, usize)> {
	let first = (x_min.floor() as i64 - 1).max(0) as usize;
	let last = (x_max.ceil() as i64).min(count as i64 - 1);
	(last >= first as i64).then_some((first, last as usize))
}

/// "Nice" (1, 2, 5 x 10^n) ticks covering `[lo, hi]`, at most 40, a tick within rounding of 0 snapped to 0.
pub fn nice_ticks(lo: f64, hi: f64, target: usize) -> Vec<f64> {
	let span = hi - lo;
	if span.is_nan() || span <= 0.0 || span.is_infinite() {
		return Vec::new();
	}
	let raw = span / target.max(1) as f64;
	let magnitude = 10f64.powf(raw.log10().floor());
	let m = raw / magnitude;
	let step = magnitude
		* if m < 1.5 {
			1.0
		} else if m < 3.0 {
			2.0
		} else if m < 7.0 {
			5.0
		} else {
			10.0
		};
	let mut tick = (lo / step).ceil() * step;
	let mut out = Vec::new();
	while tick <= hi + step * 1e-9 && out.len() < 40 {
		out.push(if tick.abs() < step * 1e-9 { 0.0 } else { tick });
		tick += step;
	}
	out
}

/// Ticks of a log axis: powers of ten every 1, 2, 3, 5 or 10 decades (counted from multiples of the spacing) so at
/// most `target + 1` remain; an axis with fewer than two powers of ten takes the positive nice ticks.
pub fn log_ticks(lo: f64, hi: f64, target: usize) -> Vec<f64> {
	if lo.is_nan() || hi.is_nan() || lo <= 0.0 || hi <= lo || hi.is_infinite() {
		return Vec::new();
	}
	let first = (lo.log10() - 1e-9).ceil() as i32;
	let last = (hi.log10() + 1e-9).floor() as i32;
	if last - first < 1 {
		return nice_ticks(lo, hi, target).into_iter().filter(|t| *t > 0.0).collect();
	}
	let every = [1, 2, 3, 5].into_iter().find(|c| ((last - first) / c + 1) as usize <= target + 1).unwrap_or(10);
	(first..=last)
		.filter(|e| e % every == 0)
		.map(|e| if e >= 0 { 10f64.powi(e) } else { 1.0 / 10f64.powi(-e) })
		.collect()
}

/// `v` to three significant digits, positional for exponents -4..=2, else `d.ddE+XX`; trailing zeros dropped; 0 is
/// "0".
pub fn format_g3(v: f64) -> String {
	if v == 0.0 {
		return "0".into();
	}
	if !v.is_finite() {
		return format!("{v}");
	}
	let sci = format!("{v:.2e}");
	let (mantissa, exponent) = sci.split_once('e').expect("scientific notation has an exponent");
	let exponent: i32 = exponent.parse().expect("an exponent is an integer");
	let trim = |s: String| {
		if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
	};
	if (-4..3).contains(&exponent) {
		let decimals = (2 - exponent).max(0) as usize;
		let mantissa: f64 = mantissa.parse().expect("a mantissa is a number");
		trim(format!("{:.*}", decimals, mantissa * 10f64.powi(exponent)))
	} else {
		let sign = if exponent < 0 { '-' } else { '+' };
		format!("{}E{sign}{:02}", trim(mantissa.to_owned()), exponent.abs())
	}
}

/// A log tick: a power of ten outside 0.01..100 as `1e<n>`, else [`format_g3`].
pub fn format_log_tick(v: f64) -> String {
	let exponent = v.log10().round();
	if (v / 10f64.powf(exponent) - 1.0).abs() > 1e-6 || exponent.abs() <= 2.0 {
		format_g3(v)
	} else {
		format!("1e{exponent}")
	}
}

/// An x position: whole when the span is at least 10, else one decimal at most.
pub fn format_position(v: f64, span: f64) -> String {
	if span >= 10.0 {
		format!("{}", v.round() as i64)
	} else {
		let s = format!("{v:.1}");
		let s = s.strip_suffix(".0").unwrap_or(&s).to_owned();
		if s == "-0" { "0".into() } else { s }
	}
}

struct ValueAxis {
	lo: f64,
	hi: f64,
	log: bool,
	top: f32,
	height: f32,
}

impl ValueAxis {
	fn new(min: f64, max: f64, log: bool, top: f32, height: f32) -> Self {
		let t = |v: f64| if log { v.log10() } else { v };
		Self { lo: t(min), hi: t(max), log, top, height }
	}

	/// Pixel y of a value: bottom edge the minimum; on a log axis values at or below the minimum on the bottom
	/// edge; clamped to +-30000.
	fn pixel(&self, v: f64) -> f32 {
		let t = if self.log { if v > 0.0 { v.log10().max(self.lo) } else { self.lo } } else { v };
		let py = self.top as f64 + self.height as f64 - (t - self.lo) / (self.hi - self.lo) * self.height as f64;
		py.clamp(-30000.0, 30000.0) as f32
	}
}

/// Points of `values[first..=last]` reduced to one vertical min/max pair per pixel column.
fn bin_columns(values: &[f32], columns: usize, plot_x: f32, axis: &ValueAxis) -> Vec<(f32, f32)> {
	let visible = values.len();
	let mut out = Vec::with_capacity(columns * 2);
	for c in 0..columns {
		let start = c * visible / columns;
		let end = ((c + 1) * visible / columns).max(start + 1).min(visible);
		if let Some((lo, hi)) = extent(&values[start..end]) {
			let x = plot_x + c as f32 + 0.5;
			out.push((x, axis.pixel(lo as f64)));
			out.push((x, axis.pixel(hi as f64)));
		}
	}
	out
}

fn readout(line: bool, data: &GraphData<'_>) -> String {
	if line {
		return data
			.profile
			.and_then(extent)
			.map(|(_, hi)| format!("max {}", format_g3(hi as f64)))
			.unwrap_or_default();
	}
	match data.series.last() {
		Some(v) if v.is_finite() => format!("= {}", format_g3(*v as f64)),
		_ => String::new(),
	}
}

/// Lays out a box of `width x height` pixels. The text keeps its size; the left margin fits the widest value label,
/// a line is kept below; tick counts drop when labels collide, the labels drop before the plot does. A line probe
/// shows nothing until its start step is reached; a point probe's series is trimmed to the start step instead.
pub fn layout(
	probe: &Probe,
	data: &GraphData<'_>,
	total_steps: u64,
	width: f32,
	height: f32,
	text: &dyn TextMetrics,
) -> GraphLayout {
	let visible = width >= MIN_VISIBLE.0 && height >= MIN_VISIBLE.1;
	let line_width = (height / 110.0).max(1.2);
	let line_height = text.line_height();
	let title_height = line_height + 2.0 * EDGE_PAD;
	let line = probe.shows_profile();
	let started = probe.start_after == 0 || data.steps_sampled >= probe.start_after as u64;
	let shown = if line && !started { GraphData::default() } else { *data };
	let (x_min, x_max, y_min, y_max) = ranges(probe, if line { &shown } else { data }, total_steps);
	let log = log_axis(probe);
	let y_values = |target| if log { log_ticks(y_min, y_max, target) } else { nice_ticks(y_min, y_max, target) };
	let y_label = |t: f64| if log { format_log_tick(t) } else { format_g3(t) };
	let mut ys = y_values(4);
	let widest = ys.iter().map(|t| text.width(&y_label(*t))).fold(0f32, f32::max);
	let mut plot_x = EDGE_PAD + widest + LABEL_GAP;
	let plot_y = title_height;
	let mut plot_width = width - plot_x - RIGHT_MARGIN;
	let mut plot_height = height - plot_y - (line_height + LABEL_GAP);
	let show_labels = visible && plot_width >= MIN_PLOT.0 && plot_height >= MIN_PLOT.1;
	if !show_labels {
		plot_x = BARE_MARGIN;
		plot_width = width - plot_x - BARE_MARGIN;
		plot_height = height - plot_y - BARE_MARGIN;
	}
	let has_plot = visible && plot_width >= MIN_PLOT.0 && plot_height >= MIN_PLOT.1;
	let mut out = GraphLayout {
		width,
		height,
		visible,
		has_plot,
		line_width,
		plot_x,
		plot_y,
		plot_width: plot_width.max(0.0),
		plot_height: plot_height.max(0.0),
		y_ticks: Vec::new(),
		x_ticks: Vec::new(),
		show_labels: false,
		curve: Vec::new(),
		title: probe.name.clone(),
		readout: String::new(),
	};
	if !has_plot {
		return out;
	}
	if show_labels {
		for target in [3, 2] {
			if (ys.len().saturating_sub(1)) as f32 * line_height * 1.1 <= plot_height {
				break;
			}
			ys = y_values(target);
		}
	}
	let axis = ValueAxis::new(y_min, y_max, log, plot_y, plot_height);
	out.y_ticks = ys
		.iter()
		.map(|&t| Tick {
			position: axis.pixel(t),
			label: if show_labels { y_label(t) } else { String::new() },
			zero: t == 0.0,
			label_at: (0.0, 0.0),
		})
		.collect();
	for target in [5, 3, 2] {
		out.x_ticks = nice_ticks(x_min, x_max, target)
			.into_iter()
			.map(|t| Tick {
				position: (plot_x as f64 + (t - x_min) / (x_max - x_min) * plot_width as f64) as f32,
				label: if show_labels { format_position(t, x_max - x_min) } else { String::new() },
				zero: t == 0.0,
				label_at: (0.0, 0.0),
			})
			.collect();
		let labels: f32 = out.x_ticks.iter().map(|t| text.width(&t.label) + X_LABEL_SPACING).sum();
		if labels - X_LABEL_SPACING <= plot_width {
			break;
		}
	}
	let (top, bottom) = (out.plot_y, out.plot_y + out.plot_height);
	for t in out.y_ticks.iter_mut().filter(|t| !t.label.is_empty()) {
		let lx = EDGE_PAD.max(out.plot_x - LABEL_GAP - text.width(&t.label));
		let ly = (t.position - line_height / 2.0).clamp(top - line_height / 2.0, bottom - line_height / 2.0);
		t.label_at = (lx, ly);
	}
	for t in out.x_ticks.iter_mut().filter(|t| !t.label.is_empty()) {
		let lw = text.width(&t.label);
		t.label_at = ((t.position - lw / 2.0).clamp(EDGE_PAD, (width - lw - EDGE_PAD).max(EDGE_PAD)), bottom + 2.0);
	}
	let columns = plot_width as usize;
	out.curve = if line {
		match shown.profile {
			Some(p) if p.len() > columns * 2 => bin_columns(p, columns, plot_x, &axis),
			Some(p) => {
				let n = p.len();
				p.iter()
					.enumerate()
					.filter(|(_, v)| v.is_finite())
					.map(|(i, v)| {
						let f = if n > 1 { i as f32 / (n - 1) as f32 } else { 0.0 };
						(plot_x + f * plot_width, axis.pixel(*v as f64))
					})
					.collect()
			}
			None => Vec::new(),
		}
	} else {
		match visible_indices(data.series.len(), x_min, x_max) {
			None => Vec::new(),
			Some((a, b)) if b - a + 1 > columns * 2 => bin_columns(&data.series[a..=b], columns, plot_x, &axis),
			Some((a, b)) => {
				let scale = plot_width as f64 / (x_max - x_min);
				(a..=b)
					.filter(|&i| data.series[i].is_finite())
					.map(|i| {
						((plot_x as f64 + (i as f64 + 1.0 - x_min) * scale) as f32, axis.pixel(data.series[i] as f64))
					})
					.collect()
			}
		}
	};
	out.show_labels = show_labels;
	out.readout = readout(line, &shown);
	out
}

/// Whether the preview's picture of `layer` shows `probe`'s box: an active probe measuring the layer.
pub fn shows_box(probe: &Probe, layer: Layer) -> bool {
	probe.active && probe.layer == layer
}

/// The preview's box size in canvas pixels: the probe's, shrunk with its aspect kept to fit the canvas.
pub fn box_size(probe: &Probe, canvas: (u32, u32)) -> (f32, f32) {
	let (w, h) = probe.box_pixels();
	let (w, h) = (w as f32, h as f32);
	let f = (canvas.0 as f32 / w).min(canvas.1 as f32 / h).min(1.);
	(w * f, h * f)
}

/// The preview box's top-left corner in canvas pixels, held inside the canvas.
pub fn box_origin(probe: &Probe, canvas: (u32, u32)) -> (f32, f32) {
	let (w, h) = box_size(probe, canvas);
	let x = (probe.box_x as f32).clamp(0., (canvas.0 as f32 - w).max(0.));
	let y = (probe.box_y as f32).clamp(0., (canvas.1 as f32 - h).max(0.));
	(x, y)
}

/// A box at `origin` of `size` canvas pixels over a picture whose top-left is at `picture` and which shows
/// `scale` logical pixels per canvas pixel: its top-left and size in whole logical pixels.
pub fn box_on_picture(
	origin: (f32, f32),
	size: (f32, f32),
	picture: (f32, f32),
	scale: f32,
) -> ((f32, f32), (f32, f32)) {
	let at = ((picture.0 + origin.0 * scale).round(), (picture.1 + origin.1 * scale).round());
	(at, ((size.0 * scale).round(), (size.1 * scale).round()))
}

/// A graph box's text size: logical pixels in the preview, pixels in an export.
pub const FONT_SIZE: f32 = 11.;
/// A graph box's background, in the preview and an export.
pub const BACKGROUND: Rgba = Rgba::new(14, 16, 22, 235);
/// A graph box's edge.
pub const BORDER: Rgba = Rgba::new(200, 200, 210, 170);
/// The grid lines of the plot.
pub const GRID: Rgba = Rgba::new(255, 255, 255, 46);
/// The zero line of the plot.
pub const ZERO: Rgba = Rgba::new(255, 255, 255, 120);
/// The plot's frame.
pub const FRAME: Rgba = Rgba::new(200, 200, 210, 130);
/// The tick labels.
pub const LABEL: Rgba = Rgba::new(225, 225, 230, 225);
/// The title.
pub const TITLE: Rgba = Rgba::new(240, 240, 245, 240);
/// The curve and the readout.
pub const CURVE: Rgba = Rgba::rgb(255, 208, 64);
/// The preview's edge of a selected box.
const SELECTED: Rgba = Rgba::rgb(0xFF, 0xE0, 0x00);
/// The preview's resize grip.
const GRIP_DOTS: Rgba = Rgba::rgb(0x90, 0x90, 0x90);

/// The shapes of the preview's box laid out as `g`, in device pixels of `grid`: the box and its edge (`focus`, else
/// yellow when `selected`), the grid, the value and position labels, the frame, the curve clipped to the plot, the
/// readout and title, and the resize grip's dots. The box is the layout's size (`g.width` x `g.height` logical
/// pixels). Hairlines are whole device pixels on the device grid and text starts on whole device pixels. `text`
/// measures the readout in logical pixels at [`FONT_SIZE`], as the layout was measured.
pub fn box_shapes(
	g: &GraphLayout,
	grid: DeviceGrid,
	selected: bool,
	focus: Option<Rgba>,
	text: &dyn TextMetrics,
) -> Vec<Shape> {
	let (w, h) = (g.width, g.height);
	let s = grid.scale;
	let size = FONT_SIZE * s;
	let label = |text: &str, at: (f32, f32), color: Rgba| Shape::Text {
		text: text.to_owned(),
		at,
		baseline: Baseline::Top,
		size,
		color,
		halo: None,
	};
	let hl = grid.hairline();
	let hairline = |color| Pen::new(color, hl).aliased();
	let half = hl / 2.;
	let (bx0, by0, bx1, by1) = (grid.x(0.).floor(), grid.y(0.).floor(), grid.x(w).floor(), grid.y(h).floor());
	let mut out = vec![
		Shape::Rect { rect: Rect::new(bx0, by0, bx1, by1), ink: Ink::Fill { color: BACKGROUND, antialias: false } },
		Shape::Rect {
			rect: Rect::new(bx0 + half, by0 + half, bx1 - half, by1 - half),
			ink: Ink::Stroke(hairline(focus.unwrap_or(if selected { SELECTED } else { BORDER }))),
		},
	];

	if g.has_plot {
		let (px, py, pw, ph) = (g.plot_x, g.plot_y, g.plot_width, g.plot_height);
		let (left, top, right, bottom) = (grid.x(px), grid.y(py), grid.x(px + pw), grid.y(py + ph));
		for t in &g.y_ticks {
			let y = grid.snap(grid.y(t.position));
			out.push(Shape::Line { from: (left, y), to: (right, y), pen: hairline(if t.zero { ZERO } else { GRID }) });
			if !t.label.is_empty() {
				out.push(label(&t.label, (grid.x(t.label_at.0).round(), grid.y(t.label_at.1).round()), LABEL));
			}
		}
		for t in &g.x_ticks {
			let x = grid.snap(grid.x(t.position));
			out.push(Shape::Line { from: (x, top), to: (x, bottom), pen: hairline(GRID) });
			if !t.label.is_empty() {
				out.push(label(&t.label, (grid.x(t.label_at.0).round(), grid.y(t.label_at.1).round()), LABEL));
			}
		}
		out.push(Shape::Rect {
			rect: Rect::new(grid.snap(left), grid.snap(top), grid.snap(right), grid.snap(bottom)),
			ink: Ink::Stroke(hairline(FRAME)),
		});
		out.push(Shape::Clip { rect: Rect::new(left, top, right, bottom), antialias: true });
		out.extend(match g.curve.as_slice() {
			[] => None,
			[(x, y)] => Some(Shape::Circle { centre: (grid.x(*x), grid.y(*y)), radius: 2. * s, ink: Ink::fill(CURVE) }),
			curve => Some(Shape::Path {
				points: curve.iter().map(|&(x, y)| (grid.x(x), grid.y(y))).collect(),
				closed: false,
				pen: Pen::new(CURVE, g.line_width * s).round(),
			}),
		});
		out.push(Shape::Unclip);
	}

	// the title strip: the readout right-aligned in the curve's colour, the title in what remains
	let mut readout_width = 0.;
	if !g.readout.is_empty() {
		readout_width = text.width(&g.readout);
		out.push(label(&g.readout, (grid.x(w - readout_width - 8.).round(), grid.y(EDGE_PAD).round()), CURVE));
	}
	let title_end = grid.x(EDGE_PAD + 3. + (w - readout_width - 16.).max(10.));
	out.push(Shape::Clip { rect: Rect::new(bx0, by0, title_end, by1), antialias: false });
	out.push(label(&g.title, (grid.x(EDGE_PAD + 3.).round(), grid.y(EDGE_PAD).round()), TITLE));
	out.push(Shape::Unclip);

	let dot = Ink::Fill { color: GRIP_DOTS, antialias: false };
	for k in 0..3 {
		let o = grid.width(3.) * k as f32;
		out.push(Shape::Rect {
			rect: Rect::new(bx1 - hl * 3. - o, by1 - hl * 3., bx1 - hl * 2. - o, by1 - hl * 2.),
			ink: dot,
		});
		out.push(Shape::Rect {
			rect: Rect::new(bx1 - hl * 3., by1 - hl * 3. - o, bx1 - hl * 2., by1 - hl * 2. - o),
			ink: dot,
		});
	}
	out
}

#[cfg(test)]
mod tests {
	use wavedude_core::model::BoxSize;

	use super::*;

	struct Fixed;
	impl TextMetrics for Fixed {
		fn line_height(&self) -> f32 {
			14.0
		}
		fn width(&self, text: &str) -> f32 {
			text.len() as f32 * 6.0
		}
	}

	#[test]
	fn g3_formatting() {
		assert_eq!(format_g3(0.0), "0");
		assert_eq!(format_g3(1.0), "1");
		assert_eq!(format_g3(-0.5), "-0.5");
		assert_eq!(format_g3(123.4), "123");
		assert_eq!(format_g3(1234.0), "1.23E+03");
		assert_eq!(format_g3(0.0001234), "0.000123");
		assert_eq!(format_g3(0.00001234), "1.23E-05");
		assert_eq!(format_g3(2e-6), "2E-06");
		assert_eq!(format_log_tick(1e-6), "1e-6");
		assert_eq!(format_log_tick(0.01), "0.01");
		assert_eq!(format_position(12.6, 20.0), "13");
		assert_eq!(format_position(2.0, 4.0), "2");
		assert_eq!(format_position(2.5, 4.0), "2.5");
	}

	#[test]
	fn ticks() {
		assert_eq!(nice_ticks(-1.0, 1.0, 4), vec![-1.0, -0.5, 0.0, 0.5, 1.0]);
		assert_eq!(log_ticks(1e-6, 1.0, 4), vec![1e-6, 1e-4, 1e-2, 1.0]);
		assert!(nice_ticks(1.0, 1.0, 4).is_empty());
	}

	#[test]
	fn amplitude_axis_is_symmetric_and_log_axis_spans_its_decades() {
		let probe = Probe::default();
		let series = [0.1f32, -0.5, 0.25];
		let data = GraphData { series: &series, ..GraphData::default() };
		let (_, _, lo, hi) = ranges(&probe, &data, 100);
		assert_eq!(lo, -hi);
		assert!((hi - 0.54).abs() < 1e-9);
		let log =
			Probe { measurement: Measurement::Value, y_scale: YScale::Logarithmic, decades: 4, ..Probe::default() };
		let series = [0.0f32, 1e-3, 1e-2];
		let (_, _, lo, hi) = ranges(&log, &GraphData { series: &series, ..GraphData::default() }, 100);
		assert!(((hi / lo).log10() - 4.0).abs() < 1e-9);
		let l = layout(&log, &GraphData { series: &series, ..GraphData::default() }, 100, 400.0, 200.0, &Fixed);
		assert!(l.has_plot && l.show_labels);
		assert_eq!(l.readout, "= 0.01");
		assert_eq!(l.curve.len(), 3);
		assert_eq!(l.curve[0].1, l.plot_y + l.plot_height, "zero on the bottom line");
	}

	#[test]
	fn boxes_fit_the_canvas_on_whole_pixels() {
		let p = Probe { box_size: BoxSize::Medium, box_x: 350, box_y: -5, ..Probe::default() };
		assert_eq!(box_size(&p, (1000, 1000)), (400., 200.));
		assert_eq!(box_size(&p, (200, 400)), (200., 100.), "shrunk with its aspect kept");
		assert_eq!(box_origin(&p, (600, 300)), (200., 0.));
		assert_eq!(box_on_picture((200., 0.), (400., 200.), (4.25, 10.), 0.33), ((70., 10.), (132., 66.)));
		assert!(shows_box(&p, p.layer));
		assert!(!shows_box(&Probe { active: false, ..p.clone() }, p.layer));
	}

	#[test]
	fn tiny_boxes_hide() {
		let l = layout(&Probe::default(), &GraphData::default(), 10, 30.0, 20.0, &Fixed);
		assert!(!l.visible && !l.has_plot);
	}

	#[test]
	fn a_box_draws_its_frame_grid_curve_and_text_on_device_pixels() {
		let series = [0.5f32, -0.5, 0.25];
		let l =
			layout(&Probe::default(), &GraphData { series: &series, ..GraphData::default() }, 3, 300., 150., &Fixed);
		assert!(l.has_plot && l.show_labels);
		let grid = DeviceGrid::new(2., 10.25, 0.);
		let shapes = box_shapes(&l, grid, true, None, &Fixed);
		assert_eq!(
			shapes[0],
			Shape::Rect { rect: Rect::new(0., 0., 600., 300.), ink: Ink::Fill { color: BACKGROUND, antialias: false } }
		);
		assert_eq!(
			shapes[1],
			Shape::Rect { rect: Rect::new(1., 1., 599., 299.), ink: Ink::Stroke(Pen::new(SELECTED, 2.).aliased()) },
			"a selected box's edge, a two-pixel hairline inside the box"
		);
		let lines = shapes.iter().filter(|s| matches!(s, Shape::Line { .. })).count();
		assert_eq!(lines, l.y_ticks.len() + l.x_ticks.len());
		let first_tick = shapes.iter().find_map(|s| match s {
			Shape::Line { from, pen, .. } => Some((from.1, pen.width)),
			_ => None,
		});
		assert_eq!(first_tick.map(|(y, w)| (y.fract(), w)), Some((0., 2.)), "on the device grid");
		let curve = shapes.iter().find_map(|s| match s {
			Shape::Path { points, pen, .. } => Some((points.len(), pen.color, pen.round, pen.width)),
			_ => None,
		});
		assert_eq!(curve, Some((3, CURVE, true, l.line_width * 2.)));
		let texts: Vec<(&str, Rgba)> = shapes
			.iter()
			.filter_map(|s| match s {
				Shape::Text { text, color, baseline: Baseline::Top, size: 22., at, .. } => {
					assert_eq!((at.0.fract(), at.1.fract()), (0., 0.), "{text} starts on a device pixel");
					Some((text.as_str(), *color))
				}
				_ => None,
			})
			.collect();
		assert_eq!(texts.len(), l.y_ticks.len() + l.x_ticks.len() + 2);
		assert_eq!(texts[texts.len() - 2], ("= 0.25", CURVE));
		assert_eq!(texts[texts.len() - 1], (l.title.as_str(), TITLE));
		let clips: Vec<bool> = shapes
			.iter()
			.filter_map(|s| match s {
				Shape::Clip { antialias, .. } => Some(*antialias),
				_ => None,
			})
			.collect();
		assert_eq!(clips, vec![true, false], "the plot, then the title");
		assert_eq!(shapes.iter().filter(|s| **s == Shape::Unclip).count(), 2);
		assert!(shapes.ends_with(&[Shape::Rect {
			rect: Rect::new(594., 282., 596., 284.),
			ink: Ink::Fill { color: GRIP_DOTS, antialias: false },
		}]));
	}

	#[test]
	fn a_box_without_a_plot_keeps_its_title_and_focus() {
		let l = layout(&Probe { name: "P".into(), ..Probe::default() }, &GraphData::default(), 10, 60., 30., &Fixed);
		assert!(!l.has_plot);
		let focus = Rgba::rgb(1, 2, 3);
		let shapes = box_shapes(&l, DeviceGrid::new(1., 0., 0.), true, Some(focus), &Fixed);
		assert_eq!(
			shapes[1],
			Shape::Rect { rect: Rect::new(0.5, 0.5, 59.5, 29.5), ink: Ink::Stroke(Pen::new(focus, 1.).aliased()) }
		);
		assert!(shapes.iter().all(|s| !matches!(s, Shape::Line { .. } | Shape::Path { .. })));
		assert!(shapes.contains(&Shape::Text {
			text: "P".into(),
			at: (6., 3.),
			baseline: Baseline::Top,
			size: FONT_SIZE,
			color: TITLE,
			halo: None,
		}));
		assert_eq!(shapes.len(), 2 + 3 + 6, "box, edge, the title in its clip, the grip's dots");
	}
}
