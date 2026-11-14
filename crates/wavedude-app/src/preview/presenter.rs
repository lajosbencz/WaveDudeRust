//! The presenter: turns the frame the pictures show (a stored frame of the run, or a frame on its own) into RGBA8 pictures,
//! one per visible layer, on a worker thread of its own (docs/ui/40-preview-pane.md, 8-bit vs 16-bit), where the
//! renderer's device does not colour them.
//!
//! 8-bit frames are coloured through the palette at the stored size and scaled by the picture; 16-bit frames go
//! through the screen renderer at the picture's size in device pixels, plain or dithered. Only the newest request
//! is rendered: a request waiting in the slot is replaced by a newer one. Results come back through a channel
//! the pane drains on the UI thread, where [`PresenterStats`] keeps the log line's figures. On wasm32, where
//! nothing blocks, a request is rendered in the call.
//!
//! Threads: [`Presenter`] is owned by the UI thread; natively, rendering runs on the `preview-presenter` thread
//! (and the renderers' rayon pool).

use std::collections::{HashMap, VecDeque};
#[cfg(not(target_arch = "wasm32"))]
use std::panic::{AssertUnwindSafe, catch_unwind};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Weak};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Condvar, Mutex};
#[cfg(not(target_arch = "wasm32"))]
use std::thread::JoinHandle;

use bytes::{Bytes, BytesMut};
use wavedude_app_state::platform::{Duration, Instant};
use wavedude_core::engine::FrameShape;
use wavedude_core::engine::Snapshot;
pub use wavedude_core::engine::as_halves;
use wavedude_core::model::Layer;
use wavedude_core::render::ScreenRenderer;
use wavedude_core::render::layer::{LayerColours, View, paint};
use wavedude_core::render::screen::Window;
use wavedude_core::run::{FrameStore, StoreError};
use wavedude_core::scene::glowing_edges::{self, EdgeImage};

/// Whether a request is rendered in the call, on the caller's thread.
pub const IN_CALL: bool = cfg!(target_arch = "wasm32");

/// Where a frame's planes come from.
#[derive(Clone)]
pub enum FrameSource {
	/// Stored frame `stored` of a run's store.
	Run { store: Arc<FrameStore>, stored: usize },
	/// The frame a store kept through its last clear, made in `generation` ([`FrameStore::latest`]).
	Latest { store: Arc<FrameStore>, generation: u64, snapshot: Arc<Snapshot> },
	/// A frame on its own: a fit's 16-bit frame.
	Snapshot(Arc<Snapshot>),
}

/// Identity of a [`FrameSource`]: equal keys hold equal planes. A store's frames are named by the store (a weak
/// reference: the address is not reused while the key lives), the generation and the index; a frame made anew
/// after a clear has another generation. A fit's snapshot is named by its address, which its source keeps alive.
#[derive(Clone)]
pub enum SourceKey {
	Run { store: Weak<FrameStore>, generation: u64, stored: usize },
	Latest { store: Weak<FrameStore>, generation: u64 },
	Snapshot(usize),
}

impl PartialEq for SourceKey {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(Self::Run { store: a, generation: g, stored: i }, Self::Run { store: b, generation: h, stored: j }) => {
				Weak::ptr_eq(a, b) && g == h && i == j
			}
			(Self::Latest { store: a, generation: g }, Self::Latest { store: b, generation: h }) => {
				Weak::ptr_eq(a, b) && g == h
			}
			(Self::Snapshot(a), Self::Snapshot(b)) => a == b,
			_ => false,
		}
	}
}

impl std::fmt::Debug for SourceKey {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Run { generation, stored, .. } => write!(f, "Run({generation}:{stored})"),
			Self::Latest { generation, .. } => write!(f, "Latest({generation})"),
			Self::Snapshot(id) => write!(f, "Snapshot({id:#x})"),
		}
	}
}

impl FrameSource {
	pub fn key(&self) -> SourceKey {
		match self {
			Self::Run { store, stored } => {
				SourceKey::Run { store: Arc::downgrade(store), generation: store.generation(), stored: *stored }
			}
			Self::Latest { store, generation, .. } => {
				SourceKey::Latest { store: Arc::downgrade(store), generation: *generation }
			}
			Self::Snapshot(s) => SourceKey::Snapshot(Arc::as_ptr(s) as usize),
		}
	}

	pub fn shape(&self) -> FrameShape {
		match self {
			Self::Run { store, .. } => *store.shape(),
			Self::Latest { snapshot, .. } | Self::Snapshot(snapshot) => snapshot.shape,
		}
	}

	/// A run's frame that was cleared or replaced since `key` was taken; other sources never vanish.
	fn vanished_since(&self, key: &SourceKey) -> bool {
		match self {
			Self::Run { store, stored } => !store.has(*stored) || self.key() != *key,
			Self::Latest { .. } | Self::Snapshot(_) => false,
		}
	}

	/// Calls `f` with the plane of `layer`; `None` when the source holds no such plane or it is being read back.
	pub fn with_plane<R>(&self, layer: Layer, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
		self.with_plane_into(layer, &mut Vec::new(), f).ok()
	}

	/// [`with_plane`](Self::with_plane) reading a spilled frame into `buf`, reused across calls.
	pub fn with_plane_into<R>(
		&self,
		layer: Layer,
		buf: &mut Vec<u8>,
		f: impl FnOnce(&[u8]) -> R,
	) -> Result<R, NoPlane> {
		match self {
			Self::Run { store, stored } => store.with_layer_into(*stored, layer, buf, f).map_err(|e| match e {
				StoreError::Pending { .. } => NoPlane::Pending,
				_ => NoPlane::Missing,
			}),
			Self::Latest { snapshot: s, .. } | Self::Snapshot(s) => {
				let plane = s.plane(layer);
				if plane.is_empty() { Err(NoPlane::Missing) } else { Ok(f(plane)) }
			}
		}
	}
}

/// Why a source handed out no plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoPlane {
	/// The source holds no such plane (any more).
	Missing,
	/// A spilled frame on its way back from storage: asked again, it is there once read.
	Pending,
}

/// The outline blended over a picture: the edges at `gain`.
#[derive(Clone, Debug)]
pub struct Outline {
	pub edges: Arc<EdgeImage>,
	pub gain: f64,
}

impl PartialEq for Outline {
	fn eq(&self, other: &Self) -> bool {
		Arc::ptr_eq(&self.edges, &other.edges) && self.gain == other.gain
	}
}

/// How one layer's picture is coloured and sized.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerRequest {
	pub layer: Layer,
	pub colours: LayerColours,
	/// The picture's size in device pixels (16-bit frames); `(0, 0)` renders at the stored size.
	pub device: (u32, u32),
	/// The part of the canvas the picture shows when zoomed (16-bit frames), in canvas pixels; None is all of it.
	pub window: Option<Window>,
	/// 10, 12, 14 or 16 dithers a 16-bit frame; anything else is plain.
	pub dither_bits: u32,
	pub outline: Option<Outline>,
}

/// A frame to present.
#[derive(Clone)]
pub struct Request {
	pub source: FrameSource,
	pub layers: Vec<LayerRequest>,
	/// Playback or a live follow: gaps between pictures count as stutter.
	pub motion: bool,
	pub at: Instant,
}

/// One coloured picture, RGBA8, unpremultiplied, alpha 255.
#[derive(Clone, Debug)]
pub struct Picture {
	pub layer: Layer,
	pub width: u32,
	pub height: u32,
	pub rgba: Bytes,
	/// The part of the canvas the pixels cover; None is all of it.
	pub window: Option<Window>,
}

/// A rendered request.
#[derive(Clone, Debug)]
pub struct Rendered {
	pub source: SourceKey,
	pub pictures: Vec<Picture>,
	/// A layer found no frame: the store cleared or dropped it while the request waited.
	pub vanished: bool,
	/// A layer's frame is being read back from the spill: the request is to be sent again.
	pub pending: bool,
	pub render: Duration,
	pub requested: Instant,
	pub motion: bool,
}

impl Rendered {
	/// A request answered with no pictures.
	#[cfg(not(target_arch = "wasm32"))]
	fn empty(request: &Request) -> Self {
		Self {
			source: request.source.key(),
			pictures: Vec::new(),
			vanished: false,
			pending: false,
			render: Duration::ZERO,
			requested: request.at,
			motion: request.motion,
		}
	}
}

#[cfg(not(target_arch = "wasm32"))]
type Slot = Arc<(Mutex<Option<Request>>, Condvar)>;

/// The worker and its newest-request slot. Dropping it stops the thread.
#[cfg(not(target_arch = "wasm32"))]
pub struct Presenter {
	slot: Slot,
	stop: Arc<AtomicBool>,
	results: Receiver<Rendered>,
	thread: Option<JoinHandle<()>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Presenter {
	pub fn start() -> std::io::Result<Self> {
		let slot: Slot = Arc::new((Mutex::new(None), Condvar::new()));
		let stop = Arc::new(AtomicBool::new(false));
		let (tx, results) = channel();
		let thread = std::thread::Builder::new().name("preview-presenter".into()).spawn({
			let slot = slot.clone();
			let stop = stop.clone();
			move || work(&slot, &stop, &tx)
		})?;
		Ok(Self { slot, stop, results, thread: Some(thread) })
	}

	/// Queues `request`, replacing one not yet started.
	pub fn request(&mut self, request: Request) {
		let (lock, wake) = &*self.slot;
		*wavedude_core::sync::lock(lock) = Some(request);
		wake.notify_one();
	}

	/// The newest finished picture set, older ones dropped.
	pub fn latest(&mut self) -> Option<Rendered> {
		self.results.try_iter().last()
	}
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Presenter {
	fn drop(&mut self) {
		self.stop.store(true, Ordering::Release);
		self.slot.1.notify_one();
		if let Some(t) = self.thread.take() {
			let _ = t.join();
		}
	}
}

#[cfg(not(target_arch = "wasm32"))]
fn work(slot: &Slot, stop: &AtomicBool, tx: &Sender<Rendered>) {
	let mut renderer = LayerRenderer::default();
	loop {
		let request = {
			let (lock, wake) = &**slot;
			let mut guard = wavedude_core::sync::lock(lock);
			loop {
				if stop.load(Ordering::Acquire) {
					return;
				}
				if let Some(r) = guard.take() {
					break r;
				}
				guard = wake.wait(guard).unwrap();
			}
		};
		// A panic loses the request's pictures, not the thread; the scratch state it left is dropped.
		let rendered = catch_unwind(AssertUnwindSafe(|| renderer.present(&request))).unwrap_or_else(|_| {
			tracing::error!("the preview presenter panicked");
			renderer = LayerRenderer::default();
			Rendered::empty(&request)
		});
		if tx.send(rendered).is_err() {
			return;
		}
	}
}

/// The renderer and the picture set of the last request.
#[cfg(target_arch = "wasm32")]
pub struct Presenter {
	renderer: LayerRenderer,
	done: Option<Rendered>,
}

#[cfg(target_arch = "wasm32")]
impl Presenter {
	pub fn start() -> std::io::Result<Self> {
		Ok(Self { renderer: LayerRenderer::default(), done: None })
	}

	/// Renders `request`.
	pub fn request(&mut self, request: Request) {
		self.done = Some(self.renderer.present(&request));
	}

	/// The picture set of the last request, once.
	pub fn latest(&mut self) -> Option<Rendered> {
		self.done.take()
	}
}

/// An outline scaled to a picture: the source, the picture's size, the RGBA pixels.
type ScaledOutline = (Weak<EdgeImage>, (u32, u32), Option<Window>, Vec<u8>);

/// Pictures of a layer kept to draw into again once the UI has let go of them.
const RECYCLED: usize = 3;

/// Per-layer scratch state kept between frames: the screen renderers, the scaled outlines, a spilled frame's
/// plane and the pictures handed out.
#[derive(Default)]
struct LayerRenderer {
	screens: HashMap<Layer, ScreenRenderer>,
	outlines: HashMap<Layer, ScaledOutline>,
	spill: Vec<u8>,
	/// Per layer, oldest first.
	given: HashMap<Layer, VecDeque<Bytes>>,
}

impl LayerRenderer {
	fn present(&mut self, request: &Request) -> Rendered {
		let started = Instant::now();
		let source = request.source.key();
		let mut pending = false;
		let pictures: Vec<Picture> = request
			.layers
			.iter()
			.filter_map(|l| {
				let picture = self.render(&request.source, l);
				pending |= matches!(picture, Err(NoPlane::Pending));
				picture.ok()
			})
			.collect();
		let vanished = !pending && pictures.len() < request.layers.len() && request.source.vanished_since(&source);
		Rendered {
			source,
			pictures,
			vanished,
			pending,
			render: started.elapsed(),
			requested: request.at,
			motion: request.motion,
		}
	}

	fn render(&mut self, source: &FrameSource, req: &LayerRequest) -> Result<Picture, NoPlane> {
		let shape = source.shape();
		let view = View::of(&shape, req.device, req.window, req.dither_bits);
		let (width, height, window) = (view.width, view.height, view.window);
		let mut rgba = self.buffer(req.layer, view.bytes());
		let screen = self.screens.entry(req.layer).or_insert_with(|| ScreenRenderer::new(1, 1, 1, 1));
		let drawn = source.with_plane_into(req.layer, &mut self.spill, |bytes| {
			paint(bytes, &shape, req.layer, &req.colours, &view, screen, &mut rgba);
		});
		drawn?;
		if let Some(outline) = &req.outline {
			let e = &outline.edges;
			let stale = self.outlines.get(&req.layer).is_none_or(|(edges, size, win, _)| {
				!std::ptr::eq(edges.as_ptr(), Arc::as_ptr(e)) || *size != (width, height) || *win != window
			});
			if stale {
				let scaled = e.scaled(window, (width, height));
				self.outlines.insert(req.layer, (Arc::downgrade(e), (width, height), window, scaled));
			}
			if let Some((_, _, _, scaled)) = self.outlines.get(&req.layer) {
				glowing_edges::blend_additive(&mut rgba, scaled, outline.gain);
			}
		}
		let rgba = rgba.freeze();
		let given = self.given.entry(req.layer).or_default();
		given.push_back(rgba.clone());
		if given.len() > RECYCLED {
			given.pop_front();
		}
		Ok(Picture { layer: req.layer, width, height, rgba, window })
	}

	/// A buffer of `len` bytes: a picture of `layer` the UI no longer holds, else a new one.
	fn buffer(&mut self, layer: Layer, len: usize) -> BytesMut {
		let given = self.given.entry(layer).or_default();
		for _ in 0..given.len() {
			match given.pop_front().expect("counted").try_into_mut() {
				Ok(mut free) => {
					free.resize(len, 0);
					return free;
				}
				Err(held) => given.push_back(held),
			}
		}
		BytesMut::zeroed(len)
	}
}

/// Pictures between two log lines.
pub const LOG_EVERY: u32 = 100;

/// The presenter's figures over a block of [`LOG_EVERY`] pictures: render time, the wait from the request to the
/// picture on the UI thread (less the render), and the gap between two presented pictures in motion. A stutter
/// is a gap, which an average hides, so the worst of each is kept.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresenterStats {
	count: u32,
	render_sum: Duration,
	render_worst: Duration,
	wait_sum: Duration,
	wait_worst: Duration,
	gap_worst: Duration,
	last_shown: Option<Instant>,
}

impl PresenterStats {
	/// Counts a picture set presented at `now`; returns the log line when a block is full.
	pub fn record(&mut self, rendered: &Rendered, now: Instant) -> Option<String> {
		let wait = now.saturating_duration_since(rendered.requested).saturating_sub(rendered.render);
		self.count += 1;
		self.render_sum += rendered.render;
		self.render_worst = self.render_worst.max(rendered.render);
		self.wait_sum += wait;
		self.wait_worst = self.wait_worst.max(wait);
		if rendered.motion
			&& let Some(last) = self.last_shown
		{
			self.gap_worst = self.gap_worst.max(now.saturating_duration_since(last));
		}
		self.last_shown = rendered.motion.then_some(now);
		if self.count < LOG_EVERY {
			return None;
		}
		let ms = |d: Duration| d.as_secs_f64() * 1e3;
		let n = f64::from(self.count);
		let line = format!(
			"presenter: {} frames, render {:.2} ms average, {:.2} worst; wait {:.2} ms average, {:.2} worst; gap {:.1} ms worst",
			self.count,
			ms(self.render_sum) / n,
			ms(self.render_worst),
			ms(self.wait_sum) / n,
			ms(self.wait_worst),
			ms(self.gap_worst),
		);
		*self = Self { last_shown: self.last_shown, ..Self::default() };
		Some(line)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use wavedude_core::engine::SampleFormat;
	use wavedude_core::f16;
	use wavedude_core::model::ColorScheme;

	#[test]
	fn halves_read_from_aligned_and_unaligned_bytes() {
		let values = [f16::from_f32(1.5), f16::from_f32(-2.0)];
		let mut bytes: Vec<u8> = vec![0];
		bytes.extend(values.iter().flat_map(|v| v.to_bits().to_ne_bytes()));
		let unaligned = as_halves(&bytes[1..]);
		let aligned_copy: Vec<u8> = bytes[1..].to_vec();
		let aligned = as_halves(&aligned_copy);
		assert_eq!(&*unaligned, &values);
		assert_eq!(&*aligned, &values);
	}

	fn shape() -> FrameShape {
		FrameShape {
			canvas_width: 4,
			canvas_height: 4,
			divisor: 1,
			format: SampleFormat::Index,
			field_brightness: 1.,
			intensity_brightness: 1.,
			intensity_root: false,
		}
	}

	fn layer(layer: Layer) -> LayerRequest {
		LayerRequest {
			layer,
			colours: LayerColours { scheme: ColorScheme::default(), black_level: 0., brightness: 1. },
			device: (0, 0),
			window: None,
			dither_bits: 0,
			outline: None,
		}
	}

	fn present(presenter: &mut Presenter, source: FrameSource, layers: Vec<LayerRequest>) -> Rendered {
		presenter.request(Request { source, layers, motion: false, at: Instant::now() });
		let deadline = Instant::now() + Duration::from_secs(10);
		loop {
			if let Some(r) = presenter.latest() {
				return r;
			}
			assert!(Instant::now() < deadline, "the presenter did not answer");
			std::thread::sleep(Duration::from_millis(1));
		}
	}

	#[test]
	fn only_a_frame_gone_from_its_store_counts_as_vanished() {
		let mut presenter = Presenter::start().unwrap();
		let mut snapshot = Snapshot::new(shape());
		snapshot.field = vec![0; 16];
		let snapshot = FrameSource::Snapshot(Arc::new(snapshot));
		let none = present(&mut presenter, snapshot.clone(), Vec::new());
		assert!(none.pictures.is_empty() && !none.vanished, "no layer to draw is not a vanished frame");
		let no_plane = present(&mut presenter, snapshot, vec![layer(Layer::Intensity)]);
		assert!(no_plane.pictures.is_empty() && !no_plane.vanished, "a missing plane is not a vanished frame");
		let dir = tempfile::tempdir().unwrap();
		let layout =
			wavedude_core::run::StoreLayout { shape: shape(), layers: &[Layer::Field], capacity: 2, capture_every: 1 };
		let store = Arc::new(FrameStore::in_dir(layout, 1 << 20, None, dir.path()).unwrap());
		let gone = present(&mut presenter, FrameSource::Run { store, stored: 0 }, vec![layer(Layer::Field)]);
		assert!(gone.vanished);
	}

	fn rendered(at: Instant, render_ms: u64, motion: bool) -> Rendered {
		Rendered {
			source: SourceKey::Snapshot(0),
			pictures: Vec::new(),
			vanished: false,
			pending: false,
			render: Duration::from_millis(render_ms),
			requested: at,
			motion,
		}
	}

	#[test]
	fn stats_report_averages_and_the_worst_gap_every_block() {
		let mut stats = PresenterStats::default();
		let t0 = Instant::now();
		let mut line = None;
		for i in 0..LOG_EVERY {
			let at = t0 + Duration::from_millis(u64::from(i) * 16 + if i == 50 { 100 } else { 0 });
			let shown = at + Duration::from_millis(5);
			line = stats.record(&rendered(at, 4, true), shown);
			if i + 1 < LOG_EVERY {
				assert!(line.is_none());
			}
		}
		let line = line.expect("a full block logs");
		assert!(line.contains("render 4.00 ms average, 4.00 worst"), "{line}");
		assert!(line.contains("wait 1.00 ms average, 1.00 worst"), "{line}");
		assert!(line.contains("gap 116.0 ms worst"), "{line}");
		assert_eq!(stats.count, 0);
	}

	#[test]
	fn idle_pictures_do_not_count_as_gaps() {
		let mut stats = PresenterStats::default();
		let t0 = Instant::now();
		stats.record(&rendered(t0, 1, false), t0);
		stats.record(&rendered(t0, 1, true), t0 + Duration::from_secs(5));
		assert_eq!(stats.gap_worst, Duration::ZERO);
		stats.record(&rendered(t0, 1, true), t0 + Duration::from_secs(5) + Duration::from_millis(20));
		assert_eq!(stats.gap_worst, Duration::from_millis(20));
	}
}
