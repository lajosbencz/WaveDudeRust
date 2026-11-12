//! The WaveDude engine for the browser: the JavaScript surface of `session::Session`.
//!
//! One call at a time per `Simulation`: wasm-bindgen refuses a second call while `stepToward` runs.

use std::sync::Arc;

use wasm_bindgen::prelude::*;
use wavedude_core::engine::gpu::GpuContext;
use wavedude_view::graph::FONT_SIZE;
use wavedude_view::overlay::LABEL_SIZE;

mod cache;
mod canvas;
mod overlay;
mod session;
#[cfg(target_arch = "wasm32")]
mod wait;

pub use canvas::View;
use overlay::{Fonts, Screen};
use session::{DEFAULT_BUDGET, Session, SessionError, layer_name, layer_of, tier_of};

#[wasm_bindgen]
extern "C" {
	#[wasm_bindgen(js_namespace = console, js_name = warn)]
	fn console_warn(message: &str);
	#[wasm_bindgen(js_namespace = console, js_name = error)]
	fn console_error(message: &str);
}

#[wasm_bindgen(start)]
fn start() {
	std::panic::set_hook(Box::new(|info| console_error(&info.to_string())));
}

/// A WebGPU device, shared by every `Simulation` opened with it.
#[wasm_bindgen(js_name = GpuDevice)]
pub struct GpuDevice(Arc<GpuContext>);

/// Requests the WebGPU adapter and device; `undefined`, with the reason on the console, when there is none.
#[wasm_bindgen(js_name = requestGpu)]
pub async fn request_gpu() -> Option<GpuDevice> {
	match GpuContext::create().await {
		Ok(context) => Some(GpuDevice(context)),
		Err(e) => {
			console_warn(&format!("WebGPU is not available: {e}"));
			None
		}
	}
}

async fn open_session(
	text: &str,
	tier: &str,
	gpu: Option<Arc<GpuContext>>,
	budget_bytes: Option<f64>,
) -> Result<Session, SessionError> {
	let budget = match budget_bytes {
		Some(b) if b.is_finite() && b >= 0. => b as u64,
		Some(b) => return Err(SessionError::Budget(b)),
		None => DEFAULT_BUDGET,
	};
	Session::open(text, tier_of(tier)?, gpu, budget).await
}

fn open_promise(
	text: String,
	tier: String,
	gpu: Option<Arc<GpuContext>>,
	budget_bytes: Option<f64>,
) -> js_sys::Promise {
	wasm_bindgen_futures::future_to_promise(async move {
		let session = open_session(&text, &tier, gpu, budget_bytes).await.map_err(JsError::from)?;
		Ok(JsValue::from(Simulation(session)))
	})
}

/// A project opened for stepping and drawing.
#[wasm_bindgen]
pub struct Simulation(Session);

#[wasm_bindgen]
impl Simulation {
	/// Opens the project `text` at `tier` ("Low", "Middle" or "High") on the CPU. The cache keeps `budgetBytes` of
	/// frames (512 MiB by default).
	#[wasm_bindgen(unchecked_return_type = "Promise<Simulation>")]
	pub fn open(
		text: String,
		tier: String,
		#[wasm_bindgen(js_name = budgetBytes)] budget_bytes: Option<f64>,
	) -> js_sys::Promise {
		open_promise(text, tier, None, budget_bytes)
	}

	/// [`open`](Self::open) on `gpu`, which stays usable for the next call (wasm-bindgen consumes an optional
	/// object argument, so the CPU has its own function); the CPU runs when the engine cannot be built on it.
	#[wasm_bindgen(js_name = openOn, unchecked_return_type = "Promise<Simulation>")]
	pub fn open_on(
		gpu: &GpuDevice,
		text: String,
		tier: String,
		#[wasm_bindgen(js_name = budgetBytes)] budget_bytes: Option<f64>,
	) -> js_sys::Promise {
		open_promise(text, tier, Some(gpu.0.clone()), budget_bytes)
	}

	/// "gpu" or "cpu".
	#[wasm_bindgen(getter)]
	pub fn engine(&self) -> String {
		self.0.engine_name().to_owned()
	}

	/// The GPU was passed to `openOn` and the CPU runs instead.
	#[wasm_bindgen(getter, js_name = gpuFellBack)]
	pub fn gpu_fell_back(&self) -> bool {
		self.0.gpu_fell_back()
	}

	/// Frames of the timeline.
	#[wasm_bindgen(getter, js_name = frameCount)]
	pub fn frame_count(&self) -> u32 {
		self.0.spec().frame_count()
	}

	/// Frames the tier stores: every n-th of the timeline.
	#[wasm_bindgen(getter, js_name = storedFrames)]
	pub fn stored_frames(&self) -> u32 {
		self.0.spec().stored_frames()
	}

	/// Frames per second of the project's video.
	#[wasm_bindgen(getter)]
	pub fn fps(&self) -> u32 {
		self.0.fps()
	}

	#[wasm_bindgen(getter, js_name = canvasWidth)]
	pub fn canvas_width(&self) -> u32 {
		self.0.spec().scene.canvas_width
	}

	#[wasm_bindgen(getter, js_name = canvasHeight)]
	pub fn canvas_height(&self) -> u32 {
		self.0.spec().scene.canvas_height
	}

	/// The layers the project shows: "field" and "intensity".
	#[wasm_bindgen(getter)]
	pub fn layers(&self) -> Vec<String> {
		self.0.layers().into_iter().map(|l| layer_name(l).to_owned()).collect()
	}

	/// The step timeline frame `frame` holds the state after.
	#[wasm_bindgen(js_name = stepOf)]
	pub fn step_of(&self, frame: u32) -> u32 {
		self.0.spec().step_of(frame)
	}

	/// Steps at most `maxSteps` toward the stored frame `frame` shows, and stores it on arriving; true once it
	/// is stored.
	#[wasm_bindgen(js_name = stepToward)]
	pub async fn step_toward(
		&mut self,
		frame: u32,
		#[wasm_bindgen(js_name = maxSteps)] max_steps: u32,
	) -> Result<bool, JsError> {
		Ok(self.0.step_toward(frame, max_steps).await?)
	}

	/// The nearest stored frame at or before `frame`, to show while seeking.
	#[wasm_bindgen(js_name = latestBefore)]
	pub fn latest_before(&self, frame: u32) -> Option<u32> {
		self.0.latest_before(frame)
	}

	/// Draws the picture of `layer` at `frame` on the canvas of `view` with what the desktop preview draws over it:
	/// the edge line, the handles and labels, the probe graph boxes. The picture is `width` x `height` CSS pixels on
	/// a screen of `dpr` device pixels per CSS pixel, `round(width * dpr)` x `round(height * dpr)` device pixels (1 to
	/// 16384 a side) inset by the edge line's `max(1, round(dpr))` on each side of the canvas, which is sized to fit.
	/// False, and the canvas untouched, while the frame is not stored. A 16-bit tier renders at the device size, an
	/// 8-bit one at the stored size, scaled.
	pub fn draw(
		&mut self,
		view: &mut View,
		frame: u32,
		layer: &str,
		width: f32,
		height: f32,
		dpr: f32,
	) -> Result<bool, JsValue> {
		let layer = layer_of(layer).map_err(JsError::from)?;
		let screen = Screen::new(width, height, dpr).map_err(JsError::from)?;
		let Some(picture) = self.0.render(frame, layer, screen.device()).map_err(JsError::from)? else {
			return Ok(false);
		};
		let labels = view.label_text(LABEL_SIZE);
		let boxes = view.box_text(FONT_SIZE, dpr);
		let fonts = Fonts { labels: &labels, boxes: &boxes };
		let groups = self.0.overlay(frame, layer, screen, fonts).map_err(JsError::from)?;
		view.draw(screen, &picture, &groups)?;
		Ok(true)
	}
}
