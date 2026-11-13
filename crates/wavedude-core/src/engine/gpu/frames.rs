//! The frames a GPU engine captured last, kept on its device for render passes on the same queue (the live
//! preview colours them there instead of reading them back).
//!
//! A capture copies its planes, as the store gets them, into a free slot in the encoder that converts them; the
//! run tags the slot with the stored frame once the capture landed. A reader holds the slot of a tag until it holds
//! another or lets go; a capture never writes the slot held, the slot published last or a slot still pending, so
//! everything a reader's pass reads was submitted before it and nothing submitted after it changes it.
//!
//! Threads: the engine's thread captures and publishes, any thread holds; one lock over the slots.

use std::sync::{Arc, Mutex};

use crate::engine::gpu::GpuContext;
use crate::engine::{FrameShape, SampleFormat};
use crate::model::Layer;
use crate::sync::lock;

/// The stored frame a slot holds: the store's [`id`](crate::run::FrameStore::id), its generation and the index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTag {
	pub store: u64,
	pub generation: u64,
	pub stored: usize,
}

/// A captured frame's planes on the device, as the store holds them: packed words, two halves or four palette
/// indices to a word, rows top first, layer `l` from word `l * layer_words`.
#[derive(Clone, Debug)]
pub struct DeviceFrame {
	pub buffer: wgpu::Buffer,
	pub shape: FrameShape,
	/// The intensity plane follows the field's.
	pub intensity: bool,
	pub layer_words: u32,
}

impl DeviceFrame {
	/// The first word of `layer`'s plane; `None` when the frame has none.
	pub fn first_word(&self, layer: Layer) -> Option<u32> {
		match layer {
			Layer::Field => Some(0),
			Layer::Intensity => self.intensity.then_some(self.layer_words),
		}
	}
}

/// Words of a plane of `shape`, before the padding of a layer.
pub(crate) fn plane_words(shape: &FrameShape) -> u32 {
	let per_word = match shape.format {
		SampleFormat::Half => 2,
		SampleFormat::Index => 4,
	};
	(shape.plane_samples() as u32).div_ceil(per_word)
}

#[derive(Default)]
struct Slot {
	frame: Option<DeviceFrame>,
	tag: Option<FrameTag>,
}

/// The slot published last, the slot held, and one per capture pending: as many as the engine's captures
/// pending allow, made as they are needed.
#[derive(Default)]
struct Slots {
	slots: Vec<Slot>,
	latest: Option<usize>,
	held: Option<usize>,
	/// By the engine's capture slot: where its pending capture copies to.
	pending: Vec<Option<usize>>,
	/// The slot of the capture finished last, until it is published or the next capture begins.
	finished: Option<usize>,
}

impl Slots {
	fn busy(&self, s: usize) -> bool {
		self.latest == Some(s) || self.held == Some(s) || self.pending.contains(&Some(s))
	}
}

/// See the module docs.
pub struct DeviceFrames {
	context: Arc<GpuContext>,
	slots: Mutex<Slots>,
}

impl DeviceFrames {
	pub(crate) fn new(context: Arc<GpuContext>) -> Self {
		Self { context, slots: Mutex::default() }
	}

	/// The context whose device the planes live on. A reader on another device compares contexts: `wgpu::Device`'s
	/// equality compares ids, which repeat across instances.
	pub fn context(&self) -> &Arc<GpuContext> {
		&self.context
	}

	/// A free slot for the capture of `shape` in the engine's capture slot `capture`, made large enough for
	/// `bytes`: the buffer to copy the planes to. A capture of `capture` not finished before is abandoned.
	pub(crate) fn begin(
		&self,
		capture: usize,
		shape: FrameShape,
		intensity: bool,
		layer_words: u32,
		bytes: u64,
	) -> wgpu::Buffer {
		let mut s = lock(&self.slots);
		if s.pending.len() <= capture {
			s.pending.resize(capture + 1, None);
		}
		s.pending[capture] = None;
		s.finished = None;
		let free = (0..s.slots.len()).find(|&i| !s.busy(i)).unwrap_or_else(|| {
			s.slots.push(Slot::default());
			s.slots.len() - 1
		});
		let slot = &mut s.slots[free];
		let buffer = match slot.frame.take() {
			Some(f) if f.buffer.size() >= bytes => f.buffer,
			_ => self.context.device.create_buffer(&wgpu::BufferDescriptor {
				label: Some("device frame"),
				size: bytes.next_multiple_of(4).max(4),
				usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
				mapped_at_creation: false,
			}),
		};
		slot.frame = Some(DeviceFrame { buffer: buffer.clone(), shape, intensity, layer_words });
		slot.tag = None;
		s.pending[capture] = Some(free);
		buffer
	}

	/// The capture of `capture` was collected.
	pub(crate) fn finish(&self, capture: usize) {
		let mut s = lock(&self.slots);
		s.finished = s.pending.get_mut(capture).and_then(Option::take);
	}

	/// Tags the frame of the capture finished last as `tag` and makes it the latest; nothing when that capture
	/// was not kept (another began since).
	pub fn publish(&self, tag: FrameTag) {
		let mut s = lock(&self.slots);
		let Some(new) = s.finished.take() else { return };
		for slot in &mut s.slots {
			if slot.tag == Some(tag) {
				slot.tag = None;
			}
		}
		s.slots[new].tag = Some(tag);
		s.latest = Some(new);
	}

	/// The frame of `tag`, held (no capture writes it) until the next `hold` or [`release`](Self::release);
	/// `None`, holding nothing, when no slot keeps it.
	pub fn hold(&self, tag: FrameTag) -> Option<DeviceFrame> {
		let mut s = lock(&self.slots);
		let found = s.slots.iter().position(|slot| slot.tag == Some(tag));
		s.held = found;
		found.and_then(|i| s.slots[i].frame.clone())
	}

	/// Holds nothing.
	pub fn release(&self) {
		lock(&self.slots).held = None;
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn tag(stored: usize) -> FrameTag {
		FrameTag { store: 1, generation: 0, stored }
	}

	/// The slots in use.
	fn busy(f: &DeviceFrames) -> usize {
		let s = lock(&f.slots);
		(0..s.slots.len()).filter(|&i| s.busy(i)).count()
	}

	const SHAPE: FrameShape = FrameShape {
		canvas_width: 8,
		canvas_height: 4,
		divisor: 1,
		format: SampleFormat::Half,
		field_brightness: 1.,
		intensity_brightness: 1.,
		intensity_root: false,
	};

	#[test]
	fn slots_cycle_without_touching_the_held_published_or_pending_ones() {
		let Ok(ctx) = super::super::context() else {
			eprintln!("skipped: no GPU adapter");
			return;
		};
		let frames = DeviceFrames::new(ctx);
		let shape = SHAPE;
		let mut held = None;
		for k in 0..20 {
			let capture = k % 2;
			let buffer = frames.begin(capture, shape, false, 64, 256);
			assert!(held.as_ref().is_none_or(|h: &DeviceFrame| h.buffer != buffer), "capture {k} wrote the held slot");
			if k >= 1 {
				frames.finish(1 - capture);
				frames.publish(tag(k - 1));
			}
			if k == 5 {
				held = frames.hold(tag(4));
				assert!(held.is_some());
			}
			assert!(busy(&frames) <= 3);
		}
		assert!(frames.hold(tag(4)).is_some(), "the held frame stays");
		assert!(frames.hold(tag(1)).is_none(), "an old frame is gone");
		assert!(frames.hold(tag(18)).is_some(), "the latest is there");
		frames.finish(1);
		frames.begin(0, shape, false, 64, 256);
		frames.publish(tag(19));
		assert!(frames.hold(tag(19)).is_none(), "a capture begun after the finish drops it");
	}

	#[test]
	fn each_capture_pending_has_a_slot_of_its_own() {
		let Ok(ctx) = super::super::context() else {
			eprintln!("skipped: no GPU adapter");
			return;
		};
		let frames = DeviceFrames::new(ctx);
		let buffers: Vec<_> = (0..6).map(|capture| frames.begin(capture, SHAPE, false, 64, 256)).collect();
		assert_eq!(busy(&frames), 6);
		assert!(buffers.iter().enumerate().all(|(i, b)| buffers[..i].iter().all(|a| a != b)), "a slot shared");
		for capture in 0..6 {
			frames.finish(capture);
			frames.publish(tag(capture));
		}
		assert_eq!(busy(&frames), 1, "the latest alone");
		assert!((0..6).all(|k| frames.hold(tag(k)).is_some()), "each landed frame is kept until its slot is reused");
	}
}
