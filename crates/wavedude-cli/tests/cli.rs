use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const EXAMPLE: &str = "Example01_Source_Mirror_Probe.wdproj";

fn cli(args: &[&str]) -> Output {
	Command::new(env!("CARGO_BIN_EXE_wavedude-cli")).args(args).output().expect("the binary starts")
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

#[test]
fn export_writes_a_png_sequence() {
	let dir = tempfile::tempdir().unwrap();
	let (target, text) = export(dir.path(), "seq.png", &["--layers", "both", "--markers"]);
	let field = files(&target.with_file_name("seq_field"));
	let intensity = files(&target.with_file_name("seq_intensity"));
	assert!(!field.is_empty() && field.len() == intensity.len(), "{text}");
	assert_eq!(decodes(&field[0]), decodes(&intensity[0]));
	assert!(field.iter().chain(&intensity).all(|f| f.metadata().unwrap().len() > 0));
	assert!(text.contains("ms/frame"), "{text}");
}

#[test]
fn export_writes_exr_root_frames() {
	let dir = tempfile::tempdir().unwrap();
	let (target, _) =
		export(dir.path(), "data.exr", &["--format", "exrroot", "--layers", "intensity", "--from", "0", "--to", "10"]);
	let written = files(&target.with_extension(""));
	assert!(!written.is_empty());
	written.iter().for_each(|f| assert_filled(f));
}

/// Codec, size and frame count of a movie by ffprobe, None without ffprobe.
fn probe_movie(path: &Path) -> Option<String> {
	let out = Command::new("ffprobe")
		.args(["-v", "error", "-select_streams", "v:0", "-count_frames", "-show_entries"])
		.args(["stream=codec_name,width,height,nb_read_frames", "-of", "csv=p=0"])
		.arg(path)
		.output()
		.ok()?;
	assert!(out.status.success(), "ffprobe {}: {}", path.display(), String::from_utf8_lossy(&out.stderr));
	Some(String::from_utf8(out.stdout).unwrap().trim().trim_end_matches(',').to_owned())
}

/// Each frame of a movie decodes; checked when ffprobe is installed.
fn assert_movie(path: &Path, codec: &str, frames: usize) {
	assert_filled(path);
	if let Some(found) = probe_movie(path) {
		assert_eq!(found, format!("{codec},960,540,{frames}"), "{}", path.display());
	}
}

/// 40 steps at Visualize every 2 hold frames 0..=19; the example's in point, step 10, starts at frame 5.
const FRAMES: usize = 15;

#[test]
fn export_writes_an_avi() {
	let dir = tempfile::tempdir().unwrap();
	let (target, text) = export(dir.path(), "movie.avi", &["--graphs", "--edges"]);
	assert_movie(&target, "rawvideo", FRAMES);
	assert!(text.contains(&format!("wrote {}", target.display())), "{text}");
}

#[test]
fn export_writes_movies_of_both_layers_overlapping_the_run() {
	let dir = tempfile::tempdir().unwrap();
	for (out, format, codec) in
		[("anim.mov", "mov", "qtrle"), ("png.mov", "movpng", "png"), ("m.mpg", "mpeg1", "mpeg1video")]
	{
		let (target, _) = export(dir.path(), out, &["--format", format, "--layers", "both", "--markers"]);
		for layer in ["field", "intensity"] {
			let stem = target.file_stem().unwrap().to_str().unwrap();
			let ext = target.extension().unwrap().to_str().unwrap();
			assert_movie(&target.with_file_name(format!("{stem}_{layer}.{ext}")), codec, FRAMES);
		}
	}
}

#[cfg(feature = "openh264-source")]
#[test]
fn export_writes_an_mp4() {
	let dir = tempfile::tempdir().unwrap();
	let (target, _) = export(dir.path(), "movie.mp4", &["--source-markers"]);
	assert_movie(&target, "h264", FRAMES);
}

#[test]
fn a_failed_export_leaves_no_partial_output() {
	let dir = tempfile::tempdir().unwrap();
	let project = example(dir.path());
	let target = dir.path().join("big.avi");
	let out = cli(&[
		"export",
		project.to_str().unwrap(),
		"--out",
		target.to_str().unwrap(),
		"--steps",
		"40",
		"--still",
		"999",
	]);
	assert!(!out.status.success());
	assert!(!target.exists());
	let png = dir.path().join("beyond.png");
	let out =
		cli(&["export", project.to_str().unwrap(), "--out", png.to_str().unwrap(), "--steps", "4", "--still", "50"]);
	assert!(!out.status.success());
	assert!(String::from_utf8_lossy(&out.stderr).contains("beyond"), "{}", String::from_utf8_lossy(&out.stderr));
	assert!(!png.exists());
}

#[test]
fn export_writes_a_still() {
	let dir = tempfile::tempdir().unwrap();
	let (target, _) = export(dir.path(), "frame.png", &["--still", "3"]);
	assert_eq!(decodes(&target), (960, 540));
}

#[test]
fn export_refuses_a_video_still_and_an_unknown_extension() {
	let dir = tempfile::tempdir().unwrap();
	let project = example(dir.path());
	let p = project.to_str().unwrap();
	let avi = dir.path().join("a.avi");
	assert!(!cli(&["export", p, "--out", avi.to_str().unwrap(), "--still", "1"]).status.success());
	let odd = dir.path().join("a.xyz");
	assert!(!cli(&["export", p, "--out", odd.to_str().unwrap()]).status.success());
}

#[test]
fn gates_writes_ten_loadable_projects() {
	let dir = tempfile::tempdir().unwrap();
	let text = ok(&["gates", "--out", dir.path().to_str().unwrap()]);
	assert!(text.contains("coupling length"), "{text}");
	let projects = files(dir.path());
	assert_eq!(projects.len(), 10, "{projects:?}");
	for path in &projects {
		assert_eq!(path.extension().unwrap(), "wdr");
		let report = ok(&["check", path.to_str().unwrap()]);
		assert!(report.contains("2 probes"), "{}: {report}", path.display());
	}
}

#[test]
fn truth_table_runs_on_a_tiny_project() {
	let dir = tempfile::tempdir().unwrap();
	let out = dir.path().to_str().unwrap();
	ok(&["gates", "--out", out, "--frequency", "100", "--width", "400", "--format", "wdproj"]);
	let project = dir.path().join("07-xor.wdproj");
	let text = ok(&["truth-table", project.to_str().unwrap(), "--steps", "900", "--settle", "32"]);
	assert!(text.contains("900 steps a run (given)"), "{text}");
	assert!(text.contains("| A | B | in A | in B | Q | R | Q expected | R expected | verdict |"), "{text}");
	for row in ["| 0 | 0 |", "| 1 | 0 |", "| 0 | 1 |", "| 1 | 1 |"] {
		assert!(text.contains(row), "{row} missing in {text}");
	}
}
