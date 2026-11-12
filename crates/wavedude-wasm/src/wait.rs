//! Waiting for a GPU readback in the browser.

use std::future::Future;
use std::pin::{Pin, pin};
use std::task::Poll;

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use wavedude_core::engine::GpuHandle;

/// The interval of the empty submissions while a readback is pending.
const NUDGE_MS: i32 = 1;

#[wasm_bindgen]
extern "C" {
	#[wasm_bindgen(js_name = setTimeout)]
	fn set_timeout(handler: &js_sys::Function, ms: i32) -> JsValue;
}

fn sleep(ms: i32) -> JsFuture {
	JsFuture::from(js_sys::Promise::new(&mut |resolve, _| {
		set_timeout(&resolve, ms);
	}))
}

/// Awaits `ready`, a capture's readback, submitting an empty batch to `gpu`'s queue every [`NUDGE_MS`] until it
/// resolves: Firefox maps a buffer on a 100 ms timer unless the queue is submitted to (docs/measurements.md).
pub(crate) async fn readback(ready: impl Future<Output = ()>, gpu: &GpuHandle) {
	let mut ready = pin!(ready);
	let mut timer = sleep(NUDGE_MS);
	std::future::poll_fn(|cx| {
		if ready.as_mut().poll(cx).is_ready() {
			return Poll::Ready(());
		}
		while Pin::new(&mut timer).poll(cx).is_ready() {
			gpu.queue.submit([]);
			timer = sleep(NUDGE_MS);
		}
		Poll::Pending
	})
	.await;
}
