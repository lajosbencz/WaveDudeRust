#![cfg(feature = "run")]
#![allow(clippy::print_stdout, reason = "the measurements are printed for the reader")]
//! Runs, the frame store, the live run and the memory warning (spec/40, spec/30 The live run, spec/70
//! "Store, frames, quality" and "Probes and graphs"). Canvases are small; every engine is the CPU one.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use waveduderust::f16;
use waveduderust::model::normalize::Normalize;
use waveduderust::model::{Layer, Measurement, Probe, ProbeKind, Project, Source, View};
use waveduderust::quality::{Brightness, PreviewQuality};
use waveduderust::run::*;
use waveduderust::unstable::engine::capture::field_index;
use waveduderust::unstable::engine::cpu::CpuEngine;
use waveduderust::unstable::engine::probes::{ProbeCollector, ProbeTrace, record};
use waveduderust::unstable::engine::{SampleFormat, Snapshot, WaveEngine};
use waveduderust::unstable::spec::{RunRequest, RunSpec, RunSpecError, SceneCache};

const GIB: u64 = 1 << 30;

fn project(w: u32, h: u32, steps: u32) -> Project {
	let mut p = Project::default();
	p.scene.width = w;
	p.scene.height = h;
	p.scene.damping_edge = -4;
	p.simulation.steps = steps;
	p.simulation.visualize_every = 2;
	p.sources.push(Source::default().with_name("S".into()).with_x(f64::from(w / 2)).with_y(f64::from(h / 2)));
	p
}

fn with_probes(mut p: Project) -> Project {
	let (cx, cy) = (p.scene.width as i32 / 2, p.scene.height as i32 / 2);
	p.probes = vec![
		Probe::default().with_name("amp".into()).with_ax(cx + 3).with_ay(cy),
		Probe::default()
			.with_name("int".into())
			.with_ax(cx + 5)
			.with_ay(cy)
			.with_layer(Layer::Intensity)
			.with_measurement(Measurement::IntegratedValue),
		Probe::default()
			.with_name("line".into())
			.with_kind(ProbeKind::Line)
			.with_ax(cx - 6)
			.with_ay(cy + 2)
			.with_bx(cx + 6)
			.with_by(cy + 4)
			.with_layer(Layer::Intensity)
			.with_measurement(Measurement::Value),
		Probe::default().with_name("off".into()).with_active(false),
	];
	p.normalize();
	p
}

/// A run of `p` as a default view shows it, which previews the intensity.
fn spec(p: &Project, q: PreviewQuality) -> RunSpec {
	spec_with(p, &View::default(), q)
}

fn spec_with(p: &Project, v: &View, q: PreviewQuality) -> RunSpec {
	RunSpec::create(p, None, q, v.into(), v.intensity.preview).unwrap()
}

fn engine(spec: &RunSpec) -> Box<dyn WaveEngine> {
	Box::new(CpuEngine::new(spec.scene.clone(), spec.options.clone()))
}

fn store(spec: &RunSpec, budget: u64) -> Arc<FrameStore> {
	Arc::new(FrameStore::in_dir(StoreLayout::of(spec), budget, None, &FrameStore::default_dir()).unwrap())
}

/// `spec` stepped straight through, each stored frame captured as it passes and every step's probe readings
/// recorded: the frames and traces a live run is held to.
fn batch(spec: &RunSpec) -> (Arc<FrameStore>, Vec<ProbeTrace>) {
	let st = store(spec, GIB);
	let mut e = engine(spec);
	let mut collector = ProbeCollector::new(&spec.probes, &spec.scene, spec.seed);
	e.set_emitters(&spec.emitters);
	e.set_probes(collector.cells());
	e.reset();
	let mut recorded = 0;
	for i in 0..spec.schedule.steps {
		e.advance_one_step().unwrap();
		if !spec.probes.is_empty() {
			e.queue_probes().unwrap();
			let readings = e.pending_probes().unwrap();
			record(e.as_mut(), readings, &mut collector, &mut recorded, spec.schedule.visualize_every).unwrap();
		}
		if i % spec.schedule.capture_every() == 0 {
			st.write(e.capture_frame(&spec.shape).unwrap()).unwrap();
		}
	}
	(st, collector.traces)
}

fn halves(bytes: &[u8]) -> Vec<f16> {
	bytemuck::cast_slice::<u8, f16>(bytes).to_vec()
}

fn snapshot_of(spec: &RunSpec, seed: f32) -> Snapshot {
	let n = spec.shape.plane_bytes();
	let plane = |k: f32| (0..n).map(|i| (i as f32 * 0.37 + seed * 11.0 + k).sin().to_bits() as u8).collect::<Vec<_>>();
	Snapshot { shape: spec.shape, step: 0, field: plane(0.0), intensity: plane(1.0) }
}

// ---- spec and store ----

#[test]
fn reduced_run_schedule() {
	let p = project(64, 48, 200);
	let s = spec(&p, PreviewQuality::new(2, 2, SampleFormat::Index));
	assert_eq!((s.schedule.frame_count(), s.schedule.stored_frames(), s.schedule.capture_every()), (100, 50, 4));
	assert_eq!((s.shape.width(), s.shape.height(), s.shape.plane_bytes()), (32, 24, 32 * 24));
	assert_eq!((s.schedule.stored_index_of(98), s.schedule.stored_index_of(99)), (49, 49));
	let (st, _) = batch(&s);
	assert_eq!(st.count(), 50);
	assert_eq!(s.schedule.covered_frames(50), 100);
	assert_eq!(st.copy_layer(49, Layer::Field).unwrap().len(), 32 * 24);
	assert_eq!(st.step_of(49), 196);
}

#[test]
fn spec_snapshot() {
	let mut p = with_probes(project(64, 48, 21));
	let s = spec(&p, PreviewQuality::FULL);
	assert_eq!(s.schedule.frame_count(), 11, "ceil(21 / 2)");
	assert_eq!(s.probes.len(), 3, "inactive probes left out");
	assert_eq!(s.layers(), &[Layer::Field, Layer::Intensity]);
	let mut v = View::default();
	let brightness = (v.field.brightness as f32, v.intensity.brightness as f32);
	assert_eq!((s.shape.field_brightness, s.shape.intensity_brightness), brightness);
	assert_eq!((s.scene.grid_width(), s.scene.rim), (72, 4));
	v.intensity.preview = false;
	p.probes.clear();
	assert_eq!(spec_with(&p, &v, PreviewQuality::FULL).layers(), &[Layer::Field]);
	let b = Brightness::default();
	assert_eq!(RunSpec::create(&p, None, PreviewQuality::FULL, b, true).unwrap().layers().len(), 2);
	p.scene.mode = waveduderust::model::SceneMode::Image;
	assert_eq!(RunSpec::create(&p, None, PreviewQuality::FULL, b, false).unwrap_err(), RunSpecError::NoImage);
}

#[test]
fn empty_store_layout_and_refused_writes() {
	let s = spec(&project(64, 48, 20), PreviewQuality::FULL);
	let st = store(&s, GIB);
	let layout = st.layout();
	assert_eq!((st.capacity(), st.count(), st.layers().len()), (10, 0, 2));
	assert_eq!(st.frame_bytes(), 2 * 64 * 48 * 2);
	assert_eq!(layout.file_bytes(10), 64 + 10 * 2 * 64 * 48 * 2, "SizeOf");
	assert!(matches!(st.copy_layer(0, Layer::Field), Err(StoreError::NotStored { frame: 0, count: 0 })));
	assert!(st.spill_bytes().is_none());
	// writes: beyond the capacity of 3, foreign snapshots
	let s = spec(&project(64, 48, 6), PreviewQuality::FULL);
	let st = store(&s, GIB);
	for k in 0..3 {
		assert_eq!(st.write(snapshot_of(&s, k as f32)).unwrap(), k);
	}
	assert!(matches!(st.write(snapshot_of(&s, 9.0)), Err(StoreError::Full(3))));
	let index = s.with_quality(PreviewQuality::DEFAULT);
	let st = store(&s, GIB);
	assert!(matches!(st.write(snapshot_of(&index, 0.0)), Err(StoreError::ShapeMismatch)));
	let mut short = snapshot_of(&s, 0.0);
	short.intensity.clear();
	assert!(matches!(st.write(short), Err(StoreError::ShapeMismatch)));
	let p = project(64, 48, 6);
	let field_only = spec_with(&p, &View::default().with_intensity(hidden()), PreviewQuality::FULL);
	let st = store(&field_only, GIB);
	st.write(snapshot_of(&field_only, 0.0)).unwrap();
	assert!(matches!(st.copy_layer(0, Layer::Intensity), Err(StoreError::NoLayer(Layer::Intensity))));
}

#[test]
fn copy_layer_is_the_half_of_every_cell() {
	let s = spec(&project(64, 48, 9), PreviewQuality::FULL);
	let mut e = engine(&s);
	e.set_emitters(&s.emitters);
	for _ in 0..9 {
		e.advance_one_step().unwrap();
	}
	let (field, intensity) = e.prepare_frame().unwrap();
	let st = store(&s, GIB);
	st.write(e.capture_frame(&s.shape).unwrap()).unwrap();
	let f = halves(&st.copy_layer(0, Layer::Field).unwrap());
	let i = halves(&st.copy_layer(0, Layer::Intensity).unwrap());
	assert_eq!(f, field.iter().map(|&v| f16::from_f32(v)).collect::<Vec<_>>());
	assert_eq!(i, intensity.iter().map(|&v| f16::from_f32(v.sqrt())).collect::<Vec<_>>(), "sqrt(I) by default");
	assert!(f.iter().any(|v| v.to_f32() != 0.0));
	let root_off = s.with_quality(PreviewQuality { intensity_root: false, ..PreviewQuality::FULL });
	let i = halves(&e.capture_frame(&root_off.shape).unwrap().intensity);
	assert_eq!(i, intensity.iter().map(|&v| f16::from_f32(v)).collect::<Vec<_>>(), "I when the shape says so");
}

#[test]
fn a_worker_sweeps_the_spill_files_a_dead_process_left() {
	let dir = tempfile::tempdir().unwrap();
	std::fs::write(dir.path().join("frames-abc.wdfs"), [0u8; 64]).unwrap();
	std::fs::write(dir.path().join("keep.txt"), b"x").unwrap();
	assert_eq!(sweep_leftovers(dir.path()).unwrap(), 64);
	assert!(dir.path().join("keep.txt").exists() && !dir.path().join("frames-abc.wdfs").exists());
	assert_eq!(sweep_leftovers(&dir.path().join("missing")).unwrap(), 0);
	std::fs::write(dir.path().join("frames-def.wdfs"), [0u8; 64]).unwrap();
	let _w = worker(dir.path());
	assert!(!dir.path().join("frames-def.wdfs").exists(), "the worker sweeps at its start");
}

#[test]
fn tiered_store() {
	let dir = tempfile::tempdir().unwrap();
	let s = spec(&project(40, 30, 20), PreviewQuality::FULL);
	let frame = s.frame_bytes();
	let st = FrameStore::in_dir(StoreLayout::of(&s), 4 * frame + frame / 2, None, dir.path()).unwrap();
	assert_eq!((st.capacity(), st.ram_frames(), st.first_spilled()), (10, 4, 4));
	assert_eq!(st.spill_bytes(), Some(StoreLayout::of(&s).file_bytes(6)));
	let written: Vec<Snapshot> = (0..10).map(|k| snapshot_of(&s, k as f32)).collect();
	for w in &written {
		st.write(w.clone()).unwrap();
	}
	assert_eq!((st.frames_in_memory(), st.frames_on_disk()), (4, 6));
	for (k, w) in written.iter().enumerate() {
		assert_eq!(st.copy_layer(k, Layer::Field).unwrap(), w.field, "field of frame {k}");
		assert_eq!(st.copy_layer(k, Layer::Intensity).unwrap(), w.intensity, "intensity of frame {k}");
		assert_eq!(st.with_layer(k, Layer::Field, |b| b[7]).unwrap(), w.field[7]);
	}
	drop(st);
	assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "the file goes with the store");

	let st = FrameStore::in_dir(StoreLayout::of(&s), 10 * frame, None, dir.path()).unwrap();
	for w in &written {
		st.write(w.clone()).unwrap();
	}
	assert!(st.spill_bytes().is_none());
	assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "a run within the budget creates no file");

	let st = FrameStore::in_dir(StoreLayout::of(&s), 0, None, dir.path()).unwrap();
	st.write(written[3].clone()).unwrap();
	assert_eq!(st.copy_layer(0, Layer::Intensity).unwrap(), written[3].intensity);
	assert_eq!((st.frames_in_memory(), st.frames_on_disk(), st.ram_bytes()), (0, 1, 0));
}

#[test]
fn readers_on_other_threads_see_every_written_frame() {
	let dir = tempfile::tempdir().unwrap();
	let s = spec(&project(40, 30, 200), PreviewQuality::FULL);
	let st = Arc::new(FrameStore::in_dir(StoreLayout::of(&s), 30 * s.frame_bytes(), None, dir.path()).unwrap());
	let done = Arc::new(AtomicBool::new(false));
	let readers: Vec<_> = (0..3)
		.map(|_| {
			let (st, done, s) = (st.clone(), done.clone(), s.clone());
			std::thread::spawn(move || {
				// A reader may first run after the last write: it reads at least once either way.
				let mut reads = 0;
				while reads == 0 || !done.load(Ordering::Acquire) {
					let n = st.count();
					if n > 0 {
						let k = reads % n;
						assert_eq!(st.copy_layer(k, Layer::Field).unwrap(), snapshot_of(&s, k as f32).field);
						reads += 1;
					}
				}
			})
		})
		.collect();
	for k in 0..s.schedule.stored_frames() {
		st.write(snapshot_of(&s, k as f32)).unwrap();
	}
	done.store(true, Ordering::Release);
	for r in readers {
		r.join().unwrap();
	}
}

#[test]
fn the_memory_tier_waits_for_the_release() {
	let s = spec(&project(40, 30, 4), PreviewQuality::FULL);
	let released = Arc::new(AtomicBool::new(false));
	let flag = released.clone();
	let release: Release = Box::new(move || {
		std::thread::sleep(Duration::from_millis(50));
		flag.store(true, Ordering::Release);
	});
	let st = FrameStore::in_dir(StoreLayout::of(&s), GIB, Some(release), &FrameStore::default_dir()).unwrap();
	assert!(!released.load(Ordering::Acquire));
	let block = st.next_buffer();
	assert!(released.load(Ordering::Acquire), "the first block came after the release");
	assert_eq!(block.field.len(), s.shape.plane_bytes());
	st.write(block).unwrap();
}

/// The signal a store's drop fires, which the next store's producer waits for instead of polling.
#[test]
fn the_release_signal_fires_when_the_last_handle_drops() {
	let s = spec(&project(40, 30, 4), PreviewQuality::FULL);
	let st = store(&s, GIB);
	let signal = st.release_signal();
	let reader = st.clone();
	drop(st);
	assert!(!signal.wait(Duration::from_millis(20)), "a reader still holds the store");
	drop(reader);
	assert!(signal.wait(Duration::from_secs(5)), "the last handle dropped");
}

#[test]
fn frames_in_budget() {
	let mut p = project(1920, 1080, 2500);
	p.sources.clear();
	let full = spec(&p, PreviewQuality::FULL);
	// Two layers of 2-byte samples at full size; of 1-byte indices at half size.
	assert_eq!(full.frame_bytes(), 2 * 1920 * 1080 * 2);
	let reduced = full.with_quality(PreviewQuality::new(2, 1, SampleFormat::Index));
	assert_eq!(reduced.frame_bytes(), 2 * 960 * 540);
	assert_eq!(full.frame_bytes(), 8 * reduced.frame_bytes());
	// spec/70 FramesInBudget: 129 full-HD frames of both planes per GiB (spec/70's GB), 1035 half-size index frames
	assert_eq!(Budget::frames_in(GIB, full.frame_bytes()), 129);
	assert_eq!(Budget::frames_in(GIB, reduced.frame_bytes()), 1035);
	assert_eq!(Budget::frames_in(0, reduced.frame_bytes()), 0);
}

#[test]
fn store_bytes_at_equals_the_store_a_run_allocates() {
	let p = project(67, 45, 21);
	let s = spec(&p, PreviewQuality::FULL);
	let r = request(&p, PreviewQuality::FULL);
	for res in [1, 2, 4] {
		for frames in [1, 2, 4] {
			for format in [SampleFormat::Half, SampleFormat::Index] {
				let q = PreviewQuality::new(res, frames, format);
				let st = store(&s.with_quality(q), GIB);
				assert_eq!(r.store_bytes_at(q), st.bytes_of(st.capacity()), "{q:?}");
				assert_eq!(r.with_quality(q).store_bytes(), r.store_bytes_at(q));
			}
		}
	}
}

#[test]
fn index_frames_are_the_palette_index_of_the_half() {
	let s = spec(&project(64, 48, 30), PreviewQuality::FULL);
	let (half, _) = batch(&s);
	let (index, _) = batch(&s.with_quality(PreviewQuality::new(1, 1, SampleFormat::Index)));
	for k in [3, 14] {
		let h = halves(&half.copy_layer(k, Layer::Field).unwrap());
		let x = index.copy_layer(k, Layer::Field).unwrap();
		assert_eq!(x, h.iter().map(|&v| field_index(v, 1.0)).collect::<Vec<_>>());
	}
}

// ---- probes ----

#[test]
fn probe_traces_of_a_run() {
	let s = spec(&with_probes(project(64, 48, 100)), PreviewQuality::FULL);
	let (_, t) = batch(&s);
	assert_eq!(t.len(), 3);
	assert_eq!(t[0].series.len(), 100, "one sample per step");
	assert_eq!(t[1].series.len(), 100);
	assert!(t[1].series.iter().all(|&v| v >= 0.0) && t[1].series[99] > 0.0);
	assert!(t[0].profiles.is_empty());
	assert_eq!(t[2].profiles.len(), 50, "one profile per timeline frame");
	assert_eq!(t[2].profiles.first().map(<[f32]>::len), Some(13));
	assert!((t[2].length - 12f64.hypot(2.0)).abs() < 1e-12);
	// Graph data at frame 20 / step 40.
	assert_eq!(t[0].series[..=s.schedule.step_of(20) as usize].len(), 41);
	assert_eq!((t[0].steps_sampled, t[2].steps_sampled), (100, 99), "the last frame closes at step index 98");
}

// ---- the live run ----

fn worker(dir: &std::path::Path) -> RunWorker {
	RunWorker::cpu(dir.to_path_buf()).unwrap()
}

/// [`spec`]'s request.
fn request(p: &Project, q: PreviewQuality) -> RunRequest {
	request_with(p, &View::default(), q)
}

fn request_with(p: &Project, v: &View, q: PreviewQuality) -> RunRequest {
	RunRequest::new(p, q, v.into(), v.intensity.preview)
}

/// The intensity layer of a default view, not shown.
fn hidden() -> waveduderust::model::DisplayLayer {
	waveduderust::model::DisplayLayer::intensity_default().with_preview(false)
}

fn goal(run: &RunRequest, position: u32, wanted: Wanted) -> Goal {
	Goal { revision: 1, run: Some(run.clone()), position, wanted, ..Goal::default() }
}

/// Polls until `done` holds (within a minute).
fn wait_until(mut done: impl FnMut() -> bool) {
	let deadline = Instant::now() + Duration::from_secs(60);
	while !done() {
		assert!(Instant::now() < deadline, "the live run did not get there");
		std::thread::sleep(Duration::from_millis(2));
	}
}

fn store_has(w: &RunWorker, frame: u32) -> bool {
	w.store().is_some_and(|s| s.has(frame as usize))
}

fn same_frames(a: &FrameStore, b: &FrameStore, frames: u32) {
	for k in 0..frames as usize {
		assert_eq!(
			a.copy_layer(k, Layer::Field).unwrap(),
			b.copy_layer(k, Layer::Field).unwrap(),
			"field of frame {k}"
		);
		assert_eq!(
			a.copy_layer(k, Layer::Intensity).unwrap(),
			b.copy_layer(k, Layer::Intensity).unwrap(),
			"intensity of frame {k}"
		);
	}
}

#[test]
fn a_waiter_wakes_for_the_frames_and_the_end_of_a_run_of_every_frame() {
	let dir = tempfile::tempdir().unwrap();
	let p = project(64, 48, 20);
	let w = worker(dir.path());
	w.set_goal(goal(&request(&p, PreviewQuality::FULL), 0, Wanted::All));
	let st = w.wait_until(|s| s.store.is_some()).store.unwrap();
	for k in 1..=10 {
		w.wait_until(|_| st.count() >= k);
	}
	let done = w.wait_until(|s| s.state == WorkState::Idle && s.adopted == 1);
	assert_eq!((st.count(), done.steps_done), (10, 19), "the last frame is the state after 19 steps");
	same_frames(&st, &batch(&spec(&p, PreviewQuality::FULL)).0, 10);
	let last = halves(&st.copy_layer(9, Layer::Field).unwrap());
	let at = |x: usize, y: usize| last[y * 64 + x].to_f32();
	assert!(at(32, 24) != 0.0, "field at the source");
	assert_eq!(at(2, 2), 0.0, "19 steps reach no corner");
}

#[test]
fn the_live_run_makes_the_batch_runs_frames_the_positions_first_or_all_on_the_way() {
	let dir = tempfile::tempdir().unwrap();
	let s = spec(&with_probes(project(64, 48, 100)), PreviewQuality::FULL);
	let s_run = request(&with_probes(project(64, 48, 100)), PreviewQuality::FULL);
	let (r, run_traces) = batch(&s);
	let w = worker(dir.path());
	w.set_goal(goal(&s_run, 20, Wanted::Position));
	wait_until(|| store_has(&w, 20));
	let st = w.store().unwrap();
	assert_eq!(st.copy_layer(20, Layer::Field).unwrap(), r.copy_layer(20, Layer::Field).unwrap());
	assert_eq!(st.copy_layer(20, Layer::Intensity).unwrap(), r.copy_layer(20, Layer::Intensity).unwrap());
	wait_until(|| w.status().state == WorkState::Idle);
	assert_eq!((st.present_count(), st.count()), (1, 0), "the position alone, and no prefix");
	w.with_traces(|t| {
		assert_eq!(t[0].series, run_traces[0].series[..41]);
		assert_eq!(t[1].series, run_traces[1].series[..41]);
		assert_eq!(t[2].profiles.len(), 21, "a profile per frame on the way");
		assert_eq!(t[2].profiles.get(20), run_traces[2].profiles.get(20));
	});
	assert_eq!((w.status().adopted, w.status().traces_steps), (1, 41));

	w.set_goal(goal(&s_run, 20, Wanted::All));
	wait_until(|| w.store().is_some_and(|s| s.is_complete()));
	wait_until(|| w.status().state == WorkState::Idle);
	same_frames(&st, &r, s.schedule.stored_frames());
	w.with_traces(|t| assert_eq!(t, &run_traces[..], "the traces of the whole run"));
	assert!(Arc::ptr_eq(&st, &w.store().unwrap()), "one store throughout");

	let dir = tempfile::tempdir().unwrap();
	let w = worker(dir.path());
	w.set_goal(goal(&s_run, 20, Wanted::All));
	wait_until(|| store_has(&w, 20));
	let st = w.store().unwrap();
	assert!(st.count() >= 21, "the frames before the position landed on the way to it");
	wait_until(|| st.is_complete() && w.status().state == WorkState::Idle);
	assert_eq!(w.status().steps_done, s.schedule.steps, "one pass, no wrap");
	same_frames(&st, &r, s.schedule.stored_frames());
}

/// A spill never opened: the frames fit in memory.
struct NoSpill;

impl SpillFactory for NoSpill {
	fn poll_ready(&self, _: &mut std::task::Context<'_>) -> std::task::Poll<()> {
		std::task::Poll::Ready(())
	}

	fn open(&self, _store: u64, _layout: &StoreLayout, _bytes: u64) -> std::io::Result<Box<dyn SpillBackend>> {
		Err(std::io::Error::other("the frames fit in memory"))
	}
}

#[test]
fn a_pumped_live_run_makes_the_batch_runs_frames() {
	let p = with_probes(project(64, 48, 40));
	let s = spec(&p, PreviewQuality::FULL);
	let (r, traces) = batch(&s);
	let start: PumpedFactory = Box::new(|scene, options| -> EngineFuture {
		Box::pin(async move { Box::new(CpuEngine::new(scene, options)) as Box<dyn WaveEngine> })
	});
	let w = RunWorker::pumped(start, GIB, GIB, Box::new(NoSpill), Arc::default());
	w.set_goal(goal(&request(&p, PreviewQuality::FULL), 0, Wanted::All));
	while w.pump(Duration::from_millis(50), std::task::Waker::noop()) != Pump::Idle {}
	assert_eq!(w.status().state, WorkState::Idle);
	let st = w.store().unwrap();
	assert!(st.is_complete());
	same_frames(&st, &r, s.schedule.stored_frames());
	w.with_traces(|t| assert_eq!(t, &traces[..]));
}

#[test]
fn the_traces_of_the_frame_kept_through_a_clear_are_held_with_it() {
	let dir = tempfile::tempdir().unwrap();
	let mut p = with_probes(project(64, 48, 100));
	let s_run = request(&p, PreviewQuality::FULL);
	let w = worker(dir.path());
	w.set_goal(goal(&s_run, 20, Wanted::Position));
	wait_until(|| store_has(&w, 20) && w.status().state == WorkState::Idle);
	let before = w.with_traces(<[ProbeTrace]>::to_vec);
	w.with_held_traces(|t| assert!(t.is_empty(), "nothing before the first generation"));
	w.set_goal(Goal { revision: 2, paused: true, ..goal(&s_run, 20, Wanted::Position) });
	wait_until(|| w.status().adopted == 2 && w.status().state == WorkState::Idle);
	p.sources[0].frequency += 1.0;
	w.set_goal(Goal { revision: 3, paused: true, ..goal(&request(&p, PreviewQuality::FULL), 20, Wanted::All) });
	wait_until(|| w.status().adopted == 3);
	assert!(w.store().unwrap().latest().is_some(), "the frame kept through the clear");
	w.with_held_traces(|t| assert_eq!(t, &before[..], "the previous frames' traces, with the frame kept"));
	p.probes.truncate(1);
	w.set_goal(Goal { revision: 4, paused: true, ..goal(&request(&p, PreviewQuality::FULL), 20, Wanted::All) });
	wait_until(|| w.status().adopted == 4 && w.with_traces(|t| t.len() == 1));
	w.with_held_traces(|t| assert!(t.is_empty(), "other probes: nothing fits the frame kept"));
}

#[cfg(feature = "gpu")]
#[test]
fn a_live_run_on_the_gpu_keeps_its_frames_on_the_device_tagged_as_stored() {
	use waveduderust::unstable::engine::gpu::{FrameTag, GpuEngine, context};
	let ctx = match context() {
		Ok(c) => c,
		Err(e) => {
			eprintln!("skipped: {e}");
			return;
		}
	};
	let dir = tempfile::tempdir().unwrap();
	let w = RunWorker::new(
		Box::new(|scene, options| Box::new(GpuEngine::new(scene, options).expect("GPU engine"))),
		dir.path().to_path_buf(),
		Arc::default(),
	)
	.unwrap();
	let p = project(64, 48, 100);
	let q = PreviewQuality { format: SampleFormat::Index, ..PreviewQuality::FULL };
	w.set_goal(goal(&request(&p, q), 20, Wanted::Position));
	wait_until(|| store_has(&w, 20) && w.status().state == WorkState::Idle);
	let (status, store) = (w.status(), w.store().unwrap());
	let frames = status.device_frames.expect("a GPU engine's frames");
	let tag = |stored| FrameTag { store: store.id(), generation: store.generation(), stored };
	let frame = frames.hold(tag(20)).expect("the frame stored last");
	let target = ctx.device.create_buffer(&wgpu::BufferDescriptor {
		label: None,
		size: frame.buffer.size(),
		usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
		mapped_at_creation: false,
	});
	let mut encoder = ctx.device.create_command_encoder(&Default::default());
	encoder.copy_buffer_to_buffer(&frame.buffer, 0, &target, 0, frame.buffer.size());
	ctx.queue.submit([encoder.finish()]);
	target.slice(..).map_async(wgpu::MapMode::Read, |r| r.unwrap());
	ctx.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
	let bytes = target.slice(..).get_mapped_range().unwrap().to_vec();
	for layer in [Layer::Field, Layer::Intensity] {
		let plane = store.copy_layer(20, layer).unwrap();
		let first = frame.first_word(layer).expect("the intensity is tracked") as usize * 4;
		assert_eq!(&bytes[first..first + plane.len()], plane, "{layer:?}");
	}
	assert!(frames.hold(tag(19)).is_none(), "a frame not captured");
	assert!(frames.hold(FrameTag { store: store.id() + 1, ..tag(20) }).is_none(), "another store's frame");
}

#[test]
fn a_panicking_engine_fails_the_live_run() {
	let dir = tempfile::tempdir().unwrap();
	let w = RunWorker::new(
		Box::new(|_, _| -> Box<dyn WaveEngine> { panic!("the engine broke") }),
		dir.path().to_path_buf(),
		Arc::default(),
	)
	.unwrap();
	let p = project(64, 48, 100);
	w.set_goal(goal(&request(&p, PreviewQuality::FULL), 5, Wanted::Position));
	wait_until(|| matches!(w.status().state, WorkState::Failed(_)));
}

#[test]
fn the_engine_is_kept_across_edits_of_the_same_grid() {
	let dir = tempfile::tempdir().unwrap();
	let built = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let counting = built.clone();
	let w = RunWorker::new(
		Box::new(move |scene, options| {
			counting.fetch_add(1, Ordering::AcqRel);
			Box::new(CpuEngine::new(scene, options))
		}),
		dir.path().to_path_buf(),
		Arc::default(),
	)
	.unwrap();
	let mut p = project(64, 48, 100);
	let mut revision = 0;
	let mut edit = |p: &Project| {
		revision += 1;
		w.set_goal(Goal { revision, ..goal(&request(p, PreviewQuality::FULL), 5, Wanted::Position) });
		wait_until(|| w.status().adopted == revision && store_has(&w, 5) && w.status().state == WorkState::Idle);
		built.load(Ordering::Acquire)
	};
	assert_eq!(edit(&p), 1);
	p.sources[0].frequency += 1.0;
	assert_eq!(edit(&p), 1, "a source edit keeps the engine");
	p.parts.push(waveduderust::model::Part::new(waveduderust::model::PartKind::Rectangle(Default::default()), 20, 20));
	assert_eq!(edit(&p), 1, "a part edit swaps the scene in");
	assert_eq!(edit(&p), 1, "tracking the intensity is switched on the engine");
	p.scene.width = 80;
	assert_eq!(edit(&p), 2, "another grid builds anew");
}

#[test]
fn scrubbing_a_finished_run_computes_nothing() {
	let dir = tempfile::tempdir().unwrap();
	let p = project(64, 48, 100);
	let w = worker(dir.path());
	let r = request(&p, PreviewQuality::FULL);
	w.set_goal(Goal { revision: 1, ..goal(&r, 0, Wanted::All) });
	wait_until(|| w.status().adopted == 1 && w.store().is_some_and(|s| s.is_complete()));
	wait_until(|| w.status().state == WorkState::Idle);
	let done = w.status().steps_done;
	for k in 0..300u32 {
		let wanted = if k % 2 == 0 { Wanted::Position } else { Wanted::All };
		w.set_goal(Goal { revision: 1, ..goal(&r, (k * 37) % 100, wanted) });
	}
	std::thread::sleep(Duration::from_millis(200));
	wait_until(|| w.status().state == WorkState::Idle);
	let st = w.status();
	assert_eq!((st.steps_done, st.adopted), (done, 1), "a stored frame is not computed again");
	assert!(w.store().unwrap().is_complete());
}

#[test]
fn a_store_that_fits_in_memory_never_spills_across_edits() {
	let dir = tempfile::tempdir().unwrap();
	let mut p = project(64, 48, 100);
	let w = worker(dir.path());
	for revision in 1..=4 {
		p.sources[0].frequency += 1.0;
		w.set_goal(Goal { revision, ..goal(&request(&p, PreviewQuality::FULL), 0, Wanted::All) });
		wait_until(|| w.status().adopted == revision && w.store().is_some_and(|s| s.is_complete()));
		let st = w.store().unwrap();
		assert!(st.spill_bytes().is_none() && st.frames_on_disk() == 0, "generation {revision} spilled");
	}
}

#[test]
fn a_rebuilt_engine_keeps_the_frames_and_the_traces() {
	let dir = tempfile::tempdir().unwrap();
	let built = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let counting = built.clone();
	let w = RunWorker::new(
		Box::new(move |scene, options| {
			counting.fetch_add(1, Ordering::AcqRel);
			Box::new(CpuEngine::new(scene, options))
		}),
		dir.path().to_path_buf(),
		Arc::default(),
	)
	.unwrap();
	let p = with_probes(project(64, 48, 100));
	let s = spec(&p, PreviewQuality::FULL);
	w.set_goal(goal(&request(&p, PreviewQuality::FULL), 20, Wanted::All));
	wait_until(|| w.store().is_some_and(|s| s.is_complete()) && w.status().state == WorkState::Idle);
	let st = w.store().unwrap();
	let traces = w.with_traces(<[ProbeTrace]>::to_vec);
	w.rebuild_engine();
	w.set_goal(Goal { revision: 2, ..goal(&request(&p, PreviewQuality::FULL), 30, Wanted::All) });
	wait_until(|| built.load(Ordering::Acquire) == 2 && w.status().state == WorkState::Idle);
	assert!(st.is_complete(), "the frames stayed");
	w.with_traces(|t| assert_eq!(t, &traces[..], "the traces stayed"));
	same_frames(&st, &batch(&s).0, s.schedule.stored_frames());
}

#[test]
fn the_probes_tail_is_recorded_from_wherever_the_engine_stands() {
	let dir = tempfile::tempdir().unwrap();
	let s = spec(&with_probes(project(96, 72, 4000)), PreviewQuality::new(1, 4, SampleFormat::Index));
	let s_run = request(&with_probes(project(96, 72, 4000)), PreviewQuality::new(1, 4, SampleFormat::Index));
	let last = s.schedule.stored_frames() - 1;
	let w = worker(dir.path());
	w.set_goal(goal(&s_run, last * s.schedule.frame_divisor(), Wanted::Position));
	wait_until(|| store_has(&w, last) && w.status().state == WorkState::Idle);
	w.set_goal(goal(&s_run, 0, Wanted::Position));
	wait_until(|| store_has(&w, 0) && w.status().state == WorkState::Idle);
	let to_last = last * s.schedule.capture_every() + 1;
	assert_eq!(w.status().traces_steps, to_last, "the traces reach the last frame, not the last step");
	w.set_goal(goal(&s_run, 0, Wanted::All));
	let st = w.store().unwrap();
	wait_until(|| st.is_complete() && w.status().state == WorkState::Idle);
	assert_eq!(w.status().traces_steps, 4000, "the tail was recorded although the engine stood before it");
	w.with_traces(|t| assert_eq!(t, &batch(&s).1[..]));
}

#[test]
fn a_pause_with_another_spec_adopts_it() {
	let dir = tempfile::tempdir().unwrap();
	let mut p = project(96, 72, 4000);
	let s_run = request(&p, PreviewQuality::FULL);
	let w = worker(dir.path());
	w.set_goal(Goal { revision: 1, ..goal(&s_run, 99, Wanted::All) });
	wait_until(|| w.status().steps_done > 100);
	w.set_goal(Goal { revision: 2, paused: true, ..goal(&s_run, 99, Wanted::All) });
	wait_until(|| w.status().state == WorkState::Paused);
	p.sources[0].frequency += 1.0;
	let edited_run = request(&p, PreviewQuality::FULL);
	w.set_goal(Goal { revision: 3, paused: true, ..goal(&edited_run, 99, Wanted::All) });
	wait_until(|| w.status().adopted == 3 && store_has(&w, 99));
	wait_until(|| w.status().state == WorkState::Paused);
	assert_eq!(w.store().unwrap().present_count(), 1, "while paused, the frame looked at alone is made");
	w.set_goal(Goal { revision: 4, ..goal(&edited_run, 99, Wanted::All) });
	wait_until(|| store_has(&w, 99));
}

#[test]
fn a_baked_brightness_edit_keeps_the_store() {
	let dir = tempfile::tempdir().unwrap();
	let p = project(64, 48, 100);
	let q = PreviewQuality::new(1, 1, SampleFormat::Index);
	let w = worker(dir.path());
	w.set_goal(Goal { revision: 1, ..goal(&request(&p, q), 5, Wanted::Position) });
	wait_until(|| w.status().adopted == 1 && store_has(&w, 5));
	let st = w.store().unwrap();
	let mut v = View::default();
	v.field.brightness *= 2.0;
	w.set_goal(Goal { revision: 2, ..goal(&request_with(&p, &v, q), 5, Wanted::Position) });
	wait_until(|| w.status().adopted == 2 && store_has(&w, 5));
	assert!(Arc::ptr_eq(&st, &w.store().unwrap()), "the same buffers take the frames of the new brightness");
	assert_eq!(st.generation(), 1);
}

#[test]
fn a_probe_change_keeps_the_frames_and_records_anew() {
	let dir = tempfile::tempdir().unwrap();
	let mut p = with_probes(project(64, 48, 60));
	let s = spec(&p, PreviewQuality::FULL);
	let s_run = request(&p, PreviewQuality::FULL);
	let w = worker(dir.path());
	w.set_goal(goal(&s_run, 5, Wanted::All));
	wait_until(|| w.store().is_some_and(|s| s.is_complete()));
	let st = w.store().unwrap();
	p.probes[0].ax += 2;
	let moved = spec(&p, PreviewQuality::FULL);
	let moved_run = request(&p, PreviewQuality::FULL);
	assert!(s.same_frames_as(&moved) && !s.same_probes_as(&moved));
	w.set_goal(Goal { revision: 2, ..goal(&moved_run, 5, Wanted::All) });
	wait_until(|| w.status().adopted == 2);
	assert!(st.is_complete(), "the frames stay");
	wait_until(|| w.status().traces_steps == 60 && w.status().state == WorkState::Idle);
	let (r, traces) = batch(&moved);
	w.with_traces(|t| assert_eq!(t, &traces[..]));
	same_frames(&st, &r, moved.schedule.stored_frames());
}

#[test]
fn hiding_the_intensity_keeps_the_frames_that_track_it() {
	let dir = tempfile::tempdir().unwrap();
	let p = project(64, 48, 60);
	let mut v = View::default();
	let w = worker(dir.path());
	w.set_goal(goal(&request(&p, PreviewQuality::FULL), 5, Wanted::All));
	wait_until(|| w.store().is_some_and(|s| s.is_complete()));
	let st = w.store().unwrap();
	let generation = st.generation();
	for (revision, shown) in [(2, false), (3, true)] {
		v.intensity.preview = shown;
		w.set_goal(Goal { revision, ..goal(&request_with(&p, &v, PreviewQuality::FULL), 5, Wanted::All) });
		wait_until(|| w.status().adopted == revision);
		let now = w.store().unwrap();
		assert!(Arc::ptr_eq(&st, &now) && now.is_complete(), "the frames stay (intensity shown: {shown})");
		assert_eq!((now.generation(), now.layers()), (generation, &[Layer::Field, Layer::Intensity][..]));
	}
}

#[test]
fn a_frame_change_starts_over_in_the_same_store() {
	let dir = tempfile::tempdir().unwrap();
	let mut p = with_probes(project(64, 48, 60));
	let s_run = request(&p, PreviewQuality::FULL);
	let w = worker(dir.path());
	w.set_goal(goal(&s_run, 29, Wanted::All));
	wait_until(|| w.store().is_some_and(|s| s.is_complete()));
	let st = w.store().unwrap();
	p.scene.refractive_index = 1.5;
	let other = spec(&p, PreviewQuality::FULL);
	let other_run = request(&p, PreviewQuality::FULL);
	w.set_goal(Goal { revision: 2, ..goal(&other_run, 3, Wanted::Position) });
	wait_until(|| w.status().adopted == 2 && store_has(&w, 3) && w.status().state == WorkState::Idle);
	assert!(Arc::ptr_eq(&st, &w.store().unwrap()), "the same layout keeps the store");
	assert_eq!((st.present_count(), st.generation()), (1, 1), "cleared once, the position's frame back");
	let (r, _) = batch(&other);
	assert_eq!(st.copy_layer(3, Layer::Field).unwrap(), r.copy_layer(3, Layer::Field).unwrap());
	let index_run = request(&p, PreviewQuality::DEFAULT);
	w.set_goal(Goal { revision: 3, ..goal(&index_run, 3, Wanted::Position) });
	wait_until(|| w.status().adopted == 3 && store_has(&w, 3));
	assert!(!Arc::ptr_eq(&st, &w.store().unwrap()), "another layout, another store");
}

#[test]
fn the_fit_frame_is_the_positions_frame_in_halves() {
	let dir = tempfile::tempdir().unwrap();
	let s_run = request(&project(64, 48, 40), PreviewQuality::DEFAULT);
	let full = spec(&project(64, 48, 40), PreviewQuality::FULL);
	let (r, _) = batch(&full);
	let w = worker(dir.path());
	let mut g = goal(&s_run, 7, Wanted::Position);
	g.fit = Some(FitRequest { id: 1, frame: 7 });
	w.set_goal(g);
	let fit = loop {
		if let Some(f) = w.take_fit() {
			break f;
		}
		std::thread::sleep(Duration::from_millis(2));
	};
	assert_eq!((fit.id, fit.frame, fit.snapshot.shape.format, fit.snapshot.step), (1, 7, SampleFormat::Half, 14));
	assert_eq!(fit.snapshot.field, r.copy_layer(7, Layer::Field).unwrap());
	wait_until(|| store_has(&w, 7) && w.status().state == WorkState::Idle);
	assert!(w.take_fit().is_none(), "served once");
}

#[test]
fn a_paused_live_run_holds_its_steps() {
	let dir = tempfile::tempdir().unwrap();
	let s_run = request(&project(96, 72, 4000), PreviewQuality::new(1, 4, SampleFormat::Index));
	let w = worker(dir.path());
	w.set_goal(goal(&s_run, 999, Wanted::All));
	wait_until(|| w.status().steps_done > CHECK_INTERVAL);
	w.set_goal(Goal { paused: true, ..goal(&s_run, 999, Wanted::All) });
	wait_until(|| w.status().state == WorkState::Paused);
	let held = w.status().steps_done;
	std::thread::sleep(Duration::from_millis(60));
	assert_eq!(w.status().steps_done, held, "a paused run does not step");
	w.set_goal(goal(&s_run, 999, Wanted::All));
	wait_until(|| w.status().steps_done > held);
	w.set_goal(goal(&s_run, 999, Wanted::Position));
	wait_until(|| w.status().state == WorkState::Idle);
	assert!(store_has(&w, 999 / 4) && !w.store().unwrap().is_complete(), "stopped after the position's frame");
}

#[test]
fn a_sparse_store_tracks_presence_and_reuses_its_buffers() {
	let dir = tempfile::tempdir().unwrap();
	let s = spec(&project(40, 30, 20), PreviewQuality::FULL);
	let frame = s.frame_bytes();
	let st = FrameStore::in_dir(StoreLayout::of(&s), 4 * frame + frame / 2, None, dir.path()).unwrap();
	st.write_at(7, snapshot_of(&s, 7.0)).unwrap();
	st.write_at(2, snapshot_of(&s, 2.0)).unwrap();
	assert_eq!((st.count(), st.present_count(), st.has(2), st.has(3)), (0, 2, true, false));
	assert_eq!((st.frames_in_memory(), st.frames_on_disk()), (1, 1));
	assert_eq!(st.present_through(2), Some(2));
	assert_eq!(st.first_missing_from(2), Some(3));
	assert!(matches!(st.copy_layer(3, Layer::Field), Err(StoreError::NotStored { frame: 3, .. })));
	assert_eq!(st.copy_layer(7, Layer::Field).unwrap(), snapshot_of(&s, 7.0).field);
	st.write_at(7, snapshot_of(&s, 70.0)).unwrap();
	assert_eq!(st.copy_layer(7, Layer::Field).unwrap(), snapshot_of(&s, 7.0).field, "a present frame stays");
	for k in [0, 1, 3] {
		st.write_at(k, snapshot_of(&s, k as f32)).unwrap();
	}
	assert_eq!((st.count(), st.present_through(0)), (4, Some(3)));
	st.clear();
	assert_eq!((st.generation(), st.count(), st.present_count(), st.has(2)), (1, 0, 0, false));
	let block = st.buffer_for(0);
	assert_eq!(block.field.len(), s.shape.plane_bytes(), "a cleared frame's buffer comes back");
	st.write_at(0, block).unwrap();
	for k in 1..10 {
		st.write_at(k, snapshot_of(&s, k as f32)).unwrap();
	}
	assert!(st.is_complete());
	assert_eq!((st.frames_in_memory(), st.frames_on_disk()), (4, 6));
	assert_eq!(st.copy_layer(9, Layer::Field).unwrap(), snapshot_of(&s, 9.0).field);
}

// ---- memory warning ----

#[test]
fn memory_warning_levers_and_proposal() {
	let mut p = project(1920, 1080, 2500);
	p.sources.clear();
	let s = request(&p, PreviewQuality::FULL);
	let need = s.store_bytes();
	assert_eq!(need, 1920 * 1080 * 2 * 2 * 1250);
	assert!(MemoryWarning::check(&s, need, need).is_none());

	// The cache alone suffices: it comes before any quality.
	let w = MemoryWarning::check(&s, need - 1, 2 * need).unwrap();
	assert_eq!(w.enabled(), Levers { cache_auto: true, resolution: true, frames: true, samples: true });
	assert_eq!(w.proposal(), Levers { cache_auto: true, ..Levers::default() });
	assert!(w.fits(w.proposal()) && w.spill_of(Levers::default()) == 1);

	// Without the cache: 8-bit and every second frame both halve; the first in order wins.
	let w = MemoryWarning::check(&s, need / 2, need / 2).unwrap();
	assert!(!w.enabled().cache_auto);
	let samples = Levers { samples: true, ..Levers::default() };
	let frames = Levers { frames: true, ..Levers::default() };
	assert_eq!(w.bytes_of(samples), w.bytes_of(frames));
	assert_eq!(w.proposal(), samples);
	assert_eq!(w.quality_of(samples), PreviewQuality::new(1, 1, SampleFormat::Index));

	// Only a quarter fits with one lever: the resolution.
	let w = MemoryWarning::check(&s, need / 4, need / 4).unwrap();
	assert_eq!(w.proposal(), Levers { resolution: true, ..Levers::default() });

	// A fifth: no single lever fits; of the two-lever eighths the first in order.
	let w = MemoryWarning::check(&s, need / 5, need / 5).unwrap();
	assert_eq!(w.proposal(), Levers { samples: true, resolution: true, ..Levers::default() });

	// Nothing fits: every enabled lever.
	let w = MemoryWarning::check(&s, 1, 1).unwrap();
	assert_eq!(w.proposal(), Levers { cache_auto: false, resolution: true, frames: true, samples: true });

	// Levers at their lowest notch are greyed.
	let low = s.with_quality(PreviewQuality::new(4, 4, SampleFormat::Index));
	let w = MemoryWarning::check(&low, 1, 1).unwrap();
	assert_eq!(w.enabled(), Levers::default());
	assert_eq!(w.quality_of(Levers { resolution: true, frames: true, samples: true, cache_auto: false }), low.quality);
}

#[test]
fn specs_of_an_unchanged_scene_share_its_medium() {
	let mut p = project(64, 32, 20);
	let mut cache = SceneCache::default();
	let a = cache.spec(&p, PreviewQuality::FULL, Brightness::default(), true).unwrap();
	p.seed += 1;
	p.sources[0].frequency += 1.;
	let b = cache.spec(&p, PreviewQuality::DEFAULT, Brightness::default(), true).unwrap();
	assert!(Arc::ptr_eq(&a.scene, &b.scene), "seed, sources and quality leave the medium");
	p.scene.refractive_index = 1.5;
	let c = cache.spec(&p, PreviewQuality::FULL, Brightness::default(), true).unwrap();
	assert!(!Arc::ptr_eq(&a.scene, &c.scene));
	assert_eq!(*c.scene, RunSpec::scene_of(&p, None).unwrap());
	p.scene.edges = waveduderust::model::Edges::Reflective;
	let d = cache.spec(&p, PreviewQuality::FULL, Brightness::default(), true).unwrap();
	assert_eq!(d.scene.edges, waveduderust::model::Edges::Reflective, "the edges rebuild it too");
}

#[test]
fn a_frame_for_later_is_left_for_the_frame_looked_at_and_the_tail_comes_before_the_wrap() {
	let dir = tempfile::tempdir().unwrap();
	let s = spec(&with_probes(project(96, 72, 4000)), PreviewQuality::new(1, 4, SampleFormat::Index));
	let s_run = request(&with_probes(project(96, 72, 4000)), PreviewQuality::new(1, 4, SampleFormat::Index));
	let w = worker(dir.path());
	w.set_goal(goal(&s_run, 0, Wanted::All));
	wait_until(|| w.status().steps_done > 200);
	w.set_goal(goal(&s_run, 999, Wanted::Position));
	wait_until(|| store_has(&w, 999 / 4));
	wait_until(|| w.status().state == WorkState::Idle);
	let st = w.store().unwrap();
	assert!(st.present_count() < st.capacity(), "the fill was left for the frame looked at");
	w.set_goal(goal(&s_run, 500, Wanted::All));
	wait_until(|| st.is_complete() && w.status().state == WorkState::Idle);
	assert_eq!(w.status().traces_steps, 4000, "the probes' tail was recorded");
	w.with_traces(|t| assert_eq!(t, &batch(&s).1[..]));
}

/// `part`: the first part moves (the scene recomposed per edit) instead of the first source.
fn drag_burst(wanted: Wanted, part: bool) {
	let path = std::path::Path::new("tests/fixtures/examples/Example16_Wave_in_2_boxes.wdproj");
	let (mut project, view) = waveduderust::io::load_project(path).unwrap();
	let quality = PreviewQuality {
		resolution_divisor: 2,
		frame_divisor: 2,
		format: SampleFormat::Index,
		..PreviewQuality::DEFAULT
	};
	let dir = tempfile::tempdir().unwrap();
	let w = RunWorker::new(
		Box::new(|scene, options| waveduderust::unstable::engine::build(scene, options, true).engine),
		dir.path().to_path_buf(),
		Arc::default(),
	)
	.unwrap();
	let position = 100;
	let goal = |p: &Project, revision| Goal {
		revision,
		run: Some(RunRequest::new(p, quality, (&view).into(), view.intensity.preview)),
		position,
		wanted,
		..Goal::default()
	};
	w.set_goal(goal(&project, 1));
	wait_until(|| w.status().adopted == 1 && store_has(&w, position / 2));
	let one = Instant::now();
	nudge(&mut project, part);
	w.set_goal(goal(&project, 2));
	wait_until(|| w.status().adopted == 2 && store_has(&w, position / 2));
	let frame_time = one.elapsed();
	let started = Instant::now();
	let mut shown = 0;
	let mut last_seen = w.store().map_or(0, |s| s.generation());
	let edits = 120;
	for i in 0..edits {
		nudge(&mut project, part);
		w.set_goal(goal(&project, 3 + i));
		let until = started + Duration::from_millis((i + 1) * 1000 / 32);
		while Instant::now() < until {
			// A frame made shows until the next one: as the store's latest once its generation ends.
			if let Some(store) = w.store()
				&& store.generation() != last_seen
			{
				last_seen = store.generation();
				shown += usize::from(store.latest().is_some());
			}
			std::thread::sleep(Duration::from_millis(1));
		}
	}
	let last_edit = Instant::now();
	let last = 2 + edits;
	wait_until(|| w.status().adopted == last && store_has(&w, position / 2));
	let lag = last_edit.elapsed();
	println!(
		"Example16 frame {position} at half resolution on the GPU, {wanted:?} wanted, {} moved: one frame {:.0} ms; {edits} edits at 32/s over {:.1} s: {shown} frames shown ({:.0} ms apart), the final frame {:.0} ms after the last edit ({:.1} frame times)",
		if part { "a part" } else { "a source" },
		frame_time.as_secs_f64() * 1e3,
		started.elapsed().as_secs_f64(),
		started.elapsed().as_secs_f64() * 1e3 / shown.max(1) as f64,
		lag.as_secs_f64() * 1e3,
		lag.as_secs_f64() / frame_time.as_secs_f64().max(1e-9)
	);
}

fn nudge(project: &mut Project, part: bool) {
	if part {
		project.parts[0].base.x += 1;
	} else {
		project.sources[0].x += 1.0;
	}
}

mod ignored_timing {
	use super::*;

	#[test]
	#[ignore = "timing: cargo test --release -p waveduderust --test run -- --ignored ignored_timing:: --nocapture"]
	fn store_write_timing() {
		let mut p = project(1920, 1080, 200);
		p.sources.clear();
		let s = spec(&p, PreviewQuality::FULL);
		let shots: Vec<Snapshot> = (0..4).map(|k| snapshot_of(&s, k as f32)).collect();
		// A capture writes the frame into the store's buffer; this copy stands in for it.
		let write = |st: &FrameStore, shot: &Snapshot| {
			let mut b = st.next_buffer();
			b.field.clear();
			b.field.extend_from_slice(&shot.field);
			b.intensity.clear();
			b.intensity.extend_from_slice(&shot.intensity);
			st.write(b).unwrap();
		};
		let st = store(&s, 4 * GIB);
		std::thread::sleep(Duration::from_millis(500));
		let started = Instant::now();
		let mut slowest = Duration::ZERO;
		for k in 0..100 {
			let t = Instant::now();
			write(&st, &shots[k % 4]);
			slowest = slowest.max(t.elapsed());
		}
		let steady = started.elapsed();
		let st2 = store(&s, 4 * GIB);
		let started = Instant::now();
		for k in 0..100 {
			write(&st2, &shots[k % 4]);
		}
		let cold = started.elapsed();
		let dir = tempfile::tempdir().unwrap();
		let st3 = FrameStore::in_dir(StoreLayout::of(&s), 0, None, dir.path()).unwrap();
		let started = Instant::now();
		for k in 0..100 {
			write(&st3, &shots[k % 4]);
		}
		let disk = started.elapsed();
		let ms = |d: Duration| d.as_secs_f64() * 1e3 / 100.0;
		println!(
			"full-HD both planes f16 ({} MB a frame): {:.2} ms a frame after a pause ({:.2} slowest), {:.2} from a cold start, {:.2} to the file",
			s.frame_bytes() as f64 / 1e6,
			ms(steady),
			slowest.as_secs_f64() * 1e3,
			ms(cold),
			ms(disk)
		);
	}

	/// A burst of edits at 32 a second on the GPU (docs/measurements.md, The live run): frames shown during the
	/// drag and the final frame's lag, with the position's frame alone wanted and with every frame (the frames
	/// before the position captured on the way).
	#[test]
	#[ignore = "timing: cargo test --release -p waveduderust --test run -- --ignored ignored_timing:: --nocapture"]
	fn live_run_drag_burst_timing() {
		for wanted in [Wanted::Position, Wanted::All] {
			drag_burst(wanted, false);
		}
		drag_burst(Wanted::All, true);
	}

	/// Where a part edit's time goes on Example16 at half resolution on the GPU: composing the scene pixels, the
	/// medium from them, the engine taking the new scene.
	#[test]
	#[ignore = "timing: cargo test --release -p waveduderust --test run -- --ignored ignored_timing:: --nocapture"]
	fn part_edit_cost() {
		let path = std::path::Path::new("tests/fixtures/examples/Example16_Wave_in_2_boxes.wdproj");
		let (mut project, view) = waveduderust::io::load_project(path).unwrap();
		let quality = PreviewQuality {
			resolution_divisor: 2,
			frame_divisor: 2,
			format: SampleFormat::Index,
			..PreviewQuality::DEFAULT
		};
		let mut cache = SceneCache::default();
		let first = cache.spec(&project, quality, (&view).into(), view.intensity.preview).unwrap();
		let mut engine = waveduderust::unstable::engine::build(first.scene.clone(), first.options.clone(), true).engine;
		engine.set_emitters(&first.emitters);
		let mut times = [0f64; 3];
		let rounds = 10;
		for _ in 0..rounds {
			project.parts[0].base.x += 1;
			let t = Instant::now();
			let pixels = cache.pixels(&project).unwrap();
			times[0] += t.elapsed().as_secs_f64();
			let t = Instant::now();
			let scene = Arc::new(RunSpec::scene_of(&project, Some(&pixels)).unwrap());
			times[1] += t.elapsed().as_secs_f64();
			let t = Instant::now();
			assert!(engine.try_replace_scene(&scene));
			engine.reset();
			times[2] += t.elapsed().as_secs_f64();
		}
		println!(
			"Example16 part edit, {}x{} canvas: compose {:.1} ms, medium {:.1} ms, scene into the GPU engine {:.1} ms",
			project.scene.width,
			project.scene.height,
			times[0] * 1e3 / rounds as f64,
			times[1] * 1e3 / rounds as f64,
			times[2] * 1e3 / rounds as f64
		);
	}
}
