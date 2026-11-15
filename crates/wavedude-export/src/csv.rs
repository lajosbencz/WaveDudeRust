//! Probe data as CSV: `;` separated, decimal point, a header row, `\n` line ends, UTF-8 without BOM; values as
//! .NET's invariant `float.ToString()` writes them.

use std::io::{self, Write};

use wavedude_core::engine::probes::{ProbeTrace, sample_distances};
use wavedude_core::model::{Measurement, Probe};

pub const SEPARATOR: char = ';';

/// The value column's header of a point probe.
pub fn column_name(measurement: Measurement) -> &'static str {
	match measurement {
		Measurement::Amplitude => "amplitude",
		Measurement::Value => "value",
		Measurement::IntegratedValue => "integrated value",
	}
}

/// A distance as .NET's `0.###`: at most three decimals, trailing zeros and the point dropped.
fn three_decimals(v: f64) -> String {
	let s = format!("{v:.3}");
	let s = s.trim_end_matches('0').trim_end_matches('.');
	if s == "-0" { "0".into() } else { s.into() }
}

/// `v` as .NET's invariant `float.ToString()`: the shortest round-trip digits, fixed for a decimal exponent in
/// -4..=8, else `d.dddE+dd`; `NaN`, `Infinity`, `-Infinity`.
fn dotnet_float(v: f32) -> String {
	if v.is_nan() {
		return "NaN".into();
	}
	if v.is_infinite() {
		return if v > 0. { "Infinity" } else { "-Infinity" }.into();
	}
	let sci = format!("{v:e}");
	let (mantissa, exponent) = sci.split_once('e').expect("`{:e}` writes an exponent");
	let exponent: i32 = exponent.parse().expect("`{:e}` writes an integer exponent");
	if v == 0. || (-4..=8).contains(&exponent) {
		format!("{v}")
	} else {
		let sign = if exponent < 0 { '-' } else { '+' };
		format!("{mantissa}E{sign}{:02}", exponent.abs())
	}
}

/// Writes a probe's recording and returns the data rows written.
///
/// Point probe or counting detector: `step;<column>` then `i;<value after step i + 1>` per sample. Line probe or
/// imaging detector: `frame;step;d0;d1;...` (`d_k = k * length / (n - 1)`, `0.###`) then one row per profile p:
/// `frame;step;v0;v1;...` with `frame = p * frame_stride`, `step = frame * visualize_every`. A detector's column is
/// `detections`.
pub fn write_probe_csv(
	out: &mut impl Write,
	probe: &Probe,
	trace: &ProbeTrace,
	visualize_every: u32,
	frame_stride: u32,
) -> io::Result<usize> {
	if !probe.shows_profile() {
		let column = if probe.is_detector() { "detections" } else { column_name(probe.measurement) };
		writeln!(out, "step{SEPARATOR}{column}")?;
		for (i, v) in trace.series.iter().enumerate() {
			writeln!(out, "{i}{SEPARATOR}{}", dotnet_float(*v))?;
		}
		return Ok(trace.series.len());
	}
	let samples = trace.profiles.first().map_or(0, <[f32]>::len);
	write!(out, "frame{SEPARATOR}step")?;
	for d in sample_distances(samples, trace.length) {
		write!(out, "{SEPARATOR}{}", three_decimals(d))?;
	}
	writeln!(out)?;
	for (p, profile) in trace.profiles.iter().enumerate() {
		let frame = p as u64 * frame_stride as u64;
		write!(out, "{frame}{SEPARATOR}{}", frame * visualize_every as u64)?;
		for v in profile {
			write!(out, "{SEPARATOR}{}", dotnet_float(*v))?;
		}
		writeln!(out)?;
	}
	Ok(trace.profiles.len())
}

#[cfg(test)]
mod tests {
	use super::*;
	use wavedude_core::model::{ProbeDetector, ProbeKind};

	fn csv(probe: &Probe, trace: &ProbeTrace, ve: u32) -> String {
		let mut out = Vec::new();
		write_probe_csv(&mut out, probe, trace, ve, 1).unwrap();
		String::from_utf8(out).unwrap()
	}

	#[test]
	fn a_line_probe_of_three_pixels_over_four() {
		let probe = Probe { kind: ProbeKind::Line, bx: 4, ..Probe::default() };
		let trace = ProbeTrace {
			profiles: vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.5, 6.0]].into(),
			length: 4.0,
			..ProbeTrace::default()
		};
		assert_eq!(csv(&probe, &trace, 2), "frame;step;0;2;4\n0;0;1;2;3\n1;2;4;5.5;6\n");
	}

	#[test]
	fn point_probe_headers_and_rows() {
		let trace = ProbeTrace { series: vec![0.5, 1e-5, -2.0], ..ProbeTrace::default() };
		let integrated = Probe { measurement: Measurement::IntegratedValue, ..Probe::default() };
		assert_eq!(csv(&integrated, &trace, 2), "step;integrated value\n0;0.5\n1;1E-05\n2;-2\n");
		assert!(csv(&Probe::default(), &trace, 1).starts_with("step;amplitude\n"));
		let value = Probe { measurement: Measurement::Value, ..Probe::default() };
		assert!(csv(&value, &trace, 1).starts_with("step;value\n"));
	}

	#[test]
	fn detectors_write_detections() {
		let trace = ProbeTrace { series: vec![1.0, 0.0, 2.0], ..ProbeTrace::default() };
		let counting = Probe { detector: ProbeDetector::Counting, kind: ProbeKind::Line, bx: 4, ..Probe::default() };
		assert_eq!(csv(&counting, &trace, 2), "step;detections\n0;1\n1;0\n2;2\n");
		let imaging = Probe { detector: ProbeDetector::Imaging, ..counting };
		let trace = ProbeTrace { profiles: vec![vec![3.0, 3.0, 1.0]].into(), length: 4.0, ..ProbeTrace::default() };
		assert_eq!(csv(&imaging, &trace, 2), "frame;step;0;2;4\n0;0;3;3;1\n");
	}

	/// Bits and what .NET 10's invariant `float.ToString()` writes for them.
	const DOTNET: [(u32, &str); 30] = [
		(0x3f00_0000, "0.5"),
		(0x3727_c5ac, "1E-05"),
		(0x38d1_b717, "0.0001"),
		(0x3421_0fb0, "1.5E-07"),
		(0x4996_b438, "1234567"),
		(0x4b18_9680, "10000000"),
		(0x4b3c_614e, "12345678"),
		(0xc000_0000, "-2"),
		(0x0000_0000, "0"),
		(0x8000_0000, "-0"),
		(0xffc0_0000, "NaN"),
		(0x7f80_0000, "Infinity"),
		(0xff80_0000, "-Infinity"),
		(0x7f7f_ffff, "3.4028235E+38"),
		(0x0000_0001, "1E-45"),
		(0x4b18_967f, "9999999"),
		(0x3dcc_cccd, "0.1"),
		(0x3eaa_aaab, "0.33333334"),
		(0x42f6_e979, "123.456"),
		(0x391d_4951, "0.00014999999"),
		(0x50ba_43b7, "2.5E+10"),
		(0x38d1_b1b9, "9.999E-05"),
		(0x4cbe_bc20, "100000000"),
		(0x4e6e_6b28, "1E+09"),
		(0x4ceb_79a3, "123456790"),
		(0x4f4a_a7e2, "3.4E+09"),
		(0x4cee_6b28, "125000000"),
		(0x3851_b717, "5E-05"),
		(0x3901_64ef, "0.0001234"),
		(0xb727_c5ac, "-1E-05"),
	];

	#[test]
	fn values_are_written_as_dotnet_writes_them() {
		for (bits, want) in DOTNET {
			assert_eq!(dotnet_float(f32::from_bits(bits)), want, "{bits:#010x}");
		}
	}

	#[test]
	fn distances_take_three_decimals() {
		assert_eq!(three_decimals(19.0 / 3.0), "6.333");
		assert_eq!(three_decimals(2.0), "2");
		assert_eq!(three_decimals(0.0005), "0.001");
	}
}
