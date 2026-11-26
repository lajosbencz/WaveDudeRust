//! The browser's services: the live run on the page's GPU (decision 0035) or on the CPU in the run worker (decision
//! 0024), its frames past memory in the spill worker's files (decision 0020), the question on leaving the page, and
//! a failure shown in the page.

use std::future::Future;
use std::pin::pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Poll, Waker};

use waveduderust::ProbeTrace;
use waveduderust::run::{FitFrame, Goal, Pump, PumpedFactory, RunWorker, SpillFactory, Status};
use waveduderust::unstable::engine::build_on;
use waveduderust::unstable::engine::gpu::{GpuContext, Kicks};
use waveduderust_remote::opfs::Opfs;
use waveduderust_remote::web::PagePort;
use waveduderust_remote::{Port, Remote};

use crate::live::Run;

/// The engine and the stores' memory at Auto: wasm32's 4 GiB address space, less the page's own.
pub(crate) const BUDGET: u64 = 1 << 30;

thread_local! {
	/// The page's spill worker and session, started with the first run.
	static SPILL: Opfs = Opfs::start(crate::wake::wake);
}

/// A run's spill of its frames past memory (decision 0020).
fn spill() -> Box<dyn SpillFactory> {
	SPILL.with(Opfs::factory)
}

/// Starts the spill worker and the session, which the exports' spool shares, unless a run started them.
pub(crate) fn start_spill() {
	SPILL.with(|_| ());
}

/// The run worker's script, beside the page.
const RUN_WORKER: &str = "run-worker.js";

/// The live run on the CPU in the run worker (decision 0024), which it starts: its frames in the page's mirror,
/// [`BUDGET`] at Auto, the rest spilled. The worker is named with the page's build as its query, which the script passes on to its
/// files, so the browser's cache never serves the worker of another build.
pub(crate) fn cpu_run() -> Remote<PagePort> {
	let url = format!("{RUN_WORKER}?build={}", js_sys::encode_uri_component(&waveduderust_remote::build_id()));
	Remote::new(PagePort::start(&url, crate::wake::wake), BUDGET, spill(), crate::wake::wake)
}

/// The live run on the page's GPU (decision 0035): a worker pumped on the page's thread, its engines on `context`'s
/// device, its frames past [`BUDGET`] spilled; a spill not ready wakes the pump through its waker.
pub(crate) fn gpu_run(context: Arc<GpuContext>) -> Pumped {
	let worker = worker(context.clone());
	worker.set_notify(Box::new(crate::wake::wake));
	let worker = Rc::new(worker);
	let wake = run(worker.clone(), context);
	Pumped { worker, wake }
}

/// A worker pumped on the page; its pump ends once it is dropped.
pub(crate) struct Pumped {
	worker: Rc<RunWorker>,
	/// Starts the pump again after a goal.
	wake: Arc<Wake>,
}

impl Drop for Pumped {
	fn drop(&mut self) {
		self.wake.raise();
	}
}

impl Run for Pumped {
	fn status(&self) -> Status {
		self.worker.status()
	}

	fn set_goal(&mut self, goal: Goal) {
		self.worker.set_goal(goal);
		self.wake.raise();
	}

	fn with_traces(&self, f: &mut dyn FnMut(&[ProbeTrace])) {
		self.worker.with_traces(f);
	}

	fn take_fit(&mut self) -> Option<FitFrame> {
		self.worker.take_fit()
	}

	fn discard_frames(&mut self) {
		self.worker.discard_frames();
		self.wake.raise();
	}
}

impl<P: Port> Run for Remote<P> {
	fn status(&self) -> Status {
		Self::status(self)
	}

	fn set_goal(&mut self, goal: Goal) {
		Self::set_goal(self, goal);
	}

	fn with_traces(&self, f: &mut dyn FnMut(&[ProbeTrace])) {
		Self::with_traces(self, f);
	}

	fn take_fit(&mut self) -> Option<FitFrame> {
		Self::take_fit(self)
	}

	fn discard_frames(&mut self) {
		Self::discard_frames(self);
	}

	fn pump(&mut self) {
		Self::pump(self);
	}
}

/// A worker whose engines run on `context`'s device.
fn worker(context: Arc<GpuContext>) -> RunWorker {
	let factory: PumpedFactory = Box::new(move |scene, options| {
		let gpu = Some(context.clone());
		Box::pin(async move { build_on(scene, options, gpu).await.engine })
	});
	RunWorker::pumped(factory, BUDGET, BUDGET, spill(), Arc::default())
}

/// The run's work between two turns of the page's event loop.
const SLICE: std::time::Duration = std::time::Duration::from_millis(8);

/// Pumps `worker` on the page as the run worker pumps its own (decision 0035): with work left a message to itself turns
/// the event loop and goes on; while the run waits, what it waits for or a raise of the returned [`Wake`] (a new goal)
/// starts it again; meanwhile, while `gpu` owes completions, empty submits kick its queue (decision 0026). The pump
/// ends once it holds the last reference to `worker`.
fn run(worker: Rc<RunWorker>, gpu: Arc<GpuContext>) -> Arc<Wake> {
	let wake = Arc::new(Wake::default());
	let (pumped, again) = (wake.clone(), Again::new(wake.clone()));
	wasm_bindgen_futures::spawn_local(async move {
		let waker = Waker::from(pumped.clone());
		while Rc::strong_count(&worker) > 1 {
			let left = worker.pump(SLICE, &waker);
			if left == Pump::Work {
				again.post();
			}
			if left == Pump::Waiting && gpu.lost().is_none() && gpu.awaits_completions() {
				first(pumped.wait(), kick(&gpu)).await;
			} else {
				pumped.wait().await;
			}
		}
	});
	wake
}

/// Kicks the queue while `gpu` awaits completions and is not lost; pending ever after.
async fn kick(gpu: &GpuContext) {
	for gap in Kicks::default() {
		gloo_timers::future::TimeoutFuture::new(u32::try_from(gap.as_micros().div_ceil(1000)).unwrap_or(u32::MAX))
			.await;
		if gpu.lost().is_some() || !gpu.awaits_completions() {
			break;
		}
		gpu.kick();
	}
	std::future::pending::<()>().await;
}

/// Waits for `wait`, polling `beside` meanwhile.
async fn first(wait: impl Future<Output = ()>, beside: impl Future<Output = ()>) {
	let (mut wait, mut beside) = (pin!(wait), pin!(beside));
	std::future::poll_fn(|cx| {
		let ready = wait.as_mut().poll(cx);
		if ready.is_pending() {
			let _ = beside.as_mut().poll(cx);
		}
		ready
	})
	.await;
}

/// What starts the pump again: a goal, the run going on, or what it waits for being ready.
#[derive(Default)]
struct Wake {
	raised: AtomicBool,
	/// The pump's task while it waits.
	waiting: Mutex<Option<Waker>>,
}

impl Wake {
	fn raise(&self) {
		self.raised.store(true, Ordering::Release);
		if let Some(waiting) = self.waiting.lock().unwrap_or_else(PoisonError::into_inner).take() {
			waiting.wake();
		}
	}

	/// Returns once raised since it last returned.
	async fn wait(&self) {
		std::future::poll_fn(|cx| {
			*self.waiting.lock().unwrap_or_else(PoisonError::into_inner) = Some(cx.waker().clone());
			if self.raised.swap(false, Ordering::AcqRel) { Poll::Ready(()) } else { Poll::Pending }
		})
		.await;
	}
}

impl std::task::Wake for Wake {
	fn wake(self: Arc<Self>) {
		self.raise();
	}
}

/// Raises the wake once the event loop turned: a message to itself, which no timer clamps.
struct Again {
	channel: web_sys::MessageChannel,
	_listener: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::MessageEvent)>,
}

impl Again {
	fn new(wake: Arc<Wake>) -> Self {
		use wasm_bindgen::JsCast as _;
		let channel = web_sys::MessageChannel::new().expect("a page makes message channels");
		let listener =
			wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |_: web_sys::MessageEvent| {
				wake.raise();
			});
		channel.port1().set_onmessage(Some(listener.as_ref().unchecked_ref()));
		Self { channel, _listener: listener }
	}

	fn post(&self) {
		let _ = self.channel.port2().post_message(&wasm_bindgen::JsValue::UNDEFINED);
	}
}

/// Leaving the page asks, the browser's own question, while `dirty` holds.
pub(crate) fn guard_unload(dirty: Rc<std::cell::Cell<bool>>) {
	use wasm_bindgen::JsCast as _;
	use wasm_bindgen::closure::Closure;
	let Some(window) = web_sys::window() else { return };
	let ask = Closure::<dyn Fn(web_sys::BeforeUnloadEvent)>::new(move |e: web_sys::BeforeUnloadEvent| {
		if dirty.get() {
			e.prevent_default();
		}
	});
	if window.add_event_listener_with_callback("beforeunload", ask.as_ref().unchecked_ref()).is_ok() {
		// the page's own: it lives as long as the page
		ask.forget();
	}
}

/// Logs `message` to the console and shows it in the page's `failure` box (web/index.html), over a window that may
/// never have drawn.
pub(crate) fn fail(message: &str) {
	web_sys::console::error_1(&message.into());
	if let Some(element) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("failure")) {
		element.set_text_content(Some(message));
		let _ = element.remove_attribute("hidden");
	}
}
