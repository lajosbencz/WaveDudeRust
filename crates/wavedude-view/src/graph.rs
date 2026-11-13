//! The probe graph box's layout: axis ranges, ticks, the curve on the pixel grid, the title and the readout; the
//! preview's boxes: which a picture shows, where, and the shapes of a laid-out box.
//!
//! Pure arithmetic over a probe's settings and its recording, in pixels relative to the box's top-left corner, so a
//! preview and an export draw the same graph. Numbers are formatted with a decimal point.

use epaint::text::{FontId, FontsView};
use epaint::{ClippedShape, Color32, Rect, Shape, Stroke, StrokeKind};
use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::model::{Display, Layer, Measurement, Probe, ScaleMode, YScale};

use crate::draw::{DeviceGrid, TextMetrics};

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
	/// Smallest top of a fitted y range, 0 for none; see [`full_scale`].
	pub floor: f64,
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
			floor: 0.0,
		}
	}

	/// The same data with fitted ranges topping at least at `floor`.
	pub fn with_floor(self, floor: f64) -> Self {
		Self { floor, ..self }
	}
}

/// What the picture shows at the palette's top for `probe`'s measurement (0 for none): below it a fitted range
/// would draw what the picture renders black.
pub fn full_scale(probe: &Probe, display: &Display) -> f64 {
	let brightness = match probe.layer {
		Layer::Field => display.field.brightness,
		Layer::Intensity => display.intensity.brightness,
	};
	match probe.measurement {
		Measurement::Amplitude => 1.0 / brightness,
		Measurement::Value => 1.0 / (brightness * brightness),
		Measurement::IntegratedValue => 0.0,
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

/// Fitted y range, its top at least `floor`: an amplitude (or data below zero) symmetric about zero, else 0 up; all
/// zeros keep the range.
fn auto_range(probe: &Probe, lo: f64, hi: f64, floor: f64, range: &mut (f64, f64)) {
	let magnitude = lo.abs().max(hi.abs());
	if magnitude <= 0.0 {
		return;
	}
	*range = if probe.measurement == Measurement::Amplitude || lo < 0.0 {
		let top = magnitude.max(floor) * (1.0 + MARGIN);
		(-top, top)
	} else {
		(0.0, hi.max(floor) * (1.0 + MARGIN))
	};
}

/// A log axis: `decades` below the fitted top, or below the fixed maximum.
fn log_range(probe: &Probe, fitted_hi: Option<f64>) -> (f64, f64) {
	let decades = probe.decades as f64;
	if let Some(hi) = fitted_hi.filter(|h| *h > 0.0) {
		let top = libm::log10(hi) + MARGIN * decades;
		return (libm::pow(10.0, top - decades), libm::pow(10.0, top));
	}
	let (mut lo, mut hi) = fixed_range(probe);
	if hi.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
		hi = 1.0;
	}
	if probe.auto_scale_y || lo.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) || lo >= hi {
		lo = hi / libm::pow(10.0, decades);
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
			fitted_hi = Some((hi as f64).max(data.floor));
			auto_range(probe, lo as f64, hi as f64, data.floor, &mut y);
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
			fitted_hi = Some((h as f64).max(data.floor));
			auto_range(probe, l as f64, h as f64, data.floor, &mut y);
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
	let magnitude = libm::pow(10.0, libm::log10(raw).floor());
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
	let first = (libm::log10(lo) - 1e-9).ceil() as i32;
	let last = (libm::log10(hi) + 1e-9).floor() as i32;
	if last - first < 1 {
		return nice_ticks(lo, hi, target).into_iter().filter(|t| *t > 0.0).collect();
	}
	let every = [1, 2, 3, 5].into_iter().find(|c| ((last - first) / c + 1) as usize <= target + 1).unwrap_or(10);
	(first..=last)
		.filter(|e| e % every == 0)
		.map(|e| if e >= 0 { libm::pow(10.0, f64::from(e)) } else { 1.0 / libm::pow(10.0, f64::from(-e)) })
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
		trim(format!("{:.*}", decimals, mantissa * libm::pow(10.0, f64::from(exponent))))
	} else {
		let sign = if exponent < 0 { '-' } else { '+' };
		format!("{}E{sign}{:02}", trim(mantissa.to_owned()), exponent.abs())
	}
}

/// A log tick: a power of ten outside 0.01..100 as `1e<n>`, else [`format_g3`].
pub fn format_log_tick(v: f64) -> String {
	let exponent = libm::log10(v).round();
	if (v / libm::pow(10.0, exponent) - 1.0).abs() > 1e-6 || exponent.abs() <= 2.0 {
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
		let t = |v: f64| if log { libm::log10(v) } else { v };
		Self { lo: t(min), hi: t(max), log, top, height }
	}

	/// Pixel y of a value: bottom edge the minimum; on a log axis values at or below the minimum on the bottom
	/// edge; clamped to +-30000.
	fn pixel(&self, v: f64) -> f32 {
		let t = if self.log { if v > 0.0 { libm::log10(v).max(self.lo) } else { self.lo } } else { v };
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
	text: &mut dyn TextMetrics,
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
pub const BACKGROUND: Color32 = Color32::from_rgba_unmultiplied_const(14, 16, 22, 235);
/// A graph box's edge.
pub const BORDER: Color32 = Color32::from_rgba_unmultiplied_const(200, 200, 210, 170);
/// The grid lines of the plot.
pub const GRID: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 46);
/// The zero line of the plot.
pub const ZERO: Color32 = Color32::from_rgba_unmultiplied_const(255, 255, 255, 120);
/// The plot's frame.
pub const FRAME: Color32 = Color32::from_rgba_unmultiplied_const(200, 200, 210, 130);
/// The tick labels.
pub const LABEL: Color32 = Color32::from_rgba_unmultiplied_const(225, 225, 230, 225);
/// The title.
pub const TITLE: Color32 = Color32::from_rgba_unmultiplied_const(240, 240, 245, 240);
/// The curve and the readout.
pub const CURVE: Color32 = Color32::from_rgb(255, 208, 64);
/// The preview's edge of a selected box.
const SELECTED: Color32 = Color32::from_rgb(0xFF, 0xE0, 0x00);
/// The preview's resize grip.
const GRIP_DOTS: Color32 = Color32::from_rgb(0x90, 0x90, 0x90);

/// The box's device rectangle `[left, top, right, bottom]` in units of `grid`.
fn device_box(g: &GraphLayout, grid: DeviceGrid) -> [f32; 4] {
	[grid.x(0.).floor(), grid.y(0.).floor(), grid.x(g.width).floor(), grid.y(g.height).floor()]
}

/// The shapes of the box laid out as `g` at the origin of `grid`, clipped to `clip` (points of the window): the box
/// and its edge (`focus`, else yellow when `selected`), the grid, the value and position labels, the frame, the curve
/// clipped to the plot, the readout and the title clipped to the title strip. The box is the layout's size
/// (`g.width` x `g.height` logical pixels). Hairlines are whole device pixels on the device grid and text starts on
/// whole device pixels; `fonts` lays the text out at [`FONT_SIZE`] at the grid's scale, as the layout was measured.
pub fn box_shapes(
	g: &GraphLayout,
	grid: DeviceGrid,
	clip: Rect,
	selected: bool,
	focus: Option<Color32>,
	fonts: &mut FontsView<'_>,
) -> Vec<ClippedShape> {
	let font = FontId::proportional(FONT_SIZE);
	let mut text = |text: &str, at: (f32, f32), color: Color32| {
		let galley = fonts.layout_no_wrap(text.to_owned(), font.clone(), color);
		Shape::galley(grid.point(grid.x(at.0).round(), grid.y(at.1).round()), galley, color)
	};
	let hl = grid.hairline();
	let hairline = |color| Stroke::new(hl / grid.scale, color);
	let half = hl / 2.;
	let [bx0, by0, bx1, by1] = device_box(g, grid);
	let mut out = vec![
		Shape::rect_filled(grid.rect((bx0, by0), (bx1, by1)), 0, BACKGROUND),
		Shape::rect_stroke(
			grid.rect((bx0 + half, by0 + half), (bx1 - half, by1 - half)),
			0,
			hairline(focus.unwrap_or(if selected { SELECTED } else { BORDER })),
			StrokeKind::Middle,
		),
	];
	let mut clipped = Vec::new();

	if g.has_plot {
		let (px, py, pw, ph) = (g.plot_x, g.plot_y, g.plot_width, g.plot_height);
		let (left, top, right, bottom) = (grid.x(px), grid.y(py), grid.x(px + pw), grid.y(py + ph));
		for t in &g.y_ticks {
			let y = grid.snap(grid.y(t.position));
			let color = if t.zero { ZERO } else { GRID };
			out.push(Shape::line_segment([grid.point(left, y), grid.point(right, y)], hairline(color)));
			if !t.label.is_empty() {
				out.push(text(&t.label, t.label_at, LABEL));
			}
		}
		for t in &g.x_ticks {
			let x = grid.snap(grid.x(t.position));
			out.push(Shape::line_segment([grid.point(x, top), grid.point(x, bottom)], hairline(GRID)));
			if !t.label.is_empty() {
				out.push(text(&t.label, t.label_at, LABEL));
			}
		}
		out.push(Shape::rect_stroke(
			grid.rect((grid.snap(left), grid.snap(top)), (grid.snap(right), grid.snap(bottom))),
			0,
			hairline(FRAME),
			StrokeKind::Middle,
		));
		let plot = grid.rect((left, top), (right, bottom)).intersect(clip);
		let point = |&(x, y): &(f32, f32)| grid.point(grid.x(x), grid.y(y));
		let curve = match g.curve.as_slice() {
			[] => None,
			[p] => Some(Shape::circle_filled(point(p), 2., CURVE)),
			curve => Some(Shape::line(curve.iter().map(point).collect(), Stroke::new(g.line_width, CURVE))),
		};
		clipped.extend(curve.map(|shape| ClippedShape::new(plot, shape)));
	}

	// the title strip: the readout right-aligned in the curve's colour, the title in what remains
	let mut readout_width = 0.;
	if !g.readout.is_empty() {
		let galley = fonts.layout_no_wrap(g.readout.clone(), font.clone(), CURVE);
		readout_width = galley.size().x;
		let at = grid.point(grid.x(g.width - readout_width - 8.).round(), grid.y(EDGE_PAD).round());
		clipped.push(ClippedShape::new(clip, Shape::galley(at, galley, CURVE)));
	}
	let title_end = grid.x(EDGE_PAD + 3. + (g.width - readout_width - 16.).max(10.));
	let title = grid.rect((bx0, by0), (title_end, by1)).intersect(clip);
	let galley = fonts.layout_no_wrap(g.title.clone(), font, TITLE);
	let at = grid.point(grid.x(EDGE_PAD + 3.).round(), grid.y(EDGE_PAD).round());
	clipped.push(ClippedShape::new(title, Shape::galley(at, galley, TITLE)));

	out.into_iter().map(|shape| ClippedShape::new(clip, shape)).chain(clipped).collect()
}

/// The preview's resize grip of the box laid out as `g` at the origin of `grid`: two rows of dots in its
/// bottom-right corner.
pub fn grip_shapes(g: &GraphLayout, grid: DeviceGrid) -> Vec<Shape> {
	let hl = grid.hairline();
	let [_, _, bx1, by1] = device_box(g, grid);
	let dot = |x: f32, y: f32| Shape::rect_filled(grid.rect((x, y), (x + hl, y + hl)), 0, GRIP_DOTS);
	(0..3)
		.flat_map(|k| {
			let o = grid.width(3.) * k as f32;
			[dot(bx1 - hl * 3. - o, by1 - hl * 3.), dot(bx1 - hl * 3., by1 - hl * 3. - o)]
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use epaint::text::{Fonts, TextOptions};
	use epaint::{Pos2, pos2};
	use wavedude_core::model::BoxSize;

	use super::*;
	use crate::draw::FontMetrics;

	struct Fixed;
	impl TextMetrics for Fixed {
		fn line_height(&mut self) -> f32 {
			14.0
		}
		fn width(&mut self, text: &str) -> f32 {
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
		assert!((libm::log10(hi / lo) - 4.0).abs() < 1e-9);
		let l = layout(&log, &GraphData { series: &series, ..GraphData::default() }, 100, 400.0, 200.0, &mut Fixed);
		assert!(l.has_plot && l.show_labels);
		assert_eq!(l.readout, "= 0.01");
		assert_eq!(l.curve.len(), 3);
		assert_eq!(l.curve[0].1, l.plot_y + l.plot_height, "zero on the bottom line");
	}

	#[test]
	fn a_series_far_below_the_full_scale_fits_to_it() {
		let tiny = [1e-20f32, 3e-20];
		let data = GraphData { series: &tiny, ..GraphData::default() }.with_floor(2.0);
		let lin = Probe { measurement: Measurement::Value, ..Probe::default() };
		let (_, _, lo, hi) = ranges(&lin, &data, 100);
		assert_eq!(lo, 0.0);
		assert!((hi - 2.0 * (1.0 + MARGIN)).abs() < 1e-9, "{hi}");
		let amp = Probe::default();
		let (_, _, lo, hi) = ranges(&amp, &data, 100);
		assert_eq!(lo, -hi);
		assert!((hi - 2.0 * (1.0 + MARGIN)).abs() < 1e-9, "{hi}");
		let log = Probe { y_scale: YScale::Logarithmic, decades: 4, ..lin };
		let (_, _, lo, hi) = ranges(&log, &data, 100);
		assert!((libm::log10(hi / lo) - 4.0).abs() < 1e-9);
		assert!(hi > 2.0 && lo < 2.0, "{lo} {hi}");
		let (_, _, _, above) = ranges(&log, &GraphData { series: &[5.0], ..GraphData::default() }.with_floor(2.0), 100);
		assert!(above > 5.0);
	}

	#[test]
	fn full_scale_follows_the_layer_brightness() {
		let mut display = Display::default();
		display.field.brightness = 4.0;
		display.intensity.brightness = 2.0;
		let field = Probe::default();
		let value = Probe { layer: Layer::Intensity, measurement: Measurement::Value, ..Probe::default() };
		assert_eq!(full_scale(&field, &display), 0.25);
		assert_eq!(full_scale(&value, &display), 0.25);
		assert_eq!(full_scale(&Probe { measurement: Measurement::IntegratedValue, ..value }, &display), 0.0);
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
		let l = layout(&Probe::default(), &GraphData::default(), 10, 30.0, 20.0, &mut Fixed);
		assert!(!l.visible && !l.has_plot);
	}

	#[test]
	fn a_box_draws_its_frame_grid_curve_and_text_on_device_pixels() {
		let mut fonts = Fonts::new(TextOptions::default(), crate::fonts::definitions());
		let mut view = fonts.with_pixels_per_point(2.);
		let series = [0.5f32, -0.5, 0.25];
		let data = GraphData { series: &series, ..GraphData::default() };
		let mut metrics = FontMetrics { fonts: &mut view, font: FontId::proportional(FONT_SIZE) };
		let l = layout(&Probe::default(), &data, 3, 300., 150., &mut metrics);
		assert!(l.has_plot && l.show_labels);
		let grid = DeviceGrid::new(2., 10.25, 0.);
		let shapes = box_shapes(&l, grid, Rect::EVERYTHING, true, None, &mut view);
		let device = |p: Pos2| (p.x * 2., p.y * 2.);
		let Shape::Rect(back) = &shapes[0].shape else { panic!("the background first") };
		assert_eq!((device(back.rect.min), device(back.rect.max), back.fill), ((20., 0.), (620., 300.), BACKGROUND));
		let Shape::Rect(edge) = &shapes[1].shape else { panic!("then its edge") };
		assert_eq!(
			(device(edge.rect.min), device(edge.rect.max), edge.stroke),
			((21., 1.), (619., 299.), Stroke::new(1., SELECTED)),
			"a selected box's edge, a two-pixel hairline inside the box"
		);
		let lines: Vec<f32> = shapes
			.iter()
			.filter_map(|s| match &s.shape {
				Shape::LineSegment { points, .. } => Some(device(points[0]).1),
				_ => None,
			})
			.collect();
		assert_eq!(lines.len(), l.y_ticks.len() + l.x_ticks.len());
		assert_eq!(lines[0].fract(), 0., "a two-pixel hairline's centre on the device grid");
		let curve = shapes.iter().find_map(|s| match &s.shape {
			Shape::Path(p) => Some((p.points.len(), p.stroke.width, s.clip_rect)),
			_ => None,
		});
		let plot = grid.rect(
			(grid.x(l.plot_x), grid.y(l.plot_y)),
			(grid.x(l.plot_x + l.plot_width), grid.y(l.plot_y + l.plot_height)),
		);
		assert_eq!(curve, Some((3, l.line_width, plot)), "clipped to the plot");
		let texts: Vec<(&str, Color32)> = shapes
			.iter()
			.filter_map(|s| match &s.shape {
				Shape::Text(t) => {
					let (x, y) = device(t.pos);
					assert_eq!((x.fract(), y.fract()), (0., 0.), "{} starts on a device pixel", t.galley.text());
					Some((t.galley.text(), t.fallback_color))
				}
				_ => None,
			})
			.collect();
		assert_eq!(texts.len(), l.y_ticks.len() + l.x_ticks.len() + 2);
		assert_eq!(texts[texts.len() - 2], ("= 0.25", CURVE));
		assert_eq!(texts[texts.len() - 1], (l.title.as_str(), TITLE));
		assert!(shapes.last().is_some_and(|s| s.clip_rect.max.x < 310.), "the title clipped before the readout");
		let grip = grip_shapes(&l, grid);
		let Some(Shape::Rect(dot)) = grip.first() else { panic!("the grip's dots") };
		assert_eq!((grip.len(), device(dot.rect.min), dot.fill), (6, (614., 294.), GRIP_DOTS));
	}

	#[test]
	fn a_box_without_a_plot_keeps_its_title_and_focus() {
		let mut fonts = Fonts::new(TextOptions::default(), crate::fonts::definitions());
		let mut view = fonts.with_pixels_per_point(1.);
		let l =
			layout(&Probe { name: "P".into(), ..Probe::default() }, &GraphData::default(), 10, 60., 30., &mut Fixed);
		assert!(!l.has_plot);
		let focus = Color32::from_rgb(1, 2, 3);
		let clip = Rect::from_min_max(Pos2::ZERO, pos2(50., 50.));
		let shapes = box_shapes(&l, DeviceGrid::new(1., 0., 0.), clip, true, Some(focus), &mut view);
		let Shape::Rect(edge) = &shapes[1].shape else { panic!("the edge") };
		assert_eq!(
			(edge.rect, edge.stroke),
			(Rect::from_min_max(pos2(0.5, 0.5), pos2(59.5, 29.5)), Stroke::new(1., focus))
		);
		assert!(shapes.iter().all(|s| !matches!(s.shape, Shape::LineSegment { .. } | Shape::Path(_))));
		assert!(shapes.iter().all(|s| clip.contains_rect(s.clip_rect)), "within the caller's clip");
		let Some(Shape::Text(title)) = shapes.last().map(|s| &s.shape) else { panic!("the title last") };
		assert_eq!((title.galley.text(), title.pos, title.fallback_color), ("P", pos2(6., 3.), TITLE));
		assert_eq!(shapes.len(), 3, "box, edge, the title in its clip");
	}
}
