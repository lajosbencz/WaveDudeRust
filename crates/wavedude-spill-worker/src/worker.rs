//! The worker's loop: the page's messages one at a time, in the order posted, so a read or a removal follows the
//! writes posted before it. A store generation's frames are one file of the session folder, `<store>-<generation>`,
//! opened at its first write with a synchronous access handle and closed at its removal. Frames are appended and
//! their places indexed, as the frames a store keeps in memory are never written; a frame written again of the same
//! length takes its place, and reads as not spilled until that write succeeds. A message it cannot read ends the
//! loop: the worker closes and removes its files, then posts [`FromSpill::Gone`].

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::task::{Poll, Waker};

use js_sys::{Array, Promise, Reflect, Uint8Array};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use wavedude_spill_worker::{FOLDER, Failure, FailureKind, FromSpill, ToSpill, VERSION};
use web_sys::wasm_bindgen::closure::Closure;
use web_sys::wasm_bindgen::{JsCast, JsValue};
use web_sys::{
	DedicatedWorkerGlobalScope, FileSystemDirectoryHandle, FileSystemFileHandle, FileSystemGetFileOptions,
	FileSystemReadWriteOptions, FileSystemSyncAccessHandle, MessageEvent,
};

/// Logs to the console, reports panics to the page, says it listens and handles the page's messages.
pub(crate) fn start() {
	tracing_subscriber::registry()
		.with(if cfg!(debug_assertions) { LevelFilter::TRACE } else { LevelFilter::DEBUG })
		.with(
			tracing_subscriber::fmt::layer()
				.with_writer(tracing_web::MakeWebConsoleWriter::new())
				.with_ansi(false)
				.without_time(),
		)
		.try_init()
		.expect("the spill worker sets the only subscriber");
	report_panics();
	let inbox = Rc::new(Inbox::default());
	let listener = {
		let inbox = inbox.clone();
		Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| inbox.push(e.data()))
	};
	scope().set_onmessage(Some(listener.as_ref().unchecked_ref()));
	post(&FromSpill::Ready { version: VERSION }, None);
	spawn_local(async move {
		let mut spills = Spills::default();
		loop {
			let data = inbox.next().await;
			match message(&data) {
				Ok((message, buffer)) => spills.handle(message, buffer).await,
				Err(e) => {
					tracing::error!("the spill worker could not read a message: {e}");
					spills.remove_where(|_| true).await;
					post(&FromSpill::Gone(format!("it could not read a message: {e}")), None);
					break;
				}
			}
		}
		scope().set_onmessage(None);
		drop(listener);
	});
}

/// The page's messages not handled yet.
#[derive(Default)]
struct Inbox {
	data: RefCell<VecDeque<JsValue>>,
	/// The loop's task while it waits.
	waiting: RefCell<Option<Waker>>,
}

impl Inbox {
	fn push(&self, data: JsValue) {
		self.data.borrow_mut().push_back(data);
		if let Some(waiting) = self.waiting.take() {
			waiting.wake();
		}
	}

	async fn next(&self) -> JsValue {
		std::future::poll_fn(|cx| match self.data.borrow_mut().pop_front() {
			Some(data) => Poll::Ready(data),
			None => {
				*self.waiting.borrow_mut() = Some(cx.waker().clone());
				Poll::Pending
			}
		})
		.await
	}
}

/// A message's data, `[message, buffer]` as the page posts it, both `ArrayBuffer`s, the buffer `undefined` when
/// there is none.
fn message(data: &JsValue) -> postcard::Result<(ToSpill, Option<Uint8Array>)> {
	let parts = Array::from(data);
	let buffer = parts.get(1);
	let buffer = (!buffer.is_undefined()).then(|| Uint8Array::new(&buffer));
	Ok((postcard::from_bytes(&Uint8Array::new(&parts.get(0)).to_vec())?, buffer))
}

/// Posts `message` to the page as `[message, buffer]`, both transferred.
fn send(message: &FromSpill, buffer: Option<Uint8Array>) -> Result<(), JsValue> {
	let encoded = postcard::to_allocvec(message).expect("a message of the spill protocol encodes");
	let message = Uint8Array::from(&encoded[..]).buffer();
	let transfer = Array::of1(&message);
	let buffer = match buffer {
		Some(bytes) => {
			let buffer = bytes.buffer();
			transfer.push(&buffer);
			buffer.into()
		}
		None => JsValue::UNDEFINED,
	};
	scope().post_message_with_transfer(&Array::of2(&message, &buffer), &transfer)
}

fn post(message: &FromSpill, buffer: Option<Uint8Array>) {
	send(message, buffer).expect("a message's buffers transfer");
}

fn scope() -> DedicatedWorkerGlobalScope {
	js_sys::global().dyn_into().expect("the spill worker runs in a dedicated worker")
}

/// Logs each panic of the worker and posts it to the page as [`FromSpill::Gone`]; the worker's program ends with it.
fn report_panics() {
	let default = std::panic::take_hook();
	std::panic::set_hook(Box::new(move |info| {
		let why = info.to_string();
		tracing::error!("the spill worker panicked: {why}");
		if let Err(e) = send(&FromSpill::Gone(format!("it panicked: {why}")), None) {
			tracing::error!("the panic could not be posted to the page: {e:?}");
		}
		default(info);
	}));
}

/// The spilled frames of this tab's stores.
#[derive(Default)]
struct Spills {
	/// The session folder once the page named it, or why it could not be opened.
	session: Option<Result<FileSystemDirectoryHandle, Failure>>,
	/// By (store, generation).
	files: HashMap<(u64, u64), Frames>,
}

impl Spills {
	async fn handle(&mut self, message: ToSpill, buffer: Option<Uint8Array>) {
		match message {
			ToSpill::Session { name } => {
				let session = open_session(&name).await;
				match &session {
					Ok(_) => tracing::debug!("spilling into session {name}"),
					Err(e) => tracing::warn!("the session folder {name} could not be opened: {}", e.message),
				}
				self.session = Some(session);
			}
			ToSpill::Write { id, store, generation, frame } => {
				let result = match buffer {
					Some(planes) => self.write((store, generation), frame, &planes).await,
					None => Err(other(format!("frame {frame} came without planes"))),
				};
				post(&FromSpill::Done { id, result }, None);
			}
			ToSpill::Read { id, store, generation, frame } => match self.read((store, generation), frame) {
				Ok(planes) => post(&FromSpill::Done { id, result: Ok(()) }, Some(planes)),
				Err(e) => post(&FromSpill::Done { id, result: Err(e) }, None),
			},
			ToSpill::DropGeneration { store, generation } => self.remove((store, generation)).await,
			ToSpill::DropStore { store } => self.remove_where(|&(s, _)| s == store).await,
		}
	}

	fn session(&self) -> Result<FileSystemDirectoryHandle, Failure> {
		match &self.session {
			Some(session) => session.clone(),
			None => Err(other("the page named no session folder".to_owned())),
		}
	}

	async fn write(&mut self, key: (u64, u64), frame: usize, planes: &Uint8Array) -> Result<(), Failure> {
		if !self.files.contains_key(&key) {
			let frames = Frames::open(&self.session()?, &file_name(key)).await?;
			self.files.insert(key, frames);
		}
		self.files.get_mut(&key).expect("opened above").write(frame, planes)
	}

	fn read(&self, key: (u64, u64), frame: usize) -> Result<Uint8Array, Failure> {
		let not_spilled = || Failure {
			kind: FailureKind::NotFound,
			message: format!("frame {frame} of {} was not spilled", file_name(key)),
		};
		let frames = self.files.get(&key).ok_or_else(not_spilled)?;
		let place = frames.places.get(&frame).ok_or_else(not_spilled)?;
		frames.read(place)
	}

	/// Closes the file of `key` and removes it.
	async fn remove(&mut self, key: (u64, u64)) {
		let Some(frames) = self.files.remove(&key) else { return };
		drop(frames);
		let name = file_name(key);
		let removed = match self.session() {
			Ok(session) => JsFuture::from(session.remove_entry(&name)).await.map_err(|e| failure(&e)).map(drop),
			Err(e) => Err(e),
		};
		if let Err(e) = removed {
			tracing::warn!("the spilled frames in {name} could not be removed: {}", e.message);
		}
	}

	/// Closes the files whose key `which` holds and removes them.
	async fn remove_where(&mut self, which: impl Fn(&(u64, u64)) -> bool) {
		let keys: Vec<(u64, u64)> = self.files.keys().copied().filter(|key| which(key)).collect();
		for key in keys {
			self.remove(key).await;
		}
	}
}

fn file_name((store, generation): (u64, u64)) -> String {
	format!("{store}-{generation}")
}

/// A frame's bytes in its file.
#[derive(Clone, Copy)]
struct Place {
	at: u64,
	len: u64,
}

/// One store generation's file.
struct Frames {
	file: FileSystemSyncAccessHandle,
	/// By frame.
	places: HashMap<usize, Place>,
	/// The bytes written: where the next frame goes.
	end: u64,
}

impl Frames {
	async fn open(session: &FileSystemDirectoryHandle, name: &str) -> Result<Self, Failure> {
		let options = FileSystemGetFileOptions::new();
		options.set_create(true);
		let handle: FileSystemFileHandle = awaited(session.get_file_handle_with_options(name, &options)).await?;
		let file = awaited(handle.create_sync_access_handle()).await?;
		Ok(Self { file, places: HashMap::new(), end: 0 })
	}

	/// Writes frame `frame`, over its bytes before when of the same length; its place is indexed once written.
	fn write(&mut self, frame: usize, planes: &Uint8Array) -> Result<(), Failure> {
		let len = u64::from(planes.length());
		let place = match self.places.remove(&frame) {
			Some(place) if place.len == len => place,
			_ => Place { at: self.end, len },
		};
		let written = self.file.write_with_js_u8_array_and_options(planes, &at(place.at)).map_err(|e| failure(&e))?;
		if written as u64 != len {
			return Err(other(format!("browser storage wrote {written} of the frame's {len} bytes")));
		}
		self.end = self.end.max(place.at + len);
		self.places.insert(frame, place);
		Ok(())
	}

	fn read(&self, place: &Place) -> Result<Uint8Array, Failure> {
		let planes = Uint8Array::new_with_length(u32::try_from(place.len).expect("a frame fits a buffer"));
		let read = self.file.read_with_js_u8_array_and_options(&planes, &at(place.at)).map_err(|e| failure(&e))?;
		if read as u64 != place.len {
			return Err(other(format!("browser storage read {read} of the frame's {} bytes", place.len)));
		}
		Ok(planes)
	}
}

impl Drop for Frames {
	fn drop(&mut self) {
		self.file.close();
	}
}

fn at(offset: u64) -> FileSystemReadWriteOptions {
	let options = FileSystemReadWriteOptions::new();
	options.set_at(offset as f64);
	options
}

/// The session folder `name` in [`FOLDER`].
async fn open_session(name: &str) -> Result<FileSystemDirectoryHandle, Failure> {
	let sync = Reflect::get(&js_sys::global(), &"FileSystemFileHandle".into())
		.and_then(|c| Reflect::get(&c, &"prototype".into()))
		.and_then(|p| Reflect::has(&p, &"createSyncAccessHandle".into()))
		.unwrap_or(false);
	if !sync {
		return Err(Failure {
			kind: FailureKind::Unsupported,
			message: "this browser has no synchronous access handles to spill frames with".to_owned(),
		});
	}
	let root: FileSystemDirectoryHandle = awaited(scope().navigator().storage().get_directory()).await?;
	let spill = folder(&root, FOLDER).await?;
	folder(&spill, name).await
}

async fn folder(parent: &FileSystemDirectoryHandle, name: &str) -> Result<FileSystemDirectoryHandle, Failure> {
	awaited(parent.get_directory_handle(name)).await
}

async fn awaited<T: JsCast>(promise: Promise) -> Result<T, Failure> {
	let value = JsFuture::from(promise).await.map_err(|e| failure(&e))?;
	value.dyn_into().map_err(|v| other(format!("browser storage answered {v:?}")))
}

fn other(message: String) -> Failure {
	Failure { kind: FailureKind::Other, message }
}

/// A failed storage call: its kind by the error's name, its name and message.
fn failure(e: &JsValue) -> Failure {
	let field = |key: &str| Reflect::get(e, &key.into()).ok().and_then(|v| v.as_string());
	let name = field("name");
	let kind = FailureKind::of_name(name.as_deref());
	let detail = match (name, field("message")) {
		(Some(name), Some(message)) => format!("{name}: {message}"),
		_ => format!("{e:?}"),
	};
	Failure { kind, message: format!("browser storage: {detail}") }
}
