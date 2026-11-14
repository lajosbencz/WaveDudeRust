//! The live run across a port: the page's `Remote` against the worker's `Host` on the CPU engine (decision 0024).

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};

use wavedude_app_state::live::{Backend, EngineLabel, LiveController, LiveSettings, Notice, RunFailure};
use wavedude_app_state::remote::protocol::{Layout, Lost, RunStatus, State, ToPage, ToWorker, VERSION};
use wavedude_app_state::remote::{Envelope, GpuRequest, Host, Loopback, Port, Remote, RequestGpu, WorkerGone};
use wavedude_core::engine::cpu::CpuEngine;
use wavedude_core::engine::gpu::GpuContext;
use wavedude_core::engine::probes::ProbeTrace;
use wavedude_core::engine::{GpuHandle, WaveEngine};
use wavedude_core::model::{Layer, Probe, ProbeKind, Project, SceneMode, Source};
use wavedude_core::quality::PreviewQuality;
use wavedude_core::run::{
	EngineFuture, FrameStore, Goal, Pump, PumpedFactory, RunError, RunWorker, SpillBackend, SpillFactory, StoreError,
	StoreLayout, Token, Wanted, WorkState,
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
	(remote, Host::new(worker, gpu, no_new_gpu()))
}

/// A request for a new GPU device that never gets one.
fn no_new_gpu() -> RequestGpu {
	Box::new(|| -> GpuRequest { Box::pin(async { Err("no new GPU in the tests".into()) }) })
}

/// Pumps both ends until `done` holds (within a minute).
fn drive(remote: &mut Remote<Loopback>, host: &mut Host<Loopback>, mut done: impl FnMut(&Remote<Loopback>) -> bool) {
	let deadline = Instant::now() + Duration::from_secs(60);
	loop {
		host.pump(SLICE, Waker::noop());
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

fn layout(p: &Project) -> StoreLayout {
	StoreLayout::of(&RunSpec::create(p, None, PreviewQuality::FULL, false).unwrap())
}

/// The same goal run directly on a pumped worker in memory.
fn direct(p: &Project, wanted: Wanted) -> RunWorker {
	let start: PumpedFactory = Box::new(|scene, options| -> EngineFuture {
		Box::pin(async move { Box::new(CpuEngine::new(scene, options)) as Box<dyn WaveEngine> })
	});
	let w = RunWorker::pumped(start, GIB, Box::new(Spill::default()), Arc::default());
	w.set_goal(goal(p, 1, wanted));
	while w.pump(SLICE, Waker::noop()) != Pump::Idle {}
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
	fn poll_ready(&self, _: &mut Context<'_>) -> Poll<()> {
		if self.ready.load(Ordering::Relaxed) { Poll::Ready(()) } else { Poll::Pending }
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
	host.pump(SLICE, Waker::noop());
	assert_eq!(host.frames_in_flight(), 0, "every frame acknowledged");
}

#[test]
fn a_mirror_past_its_memory_spills_and_acknowledges_a_frame_once_written() {
	let p = project(60);
	let layout = layout(&p);
	let spill = Spill::default();
	let disk = spill.disk.clone();
	let (mut remote, mut host) = connect(2 * layout.frame_bytes() as u64, Box::new(spill));
	remote.set_goal(goal(&p, 1, Wanted::All));
	stall(&mut remote, &mut host, 50, layout.writes_in_flight());
	assert_eq!(lock(&disk).writes.len(), layout.writes_in_flight(), "the frames in memory are acknowledged at once");
	assert_eq!(host.frames_in_flight(), layout.writes_in_flight(), "the spilled ones once written");
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
		host.pump(SLICE, Waker::noop());
	}
	let mirror = remote.status().store.unwrap();
	assert_eq!((mirror.frames_in_memory(), mirror.frames_on_disk()), (2, layout.capacity - 2));
	same_frames(&mirror, &direct(&p, Wanted::All).store().unwrap(), &disk);
}

#[test]
fn the_worker_is_never_more_than_the_spills_writes_ahead_of_the_page() {
	let p = project(60);
	let most = layout(&p).writes_in_flight();
	let (mut remote, mut host) = connect(GIB, Box::new(Spill::default()));
	remote.set_goal(goal(&p, 1, Wanted::All));
	for _ in 0..100 {
		assert_ne!(host.pump(SLICE, Waker::noop()), Pump::Idle);
		assert!(host.frames_in_flight() <= most);
	}
	assert_eq!(host.frames_in_flight(), most, "without acknowledgements the run waits");
	assert_eq!(host.pump(SLICE, Waker::noop()), Pump::Waiting, "for the acknowledgements, not working");
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
		host.pump(SLICE, Waker::noop());
		if let Some(n) = c.take_notices().into_iter().find(|n| matches!(n, Notice::RunFailed { .. })) {
			break n;
		}
		assert!(Instant::now() < deadline, "no failure");
	};
	assert_eq!(failed, Notice::RunFailed(RunFailure::NoImage));

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

/// The messages `page` received, decoded.
fn received(page: &Loopback) -> Vec<ToPage> {
	std::iter::from_fn(|| page.receive()).map(|e| postcard::from_bytes(&e.message).unwrap()).collect()
}

/// The engines `host` built for `page` until `n` were, with why the CPU's ran in place of the GPU's.
fn engines_built(page: &Loopback, host: &mut Host<Loopback>, n: usize) -> Vec<(EngineLabel, Option<String>)> {
	let mut built = Vec::new();
	let deadline = Instant::now() + Duration::from_secs(60);
	while built.len() < n {
		host.pump(SLICE, Waker::noop());
		built.extend(received(page).into_iter().filter_map(|m| match m {
			ToPage::EngineBuilt { engine, fallback } => Some((engine, fallback)),
			_ => None,
		}));
		assert!(Instant::now() < deadline, "no engine built: {built:?}");
	}
	built
}

/// Pumps `host` until it posts a failed run.
fn failed(page: &Loopback, host: &mut Host<Loopback>) {
	let deadline = Instant::now() + Duration::from_secs(60);
	while !received(page).iter().any(|m| matches!(m, ToPage::Status(RunStatus { state: State::Failed { .. }, .. }))) {
		host.pump(SLICE, Waker::noop());
		assert!(Instant::now() < deadline, "the run did not fail");
	}
}

fn send_to_worker(page: &Loopback, message: &ToWorker) {
	page.post(Envelope { message: postcard::to_allocvec(message).unwrap(), buffer: None });
}

#[test]
fn the_engine_built_and_why_the_gpu_fell_back_reach_the_page() {
	let (page, worker) = Loopback::pair();
	let mut host = Host::new(worker, Err("no adapter here".into()), no_new_gpu());
	send_to_worker(&page, &ToWorker::Init { version: VERSION, use_gpu: true });
	send_to_worker(&page, &ToWorker::Goal { goal: goal(&project(20), 1, Wanted::Position), image: None });
	let built = engines_built(&page, &mut host, 1);
	assert_eq!(built, [(EngineLabel::of(None), Some("no adapter here".into()))]);
}

#[test]
fn the_engine_reads_as_starting_until_the_worker_is_ready() {
	let (page, worker) = Loopback::pair();
	let mut remote = Remote::new(page, Arc::new(AtomicBool::new(true)), GIB, Box::new(Spill::default()));
	assert_eq!(remote.engine(), EngineLabel::Starting);
	send(&worker, &ToPage::Ready { version: VERSION, gpu: Ok("RTX".into()) }, None);
	remote.pump();
	assert_eq!(remote.engine(), EngineLabel::Gpu("RTX".into()));
}

/// A GPU device, or why there is none.
fn gpu_device() -> Result<GpuHandle, String> {
	let mut request = std::pin::pin!(GpuContext::create());
	match request.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
		Poll::Ready(device) => device.map_err(|e| e.to_string()),
		Poll::Pending => Err("a native device request answers at once".into()),
	}
}

/// Loses `device` and waits for the loss to be told.
fn lose(device: &GpuHandle) {
	device.device.destroy();
	device.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
	assert!(device.lost().is_some());
}

#[test]
fn an_engine_built_after_the_device_was_lost_gets_a_new_device_or_runs_on_the_cpu_saying_why() {
	let first = match gpu_device() {
		Ok(device) => device,
		Err(e) => return eprintln!("skipped: {e}"),
	};
	// The devices requested: the first request gets one, the next none.
	let made = Arc::new(Mutex::new(Vec::<Result<GpuHandle, String>>::new()));
	let request: RequestGpu = Box::new({
		let made = made.clone();
		move || -> GpuRequest {
			let mut made = lock(&made);
			let fresh = if made.is_empty() { gpu_device() } else { Err("the adapter is gone".into()) };
			made.push(fresh.clone());
			Box::pin(async move { fresh })
		}
	});
	let (page, worker) = Loopback::pair();
	let mut host = Host::new(worker, Ok(first.clone()), request);
	send_to_worker(&page, &ToWorker::Init { version: VERSION, use_gpu: true });
	send_to_worker(&page, &ToWorker::Goal { goal: goal(&project(20), 1, Wanted::Position), image: None });
	let gpu = EngineLabel::of(Some(&first.info.name));
	assert_eq!(engines_built(&page, &mut host, 1), [(gpu.clone(), None)]);

	// The run on a lost device fails; the next goal builds its engine anew.
	let mut revision = 1;
	let mut fail_then_go_on = |host: &mut Host<Loopback>| {
		revision += 1;
		send_to_worker(&page, &ToWorker::Rebuild { use_gpu: true });
		failed(&page, host);
		let next = goal(&project(20), revision, Wanted::Position);
		send_to_worker(&page, &ToWorker::Goal { goal: next, image: None });
	};
	lose(&first);
	fail_then_go_on(&mut host);
	assert_eq!(engines_built(&page, &mut host, 1), [(gpu, None)], "on a new device");
	let second = lock(&made)[0].clone().unwrap();
	assert!(!Arc::ptr_eq(&first, &second));

	lose(&second);
	fail_then_go_on(&mut host);
	let cpu = (EngineLabel::of(None), Some("the adapter is gone".to_owned()));
	assert_eq!(engines_built(&page, &mut host, 1), std::slice::from_ref(&cpu));
	send_to_worker(&page, &ToWorker::Rebuild { use_gpu: true });
	assert_eq!(engines_built(&page, &mut host, 1), [cpu]);
	assert_eq!(lock(&made).len(), 2, "a failed request is kept");
}

/// Pumps both ends `n` times, the page's spill writes left in flight, at most `most` frames unacknowledged.
fn stall(remote: &mut Remote<Loopback>, host: &mut Host<Loopback>, n: usize, most: usize) {
	for _ in 0..n {
		host.pump(SLICE, Waker::noop());
		remote.pump();
		assert!(host.frames_in_flight() <= most);
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
	let layout = layout(&p);
	let most = layout.writes_in_flight();
	let spill = Spill::default();
	let disk = spill.disk.clone();
	let (mut remote, mut host) = connect(2 * layout.frame_bytes() as u64, Box::new(spill));
	remote.set_goal(goal(&p, 1, Wanted::All));
	stall(&mut remote, &mut host, 50, most);
	assert_eq!(host.frames_in_flight(), most);
	let first = stick(&remote, &disk);

	remote.discard_frames();
	remote.set_goal(goal(&p, 2, Wanted::All));
	stall(&mut remote, &mut host, 50, most);
	assert_eq!(host.frames_in_flight(), most, "the store left is not waited for");
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
		host.pump(SLICE, Waker::noop());
	}
	assert_eq!(host.frames_in_flight(), 0, "every frame acknowledged or released");
	assert_eq!(lock(&disk).writes.len(), 2 * most, "the stuck writes are still in flight");
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
	let p = project(40);
	let most = layout(&p).writes_in_flight();
	let (mut remote, mut host) = connect(GIB, Box::new(spill));
	remote.set_goal(goal(&p, 1, Wanted::All));
	stall(&mut remote, &mut host, 50, most);
	assert!(remote.status().store.is_none());
	assert_eq!(host.frames_in_flight(), most, "no frame acknowledged before its mirror");
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
	let layout = layout(&project(20));
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
fn a_worker_lost_before_it_was_ready_did_not_start_and_later_runs_say_so_again() {
	let (mut remote, worker) = page();
	let lost = Lost::Silent { url: "run-worker.js".into(), secs: 20 };
	send(&worker, &ToPage::Lost(lost.clone()), None);
	remote.pump();
	assert_eq!(remote.worker_gone(), Some(WorkerGone::NotStarted(lost.clone())));
	assert_eq!(remote.engine(), EngineLabel::Missing);

	let p = project(20);
	let mut c = LiveController::new(remote, LiveSettings::default());
	let gone = Notice::RunFailed(RunFailure::WorkerGone(WorkerGone::NotStarted(lost)));
	for _ in 0..2 {
		c.run_user(&p, Instant::now());
		c.tick(Instant::now(), &p);
		assert!(c.take_notices().contains(&gone), "each Run says why nothing runs");
		c.tick(Instant::now(), &p);
		assert!(!c.take_notices().contains(&gone), "once");
	}
	assert!(
		worker.receive().is_some_and(|e| matches!(
			postcard::from_bytes(&e.message).unwrap(),
			ToWorker::Init { version: VERSION, .. }
		))
	);
	assert!(worker.receive().is_none(), "nothing is posted to a worker gone");
}

#[test]
fn a_worker_lost_once_ready_stopped_and_fails_the_run_for_good() {
	let (mut remote, worker) = page();
	send(&worker, &ToPage::Ready { version: VERSION, gpu: Err("none".into()) }, None);
	let lost = Lost::Panicked("the worker ran out of memory".into());
	send(&worker, &ToPage::Lost(lost.clone()), None);
	remote.pump();
	let WorkState::Failed(e) = remote.status().state else { panic!("not failed") };
	assert_eq!(e.to_string(), "the run worker stopped: the worker ran out of memory");
	assert_eq!(remote.worker_gone(), Some(WorkerGone::Stopped(lost)));
	send(&worker, &ToPage::Status(RunStatus::default()), None);
	remote.pump();
	assert!(matches!(remote.status().state, WorkState::Failed(_)), "nothing after the loss counts");
}

#[test]
fn a_worker_of_another_protocol_version_is_gone_and_does_not_listen_to_the_page() {
	let (mut remote, worker) = page();
	let other = VERSION + 1;
	send(&worker, &ToPage::Ready { version: other, gpu: Err("none".into()) }, None);
	remote.pump();
	assert_eq!(remote.worker_gone(), Some(WorkerGone::OtherVersion { page: VERSION, worker: other }));

	let (page, worker) = Loopback::pair();
	let mut host = Host::new(worker, Err("none".into()), no_new_gpu());
	send_to_worker(&page, &ToWorker::Init { version: VERSION + 1, use_gpu: false });
	page.post(Envelope { message: vec![0xff; 3], buffer: None });
	assert_eq!(host.pump(SLICE, Waker::noop()), Pump::Idle, "an unreadable message of another version is dropped");
	assert!(matches!(received(&page).first(), Some(ToPage::Ready { version: VERSION, .. })));
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
	stall(&mut remote, &mut host, 5, 0);
	assert!(Arc::ptr_eq(&first, &failed(&remote).unwrap()), "one failure while it lasts");

	remote.set_goal(goal(&project(20), 2, Wanted::Position));
	drive(&mut remote, &mut host, |r| r.status().adopted == 2 && r.status().state == WorkState::Idle);
	remote.set_goal(goal(&bad, 3, Wanted::Position));
	drive(&mut remote, &mut host, |r| failed(r).is_some());
	assert!(!Arc::ptr_eq(&first, &failed(&remote).unwrap()), "another failure");
}

#[test]
fn traces_started_over_before_the_status_says_so_start_over_on_the_page() {
	let p = with_probes(project(200));
	let (mut remote, mut host) = connect(GIB, Box::new(Spill::default()));
	remote.set_goal(goal(&p, 1, Wanted::All));
	drive(&mut remote, &mut host, finished(1));
	let mut q = p.clone();
	q.probes[0].ax += 1;
	remote.set_goal(goal(&q, 2, Wanted::All));
	for _ in 0..20 {
		host.pump(Duration::ZERO, Waker::noop());
		remote.pump();
	}
	drive(&mut remote, &mut host, finished(2));
	assert_eq!(traces(&remote), direct(&q, Wanted::All).with_traces(<[ProbeTrace]>::to_vec));
}
