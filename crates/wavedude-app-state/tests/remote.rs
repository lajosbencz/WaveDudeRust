//! The live run across a port: the page's `Remote` against the worker's `Host` on the CPU engine (decision 0024).

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use wavedude_app_state::live::{Backend, LiveController, LiveSettings, Notice, engine_label};
use wavedude_app_state::remote::protocol::{Layout, RunStatus, ToPage, ToWorker};
use wavedude_app_state::remote::{Envelope, Host, Loopback, Port, Remote};
use wavedude_core::engine::cpu::CpuEngine;
use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::engine::{GpuHandle, WaveEngine};
use wavedude_core::model::{Layer, Probe, ProbeKind, Project, SceneMode, Source};
use wavedude_core::quality::PreviewQuality;
use wavedude_core::run::{
	EngineFuture, FrameStore, Goal, PumpedFactory, RunError, RunWorker, SpillBackend, SpillFactory, StoreError,
	StoreLayout, Token, WRITES_IN_FLIGHT, Wanted, WorkState,
};
use wavedude_core::spec::{RunRequest, RunSpec, RunSpecError};
use wavedude_core::sync::lock;

const GIB: u64 = 1 << 30;
const SLICE: Duration = Duration::from_millis(10);

fn project(steps: u32) -> Project {
	let mut p = Project::default();
	p.scene.width = 64;
	p.scene.height = 48;
	p.scene.damping_edge = -4;
	p.simulation.steps = steps;
	p.simulation.visualize_every = 2;
	p.sources.push(Source { name: "S".into(), x: 32.0, y: 24.0, ..Source::default() });
	p
}

fn with_probes(mut p: Project) -> Project {
	p.probes = vec![
		Probe { name: "amp".into(), ax: 35, ay: 24, ..Probe::default() },
		Probe { name: "line".into(), kind: ProbeKind::Line, ax: 26, ay: 26, bx: 38, by: 28, ..Probe::default() },
	];
	p
}

fn goal(p: &Project, revision: u64, wanted: Wanted) -> Goal {
	let run = RunRequest::new(p, PreviewQuality::FULL, false);
	Goal { revision, run: Some(run), wanted, ..Goal::default() }
}

fn connect(frame_memory: u64, spill: Box<dyn SpillFactory>) -> (Remote<Loopback>, Host<Loopback>) {
	connect_on(frame_memory, spill, false, Err("no GPU in the tests".into()))
}

fn connect_on(
	frame_memory: u64,
	spill: Box<dyn SpillFactory>,
	use_gpu: bool,
	gpu: Result<GpuHandle, String>,
) -> (Remote<Loopback>, Host<Loopback>) {
	let (page, worker) = Loopback::pair();
	let remote = Remote::new(page, Arc::new(AtomicBool::new(use_gpu)), frame_memory, spill);
	(remote, Host::new(worker, gpu))
}

/// Pumps both ends until `done` holds (within a minute).
fn drive(remote: &mut Remote<Loopback>, host: &mut Host<Loopback>, mut done: impl FnMut(&Remote<Loopback>) -> bool) {
	let deadline = Instant::now() + Duration::from_secs(60);
	loop {
		host.pump(SLICE);
		remote.pump();
		if done(remote) {
			return;
		}
		assert!(Instant::now() < deadline, "the run did not get there: {:?}", remote.status().state);
	}
}

/// The run of `revision` is idle and its store complete.
fn finished(revision: u64) -> impl FnMut(&Remote<Loopback>) -> bool {
	move |r| {
		let s = r.status();
		s.adopted == revision && s.state == WorkState::Idle && s.store.is_some_and(|s| s.is_complete())
	}
}

/// The same goal run directly on a pumped worker in memory.
fn direct(p: &Project, wanted: Wanted) -> RunWorker {
	let start: PumpedFactory = Box::new(|scene, options| -> EngineFuture {
		Box::pin(async move { Box::new(CpuEngine::new(scene, options)) as Box<dyn WaveEngine> })
	});
	let w = RunWorker::pumped(start, GIB, Box::new(Spill::default()), Arc::default());
	w.set_goal(goal(p, 1, wanted));
	while w.pump(SLICE) {}
	w
}

/// Plane `layer` of frame `k`, its spilled read completed by `disk` while pending.
fn plane(store: &FrameStore, k: usize, layer: Layer, disk: &Arc<Mutex<Disk>>) -> Vec<u8> {
	loop {
		match store.copy_layer(k, layer) {
			Err(StoreError::Pending { .. }) => lock(disk).complete(),
			plane => return plane.unwrap(),
		}
	}
}

fn same_frames(mirror: &FrameStore, direct: &FrameStore, disk: &Arc<Mutex<Disk>>) {
	assert_eq!((mirror.capacity(), mirror.layers()), (direct.capacity(), direct.layers()));
	for k in 0..direct.capacity() {
		for &layer in direct.layers() {
			assert_eq!(plane(mirror, k, layer, disk), direct.copy_layer(k, layer).unwrap(), "{layer:?} of frame {k}");
		}
	}
}

fn traces(b: &impl Backend) -> Vec<ProbeTrace> {
	let mut out = Vec::new();
	b.with_traces(&mut |t| out = t.to_vec());
	out
}

/// (store, generation, frame).
type Key = (u64, u64, usize);

/// Frames and reads that complete when the test says; the writes of the `stuck` (store, generation)s never do.
#[derive(Default)]
struct Disk {
	frames: HashMap<Key, Vec<u8>>,
	writes: Vec<(Key, Vec<u8>, Token<()>)>,
	reads: Vec<(Key, Token<Vec<u8>>)>,
	stuck: Vec<(u64, u64)>,
}

impl Disk {
	fn complete(&mut self) {
		let stuck = std::mem::take(&mut self.stuck);
		let (held, done): (Vec<_>, Vec<_>) =
			self.writes.drain(..).partition(|((store, generation, _), ..)| stuck.contains(&(*store, *generation)));
		for (key, planes, write) in done {
			self.frames.insert(key, planes);
			write.complete(Ok(()));
		}
		(self.writes, self.stuck) = (held, stuck);
		for (key, read) in self.reads.drain(..) {
			read.complete(self.frames.get(&key).cloned().ok_or_else(|| io::ErrorKind::NotFound.into()));
		}
	}
}

/// Spills into one [`Disk`]; ready while `ready`, refused with `refused`.
#[derive(Clone)]
struct Spill {
	disk: Arc<Mutex<Disk>>,
	ready: Arc<AtomicBool>,
	refused: Option<String>,
}

impl Default for Spill {
	fn default() -> Self {
		Self { disk: Arc::default(), ready: Arc::new(AtomicBool::new(true)), refused: None }
	}
}

impl SpillFactory for Spill {
	fn ready(&self) -> bool {
		self.ready.load(Ordering::Relaxed)
	}

	fn open(&self, store: u64, _layout: &StoreLayout, _bytes: u64) -> io::Result<Box<dyn SpillBackend>> {
		match &self.refused {
			Some(why) => Err(io::Error::new(io::ErrorKind::StorageFull, why.clone())),
			None => Ok(Box::new(Spilled { disk: self.disk.clone(), store })),
		}
	}
}

struct Spilled {
	disk: Arc<Mutex<Disk>>,
	store: u64,
}

impl SpillBackend for Spilled {
	fn write(&self, generation: u64, frame: usize, planes: &[&[u8]]) -> Token<()> {
		let write = Token::default();
		lock(&self.disk).writes.push(((self.store, generation, frame), planes.concat(), write.clone()));
		write
	}

	fn read(&self, generation: u64, frame: usize) -> Token<Vec<u8>> {
		let read = Token::default();
		lock(&self.disk).reads.push(((self.store, generation, frame), read.clone()));
		read
	}

	fn drop_generation(&self, generation: u64) {
		lock(&self.disk).frames.retain(|(s, g, _), _| (*s, *g) != (self.store, generation));
	}
}

#[test]
fn the_frames_and_traces_reach_the_page_as_a_direct_run_makes_them() {
	let p = with_probes(project(40));
	let (mut remote, mut host) = connect(GIB, Box::new(Spill::default()));
	remote.set_goal(goal(&p, 1, Wanted::All));
	drive(&mut remote, &mut host, finished(1));
	let w = direct(&p, Wanted::All);
	same_frames(&remote.status().store.unwrap(), &w.store().unwrap(), &Arc::default());
	assert_eq!(traces(&remote), w.with_traces(<[ProbeTrace]>::to_vec));
	assert_eq!(remote.status().steps_done, w.status().steps_done);
	host.pump(SLICE);
	assert_eq!(host.frames_in_flight(), 0, "every frame acknowledged");
}

#[test]
fn a_mirror_past_its_memory_spills_and_acknowledges_a_frame_once_written() {
	let p = project(60);
	let layout = StoreLayout::of(&RunSpec::create(&p, None, PreviewQuality::FULL, false).unwrap());
	let spill = Spill::default();
	let disk = spill.disk.clone();
	let (mut remote, mut host) = connect(2 * layout.frame_bytes() as u64, Box::new(spill));
	remote.set_goal(goal(&p, 1, Wanted::All));
	for _ in 0..50 {
		host.pump(SLICE);
		remote.pump();
		assert!(host.frames_in_flight() <= WRITES_IN_FLIGHT);
	}
	assert_eq!(lock(&disk).writes.len(), WRITES_IN_FLIGHT, "the frames in memory are acknowledged at once");
	assert_eq!(host.frames_in_flight(), WRITES_IN_FLIGHT, "the spilled ones once written");
	let mut pumps = 0;
	drive(&mut remote, &mut host, |r| {
		pumps += 1;
		if pumps % 3 == 0 {
			lock(&disk).complete();
		}
		finished(1)(r)
	});
	while host.frames_in_flight() > 0 {
		lock(&disk).complete();
		remote.pump();
		host.pump(SLICE);
	}
	let mirror = remote.status().store.unwrap();
	assert_eq!((mirror.frames_in_memory(), mirror.frames_on_disk()), (2, layout.capacity - 2));
	same_frames(&mirror, &direct(&p, Wanted::All).store().unwrap(), &disk);
}

#[test]
fn the_worker_is_never_more_than_the_spills_writes_ahead_of_the_page() {
	let (mut remote, mut host) = connect(GIB, Box::new(Spill::default()));
	remote.set_goal(goal(&project(60), 1, Wanted::All));
	for _ in 0..100 {
		host.pump(SLICE);
		assert!(host.frames_in_flight() <= WRITES_IN_FLIGHT);
	}
	assert_eq!(host.frames_in_flight(), WRITES_IN_FLIGHT, "without acknowledgements the run waits");
	drive(&mut remote, &mut host, finished(1));
}

#[test]
fn a_new_generation_clears_the_mirror_and_a_discard_replaces_it() {
	let p = with_probes(project(40));
	let disk = Arc::default();
	let (mut remote, mut host) = connect(GIB, Box::new(Spill::default()));
	remote.set_goal(goal(&p, 1, Wanted::All));
	drive(&mut remote, &mut host, finished(1));
	let first = remote.status().store.unwrap();
	let generation = first.generation();
	let before = traces(&remote);

	let mut q = p.clone();
	q.sources[0].frequency += 1.0;
	remote.set_goal(goal(&q, 2, Wanted::All));
	drive(&mut remote, &mut host, finished(2));
	let store = remote.status().store.unwrap();
	assert!(Arc::ptr_eq(&store, &first) && store.generation() > generation, "the same mirror, cleared");
	assert!(store.latest().is_some(), "the frame written last is kept through the clear");
	let mut held = Vec::new();
	remote.with_held_traces(&mut |t| held = t.to_vec());
	assert_eq!(held, before, "the traces of the frame kept");
	let w = direct(&q, Wanted::All);
	same_frames(&store, &w.store().unwrap(), &disk);
	assert_eq!(traces(&remote), w.with_traces(<[ProbeTrace]>::to_vec));

	remote.discard_frames();
	remote.set_goal(goal(&q, 3, Wanted::All));
	drive(&mut remote, &mut host, finished(3));
	let store = remote.status().store.unwrap();
	assert!(!Arc::ptr_eq(&store, &first), "the frames went with their mirror");
	same_frames(&store, &w.store().unwrap(), &disk);
	remote.with_held_traces(&mut |t| assert!(t.is_empty()));
	assert_eq!(traces(&remote), w.with_traces(<[ProbeTrace]>::to_vec));
}

#[test]
fn a_scene_without_its_image_fails_as_one_and_an_image_crosses_with_its_goal() {
	let mut p = project(20);
	p.scene.mode = SceneMode::Image;
	let (remote, mut host) = connect(GIB, Box::new(Spill::default()));
	let mut c = LiveController::new(remote, LiveSettings::default());
	let t = Instant::now();
	c.project_replaced(&p, t);
	let deadline = Instant::now() + Duration::from_secs(60);
	let failed = loop {
		c.tick(Instant::now(), &p);
		host.pump(SLICE);
		if let Some(n) = c.take_notices().into_iter().find(|n| matches!(n, Notice::RunFailed { .. })) {
			break n;
		}
		assert!(Instant::now() < deadline, "no failure");
	};
	assert!(matches!(failed, Notice::RunFailed { no_image: true, .. }), "{failed:?}");

	let mut png = Vec::new();
	let mut encoder = png::Encoder::new(&mut png, 32, 24);
	encoder.set_color(png::ColorType::Rgba);
	encoder.set_depth(png::BitDepth::Eight);
	encoder.write_header().unwrap().write_image_data(&[200; 32 * 24 * 4]).unwrap();
	p.scene.image_data = Some(png.into());
	p.scene.image_size = (32, 24);
	let (mut remote, mut host) = connect(GIB, Box::new(Spill::default()));
	remote.set_goal(goal(&p, 1, Wanted::All));
	drive(&mut remote, &mut host, finished(1));
	assert_eq!(remote.status().store.unwrap().shape().canvas_width, 32, "the image's canvas");
}

#[test]
fn the_engine_built_and_why_the_gpu_fell_back_reach_the_page() {
	let (mut remote, mut host) = connect_on(GIB, Box::new(Spill::default()), true, Err("no adapter here".into()));
	remote.set_goal(goal(&project(20), 1, Wanted::Position));
	drive(&mut remote, &mut host, |r| r.fallback().is_some());
	assert_eq!(remote.fallback(), Some("no adapter here"));
	assert_eq!(remote.engine_name(), engine_label(None));
}

/// Pumps both ends `n` times, the page's spill writes left in flight.
fn stall(remote: &mut Remote<Loopback>, host: &mut Host<Loopback>, n: usize) {
	for _ in 0..n {
		host.pump(SLICE);
		remote.pump();
		assert!(host.frames_in_flight() <= WRITES_IN_FLIGHT);
	}
}

/// The page's spill writes of the mirror's generation in force never complete.
fn stick(remote: &Remote<Loopback>, disk: &Arc<Mutex<Disk>>) -> u64 {
	let store = remote.status().store.unwrap();
	lock(disk).stuck.push((store.id(), store.generation()));
	store.id()
}

#[test]
fn spill_writes_stuck_in_a_generation_or_store_left_do_not_hold_the_worker() {
	let p = project(60);
	let layout = StoreLayout::of(&RunSpec::create(&p, None, PreviewQuality::FULL, false).unwrap());
	let spill = Spill::default();
	let disk = spill.disk.clone();
	let (mut remote, mut host) = connect(2 * layout.frame_bytes() as u64, Box::new(spill));
	remote.set_goal(goal(&p, 1, Wanted::All));
	stall(&mut remote, &mut host, 50);
	assert_eq!(host.frames_in_flight(), WRITES_IN_FLIGHT);
	let first = stick(&remote, &disk);

	remote.discard_frames();
	remote.set_goal(goal(&p, 2, Wanted::All));
	stall(&mut remote, &mut host, 50);
	assert_eq!(host.frames_in_flight(), WRITES_IN_FLIGHT, "the store left is not waited for");
	assert_ne!(stick(&remote, &disk), first, "a new mirror");

	let mut q = p.clone();
	q.sources[0].frequency += 1.0;
	remote.set_goal(goal(&q, 3, Wanted::All));
	drive(&mut remote, &mut host, |r| {
		lock(&disk).complete();
		finished(3)(r)
	});
	for _ in 0..100 {
		if host.frames_in_flight() == 0 {
			break;
		}
		lock(&disk).complete();
		remote.pump();
		host.pump(SLICE);
	}
	assert_eq!(host.frames_in_flight(), 0, "every frame acknowledged or released");
	assert_eq!(lock(&disk).writes.len(), 2 * WRITES_IN_FLIGHT, "the stuck writes are still in flight");
	same_frames(&remote.status().store.unwrap(), &direct(&q, Wanted::All).store().unwrap(), &disk);
}

#[test]
fn frames_the_page_cannot_keep_fail_the_run_with_the_pages_reason() {
	let spill = Spill { refused: Some("no room for frames".into()), ..Spill::default() };
	let (mut remote, mut host) = connect(0, Box::new(spill));
	remote.set_goal(goal(&project(20), 1, Wanted::All));
	drive(&mut remote, &mut host, |r| matches!(r.status().state, WorkState::Failed(_)));
	let WorkState::Failed(e) = remote.status().state else { unreachable!() };
	assert!(e.to_string().contains("no room for frames"), "{e}");
	assert!(remote.status().store.is_none());
}

#[test]
fn a_store_waits_for_the_pages_spill_to_answer() {
	let spill = Spill::default();
	spill.ready.store(false, Ordering::Relaxed);
	let ready = spill.ready.clone();
	let (mut remote, mut host) = connect(GIB, Box::new(spill));
	remote.set_goal(goal(&project(40), 1, Wanted::All));
	stall(&mut remote, &mut host, 50);
	assert!(remote.status().store.is_none());
	assert_eq!(host.frames_in_flight(), WRITES_IN_FLIGHT, "no frame acknowledged before its mirror");
	ready.store(true, Ordering::Relaxed);
	drive(&mut remote, &mut host, finished(1));
}

/// A page whose worker end the test plays.
fn page() -> (Remote<Loopback>, Loopback) {
	let (page, worker) = Loopback::pair();
	(Remote::new(page, Arc::new(AtomicBool::new(false)), GIB, Box::new(Spill::default())), worker)
}

fn send(worker: &Loopback, message: &ToPage, buffer: Option<Vec<u8>>) {
	worker.post(Envelope { message: postcard::to_allocvec(message).unwrap(), buffer });
}

fn acks(worker: &Loopback) -> Vec<(Key, Result<(), String>)> {
	std::iter::from_fn(|| worker.receive())
		.filter_map(|e| match postcard::from_bytes(&e.message).unwrap() {
			ToWorker::Ack { store, generation, frame, result } => Some(((store, generation, frame), result)),
			_ => None,
		})
		.collect()
}

#[test]
fn a_frame_of_a_store_or_generation_left_is_acknowledged_unwritten() {
	let (mut remote, worker) = page();
	let layout = StoreLayout::of(&RunSpec::create(&project(20), None, PreviewQuality::FULL, false).unwrap());
	send(&worker, &ToPage::StoreOpened { id: 7, layout: Layout::from(&layout) }, None);
	let status = RunStatus { store: Some(7), generation: 2, ..RunStatus::default() };
	send(&worker, &ToPage::Status(status), None);
	let planes = || Some(vec![1; layout.frame_bytes()]);
	send(&worker, &ToPage::Frame { store: 7, generation: 1, frame: 0 }, planes());
	send(&worker, &ToPage::Frame { store: 9, generation: 2, frame: 0 }, planes());
	send(&worker, &ToPage::Frame { store: 7, generation: 2, frame: 1 }, planes());
	remote.pump();
	assert_eq!(acks(&worker), [((7, 1, 0), Ok(())), ((9, 2, 0), Ok(())), ((7, 2, 1), Ok(()))]);
	let mirror = remote.status().store.unwrap();
	assert!(!mirror.has(0) && mirror.has(1));
}

#[test]
fn a_crashed_worker_fails_the_run_for_good() {
	let (mut remote, worker) = page();
	send(&worker, &ToPage::Crashed("the worker ran out of memory".into()), None);
	remote.pump();
	let WorkState::Failed(e) = remote.status().state else { panic!("not failed") };
	assert_eq!(e.to_string(), "the worker ran out of memory");
	send(&worker, &ToPage::Status(RunStatus::default()), None);
	remote.pump();
	assert!(matches!(remote.status().state, WorkState::Failed(_)), "nothing after the crash counts");
}

#[test]
fn a_failure_lasts_until_the_next_goal_and_the_next_failure_is_another() {
	let mut bad = project(20);
	bad.scene.mode = SceneMode::Image;
	let failed = |r: &Remote<Loopback>| match r.status().state {
		WorkState::Failed(e) => Some(e),
		_ => None,
	};
	let (mut remote, mut host) = connect(GIB, Box::new(Spill::default()));
	remote.set_goal(goal(&bad, 1, Wanted::Position));
	drive(&mut remote, &mut host, |r| failed(r).is_some());
	let first = failed(&remote).unwrap();
	assert!(matches!(*first, RunError::Spec(RunSpecError::NoImage)));
	stall(&mut remote, &mut host, 5);
	assert!(Arc::ptr_eq(&first, &failed(&remote).unwrap()), "one failure while it lasts");

	remote.set_goal(goal(&project(20), 2, Wanted::Position));
	drive(&mut remote, &mut host, |r| r.status().adopted == 2 && r.status().state == WorkState::Idle);
	remote.set_goal(goal(&bad, 3, Wanted::Position));
	drive(&mut remote, &mut host, |r| failed(r).is_some());
	assert!(!Arc::ptr_eq(&first, &failed(&remote).unwrap()), "another failure");
}
