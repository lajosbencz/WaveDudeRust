//! The worker's loop: [`Host::pump`] while the run has work, the event loop turning between two pumps so the
//! page's messages and the GPU's callbacks come in; asleep until a message arrives once it has none.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use js_sys::{Function, Promise};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use wavedude_app_state::log::max_level;
use wavedude_app_state::platform::Duration;
use wavedude_app_state::remote::Host;
use wavedude_app_state::remote::web::{WorkerPort, report_panics};
use wavedude_core::engine::gpu::GpuContext;
use web_sys::wasm_bindgen::closure::Closure;
use web_sys::wasm_bindgen::{JsCast, JsValue};
use web_sys::{MessageChannel, MessageEvent};

/// The run's work between two looks at the page's messages.
const BUDGET: Duration = Duration::from_millis(16);

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
	let wake = Rc::new(Wake::default());
	let port = WorkerPort::new({
		let wake = wake.clone();
		move || wake.raise()
	});
	spawn_local(async move {
		let gpu = GpuContext::create().await.map_err(|e| e.to_string());
		if let Err(why) = &gpu {
			tracing::warn!("the run worker has no GPU: {why}");
		}
		let mut host = Host::new(port, gpu);
		let again = Again::new(wake.clone());
		loop {
			if host.pump(BUDGET) {
				again.post();
			}
			wake.wait().await;
		}
	});
}

/// What wakes the loop: a message arrived, or the run goes on.
#[derive(Default)]
struct Wake {
	raised: Cell<bool>,
	/// The resolve of the promise the loop awaits.
	waiting: RefCell<Option<Function>>,
}

impl Wake {
	fn raise(&self) {
		match self.waiting.take() {
			Some(resolve) => {
				resolve.call0(&JsValue::UNDEFINED).expect("a promise's resolve does not throw");
			}
			None => self.raised.set(true),
		}
	}

	/// Returns once raised since it last returned.
	async fn wait(&self) {
		// raised by a task, which ran in an event loop turn since the last wait
		if self.raised.take() {
			return;
		}
		let raised = Promise::new(&mut |resolve, _| *self.waiting.borrow_mut() = Some(resolve));
		JsFuture::from(raised).await.expect("the wake's promise resolves");
	}
}

/// Raises the wake once the event loop turned: a message to itself, which no timer clamps.
struct Again {
	channel: MessageChannel,
	_listener: Closure<dyn FnMut(MessageEvent)>,
}

impl Again {
	fn new(wake: Rc<Wake>) -> Self {
		let channel = MessageChannel::new().expect("a worker makes message channels");
		let listener = Closure::<dyn FnMut(MessageEvent)>::new(move |_: MessageEvent| wake.raise());
		channel.port1().set_onmessage(Some(listener.as_ref().unchecked_ref()));
		Self { channel, _listener: listener }
	}

	fn post(&self) {
		self.channel.port2().post_message(&JsValue::UNDEFINED).expect("a message channel takes undefined");
	}
}
