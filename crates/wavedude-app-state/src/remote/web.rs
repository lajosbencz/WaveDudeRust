//! The browser's [`Port`]s: the page's over the run worker it starts, a module worker, and the worker's over its
//! global scope. An envelope posts as `[message, buffer]`, both `ArrayBuffer`s and transferred, the buffer
//! `undefined` when there is none.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use js_sys::{Array, Uint8Array};
use web_sys::wasm_bindgen::closure::Closure;
use web_sys::wasm_bindgen::{JsCast, JsValue};
use web_sys::{DedicatedWorkerGlobalScope, ErrorEvent, Event, MessageEvent, Worker, WorkerOptions, WorkerType};

use super::protocol::{Lost, ToPage};
use super::{Envelope, Port, envelope, hold};
use crate::platform::Duration;

/// How long the page waits for the worker's first message.
const READY_WITHIN: Duration = Duration::from_secs(20);

type Inbox = Rc<RefCell<VecDeque<Envelope>>>;

/// The message and the list of buffers it transfers.
fn to_js(envelope: Envelope) -> (Array, Array) {
	let message = Uint8Array::from(&envelope.message[..]).buffer();
	let transfer = Array::of1(&message);
	let buffer = match envelope.buffer {
		Some(bytes) => {
			let buffer = Uint8Array::from(&bytes[..]).buffer();
			transfer.push(&buffer);
			buffer.into()
		}
		None => JsValue::UNDEFINED,
	};
	(Array::of2(&message, &buffer), transfer)
}

/// The envelope of a message's data; anything else reads as an empty, undecodable message.
fn from_js(data: &JsValue) -> Envelope {
	let bytes = |v: JsValue| Uint8Array::new(&v).to_vec();
	let parts = Array::from(data);
	let buffer = parts.get(1);
	Envelope { message: bytes(parts.get(0)), buffer: (!buffer.is_undefined()).then(|| bytes(buffer)) }
}

/// Where the page's end is with the worker.
enum Link {
	/// The worker has not spoken yet: what the page posts waits, as a module worker listens once its module ran.
	Starting(Vec<Envelope>),
	Open(Worker),
	/// The worker failed: what the page posts goes nowhere.
	Gone,
}

/// The page's end: the run worker it started.
pub struct PagePort {
	link: Rc<RefCell<Link>>,
	inbox: Inbox,
	/// None when the worker could not be made.
	_worker: Option<Started>,
}

/// The worker, its listeners and the wait for its first message, which go with it.
struct Started {
	worker: Worker,
	_onmessage: Closure<dyn FnMut(MessageEvent)>,
	_onerror: Closure<dyn FnMut(Event)>,
	silent: Timer,
	_timeout: Closure<dyn FnMut()>,
}

/// A timeout of the page, cleared once due no more.
#[derive(Clone)]
struct Timer(Rc<Cell<Option<i32>>>);

impl Timer {
	fn clear(&self) {
		if let Some(handle) = self.0.take() {
			web_sys::window().expect("the run worker is started from a page").clear_timeout_with_handle(handle);
		}
	}
}

impl PagePort {
	/// Starts the module worker at `url`; `on_message` is called as a message arrives. A worker that cannot be
	/// made, fails to load, stops, or does not speak in time arrives as [`ToPage::Lost`].
	pub fn start(url: &str, on_message: impl Fn() + 'static) -> Self {
		let options = WorkerOptions::new();
		options.set_type(WorkerType::Module);
		let worker = match Worker::new_with_options(url, &options) {
			Ok(worker) => worker,
			Err(e) => {
				let lost = Lost::NotCreated { url: url.to_owned(), detail: format!("{e:?}") };
				let inbox = Inbox::new(RefCell::new([envelope(&ToPage::Lost(lost), None)].into()));
				return Self { link: Rc::new(RefCell::new(Link::Gone)), inbox, _worker: None };
			}
		};
		let on_message = Rc::new(on_message);
		let link = Rc::new(RefCell::new(Link::Starting(Vec::new())));
		let inbox = Inbox::default();
		let timer = Timer(Rc::default());
		let fail = {
			let (worker, link, inbox, on_message, timer) =
				(worker.clone(), link.clone(), inbox.clone(), on_message.clone(), timer.clone());
			move |lost: Lost| {
				timer.clear();
				worker.terminate();
				*link.borrow_mut() = Link::Gone;
				inbox.borrow_mut().push_back(envelope(&ToPage::Lost(lost), None));
				on_message();
			}
		};
		let onmessage = {
			let (worker, link, inbox, timer) = (worker.clone(), link.clone(), inbox.clone(), timer.clone());
			Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
				let mut link = link.borrow_mut();
				if matches!(*link, Link::Gone) {
					return;
				}
				if let Link::Starting(held) = std::mem::replace(&mut *link, Link::Open(worker.clone())) {
					timer.clear();
					for envelope in held {
						post_to(&worker, envelope);
					}
				}
				drop(link);
				inbox.borrow_mut().push_back(from_js(&e.data()));
				on_message();
			})
		};
		let onerror = {
			let (fail, url) = (fail.clone(), url.to_owned());
			Closure::<dyn FnMut(Event)>::new(move |e: Event| {
				fail(Lost::Failed { url: url.clone(), detail: e.dyn_ref::<ErrorEvent>().map(ErrorEvent::message) });
			})
		};
		worker.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
		worker.set_onerror(Some(onerror.as_ref().unchecked_ref()));
		let silent = {
			let (timer, url) = (timer.clone(), url.to_owned());
			Closure::once(move || {
				timer.0.set(None);
				fail(Lost::Silent { url, secs: READY_WITHIN.as_secs() });
			})
		};
		let handle = web_sys::window()
			.expect("the run worker is started from a page")
			.set_timeout_with_callback_and_timeout_and_arguments_0(
				silent.as_ref().unchecked_ref(),
				READY_WITHIN.as_millis() as i32,
			)
			.expect("a page sets timeouts");
		timer.0.set(Some(handle));
		let started = Started { worker, _onmessage: onmessage, _onerror: onerror, silent: timer, _timeout: silent };
		Self { link, inbox, _worker: Some(started) }
	}
}

fn post_to(worker: &Worker, envelope: Envelope) {
	let (message, transfer) = to_js(envelope);
	worker.post_message_with_transfer(&message, &transfer).expect("an envelope's buffers transfer");
}

impl Port for PagePort {
	fn post(&self, envelope: Envelope) {
		match &mut *self.link.borrow_mut() {
			Link::Starting(held) => hold(held, envelope),
			Link::Open(worker) => post_to(worker, envelope),
			Link::Gone => {}
		}
	}

	fn receive(&self) -> Option<Envelope> {
		self.inbox.borrow_mut().pop_front()
	}
}

impl Drop for Started {
	fn drop(&mut self) {
		self.silent.clear();
		self.worker.set_onmessage(None);
		self.worker.set_onerror(None);
		self.worker.terminate();
	}
}

/// The worker's end: its global scope.
pub struct WorkerPort {
	scope: DedicatedWorkerGlobalScope,
	inbox: Inbox,
	_listener: Closure<dyn FnMut(MessageEvent)>,
}

impl WorkerPort {
	/// The end of the worker this runs in; `on_message` is called as a message arrives.
	pub fn new(on_message: impl Fn() + 'static) -> Self {
		let scope = scope();
		let inbox = Inbox::default();
		let listener = {
			let inbox = inbox.clone();
			Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
				inbox.borrow_mut().push_back(from_js(&e.data()));
				on_message();
			})
		};
		scope.set_onmessage(Some(listener.as_ref().unchecked_ref()));
		Self { scope, inbox, _listener: listener }
	}
}

impl Port for WorkerPort {
	fn post(&self, envelope: Envelope) {
		let (message, transfer) = to_js(envelope);
		self.scope.post_message_with_transfer(&message, &transfer).expect("an envelope's buffers transfer");
	}

	fn receive(&self) -> Option<Envelope> {
		self.inbox.borrow_mut().pop_front()
	}
}

fn scope() -> DedicatedWorkerGlobalScope {
	js_sys::global().dyn_into().expect("the run worker runs in a dedicated worker")
}

/// Logs each panic of the worker and posts it to the page as [`ToPage::Lost`]; the worker's program ends
/// with it.
pub fn report_panics() {
	let default = std::panic::take_hook();
	std::panic::set_hook(Box::new(move |info| {
		let why = info.to_string();
		tracing::error!("the run worker panicked: {why}");
		let (message, transfer) = to_js(envelope(&ToPage::Lost(Lost::Panicked(why)), None));
		if let Err(e) = scope().post_message_with_transfer(&message, &transfer) {
			tracing::error!("the panic could not be posted to the page: {e:?}");
		}
		default(info);
	}));
}
