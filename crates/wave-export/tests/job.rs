mod common;

use std::sync::atomic::AtomicBool;

use common::*;
use half::f16;
use wave_core::engine::types::{FrameShape, SampleFormat, SampleKind};
use wave_core::io::schemes::SchemeStore;
use wave_core::model::{ColorScheme, DisplayLayer, Layer, Probe, Project};
use wave_core::render::render_half;
use wave_export::overlays::Overlays;
use wave_export::range::frame_range;
use wave_export::sinks::{self, SinkSpec};
use wave_export::{
	EncoderPool, ExportError, ExportFormat, ExportJob, ExportLayerLook, ExportLayers, ExportPlan, ExportState,
	ExportTarget, FrameSource, MemoryFrames,
};

const W: u32 = 24;
const H: u32 = 16;

fn shape() -> FrameShape {
	FrameShape {
		canvas_width: W,
		canvas_height: H,
		divisor: 1,
		format: SampleFormat::Half,
		field_brightness: 1.0,
		intensity_brightness: 1.0,
		intensity_root: true,
	}
}

/// `count` frames whose field is a ramp moving with the frame and whose intensity is its square root.
fn source(count: usize) -> MemoryFrames {
	let frames = (0..count)
		.map(|k| {
			let field: Vec<f16> =
				(0..W * H).map(|i| f16::from_f32(((i as f32 * 0.37 + k as f32) * 0.5).sin() * 0.04)).collect();
			let intensity = field.iter().map(|v| f16::from_f32(v.to_f32().abs().sqrt())).collect();
			(field, intensity)
		})
		.collect();
	MemoryFrames { shape: shape(), frames }
}

fn look(layer: Layer) -> ExportLayerLook {
	let display = if layer == Layer::Field { DisplayLayer::field_default() } else { DisplayLayer::intensity_default() };
	let kind = if layer == Layer::Field { SampleKind::Field } else { SampleKind::IntensityRoot };
	ExportLayerLook::new(&ColorScheme::grey(), &display, kind, None, (W, H)).unwrap()
}

fn rendered(src: &MemoryFrames, frame: usize, layer: Layer) -> Vec<u8> {
	let mut bytes = vec![0u8; src.shape.plane_bytes()];
	src.copy_layer(frame, layer, &mut bytes).unwrap();
	let halves: Vec<f16> = bytemuck::pod_collect_to_vec(&bytes);
	render_half(&halves, &look(layer).table)
}

fn spec(first: usize, count: usize) -> SinkSpec {
	SinkSpec { width: W, height: H, fps: 30, quality: 90, first_frame: first, frame_count: count }
}

fn target(format: ExportFormat, path: &std::path::Path, layer: Layer, first: usize, count: usize) -> ExportTarget {
	ExportTarget {
		layer,
		look: look(layer),
		sink: sinks::open(format, path, spec(first, count), false, &EncoderPool::default()).unwrap(),
	}
}

fn png_names(dir: &std::path::Path) -> Vec<String> {
	let mut names: Vec<String> =
		std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
	names.sort();
	names
}

#[test]
fn a_range_of_a_ten_frame_run_to_png_and_avi() {
	let dir = scratch("job-range");
	let src = source(10);
	let range = frame_range(4, 14, 2, src.frame_count()).unwrap();
	assert_eq!(range, 2..=7);
	let job = ExportJob {
		source: &src,
		frames: range,
		targets: vec![
			target(ExportFormat::Png, &dir.join("field.png"), Layer::Field, 2, 6),
			target(ExportFormat::Avi, &dir.join("run_intensity.avi"), Layer::Intensity, 2, 6),
		],
		overlays: None,
		keep_partial: true,
	};
	let mut seen = Vec::new();
	let report = job.run(|done, total| seen.push((done, total)), &AtomicBool::new(false));
	assert!(matches!(report.state, ExportState::Completed), "{:?}", report.state);
	assert_eq!(report.frames_done, 6);
	assert_eq!(seen.last(), Some(&(6, 6)));
	let names = png_names(&dir.join("field"));
	assert_eq!(names, (2..=7).map(|f| format!("field_{f:04}.png")).collect::<Vec<_>>());
	for f in 2..=7 {
		let decoded = image::open(dir.join("field").join(format!("field_{f:04}.png"))).unwrap().into_rgb8().into_raw();
		assert_eq!(decoded, rgb(&rendered(&src, f, Layer::Field)), "file {f} holds frame {f}");
	}
	let avi = std::fs::read(dir.join("run_intensity.avi")).unwrap();
	let dmlh = avi.windows(4).position(|w| w == b"dmlh").unwrap();
	assert_eq!(u32::from_le_bytes(avi[dmlh + 8..dmlh + 12].try_into().unwrap()), 6, "the AVI holds 6 frames");
	if has_ffmpeg() {
		let decoded = ffmpeg_frames(&dir.join("run_intensity.avi"), W, H);
		assert_eq!(decoded.len(), 6);
		for (k, d) in decoded.iter().enumerate() {
			assert_eq!(d, &rgb(&rendered(&src, 2 + k, Layer::Intensity)));
		}
	}
	std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cancel_stops_before_the_next_frame() {
	let dir = scratch("job-cancel");
	let src = source(10);
	let cancel = AtomicBool::new(false);
	let job = ExportJob {
		source: &src,
		frames: 0..=9,
		targets: vec![target(ExportFormat::Png, &dir.join("c.png"), Layer::Field, 0, 10)],
		overlays: None,
		keep_partial: true,
	};
	let report = job.run(
		|done, _| {
			if done == 3 {
				cancel.store(true, std::sync::atomic::Ordering::Relaxed);
			}
		},
		&cancel,
	);
	assert!(matches!(report.state, ExportState::Cancelled));
	assert_eq!(report.frames_done, 3);
	assert_eq!(png_names(&dir.join("c")).len(), 3);
	std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_frame_beyond_the_store_fails_after_the_existing_ones() {
	let dir = scratch("job-missing");
	let src = source(5);
	let job = ExportJob {
		source: &src,
		frames: 2..=7,
		targets: vec![
			target(ExportFormat::Png, &dir.join("m.png"), Layer::Field, 2, 6),
			target(ExportFormat::Mov, &dir.join("m.mov"), Layer::Field, 2, 6),
		],
		overlays: None,
		keep_partial: true,
	};
	let report = job.run(|_, _| {}, &AtomicBool::new(false));
	match report.state {
		ExportState::Failed(ExportError::FrameMissing { frame: 5, count: 5 }) => {}
		other => panic!("{other:?}"),
	}
	assert_eq!(report.frames_done, 3);
	assert_eq!(png_names(&dir.join("m")), ["m_0002.png", "m_0003.png", "m_0004.png"]);
	let last = image::open(dir.join("m").join("m_0004.png")).unwrap().into_rgb8().into_raw();
	assert_eq!(last, rgb(&rendered(&src, 4, Layer::Field)));
	if has_ffmpeg() {
		assert_eq!(ffmpeg_frames(&dir.join("m.mov"), W, H).len(), 3, "the movie is closed with the frames written");
	}
	std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn exr_takes_the_stored_halves_and_a_still_is_one_frame() {
	let dir = scratch("job-exr");
	let src = source(4);
	let still = dir.join("ball_intensity_0003.exr");
	let job = ExportJob {
		source: &src,
		frames: 3..=3,
		targets: vec![ExportTarget {
			layer: Layer::Intensity,
			look: look(Layer::Intensity),
			sink: sinks::open(ExportFormat::ExrRoot, &still, spec(3, 1), true, &EncoderPool::default()).unwrap(),
		}],
		overlays: None,
		keep_partial: true,
	};
	assert!(matches!(job.run(|_, _| {}, &AtomicBool::new(false)).state, ExportState::Completed));
	use exr::prelude::{FlatSamples, ReadChannels, ReadLayers, read};
	let image = read().no_deep_data().largest_resolution_level().all_channels().first_valid_layer().all_attributes();
	let image = image.from_file(&still).unwrap();
	let FlatSamples::F16(values) = &image.layer_data.channel_data.list[0].sample_data else { panic!("HALF") };
	let expected = &src.frames[3].1;
	assert_eq!(
		values.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
		expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
	);
	std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_indexed_store_is_refused() {
	let mut src = source(2);
	src.shape.format = SampleFormat::Index;
	let dir = scratch("job-indexed");
	let job = ExportJob {
		source: &src,
		frames: 0..=1,
		targets: vec![target(ExportFormat::Png, &dir.join("i.png"), Layer::Field, 0, 2)],
		overlays: None,
		keep_partial: true,
	};
	assert!(matches!(
		job.run(|_, _| {}, &AtomicBool::new(false)).state,
		ExportState::Failed(ExportError::IndexedStore)
	));
	std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn overlays_are_burned_into_their_layer_only() {
	let dir = scratch("job-overlays");
	let src = source(3);
	let mut project = Project::default();
	project.scene.width = W;
	project.scene.height = H;
	project.probes.push(Probe { name: "p".into(), ax: 12, ay: 8, ..Probe::default() });
	project.display.probe_graphs_in_export = false;
	let overlays = Overlays::new(&project, W, H, &[], 6);
	let job = ExportJob {
		source: &src,
		frames: 1..=1,
		targets: vec![
			target(ExportFormat::Png, &dir.join("f.png"), Layer::Field, 1, 1),
			target(ExportFormat::Png, &dir.join("i.png"), Layer::Intensity, 1, 1),
		],
		overlays: Some(overlays),
		keep_partial: true,
	};
	assert!(matches!(job.run(|_, _| {}, &AtomicBool::new(false)).state, ExportState::Completed));
	let field = image::open(dir.join("f").join("f_0001.png")).unwrap().into_rgb8();
	assert_eq!(field.get_pixel(12, 8).0, [255, 255, 255], "the probe's cross on its layer");
	assert_eq!(field.get_pixel(12 + 2, 8 + 3).0, [0, 0, 0], "black around the white arm");
	let intensity = image::open(dir.join("i").join("i_0001.png")).unwrap().into_rgb8().into_raw();
	assert_eq!(intensity, rgb(&rendered(&src, 1, Layer::Intensity)), "nothing on the other layer");
	std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_cancelled_job_removes_its_outputs_unless_kept() {
	for keep_partial in [false, true] {
		let dir = scratch(if keep_partial { "job-keep" } else { "job-discard" });
		let src = source(10);
		let cancel = AtomicBool::new(false);
		let job = ExportJob {
			source: &src,
			frames: 0..=9,
			targets: vec![
				target(ExportFormat::Png, &dir.join("c.png"), Layer::Field, 0, 10),
				target(ExportFormat::Avi, &dir.join("c.avi"), Layer::Intensity, 0, 10),
			],
			overlays: None,
			keep_partial,
		};
		let report = job.run(
			|done, _| {
				if done == 3 {
					cancel.store(true, std::sync::atomic::Ordering::Relaxed);
				}
			},
			&cancel,
		);
		assert!(matches!(report.state, ExportState::Cancelled));
		if keep_partial {
			assert_eq!(report.written.len(), 4, "three images and the movie");
			assert!(report.written.iter().all(|p| p.is_file()));
		} else {
			assert!(report.written.is_empty());
			assert!(!dir.join("c").exists(), "the sequence folder it made is gone");
			assert!(!dir.join("c.avi").exists());
		}
		std::fs::remove_dir_all(&dir).unwrap();
	}
}

/// Frames that appear one by one from another thread, as a running run's.
struct Growing {
	frames: MemoryFrames,
	held: std::sync::Mutex<usize>,
	grown: std::sync::Condvar,
}

impl FrameSource for Growing {
	fn frame_count(&self) -> usize {
		*self.held.lock().unwrap()
	}

	fn shape(&self) -> FrameShape {
		self.frames.shape
	}

	fn copy_layer(&self, frame: usize, layer: Layer, out: &mut [u8]) -> anyhow::Result<()> {
		anyhow::ensure!(frame < self.frame_count(), "frame {frame} is not held yet");
		self.frames.copy_layer(frame, layer, out)
	}

	fn wait_for(&self, frame: usize) -> anyhow::Result<bool> {
		let held = self.grown.wait_while(self.held.lock().unwrap(), |held| frame >= *held).unwrap();
		Ok(frame < *held)
	}
}

#[test]
fn a_job_waits_for_frames_still_being_made() {
	let dir = scratch("job-growing");
	let src = Growing { frames: source(6), held: std::sync::Mutex::new(1), grown: std::sync::Condvar::new() };
	std::thread::scope(|scope| {
		scope.spawn(|| {
			for n in 2..=6 {
				std::thread::sleep(std::time::Duration::from_millis(5));
				*src.held.lock().unwrap() = n;
				src.grown.notify_all();
			}
		});
		let job = ExportJob {
			source: &src,
			frames: 0..=5,
			targets: vec![target(ExportFormat::Png, &dir.join("g.png"), Layer::Field, 0, 6)],
			overlays: None,
			keep_partial: false,
		};
		let report = job.run(|_, _| {}, &AtomicBool::new(false));
		assert!(matches!(report.state, ExportState::Completed), "{:?}", report.state);
		assert_eq!(report.written.len(), 6);
	});
	let last = image::open(dir.join("g").join("g_0005.png")).unwrap().into_rgb8().into_raw();
	assert_eq!(last, rgb(&rendered(&src.frames, 5, Layer::Field)));
	std::fs::remove_dir_all(&dir).unwrap();
}

fn plan_project() -> Project {
	let mut project = Project::default();
	project.scene.width = W;
	project.scene.height = H;
	project
}

#[test]
fn a_plan_builds_one_target_per_layer_with_suffixed_paths() {
	let dir = scratch("plan-both");
	let src = source(4);
	let project = plan_project();
	let plan = ExportPlan {
		project: &project,
		format: ExportFormat::Avi,
		layers: ExportLayers::Both,
		path: dir.join("sub").join("run.avi"),
		still: None,
		frame_count: src.frame_count(),
		traces: &[],
		total_steps: 8,
		keep_partial: false,
	};
	let job = plan.build_job(&src, &SchemeStore::default()).unwrap();
	assert_eq!(job.frames, 0..=3);
	let report = job.run(|_, _| {}, &AtomicBool::new(false));
	assert!(matches!(report.state, ExportState::Completed), "{:?}", report.state);
	assert_eq!(report.written, [dir.join("sub").join("run_field.avi"), dir.join("sub").join("run_intensity.avi")]);
	let still = ExportPlan { still: Some(4), format: ExportFormat::Png, ..plan.clone() };
	assert!(matches!(still.frames(), Err(ExportError::StillBeyond { frame: 4, count: 4 })));
	std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_plan_that_cannot_open_an_output_removes_those_it_opened() {
	let dir = scratch("plan-fail");
	std::fs::create_dir_all(dir.join("run_intensity.avi")).unwrap();
	let src = source(2);
	let project = plan_project();
	let plan = ExportPlan {
		project: &project,
		format: ExportFormat::Avi,
		layers: ExportLayers::Both,
		path: dir.join("run.avi"),
		still: None,
		frame_count: src.frame_count(),
		traces: &[],
		total_steps: 4,
		keep_partial: false,
	};
	assert!(plan.build_job(&src, &SchemeStore::default()).is_err());
	assert!(!dir.join("run_field.avi").exists(), "the field's movie is removed");
	assert!(dir.join("run_intensity.avi").is_dir(), "what was there stays");
	std::fs::remove_dir_all(&dir).unwrap();
}
