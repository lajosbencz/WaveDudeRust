#![cfg(not(target_arch = "wasm32"))]
//! The stepping facade agrees with an engine driven by hand.

use waveduderust::model::normalize::Normalize;
use waveduderust::model::{Layer, Measurement, Probe, ProbeKind, Project, Source};
use waveduderust::quality::{Brightness, PreviewQuality};
use waveduderust::unstable::engine::probes::{ProbeCollector, record};
use waveduderust::unstable::engine::{self, WaveEngine};
use waveduderust::unstable::spec::RunSpec;
use waveduderust::{Backend, Simulation, SimulationError};

fn project() -> Project {
	let mut p = Project::default();
	p.scene.width = 48;
	p.scene.height = 32;
	p.scene.damping_edge = -4;
	p.simulation.steps = 60;
	p.simulation.visualize_every = 3;
	p.sources.push(Source::default().with_name("S".into()).with_x(24.0).with_y(16.0));
	p.probes = vec![
		Probe::default().with_name("amp".into()).with_ax(28).with_ay(16),
		Probe::default()
			.with_name("int".into())
			.with_ax(30)
			.with_ay(16)
			.with_layer(Layer::Intensity)
			.with_measurement(Measurement::IntegratedValue),
		Probe::default()
			.with_name("line".into())
			.with_kind(ProbeKind::Line)
			.with_ax(18)
			.with_ay(20)
			.with_bx(30)
			.with_by(22)
			.with_layer(Layer::Intensity),
		Probe::default().with_name("off".into()).with_active(false),
	];
	p.normalize();
	p
}

/// A simulation of `p` as a default view shows it, which previews the intensity.
fn cpu(p: &Project) -> Simulation {
	Simulation::builder(p).backend(Backend::Cpu).track_intensity(true).build().unwrap()
}

#[test]
fn field_and_traces_match_a_hand_driven_engine() {
	let p = project();
	let spec = RunSpec::create(&p, None, PreviewQuality::FULL, Brightness::default(), true).unwrap();
	// The engine `Backend::Cpu` builds: the CPU engine this build and this CPU default to.
	let mut e: Box<dyn WaveEngine> = engine::build(spec.scene.clone(), spec.options.clone(), false).engine;
	let mut collector = ProbeCollector::new(&spec.probes, &spec.scene, spec.seed);
	e.set_emitters(&spec.emitters);
	e.set_probes(collector.cells());
	e.reset();
	let mut recorded = 0;
	for _ in 0..40 {
		e.advance_one_step().unwrap();
		e.queue_probes().unwrap();
		let pending = e.pending_probes().unwrap();
		record(e.as_mut(), pending, &mut collector, &mut recorded, spec.schedule.visualize_every).unwrap();
	}

	let mut sim = cpu(&p);
	sim.run(25).unwrap();
	let early = sim.traces().unwrap().map(|(_, t)| t.series.len()).collect::<Vec<_>>();
	assert_eq!(early, [25, 25, 0], "the line probe records profiles, not a series");
	sim.run(15).unwrap();

	assert_eq!(sim.time(), 40);
	assert!(!sim.is_gpu() && sim.fallback().is_none());
	let field = sim.field().unwrap();
	assert_eq!((field.width, field.height), (48, 32));
	assert_eq!(field.field, e.prepare_frame().unwrap().0);
	assert!(field.field.iter().any(|&v| v != 0.0));
	let names = sim.traces().unwrap().map(|(p, _)| p.name.clone()).collect::<Vec<_>>();
	assert_eq!(names, ["amp", "int", "line"]);
	let traces = sim.traces().unwrap().map(|(_, t)| t.clone()).collect::<Vec<_>>();
	assert_eq!(traces, collector.traces, "reading the traces twice records nothing twice");
}

#[test]
fn a_run_past_the_drains_keeps_every_reading() {
	let mut p = project();
	p.simulation.steps = 600;
	p.normalize();
	let mut sim = cpu(&p);
	sim.run(600).unwrap();
	let lens = sim.traces().unwrap().map(|(_, t)| (t.series.len(), t.profiles.len())).collect::<Vec<_>>();
	assert_eq!(lens, [(600, 0), (600, 0), (0, 200)]);
}

/// `run` hands the engine batches it may fuse (`WaveEngine::advance_steps`), each ending at a drain of the probe
/// readings: the traces are those of one step at a time, every reading kept, within the rounding a fused sweep of a
/// faster backend than the generic one may change (tests/rev2.rs; bit for bit there and in the run worker's tests).
#[test]
fn runs_in_batches_step_as_single_steps() {
	let mut p = project();
	p.simulation.steps = 600;
	p.simulation.stencil = waveduderust::model::Stencil::NinePoint;
	p.normalize();
	let mut batched = cpu(&p);
	for steps in [7, 250, 343] {
		batched.run(steps).unwrap();
	}
	let mut single = cpu(&p);
	for _ in 0..600 {
		single.step().unwrap();
	}
	assert_eq!((batched.time(), single.time()), (600, 600));
	let traces = |sim: &mut Simulation| sim.traces().unwrap().map(|(_, t)| t.clone()).collect::<Vec<_>>();
	let (a, b) = (traces(&mut batched), traces(&mut single));
	for (a, b) in a.iter().zip(&b) {
		assert_eq!(
			(a.series.len(), a.profiles.len(), a.steps_sampled),
			(b.series.len(), b.profiles.len(), b.steps_sampled)
		);
		let peak = b.series.iter().fold(0f32, |m, v| m.max(v.abs()));
		let diff = a.series.iter().zip(&b.series).fold(0f32, |m, (x, y)| m.max((x - y).abs()));
		assert!(diff <= 1e-4 * peak, "the batched trace differs by {diff} of {peak}");
	}
	let (fa, fb) = (batched.field().unwrap().field, single.field().unwrap().field);
	let peak = fb.iter().fold(0f32, |m, v| m.max(v.abs()));
	assert!(peak > 0.0 && fa.iter().zip(&fb).all(|(x, y)| (x - y).abs() <= 1e-4 * peak));
}

#[test]
fn the_intensity_is_there_only_when_tracked() {
	let mut p = project();
	p.probes.clear();
	let mut plain = Simulation::builder(&p).backend(Backend::Cpu).build().unwrap();
	plain.run(5).unwrap();
	assert!(plain.field().unwrap().intensity.is_none());
	assert_eq!(plain.traces().unwrap().len(), 0);

	let mut tracked = Simulation::builder(&p).backend(Backend::Cpu).track_intensity(true).build().unwrap();
	tracked.run(30).unwrap();
	let view = tracked.field().unwrap();
	assert!(view.intensity.is_some_and(|i| i.len() == 48 * 32 && i.iter().any(|&v| v > 0.0)));
}

#[test]
fn the_builder_snapshots_and_normalises_the_project() {
	let mut p = project();
	p.simulation.steps = 0;
	let builder = Simulation::builder(&p);
	p.scene.width = 4000;
	let sim = builder.backend(Backend::Cpu).build().unwrap();
	assert!(sim.steps() >= 1, "normalised");
	let mut sim = sim;
	assert_eq!(sim.field().unwrap().width, 48);
}

#[test]
fn an_image_scene_without_its_image_is_refused() {
	let mut p = project();
	p.scene.mode = waveduderust::model::SceneMode::Image;
	p.scene.image_data = None;
	let e = Simulation::builder(&p).backend(Backend::Cpu).build().err();
	assert!(matches!(e, Some(SimulationError::Spec(_))), "{e:?}");
}

#[cfg(feature = "run")]
mod background {
	use waveduderust::run::{RunOptions, Stage};

	use super::*;

	fn options(dir: &tempfile::TempDir) -> RunOptions {
		RunOptions::default().quality(PreviewQuality::FULL).cache_dir(dir.path())
	}

	#[test]
	fn a_run_ends_with_the_traces_of_a_stepped_simulation() {
		let p = project();
		let dir = tempfile::tempdir().unwrap();
		let run = Simulation::builder(&p).backend(Backend::Cpu).track_intensity(true).spawn(options(&dir)).unwrap();
		let end = run.wait().unwrap();
		assert!(matches!(end.stage, Stage::Completed), "{:?}", end.stage);
		assert_eq!((end.steps_done, end.steps), (60, 60));
		assert_eq!(end.frames, run.schedule().frame_count());
		assert_eq!(run.wait_for_store().unwrap().count() as u32, run.schedule().stored_frames());

		let mut sim = cpu(&p);
		sim.run(60).unwrap();
		let stepped = sim.traces().unwrap().map(|(_, t)| t.clone()).collect::<Vec<_>>();
		assert_eq!(run.traces(), stepped);
		assert_eq!(run.probes().len(), 3);
	}

	/// Timeline frame `k` of a run against the stepped simulation after the same steps, the largest gap of each layer.
	fn gaps(quality: PreviewQuality, k: u32) -> (f32, f32, f32) {
		let p = project();
		let dir = tempfile::tempdir().unwrap();
		let opts = RunOptions::default().quality(quality).cache_dir(dir.path());
		let run = Simulation::builder(&p).backend(Backend::Cpu).track_intensity(true).spawn(opts).unwrap();
		run.wait().unwrap();
		let frames = run.wait_for_frames().unwrap();
		assert_eq!((frames.len(), frames.total()), (20, 20));
		assert!(frames.get(frames.total()).unwrap().is_none());
		let frame = frames.get(k).unwrap().unwrap();

		let mut sim = cpu(&p);
		sim.run(frames.steps_at(k)).unwrap();
		let want = sim.field().unwrap();
		assert_eq!((frame.width, frame.height), (want.width, want.height));
		let gap = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max);
		let peak = want.field.iter().fold(0.0f32, |m, v| m.max(v.abs()));
		(gap(&frame.field, &want.field), gap(&frame.intensity.unwrap(), &want.intensity.unwrap()), peak)
	}

	#[test]
	fn full_quality_frames_are_the_field_after_their_steps() {
		let (field, intensity, peak) = gaps(PreviewQuality::FULL, 13);
		assert!(peak > 0.01, "the wave has reached the canvas: {peak}");
		assert!(field <= peak / 1000.0 && intensity <= 1e-3, "half precision: {field} {intensity}");
	}

	#[test]
	fn index_frames_are_the_field_within_a_level() {
		let index = PreviewQuality { format: engine::SampleFormat::Index, ..PreviewQuality::FULL };
		let (field, _, _) = gaps(index, 13);
		let brightness = Brightness::default().field as f32;
		// a level of the truncating quantiser, the half rounding before it
		assert!(field <= 1.01 / 127.0 / brightness, "{field}");
	}

	#[test]
	fn a_frame_between_stored_ones_is_the_stored_frame_before_it() {
		let quality = PreviewQuality { frame_divisor: 2, ..PreviewQuality::FULL };
		let dir = tempfile::tempdir().unwrap();
		let run = Simulation::builder(&project())
			.backend(Backend::Cpu)
			.track_intensity(true)
			.spawn(options(&dir).quality(quality))
			.unwrap();
		run.wait().unwrap();
		let frames = run.wait_for_frames().unwrap();
		assert_eq!(frames.steps_at(5), frames.steps_at(4));
		assert_eq!(frames.get(5).unwrap(), frames.get(4).unwrap());
	}

	#[test]
	fn a_cancelled_run_ends_its_waits() {
		let mut p = project();
		p.simulation.steps = 1_000_000;
		p.normalize();
		let dir = tempfile::tempdir().unwrap();
		let run = Simulation::builder(&p).backend(Backend::Cpu).track_intensity(true).spawn(options(&dir)).unwrap();
		let canceller = run.canceller();
		let started = run.wait_until(|p| p.steps_done > 0);
		assert!(!started.ended());
		std::thread::spawn(move || canceller.cancel());
		let end = run.wait().unwrap();
		assert!(matches!(end.stage, Stage::Cancelled), "{:?}", end.stage);
		assert!(end.steps_done < end.steps);
	}

	#[test]
	fn a_run_of_an_image_scene_without_its_image_is_refused() {
		let mut p = project();
		p.scene.mode = waveduderust::model::SceneMode::Image;
		let dir = tempfile::tempdir().unwrap();
		let e = Simulation::builder(&p).backend(Backend::Cpu).track_intensity(true).spawn(options(&dir)).err();
		assert!(matches!(e, Some(SimulationError::Spec(_))), "{e:?}");
	}
}
