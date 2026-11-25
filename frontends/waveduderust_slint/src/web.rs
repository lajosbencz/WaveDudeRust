//! The browser's run: a worker pumped by the window's timer on the page's WebGPU device.

use std::io;
use std::sync::Arc;
use std::task::{Context, Poll};

use waveduderust::run::{PumpedFactory, RunWorker, SpillBackend, SpillFactory, StoreLayout, Token};
use waveduderust::unstable::engine::build_on;
use waveduderust::unstable::engine::gpu::GpuContext;

/// The engine and the stores' memory: wasm32's 4 GiB address space, less the page's own.
const BUDGET: u64 = 1 << 30;

/// A spill that keeps nothing: the frames past the RAM share are dropped, not read back.
struct DropSpill;

impl SpillFactory for DropSpill {
	fn poll_ready(&self, _: &mut Context<'_>) -> Poll<()> {
		Poll::Ready(())
	}

	fn open(&self, _: u64, _: &StoreLayout, _: u64) -> io::Result<Box<dyn SpillBackend>> {
		Ok(Box::new(Self))
	}
}

impl SpillBackend for DropSpill {
	fn write(&self, _: u64, _: usize, _: &[&[u8]]) -> Token<()> {
		let t = Token::default();
		t.complete(Ok(()));
		t
	}

	fn read(&self, _: u64, _: usize) -> Token<Vec<u8>> {
		let t = Token::default();
		t.complete(Err(io::Error::other("the browser build keeps no frames past memory yet")));
		t
	}

	fn drop_generation(&self, _: u64) {}
}

/// A worker whose engines run on `context`'s device.
pub(crate) fn worker(context: Arc<GpuContext>) -> RunWorker {
	let factory: PumpedFactory = Box::new(move |scene, options| {
		let gpu = Some(context.clone());
		Box::pin(async move { build_on(scene, options, gpu).await.engine })
	});
	RunWorker::pumped(factory, BUDGET, BUDGET, Box::new(DropSpill), Arc::default())
}

/// Leaving the page asks, the browser's own question, while `dirty` holds.
pub(crate) fn guard_unload(dirty: std::rc::Rc<std::cell::Cell<bool>>) {
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
