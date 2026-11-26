//! The worker's loop: [`Host::pump`], then the event loop turns so the page's messages come in. With work left a
//! message to itself wakes it again; while the run waits (page acknowledgements) what it waits for or a message wakes
//! it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};

use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use wasm_bindgen_futures::spawn_local;
use waveduderust::run::Pump;
use waveduderust::unstable::sync::lock;
use waveduderust_remote::Host;
use waveduderust_remote::web::{WorkerPort, report_panics};
use web_sys::wasm_bindgen::closure::Closure;
use web_sys::wasm_bindgen::{JsCast, JsValue};
use web_sys::{MessageChannel, MessageEvent};
use web_time::Duration;

/// The run's work between two looks at the page's messages: the most a goal waits past the steps to the run's
/// next check, and a frame made for its post. Half a page frame at 60 Hz.
const BUDGET: Duration = Duration::from_millis(8);

/// Logs to the console, reports panics to the page, takes the page's messages and runs.
pub(crate) fn start() {
	tracing_subscriber::registry()
		.with(if cfg!(debug_assertions) { LevelFilter::DEBUG } else { LevelFilter::INFO })
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
		let mut host = Host::new(port, device_memory());
		let again = Again::new(wake.clone());
		let waker = Waker::from(wake.clone());
		loop {
			let left = host.pump(BUDGET, &waker);
			if left == Pump::Work {
				again.post();
			}
			wake.wait().await;
		}
	});
}

/// `navigator.deviceMemory` in bytes (rounded to a power of 2 and clamped by the browser); `None` where the browser
/// does not tell.
fn device_memory() -> Option<u64> {
	let navigator = js_sys::Reflect::get(&js_sys::global(), &"navigator".into()).ok()?;
	let gib = js_sys::Reflect::get(&navigator, &"deviceMemory".into()).ok()?.as_f64()?;
	(gib.is_finite() && gib > 0.0).then(|| (gib * f64::from(1u32 << 30)) as u64)
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
