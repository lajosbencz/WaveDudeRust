use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use wavedude_core::io::load_project;
use wavedude_core::model::Project;
use wavedude_core::model::ranges::SIM_STEPS;

const EXAMPLE: &str = "Example01_Source_Mirror_Probe.wdproj";

/// The binary on `args` with the app's folders in a new, empty folder, so no test reads the user's.
fn cli(args: &[&str]) -> Output {
	let dirs = tempfile::tempdir().unwrap();
	cli_in(dirs.path(), args)
}

fn cli_in(dirs: &Path, args: &[&str]) -> Output {
	Command::new(env!("CARGO_BIN_EXE_wavedude-cli"))
		.arg("--config-dir")
		.arg(dirs)
		.args(args)
		.output()
		.expect("the binary starts")
}

fn ok(args: &[&str]) -> String {
	let out = cli(args);
	assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
	String::from_utf8(out.stdout).unwrap()
}

fn example(dir: &Path) -> PathBuf {
	let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../wavedude-core/tests/fixtures/examples").join(EXAMPLE);
	let copy = dir.join(EXAMPLE);
	std::fs::copy(source, &copy).expect("example copies");
	copy
}

fn decodes(path: &Path) -> (u32, u32) {
	let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).expect("file exists")));
	let mut reader = decoder.read_info().expect("PNG header");
	let mut buffer = vec![0; reader.output_buffer_size().expect("a decodable size")];
	let info = reader.next_frame(&mut buffer).expect("PNG data");
	(info.width, info.height)
}

#[test]
fn info_runs() {
	let text = ok(&["info"]);
	assert!(text.contains("threads:") && text.contains("tier:"), "{text}");
}

#[test]
fn check_reports_the_grid() {
	let dir = tempfile::tempdir().unwrap();
	let text = ok(&["check", example(dir.path()).to_str().unwrap()]);
	for key in ["canvas:", "rim:", "edges:", "stride:", "sub-steps:", "frames:", "store, full:", "store, default:"] {
		assert!(text.contains(key), "{key} missing in {text}");
	}
}

#[test]
fn check_fails_on_a_missing_file() {
	assert!(!cli(&["check", "no-such-project.wdproj"]).status.success());
}

#[test]
fn paint_writes_scene_and_edges() {
	let dir = tempfile::tempdir().unwrap();
	let project = example(dir.path());
	let scene = dir.path().join("scene.png");
	let edges = dir.path().join("edges.png");
	ok(&["paint", project.to_str().unwrap(), scene.to_str().unwrap()]);
	ok(&["paint", project.to_str().unwrap(), edges.to_str().unwrap(), "--edges"]);
	let size = decodes(&scene);
	assert!(size.0 > 0 && size.1 > 0);
	assert_eq!(decodes(&edges), size);
}

#[test]
fn dispersion_prints_a_table() {
	let text = ok(&["dispersion", "--cells", "4,10"]);
	assert!(text.contains("| 4 |") && text.contains("| 10 |"), "{text}");
	assert!(text.contains("sub-steps per step at index 1"), "{text}");
}

#[test]
fn run_writes_pictures_of_both_layers() {
	let dir = tempfile::tempdir().unwrap();
	let project = example(dir.path());
	let out = dir.path().join("frames");
	for quality in ["default", "full"] {
		let target = out.join(quality);
		ok(&[
			"run",
			project.to_str().unwrap(),
			"--out",
			target.to_str().unwrap(),
			"--steps",
			"40",
			"--every",
			"5",
			"--quality",
			quality,
		]);
		let field = target.join("field_0000.png");
		let size = decodes(&field);
		assert_eq!(decodes(&target.join("intensity_0000.png")), size);
		assert!(target.join("field_0005.png").exists(), "{quality}");
	}
}

/// The colour of the first pixel of a PNG.
fn first_pixel(path: &Path) -> [u8; 3] {
	let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).expect("file exists")));
	let mut reader = decoder.read_info().expect("PNG header");
	let mut buffer = vec![0; reader.output_buffer_size().expect("a decodable size")];
	reader.next_frame(&mut buffer).expect("PNG data");
	[buffer[0], buffer[1], buffer[2]]
}

#[test]
fn run_draws_with_the_users_colour_schemes() {
	use wavedude_app_state::settings::{AppDirs, SCHEME_FOLDER};

	let dir = tempfile::tempdir().unwrap();
	let dirs = dir.path().join("app");
	let schemes = AppDirs::under(&dirs).config.join(SCHEME_FOLDER);
	std::fs::create_dir_all(&schemes).unwrap();
	let red = r##"<ColorScheme Name="Mine" Kind="Field"><Stop At="0" Color="#FF0000" /><Stop At="1" Color="#FF0000" /></ColorScheme>"##;
	std::fs::write(schemes.join("Mine.wdscheme"), red).unwrap();
	let mut project = Project::default();
	project.scene.width = 32;
	project.scene.height = 32;
	project.display.field.color_scheme = "Mine".into();
	let path = dir.path().join("p.wdproj");
	wavedude_core::io::save_project(&project, &path).unwrap();
	let out = dir.path().join("frames");
	let args = ["run", path.to_str().unwrap(), "--out", out.to_str().unwrap(), "--steps", "2", "--every", "1"];
	let run = cli_in(&dirs, &args);
	assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
	assert_eq!(first_pixel(&out.join("field_0000.png")), [255, 0, 0]);
}

#[test]
fn steps_outside_the_model_range_are_refused() {
	let dir = tempfile::tempdir().unwrap();
	let project = example(dir.path());
	let out = dir.path().join("out");
	let outside = format!("is outside {}..={}", SIM_STEPS.min, SIM_STEPS.max);
	let (below, above) = ((SIM_STEPS.min - 1.).to_string(), (SIM_STEPS.max + 1.).to_string());
	for (command, steps) in [("probes", &below), ("export", &below), ("run", &above)] {
		let output = cli(&[command, project.to_str().unwrap(), "--out", out.to_str().unwrap(), "--steps", steps]);
		assert!(!output.status.success(), "{command} --steps {steps}");
		assert!(String::from_utf8_lossy(&output.stderr).contains(&outside), "{command}");
	}
}

#[test]
fn probes_write_csv() {
	let dir = tempfile::tempdir().unwrap();
	let project = example(dir.path());
	let out = dir.path().join("probes");
	ok(&["probes", project.to_str().unwrap(), "--out", out.to_str().unwrap(), "--steps", "40"]);
	let files: Vec<_> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().path()).collect();
	assert!(!files.is_empty());
	let text = std::fs::read_to_string(&files[0]).unwrap();
	assert!(text.starts_with("step;") || text.starts_with("frame;step;"), "{text}");
}

fn export(dir: &Path, out: &str, extra: &[&str]) -> (PathBuf, String) {
	let project = example(dir);
	let target = dir.join(out);
	let mut args = vec!["export", project.to_str().unwrap(), "--out", target.to_str().unwrap(), "--steps", "40"];
	args.extend_from_slice(extra);
	let text = ok(&args);
	(target, text)
}

fn files(dir: &Path) -> Vec<PathBuf> {
	let mut files: Vec<_> = std::fs::read_dir(dir).expect("folder exists").map(|e| e.unwrap().path()).collect();
	files.sort();
	files
}

fn assert_filled(path: &Path) {
	assert!(std::fs::metadata(path).expect("file exists").len() > 0, "{} is empty", path.display());
}

/// The u32 little-endian at `at` of `data`.
fn u32_at(data: &[u8], at: usize) -> u32 {
	u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

/// Width, height and frame count of an AVI's main header.
fn avi_header(path: &Path) -> (u32, u32, usize) {
	let data = std::fs::read(path).unwrap();
	let avih = data.windows(4).position(|w| w == b"avih").expect("an AVI main header") + 8;
	(u32_at(&data, avih + 32), u32_at(&data, avih + 36), u32_at(&data, avih + 16) as usize)
}

/// Sample count of a `QuickTime` or MP4 movie's track: the `stsz` in its top-level `moov` (big-endian atoms).
fn movie_samples(path: &Path) -> usize {
	let data = std::fs::read(path).unwrap();
	let be = |at: usize| u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
	let mut at = 0;
	let moov = loop {
		let size = match be(at) {
			1 => u64::from_be_bytes(data[at + 8..at + 16].try_into().unwrap()) as usize,
			n => n,
		};
		assert!(size >= 8, "an atom before moov runs to the end at {at}");
		if &data[at + 4..at + 8] == b"moov" {
			break at..at + size;
		}
		at += size;
	};
	let stsz = moov.start + data[moov].windows(4).position(|w| w == b"stsz").expect("a sample size table");
	be(stsz + 12)
}

/// Timeline frames of `steps` exported from step `from` to step `to` (0: the end), per docs/spec/50-export.md, Range.
fn exported_frames(project: &Project, steps: u32, from: u32, to: u32) -> usize {
	let every = project.simulation.visualize_every;
	let last = steps.div_ceil(every) - 1;
	let first = (from / every).min(last);
	let end = if to == 0 { last } else { (to / every).clamp(first, last) };
	(end - first + 1) as usize
}

/// Each export of the example at 40 steps: the output, the extra arguments, and what it writes, checked against
/// the example's canvas and the frames from its in point.
#[test]
fn exports_write_their_outputs_by_format_and_layers() {
	type Check = fn(&Path, &str, (u32, u32), usize);
	let table: &[(&str, &[&str], Check)] = &[
		("seq.png", &["--layers", "both", "--markers"], |target, text, size, frames| {
			for layer in ["field", "intensity"] {
				let written = files(&target.with_file_name(format!("seq_{layer}")));
				assert_eq!(written.len(), frames, "{layer}: {text}");
				assert!(written.iter().all(|f| decodes(f) == size), "{layer}");
			}
			assert!(text.contains("ms/frame"), "{text}");
		}),
		("frame.png", &["--still", "3"], |target, _, size, _| assert_eq!(decodes(target), size)),
		("movie.avi", &["--graphs", "--edges", "--source-markers"], |target, text, (w, h), frames| {
			assert_eq!(avi_header(target), (w, h, frames));
			assert!(text.contains(&format!("wrote {}", target.display())), "{text}");
		}),
		("anim.mov", &["--format", "mov", "--layers", "both", "--markers"], |target, _, _, frames| {
			for layer in ["field", "intensity"] {
				assert_eq!(movie_samples(&target.with_file_name(format!("anim_{layer}.mov"))), frames, "{layer}");
			}
		}),
		#[cfg(feature = "openh264-source")]
		("movie.mp4", &[], |target, _, _, frames| assert_eq!(movie_samples(target), frames)),
	];
	let dir = tempfile::tempdir().unwrap();
	let project = load_project(&example(dir.path())).unwrap();
	let size = (project.scene.width, project.scene.height);
	let frames = exported_frames(&project, 40, project.display.export_from, project.display.export_to);
	for &(out, extra, check) in table {
		let (target, text) = export(dir.path(), out, extra);
		check(&target, &text, size, frames);
	}
	let exr = ["--format", "exrroot", "--layers", "intensity", "--from", "0", "--to", "10"];
	let (target, _) = export(dir.path(), "data.exr", &exr);
	let written = files(&target.with_extension(""));
	assert_eq!(written.len(), exported_frames(&project, 40, 0, 10), "the range of --from and --to");
	written.iter().for_each(|f| assert_filled(f));
}

/// Exports that fail leave no output behind: the output, its arguments, and what the error says.
#[test]
fn a_failed_export_leaves_no_partial_output() {
	let dir = tempfile::tempdir().unwrap();
	let project = example(dir.path());
	let table: &[(&str, &[&str], &str)] = &[
		("big.avi", &["--steps", "40", "--still", "999"], ""),
		("beyond.png", &["--steps", "4", "--still", "50"], "beyond"),
		("a.avi", &["--still", "1"], ""),
		("a.xyz", &[], ""),
	];
	for &(name, extra, message) in table {
		let target = dir.path().join(name);
		let mut args = vec!["export", project.to_str().unwrap(), "--out", target.to_str().unwrap()];
		args.extend_from_slice(extra);
		let out = cli(&args);
		let stderr = String::from_utf8_lossy(&out.stderr);
		assert!(!out.status.success(), "{name}: succeeded");
		assert!(stderr.contains(message), "{name}: {stderr}");
		assert!(!target.exists(), "{name}: left its output");
	}
}

/// The shipped gate projects of examples/guides, without Next's `ExampleNN_` files.
fn shipped_gates() -> Vec<PathBuf> {
	let guides = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/guides");
	files(&guides).into_iter().filter(|p| !p.file_name().unwrap().to_string_lossy().starts_with("Example")).collect()
}

fn names(paths: &[PathBuf]) -> Vec<String> {
	paths.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect()
}

#[test]
fn gates_writes_loadable_projects_named_as_the_shipped_ones() {
	let dir = tempfile::tempdir().unwrap();
	// the smallest design the mode solver lays out: the shipped one calibrates for a minute
	let text = ok(&["gates", "--out", dir.path().to_str().unwrap(), "--frequency", "255", "--width", "100"]);
	assert!(text.contains("coupling length"), "{text}");
	let projects = files(dir.path());
	assert_eq!(names(&projects), names(&shipped_gates()));
	for path in &projects {
		let report = ok(&["check", path.to_str().unwrap()]);
		assert!(report.contains("2 probes"), "{}: {report}", path.display());
	}
}

mod ignored_slow {
	use super::*;

	#[test]
	#[ignore = "slow: about 8 s; cargo test -p wavedude-cli --test cli -- --ignored ignored_slow::"]
	fn a_shipped_gate_passes_every_row_of_its_truth_table() {
		let xor = shipped_gates().into_iter().find(|p| p.ends_with("07-xor.wdproj")).expect("the shipped XOR");
		let text = ok(&["truth-table", xor.to_str().unwrap()]);
		let rows: Vec<&str> = text.lines().filter(|l| l.starts_with("| 0 |") || l.starts_with("| 1 |")).collect();
		assert_eq!(rows.len(), 4, "{text}");
		assert!(rows.iter().all(|r| r.ends_with("| ok |")), "{text}");
	}

	#[test]
	#[ignore = "slow: calibrates for a minute; cargo test -p wavedude-cli --test cli -- --ignored ignored_slow::"]
	fn the_shipped_gates_are_this_commands_output() {
		let dir = tempfile::tempdir().unwrap();
		ok(&["gates", "--out", dir.path().to_str().unwrap()]);
		for shipped in shipped_gates() {
			let mut written = load_project(&dir.path().join(shipped.file_name().unwrap())).unwrap();
			let want = load_project(&shipped).unwrap();
			// the brightness fits of the CPU and GPU engines differ in the last digit
			for (got, want) in [
				(&mut written.display.field.brightness, want.display.field.brightness),
				(&mut written.display.intensity.brightness, want.display.intensity.brightness),
			] {
				assert!((*got - want).abs() <= want * 0.01, "{}: brightness {got} for {want}", shipped.display());
				*got = want;
			}
			assert_eq!(written, want, "{}", shipped.display());
		}
	}
}
