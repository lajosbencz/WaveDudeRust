//! Runs, the frame store, scratch frames and the memory warning (spec/40, spec/30 Scratch frames, spec/70
//! "Store, frames, quality" and "Probes and graphs"). Canvases are small; every engine is the CPU one.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use wavedude_core::engine::capture::field_index;
use wavedude_core::engine::cpu::CpuEngine;
use wavedude_core::engine::{SampleFormat, Snapshot, WaveEngine};
use wavedude_core::f16;
use wavedude_core::model::{Layer, Measurement, Probe, ProbeKind, Project, Source};
use wavedude_core::run::*;

const GIB: u64 = 1 << 30;

fn project(w: u32, h: u32, steps: u32) -> Project {
	let mut p = Project::default();
	p.scene.width = w;
	p.scene.height = h;
	p.scene.damping_edge = -4;
	p.simulation.steps = steps;
	p.simulation.visualize_every = 2;
	p.sources.push(Source { name: "S".into(), x: f64::from(w / 2), y: f64::from(h / 2), ..Source::default() });
	p
}

fn with_probes(mut p: Project) -> Project {
	let (cx, cy) = (p.scene.width as i32 / 2, p.scene.height as i32 / 2);
	p.probes = vec![
		Probe { name: "amp".into(), ax: cx + 3, ay: cy, ..Probe::default() },
		Probe { name: "int".into(), ax: cx + 5, ay: cy, measurement: Measurement::IntegratedValue, ..Probe::default() },
		Probe {
			name: "line".into(),
			kind: ProbeKind::Line,
			ax: cx - 6,
			ay: cy + 2,
			bx: cx + 6,
			by: cy + 4,
			measurement: Measurement::Value,
			..Probe::default()
		},
		Probe { name: "off".into(), active: false, ..Probe::default() },
	];
	p
}

fn spec(p: &Project, q: PreviewQuality) -> RunSpec {
	RunSpec::create(p, None, q, false).unwrap()
}

fn engine(spec: &RunSpec) -> Box<dyn WaveEngine> {
	Box::new(CpuEngine::new(spec.scene.clone(), spec.options.clone()))
}

fn store(spec: &RunSpec, budget: u64) -> Arc<FrameStore> {
	Arc::new(FrameStore::new(spec.store_layout(), budget, None).unwrap())
}

fn events() -> (Option<EventSink>, Receiver<RunEvent>) {
	let (tx, rx) = channel();
	(Some(Box::new(move |e| drop(tx.send(e)))), rx)
}

fn run(spec: &RunSpec, budget: u64) -> SimulationRun {
	let mut r = SimulationRun::start(spec.clone(), store(spec, budget), engine(spec), None).unwrap();
	assert_eq!(r.wait(), RunState::Completed);
	r
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
	assert_eq!((s.frame_count(), s.stored_frames(), s.capture_every()), (100, 50, 4));
	assert_eq!((s.shape.width(), s.shape.height(), s.shape.plane_bytes()), (32, 24, 32 * 24));
	assert_eq!((s.stored_index_of(98), s.stored_index_of(99)), (49, 49));
	let r = run(&s, GIB);
	assert_eq!(r.store().count(), 50);
	assert_eq!(r.frame_count(), 100);
	assert_eq!(r.store().copy_layer(49, Layer::Field).unwrap().len(), 32 * 24);
	assert_eq!(r.store().step_of(49), 196);
}

#[test]
fn spec_snapshot() {
	let mut p = with_probes(project(64, 48, 21));
	let s = spec(&p, PreviewQuality::FULL);
	assert_eq!(s.frame_count(), 11, "ceil(21 / 2)");
	assert_eq!(s.probes.len(), 3, "inactive probes left out");
	assert_eq!(s.layers(), &[Layer::Field, Layer::Intensity]);
	assert_eq!((s.shape.field_brightness, s.shape.intensity_brightness), (1.0, 1.5));
	assert_eq!((s.scene.grid_width(), s.scene.rim), (72, 4));
	p.display.intensity.preview = false;
	p.probes.clear();
	assert_eq!(spec(&p, PreviewQuality::FULL).layers(), &[Layer::Field]);
	assert_eq!(RunSpec::create(&p, None, PreviewQuality::FULL, true).unwrap().layers().len(), 2);
	p.scene.mode = wavedude_core::model::SceneMode::Image;
	assert_eq!(RunSpec::create(&p, None, PreviewQuality::FULL, false).unwrap_err(), RunSpecError::NoImage);
}

#[test]
fn empty_store_layout() {
	let s = spec(&project(64, 48, 20), PreviewQuality::FULL);
	let st = store(&s, GIB);
	let layout = st.layout();
	assert_eq!((st.capacity(), st.count(), st.layers().len()), (10, 0, 2));
	assert_eq!(st.frame_bytes(), 2 * 64 * 48 * 2);
	assert_eq!(layout.file_bytes(10), 64 + 10 * 2 * 64 * 48 * 2, "SizeOf");
	assert!(matches!(st.copy_layer(0, Layer::Field), Err(StoreError::NotStored { frame: 0, count: 0 })));
	assert!(st.spill_path().is_none());
}

#[test]
fn write_beyond_capacity_and_foreign_snapshots_are_refused() {
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
	let mut p = project(64, 48, 6);
	p.display.intensity.preview = false;
	let field_only = spec(&p, PreviewQuality::FULL);
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
fn tiered_store() {
	let dir = tempfile::tempdir().unwrap();
	let s = spec(&project(40, 30, 20), PreviewQuality::FULL);
	let frame = s.frame_bytes();
	let st = FrameStore::in_dir(s.store_layout(), 4 * frame + frame / 2, None, dir.path()).unwrap();
	assert_eq!((st.capacity(), st.ram_frames(), st.first_spilled()), (10, 4, 4));
	let path = st.spill_path().unwrap().to_path_buf();
	assert!(path.exists());
	assert_eq!(std::fs::metadata(&path).unwrap().len(), s.store_layout().file_bytes(6));
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
	assert!(!path.exists(), "the file goes with the store");

	let st = FrameStore::in_dir(s.store_layout(), 10 * frame, None, dir.path()).unwrap();
	for w in &written {
		st.write(w.clone()).unwrap();
	}
	assert!(st.spill_path().is_none());
	assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "a run within the budget creates no file");

	let st = FrameStore::in_dir(s.store_layout(), 0, None, dir.path()).unwrap();
	st.write(written[3].clone()).unwrap();
	assert_eq!(st.copy_layer(0, Layer::Intensity).unwrap(), written[3].intensity);
	assert_eq!((st.frames_in_memory(), st.frames_on_disk(), st.ram_bytes()), (0, 1, 0));
}

#[test]
fn readers_on_other_threads_see_every_written_frame() {
	let dir = tempfile::tempdir().unwrap();
	let s = spec(&project(40, 30, 200), PreviewQuality::FULL);
	let st = Arc::new(FrameStore::in_dir(s.store_layout(), 30 * s.frame_bytes(), None, dir.path()).unwrap());
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
	for k in 0..s.stored_frames() {
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
	let st = FrameStore::new(s.store_layout(), GIB, Some(release)).unwrap();
	assert!(!released.load(Ordering::Acquire));
	let block = st.next_buffer();
	assert!(released.load(Ordering::Acquire), "the first block came after the release");
	assert_eq!(block.field.len(), s.shape.plane_bytes());
	st.write(block).unwrap();
}

#[test]
fn frames_in_budget() {
	let mut p = project(1920, 1080, 2500);
	p.sources.clear();
	let full = spec(&p, PreviewQuality::FULL);
	assert_eq!(Budget::frames_in(GIB, full.frame_bytes()), 129);
	let reduced = full.with_quality(PreviewQuality::new(2, 1, SampleFormat::Index));
	assert_eq!(Budget::frames_in(GIB, reduced.frame_bytes()), 1035);
	assert_eq!(Budget::frames_in(0, reduced.frame_bytes()), 0);
}

#[test]
fn store_bytes_at_equals_the_store_a_run_allocates() {
	let s = spec(&project(67, 45, 21), PreviewQuality::FULL);
	for res in [1, 2, 4] {
		for frames in [1, 2, 4] {
			for format in [SampleFormat::Half, SampleFormat::Index] {
				let q = PreviewQuality::new(res, frames, format);
				let at = s.with_quality(q);
				let st = store(&at, GIB);
				assert_eq!(s.store_bytes_at(q), (st.capacity() * st.frame_bytes()) as u64, "{q:?}");
				assert_eq!(at.store_bytes(), s.store_bytes_at(q));
			}
		}
	}
}

#[test]
fn index_frames_are_the_palette_index_of_the_half() {
	let s = spec(&project(64, 48, 30), PreviewQuality::FULL);
	let half = run(&s, GIB);
	let index = run(&s.with_quality(PreviewQuality::new(1, 1, SampleFormat::Index)), GIB);
	for k in [3, 14] {
		let h = halves(&half.store().copy_layer(k, Layer::Field).unwrap());
		let x = index.store().copy_layer(k, Layer::Field).unwrap();
		assert_eq!(x, h.iter().map(|&v| field_index(v, 1.0)).collect::<Vec<_>>());
	}
}

// ---- simulation runs ----

#[test]
fn a_run_completes() {
	let s = spec(&project(64, 48, 20), PreviewQuality::FULL);
	let (sink, rx) = events();
	let mut r = SimulationRun::start(s.clone(), store(&s, GIB), engine(&s), sink).unwrap();
	assert_eq!(r.wait(), RunState::Completed);
	assert!(r.is_finished());
	let events: Vec<RunEvent> = rx.try_iter().collect();
	assert_eq!(events.first(), Some(&RunEvent::State(RunState::Running)));
	assert_eq!(events.last(), Some(&RunEvent::State(RunState::Completed)));
	assert_eq!(events.iter().filter(|e| matches!(e, RunEvent::State(RunState::Completed))).count(), 1);
	assert!(events.contains(&RunEvent::Progress { steps_done: 20, frames: 10 }));
	assert_eq!((r.store().count(), r.frame_count(), r.steps_done()), (10, 10, 20));
	let last = halves(&r.store().copy_layer(9, Layer::Field).unwrap());
	let at = |x: usize, y: usize| last[y * 64 + x].to_f32();
	assert!(at(32, 24) != 0.0, "field at the source");
	assert_eq!(at(2, 2), 0.0, "18 steps reach no corner");
	let engine = r.into_engine().unwrap();
	assert_eq!(engine.time(), 20);
}

#[test]
fn pause_resume_and_cancel() {
	let s = spec(&project(96, 72, 4000), PreviewQuality::new(1, 4, SampleFormat::Index));
	let (sink, rx) = events();
	let r = SimulationRun::start(s.clone(), store(&s, GIB), engine(&s), sink).unwrap();
	r.pause();
	assert!(r.is_pause_requested());
	let deadline = Instant::now() + Duration::from_secs(20);
	while !rx.try_iter().any(|e| e == RunEvent::State(RunState::Paused)) {
		assert!(Instant::now() < deadline, "no pause");
		std::thread::sleep(Duration::from_millis(1));
	}
	assert_eq!(r.state(), RunState::Paused);
	let held = r.steps_done();
	std::thread::sleep(Duration::from_millis(60));
	assert_eq!(r.steps_done(), held, "paused runs do not step");
	r.resume();
	while r.steps_done() == held {
		assert!(Instant::now() < deadline, "no resume");
		std::thread::sleep(Duration::from_millis(1));
	}
	r.cancel();
	let mut r = r;
	assert_eq!(r.wait(), RunState::Cancelled);
	assert!(r.steps_done() < 4000);
	assert_eq!(r.frame_count(), s.covered_frames(r.store().count() as u32));
	assert_eq!(rx.try_iter().last(), Some(RunEvent::State(RunState::Cancelled)));
}

#[test]
fn probe_traces_of_a_run() {
	let s = spec(&with_probes(project(64, 48, 100)), PreviewQuality::FULL);
	let r = run(&s, GIB);
	let t = r.traces();
	assert_eq!(t.len(), 3);
	assert_eq!(t[0].series.len(), 100, "one sample per step");
	assert_eq!(t[1].series.len(), 100);
	assert!(t[1].series.iter().all(|&v| v >= 0.0) && t[1].series[99] > 0.0);
	assert!(t[0].profiles.is_empty());
	assert_eq!(t[2].profiles.len(), 50, "one profile per timeline frame");
	assert_eq!(t[2].profiles.first().map(<[f32]>::len), Some(13));
	assert!((t[2].length - 12f64.hypot(2.0)).abs() < 1e-12);
	// Graph data at frame 20 / step 40.
	assert_eq!(t[0].series[..=s.step_of(20) as usize].len(), 41);
	assert_eq!((t[0].steps_sampled, t[2].steps_sampled), (100, 99), "the last frame closes at step index 98");
}

// ---- scratch frames ----

fn scratch() -> (ScratchRunner, Receiver<ScratchEvent>) {
	let (tx, rx) = channel();
	(ScratchRunner::cpu(Box::new(move |e| drop(tx.send(e)))).unwrap(), rx)
}

fn done(rx: &Receiver<ScratchEvent>) -> ScratchResult {
	loop {
		match rx.recv_timeout(Duration::from_secs(60)).unwrap() {
			ScratchEvent::Done(r) => return *r,
			ScratchEvent::Failed { error, .. } => panic!("{error}"),
			ScratchEvent::Abandoned { .. } => {}
		}
	}
}

#[test]
fn scratch_frame_equals_the_run_frame() {
	let s = spec(&with_probes(project(64, 48, 100)), PreviewQuality::FULL);
	let r = run(&s, GIB);
	let run_traces = r.traces();
	let (runner, rx) = scratch();
	let id = runner.request(s.clone(), 20);
	let first = done(&rx);
	assert_eq!((first.id, first.frame, first.snapshot.step), (id, 20, 40));
	assert_eq!(first.snapshot.field, r.store().copy_layer(20, Layer::Field).unwrap());
	assert_eq!(first.snapshot.intensity, r.store().copy_layer(20, Layer::Intensity).unwrap());
	assert_eq!(first.traces[0].series, run_traces[0].series[..41]);
	assert_eq!(first.traces[1].series, run_traces[1].series[..41]);
	assert_eq!(first.traces[2].profiles.iter().collect::<Vec<_>>(), [run_traces[2].profiles.get(20).unwrap()]);

	// An earlier frame from the kept engine: the integrated sums start again.
	runner.request(s.clone(), 7);
	let second = done(&rx);
	assert_eq!(second.snapshot.field, r.store().copy_layer(7, Layer::Field).unwrap());
	assert_eq!(second.traces[1].series, run_traces[1].series[..15]);

	// Another scene of the same grid swaps into the kept engine.
	let mut p = with_probes(project(64, 48, 100));
	p.scene.refractive_index = 1.5;
	let other = spec(&p, PreviewQuality::FULL);
	runner.request(other.clone(), 5);
	let third = done(&rx);
	let r2 = run(&other, GIB);
	assert_eq!(third.snapshot.field, r2.store().copy_layer(5, Layer::Field).unwrap());
	assert_ne!(third.snapshot.field, r.store().copy_layer(5, Layer::Field).unwrap());
}

#[test]
fn scratch_frames_step_on_and_come_back_from_the_cache() {
	let s = spec(&with_probes(project(64, 48, 100)), PreviewQuality::FULL);
	let r = run(&s, GIB);
	let run_traces = r.traces();
	let (runner, rx) = scratch();
	runner.request(s.clone(), 10);
	let first = done(&rx);
	runner.request(s.clone(), 30);
	let later = done(&rx);
	assert_eq!(later.snapshot.field, r.store().copy_layer(30, Layer::Field).unwrap());
	assert_eq!(later.snapshot.intensity, r.store().copy_layer(30, Layer::Intensity).unwrap());
	assert_eq!(later.traces[0].series, run_traces[0].series[..61]);
	assert_eq!(later.traces[1].series, run_traces[1].series[..61]);
	assert_eq!(later.traces[2].profiles.iter().collect::<Vec<_>>(), [run_traces[2].profiles.get(30).unwrap()]);

	let id = runner.request(s.clone(), 10);
	let cached = done(&rx);
	assert_eq!((cached.id, cached.frame), (id, 10));
	assert_eq!(cached.snapshot.field, first.snapshot.field);
	assert_eq!(cached.traces, first.traces);
}

#[test]
fn the_scratch_cache_answers_by_step_not_by_frame() {
	let mut p = with_probes(project(64, 48, 100));
	let every_2 = spec(&p, PreviewQuality::FULL);
	p.simulation.visualize_every = 4;
	let every_4 = spec(&p, PreviewQuality::FULL);
	let (runner, rx) = scratch();
	runner.request(every_2.clone(), 10);
	let step_20 = done(&rx);
	runner.request(every_4.clone(), 10);
	let step_40 = done(&rx);
	assert_eq!((step_20.snapshot.step, step_40.snapshot.step), (20, 40));
	let (fresh, fresh_rx) = scratch();
	fresh.request(every_4.clone(), 10);
	assert_eq!(step_40.snapshot.field, done(&fresh_rx).snapshot.field);
	runner.request(every_4, 5);
	let again = done(&rx);
	assert_eq!((again.frame, again.snapshot.step), (5, 20));
	assert_eq!(again.snapshot.field, step_20.snapshot.field);
}

#[test]
fn a_superseded_scratch_request_is_abandoned() {
	let s = spec(&project(256, 256, 100), PreviewQuality::FULL);
	let (runner, rx) = scratch();
	let slow = runner.request(s.clone(), 20_000);
	std::thread::sleep(Duration::from_millis(150));
	let fast = runner.request(s.clone(), 1);
	let mut abandoned = None;
	let result = loop {
		match rx.recv_timeout(Duration::from_secs(60)).unwrap() {
			ScratchEvent::Abandoned { id, steps_done, steps, .. } => abandoned = Some((id, steps_done, steps)),
			ScratchEvent::Done(r) => break r,
			ScratchEvent::Failed { error, .. } => panic!("{error}"),
		}
	};
	let (id, steps_done, steps) = abandoned.expect("the slow request was abandoned");
	assert_eq!((id, steps), (slow, 40_001));
	assert!(steps_done > 0 && steps_done < steps && steps_done % CHECK_INTERVAL == 0, "{steps_done}");
	assert_eq!((result.id, result.snapshot.step), (fast, 2));
}

// ---- memory warning ----

#[test]
fn memory_warning_levers_and_proposal() {
	let mut p = project(1920, 1080, 2500);
	p.sources.clear();
	let s = spec(&p, PreviewQuality::FULL);
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

// ---- timing (release build: cargo test -p wavedude-core --release --test run -- --ignored --nocapture) ----

#[test]
#[ignore]
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
	let st3 = FrameStore::in_dir(s.store_layout(), 0, None, dir.path()).unwrap();
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

#[test]
fn specs_of_an_unchanged_scene_share_its_medium() {
	let mut p = project(64, 32, 20);
	let mut cache = SceneCache::default();
	let a = cache.spec(&p, PreviewQuality::FULL, false).unwrap();
	p.seed += 1;
	p.sources[0].frequency += 1.;
	let b = cache.spec(&p, PreviewQuality::DEFAULT, false).unwrap();
	assert!(Arc::ptr_eq(&a.scene, &b.scene), "seed, sources and quality leave the medium");
	p.scene.refractive_index = 1.5;
	let c = cache.spec(&p, PreviewQuality::FULL, false).unwrap();
	assert!(!Arc::ptr_eq(&a.scene, &c.scene));
	assert_eq!(*c.scene, RunSpec::scene_of(&p, None).unwrap());
	p.scene.edges = wavedude_core::model::Edges::Reflective;
	let d = cache.spec(&p, PreviewQuality::FULL, false).unwrap();
	assert_eq!(d.scene.edges, wavedude_core::model::Edges::Reflective, "the edges rebuild it too");
}
