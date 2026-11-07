//! The probe graph box's layout: axis ranges, ticks, the curve on the pixel grid, the title and the readout.
//!
//! Pure arithmetic over a probe's settings and its recording, in pixels relative to the box's top-left corner, so a
//! preview and an export draw the same graph. Numbers are formatted with a decimal point.

use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::model::{Measurement, Probe, ProbeKind, ScaleMode, YScale};

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

/// Text metrics of the renderer, in the box's pixels.
pub trait TextMetrics {
	fn line_height(&self) -> f32;
	fn width(&self, text: &str) -> f32;
}

/// One tick: a pixel position along its axis (relative to the box), its label, whether it is the zero line.
#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
	pub position: f32,
	pub label: String,
	pub zero: bool,
}

/// The geometry of one graph box.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphLayout {
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
	/// Horizontal grid lines, labels right-aligned in the left margin.
	pub y_ticks: Vec<Tick>,
	/// Vertical grid lines, labels centred below the plot.
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
pub fn ranges(probe: &Probe, data: &GraphData, total_steps: u64) -> (f64, f64, f64, f64) {
	let mut y = fixed_range(probe);
	let mut fitted_hi = None;
	let (x_min, x_max) = if probe.kind == ProbeKind::Line {
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

/// A log tick: a power of ten outside 0.01..100 as "1e<n>", else [`format_g3`].
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

fn readout(line: bool, data: &GraphData) -> String {
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
	data: &GraphData,
	total_steps: u64,
	width: f32,
	height: f32,
	text: &dyn TextMetrics,
) -> GraphLayout {
	let visible = width >= MIN_VISIBLE.0 && height >= MIN_VISIBLE.1;
	let line_width = (height / 110.0).max(1.2);
	let line_height = text.line_height();
	let title_height = line_height + 2.0 * EDGE_PAD;
	let line = probe.kind == ProbeKind::Line;
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
		})
		.collect();
	for target in [5, 3, 2] {
		out.x_ticks = nice_ticks(x_min, x_max, target)
			.into_iter()
			.map(|t| Tick {
				position: (plot_x as f64 + (t - x_min) / (x_max - x_min) * plot_width as f64) as f32,
				label: if show_labels { format_position(t, x_max - x_min) } else { String::new() },
				zero: t == 0.0,
			})
			.collect();
		let labels: f32 = out.x_ticks.iter().map(|t| text.width(&t.label) + X_LABEL_SPACING).sum();
		if labels - X_LABEL_SPACING <= plot_width {
			break;
		}
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

#[cfg(test)]
mod tests {
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
	fn tiny_boxes_hide() {
		let l = layout(&Probe::default(), &GraphData::default(), 10, 30.0, 20.0, &Fixed);
		assert!(!l.visible && !l.has_plot);
	}
}
