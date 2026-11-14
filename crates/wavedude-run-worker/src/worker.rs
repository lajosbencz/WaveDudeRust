//! The worker's loop: [`Host::pump`], then the event loop turns so the page's messages and the GPU's callbacks come
//! in. With work left a message to itself wakes it again; while the run waits (a readback, the device, the
//! page's acknowledgements, an engine) what it waits for or a message wakes it; once it has no work, a message.
//! While it waits and the device, not lost, owes completions, empty submits kick the queue on [`Kicks`]' gaps: a
//! browser may resolve completions only when it polls the device, on a submit or on a timer (decision 0026).
//! The engine built after the GPU device was lost requests a new one.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::{Pin, pin};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use wasm_bindgen_futures::{js_sys, spawn_local};
use wavedude_app_state::log::max_level;
use wavedude_app_state::platform::Duration;
use wavedude_app_state::remote::web::{WorkerPort, report_panics};
use wavedude_app_state::remote::{GpuRequest, Host};
use wavedude_core::engine::GpuHandle;
use wavedude_core::engine::gpu::{GpuContext, Kicks};
use wavedude_core::run::Pump;
use wavedude_core::sync::lock;
use web_sys::wasm_bindgen::closure::Closure;
use web_sys::wasm_bindgen::{JsCast, JsValue};
use web_sys::{MessageChannel, MessageEvent, WorkerGlobalScope};

/// The run's work between two looks at the page's messages: the most a goal waits past the steps to the run's
/// next check, and a frame made for its post. Half a page frame at 60 Hz.
const BUDGET: Duration = Duration::from_millis(8);

/// Logs to the console, reports panics to the page, takes the page's messages, makes the GPU device and runs.
pub(crate) fn start() {
	tracing_subscriber::registry()
		.with(max_level())
		.with(
			tracing_subscriber::fmt::layer()
				.with_writer(tracing_web::MakeWebConsoleWriter::new())
				.with_ansi(false)
				.without_time(),
		)
		.try_init()
		.expect("the run worker sets the only subscriber");
	report_panics();
	let wake = Arc::new(Wake::default());
	let port = WorkerPort::new({
		let wake = wake.clone();
		move || wake.raise()
	});
	spawn_local(async move {
		let gpu = request_gpu().await;
		if let Err(why) = &gpu {
			tracing::warn!("the run worker has no GPU: {why}");
		}
		let mut host = Host::new(port, gpu, Box::new(|| -> GpuRequest { Box::pin(request_gpu()) }));
		let again = Again::new(wake.clone());
		let waker = Waker::from(wake.clone());
		loop {
			let left = host.pump(BUDGET, &waker);
			if left == Pump::Work {
				again.post();
			}
			match host.gpu().filter(|gpu| left == Pump::Waiting && gpu.lost().is_none() && gpu.awaits_completions()) {
				Some(gpu) => first(wake.wait(), kick(&gpu)).await,
				None => wake.wait().await,
			}
		}
	});
}

async fn request_gpu() -> Result<GpuHandle, String> {
	GpuContext::create().await.map_err(|e| e.to_string())
}

/// Kicks the queue while `gpu` awaits completions and is not lost; pending ever after.
async fn kick(gpu: &GpuContext) {
	for gap in Kicks::default() {
		Sleep::new(gap).await;
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

/// A timeout of the worker's, cleared when dropped before it fired.
struct Sleep {
	handle: i32,
	fired: Rc<Fired>,
	_callback: Closure<dyn FnMut()>,
}

/// Set by a [`Sleep`]'s timeout, which wakes the task polling it.
#[derive(Default)]
struct Fired {
	done: Cell<bool>,
	waker: RefCell<Option<Waker>>,
}

impl Sleep {
	/// Fires after `gap`, in the whole milliseconds timeouts count, rounded up.
	fn new(gap: Duration) -> Self {
		let ms = i32::try_from(gap.as_micros().div_ceil(1000)).unwrap_or(i32::MAX);
		let fired = Rc::<Fired>::default();
		let callback = Closure::<dyn FnMut()>::new({
			let fired = fired.clone();
			move || {
				fired.done.set(true);
				if let Some(waker) = fired.waker.take() {
					waker.wake();
				}
			}
		});
		let handle = scope()
			.set_timeout_with_callback_and_timeout_and_arguments_0(callback.as_ref().unchecked_ref(), ms)
			.expect("a worker sets timeouts");
		Self { handle, fired, _callback: callback }
	}
}

impl Future for Sleep {
	type Output = ();

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
		if self.fired.done.get() {
			return Poll::Ready(());
		}
		*self.fired.waker.borrow_mut() = Some(cx.waker().clone());
		Poll::Pending
	}
}

impl Drop for Sleep {
	fn drop(&mut self) {
		scope().clear_timeout_with_handle(self.handle);
	}
}

fn scope() -> WorkerGlobalScope {
	js_sys::global().unchecked_into()
}

/// What wakes the loop: a message arrived, the run goes on, or what it waits for is ready.
#[derive(Default)]
struct Wake {
	raised: AtomicBool,
	/// The loop's task while it waits.
	waiting: Mutex<Option<Waker>>,
}

impl Wake {
	fn raise(&self) {
		self.raised.store(true, Ordering::Release);
		if let Some(waiting) = lock(&self.waiting).take() {
			waiting.wake();
		}
	}

	/// Returns once raised since it last returned.
	async fn wait(&self) {
		std::future::poll_fn(|cx| {
			*lock(&self.waiting) = Some(cx.waker().clone());
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
	channel: MessageChannel,
	_listener: Closure<dyn FnMut(MessageEvent)>,
}

impl Again {
	fn new(wake: Arc<Wake>) -> Self {
		let channel = MessageChannel::new().expect("a worker makes message channels");
		let listener = Closure::<dyn FnMut(MessageEvent)>::new(move |_: MessageEvent| wake.raise());
		channel.port1().set_onmessage(Some(listener.as_ref().unchecked_ref()));
		Self { channel, _listener: listener }
	}

	fn post(&self) {
		self.channel.port2().post_message(&JsValue::UNDEFINED).expect("a message channel takes undefined");
	}
}
