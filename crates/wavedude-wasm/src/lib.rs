//! The WaveDude engine for the browser: the JavaScript surface of `session::Session`.
//!
//! One call at a time per `Simulation`: wasm-bindgen refuses a second call while `stepToward` runs.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use js_sys::Uint8ClampedArray;
use wasm_bindgen::prelude::*;
use wavedude_core::engine::gpu::GpuContext;
use wavedude_view::draw::{Baseline, TextMetrics};
use wavedude_view::graph::FONT_SIZE;
use wavedude_view::overlay::LABEL_SIZE;

mod cache;
mod overlay;
mod session;
#[cfg(target_arch = "wasm32")]
mod wait;

use session::{DEFAULT_BUDGET, Session, SessionError, layer_name, layer_of, tier_of};

#[wasm_bindgen]
extern "C" {
	#[wasm_bindgen(js_namespace = console, js_name = warn)]
	fn console_warn(message: &str);
	#[wasm_bindgen(js_namespace = console, js_name = error)]
	fn console_error(message: &str);
}

#[wasm_bindgen]
extern "C" {
	/// What the page's `measure` returns: a `TextMetrics` of `CanvasRenderingContext2D.measureText`.
	type Measured;
	#[wasm_bindgen(method, getter)]
	fn width(this: &Measured) -> f64;
	#[wasm_bindgen(method, getter, js_name = fontBoundingBoxAscent)]
	fn ascent(this: &Measured) -> f64;
	#[wasm_bindgen(method, getter, js_name = fontBoundingBoxDescent)]
	fn descent(this: &Measured) -> f64;
}

/// Text measured by the page's `measure(text, size, baseline)` at `size` x `scale` pixels, reported in pixels
/// divided by `scale`; each text once. The first exception `measure` throws is kept and the widths read 0.
struct PageText<'a> {
	measure: &'a js_sys::Function,
	baseline: Baseline,
	size: f32,
	scale: f32,
	line_height: f32,
	widths: RefCell<HashMap<String, f32>>,
	error: RefCell<Option<JsValue>>,
}

impl<'a> PageText<'a> {
	fn new(measure: &'a js_sys::Function, baseline: Baseline, size: f32, scale: f32) -> Self {
		let mut text = Self {
			measure,
			baseline,
			size,
			scale,
			line_height: 0.,
			widths: RefCell::default(),
			error: RefCell::default(),
		};
		text.line_height = text.measured("0").map_or(0., |m| (m.ascent() + m.descent()) as f32 / scale);
		text
	}

	fn measured(&self, text: &str) -> Option<Measured> {
		let baseline = match self.baseline {
			Baseline::Alphabetic => "Alphabetic",
			Baseline::Top => "Top",
		};
		let size = JsValue::from(self.size * self.scale);
		match self.measure.call3(&JsValue::NULL, &JsValue::from(text), &size, &JsValue::from(baseline)) {
			Ok(m) => Some(m.unchecked_into()),
			Err(e) => {
				self.error.borrow_mut().get_or_insert(e);
				None
			}
		}
	}

	fn into_error(self) -> Option<JsValue> {
		self.error.into_inner()
	}
}

impl TextMetrics for PageText<'_> {
	fn line_height(&self) -> f32 {
		self.line_height
	}

	fn width(&self, text: &str) -> f32 {
		if let Some(&w) = self.widths.borrow().get(text) {
			return w;
		}
		let w = self.measured(text).map_or(0., |m| m.width() as f32 / self.scale);
		self.widths.borrow_mut().insert(text.to_owned(), w);
		w
	}
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

/// One RGBA8 picture of a layer, ready for `new ImageData(data, width, height)`.
#[wasm_bindgen]
pub struct Picture(session::Picture);

#[wasm_bindgen]
impl Picture {
	#[wasm_bindgen(getter)]
	pub fn width(&self) -> u32 {
		self.0.width
	}

	#[wasm_bindgen(getter)]
	pub fn height(&self) -> u32 {
		self.0.height
	}

	/// A copy of the pixels each time it is read.
	#[wasm_bindgen(getter)]
	pub fn data(&self) -> Uint8ClampedArray {
		Uint8ClampedArray::from(self.0.rgba.as_slice())
	}
}

/// A project opened for stepping and rendering.
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

	/// The shapes over the picture of `layer` at `frame` drawn at `width` x `height` CSS pixels on a screen of
	/// `dpr` device pixels per CSS pixel, as JSON: an array of groups `{scale, origin: [x, y], shapes}` in drawing
	/// order, whose shapes (`Shape` of the `wavedude-view` crate, colours as `#rrggbbaa`) are in units of `scale`
	/// device pixels from `origin`, in device pixels from the picture's top-left. `measure(text, size, baseline)`
	/// returns the page's `measureText` of `text` at `size` CSS pixels in the font of `baseline` ("Alphabetic" or
	/// "Top").
	pub fn overlay(
		&self,
		frame: u32,
		layer: &str,
		width: f32,
		height: f32,
		dpr: f32,
		measure: &js_sys::Function,
	) -> Result<String, JsValue> {
		let labels = PageText::new(measure, Baseline::Alphabetic, LABEL_SIZE, 1.);
		let boxes = PageText::new(measure, Baseline::Top, FONT_SIZE, dpr);
		let screen = overlay::Screen { width, height, dpr };
		let layer = layer_of(layer).map_err(JsError::from)?;
		let fonts = overlay::Fonts { labels: &labels, boxes: &boxes };
		let groups = self.0.overlay(frame, layer, screen, fonts).map_err(JsError::from)?;
		if let Some(e) = labels.into_error().or_else(|| boxes.into_error()) {
			return Err(e);
		}
		Ok(serde_json::to_string(&groups).expect("shapes serialise to JSON"))
	}

	/// The picture of `layer` at `frame`; `undefined` while the frame is not stored. A 16-bit tier renders at
	/// `width` x `height` pixels, an 8-bit one at the stored size.
	pub fn render(&mut self, frame: u32, layer: &str, width: u32, height: u32) -> Result<Option<Picture>, JsError> {
		let picture = self.0.render(frame, layer_of(layer)?, (width, height))?;
		Ok(picture.map(Picture))
	}
}
