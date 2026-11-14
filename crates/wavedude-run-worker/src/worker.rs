//! The worker's loop: [`Host::pump`], then the event loop turns so the page's messages and the GPU's callbacks come
//! in. With work left a message to itself wakes it again; while the run waits (a readback, the device, the
//! page's acknowledgements, an engine) what it waits for or a message wakes it; once it has no work, a message.
//! The engine built after the GPU device was lost requests a new one.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use wasm_bindgen_futures::spawn_local;
use wavedude_app_state::log::max_level;
use wavedude_app_state::platform::Duration;
use wavedude_app_state::remote::web::{WorkerPort, report_panics};
use wavedude_app_state::remote::{GpuRequest, Host};
use wavedude_core::engine::GpuHandle;
use wavedude_core::engine::gpu::GpuContext;
use wavedude_core::run::Pump;
use wavedude_core::sync::lock;
use web_sys::wasm_bindgen::closure::Closure;
use web_sys::wasm_bindgen::{JsCast, JsValue};
use web_sys::{MessageChannel, MessageEvent};

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
			if host.pump(BUDGET, &waker) == Pump::Work {
				again.post();
			}
			wake.wait().await;
		}
	});
}

async fn request_gpu() -> Result<GpuHandle, String> {
	GpuContext::create().await.map_err(|e| e.to_string())
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
