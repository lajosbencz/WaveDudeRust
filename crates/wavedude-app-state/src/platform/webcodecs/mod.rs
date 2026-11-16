//! H.264 through the browser's WebCodecs `VideoEncoder` ([`WebCodecsFactory`]): Annex B output, the lowest codec
//! string level that holds the picture size and rate, variable bitrate at the export quality's bitrate, an IDR every
//! `fps` pictures, I420 pictures in BT.601 studio range. The supported profiles at a size and rate are asked once with
//! `VideoEncoder.isConfigSupported` ([`support`]).
//!
//! The encoder takes a picture while fewer than [`QUEUED`] encode requests wait for it (`encodeQueueSize`), each
//! `dequeue` waking the window: an encoder may hold its output until later pictures or a flush come, so the pictures
//! not out yet do not hold the next back (decision 0005, The browser encoder's queue). Its output arrives in
//! callbacks, queued ([`Chunks`]) and drained in the order it came, which is decode order: an encoder coding B-frames
//! gives its pictures out of display order (decision 0005, B-frames and the MP4 muxer), so each chunk's timestamp
//! names its picture. Each callback wakes the window ([`crate::wake`]).

#[cfg(target_arch = "wasm32")]
mod encoder;

#[cfg(target_arch = "wasm32")]
pub use encoder::{Support, WebCodecsFactory, support};

use std::collections::{BTreeSet, VecDeque};

use wavedude_export::h264::{AccessUnit, Profile, Yuv420};
use wavedude_export::{ExportError, Result};

/// Encode requests waiting for the encoder at which it is not ready.
const QUEUED: u32 = 6;

/// Of each level of H.264 Table A-1 but 1b: `level_idc`, MaxMBPS (macroblocks/s), MaxFS (macroblocks).
const LEVELS: [(u8, u64, u64); 19] = [
	(10, 1_485, 99),
	(11, 3_000, 396),
	(12, 6_000, 396),
	(13, 11_880, 396),
	(20, 11_880, 396),
	(21, 19_800, 792),
	(22, 20_250, 1_620),
	(30, 40_500, 1_620),
	(31, 108_000, 3_600),
	(32, 216_000, 5_120),
	(40, 245_760, 8_192),
	(41, 245_760, 8_192),
	(42, 522_240, 8_704),
	(50, 589_824, 22_080),
	(51, 983_040, 36_864),
	(52, 2_073_600, 36_864),
	(60, 4_177_920, 139_264),
	(61, 8_355_840, 139_264),
	(62, 16_711_680, 139_264),
];

/// The `level_idc` of the lowest level whose MaxFS, MaxMBPS and side limit (A.3.1: each side at most
/// `sqrt(8 * MaxFS)` macroblocks) hold `width x height` at `fps`; `None` beyond level 6.2.
fn level(width: u32, height: u32, fps: u32) -> Option<u8> {
	let (w, h) = (u64::from(width.div_ceil(16)), u64::from(height.div_ceil(16)));
	let (frame, rate) = (w * h, w * h * u64::from(fps));
	LEVELS
		.iter()
		.find(|&&(_, mbps, fs)| frame <= fs && rate <= mbps && w * w <= 8 * fs && h * h <= 8 * fs)
		.map(|&(idc, ..)| idc)
}

/// The `avc1.PPCCLL` codec string of `profile` at `width x height` and `fps`; `None` beyond level 6.2.
fn codec(profile: Profile, width: u32, height: u32, fps: u32) -> Option<String> {
	let (idc, constraints) = match profile {
		Profile::Baseline => (0x42, 0xE0),
		Profile::Main => (0x4D, 0x00),
		Profile::High => (0x64, 0x00),
	};
	level(width, height, fps).map(|level| format!("avc1.{idc:02X}{constraints:02X}{level:02X}"))
}

/// Picture `index` starts a group of `fps` pictures.
fn is_key(index: u64, fps: u32) -> bool {
	index.is_multiple_of(u64::from(fps))
}

/// Picture `index`'s time in microseconds, rounded.
fn timestamp(index: u64, fps: u32) -> i64 {
	let fps = u64::from(fps);
	((index * 1_000_000 + fps / 2) / fps) as i64
}

/// The index of the picture whose [`timestamp`] is `time`; `None` when no picture is at that time.
fn picture_at(time: i64, fps: u32) -> Option<u64> {
	let index = u64::try_from(time).ok()?.checked_mul(u64::from(fps))?.checked_add(500_000)? / 1_000_000;
	(timestamp(index, fps) == time).then_some(index)
}

/// `picture`'s planes one after the other, as an I420 `VideoFrame` without a layout takes them.
fn pack_i420(picture: &Yuv420, out: &mut Vec<u8>) {
	out.clear();
	out.reserve(picture.y.len() + picture.cb.len() + picture.cr.len());
	for plane in [&picture.y, &picture.cb, &picture.cr] {
		out.extend_from_slice(plane);
	}
}

/// What came out of the encoder and was not drained, the pictures still in it and its first failure, which every
/// later call returns. A chunk's timestamp names its picture, one of those submitted; a time no picture has or a
/// picture given twice fails the encoding.
#[derive(Debug)]
struct Chunks {
	fps: u32,
	/// Annex B access units, whether each is a sync sample, and its picture's index.
	queue: VecDeque<(Vec<u8>, bool, u64)>,
	submitted: u64,
	/// Every picture below came out.
	through: u64,
	/// The pictures from `through + 1` on that came out.
	ahead: BTreeSet<u64>,
	failure: Option<String>,
}

impl Chunks {
	fn new(fps: u32) -> Self {
		Self { fps, queue: VecDeque::new(), submitted: 0, through: 0, ahead: BTreeSet::new(), failure: None }
	}

	/// The first failure.
	fn check(&self) -> Result<()> {
		self.failure.as_ref().map_or(Ok(()), |e| Err(ExportError::H264(e.clone())))
	}

	/// Keeps the first failure.
	fn fail(&mut self, message: String) {
		self.failure.get_or_insert(message);
	}

	/// The next picture's index, and whether it starts a group; counts it as submitted.
	fn submit(&mut self) -> (u64, bool) {
		let index = self.submitted;
		self.submitted += 1;
		(index, is_key(index, self.fps))
	}

	/// Pictures that came out.
	fn received(&self) -> u64 {
		self.through + self.ahead.len() as u64
	}

	/// Queues the access unit `data` of the picture at `time` microseconds.
	fn push(&mut self, time: i64, data: Vec<u8>, sync: bool) {
		let Some(index) = picture_at(time, self.fps).filter(|&i| i < self.submitted) else {
			self.fail(format!(
				"the browser's encoder gave a picture at {time} us, where none of the {} submitted is",
				self.submitted
			));
			return;
		};
		if index < self.through || !self.ahead.insert(index) {
			self.fail(format!("the browser's encoder gave the picture at {time} us twice"));
			return;
		}
		while self.ahead.remove(&self.through) {
			self.through += 1;
		}
		self.queue.push_back((data, sync, index));
	}

	/// Takes a picture while fewer than [`QUEUED`] encode requests wait for the encoder.
	fn ready(&self, queued: u32) -> Result<bool> {
		self.check()?;
		Ok(queued < QUEUED)
	}

	/// Hands the queued access units to `out` in the order they came.
	fn drain(&mut self, out: &mut dyn FnMut(AccessUnit<'_>) -> Result<()>) -> Result<()> {
		self.check()?;
		while let Some((stream, sync, picture)) = self.queue.pop_front() {
			out(AccessUnit { stream: &stream, sync, picture })?;
		}
		Ok(())
	}

	/// Every picture submitted came out, after the flush.
	fn flushed(&self) -> Result<()> {
		self.check()?;
		if self.received() == self.submitted {
			Ok(())
		} else {
			Err(ExportError::H264(format!(
				"the browser's encoder gave {} of {} pictures",
				self.received(),
				self.submitted
			)))
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_level_is_the_lowest_that_holds_the_size_and_rate() {
		assert_eq!(level(176, 144, 15), Some(10));
		assert_eq!(level(352, 288, 30), Some(13));
		assert_eq!(level(640, 480, 30), Some(30));
		assert_eq!(level(1280, 720, 30), Some(31));
		assert_eq!(level(1920, 1080, 30), Some(40));
		assert_eq!(level(1920, 1080, 60), Some(42));
		assert_eq!(level(3840, 2160, 30), Some(51));
		assert_eq!(level(3840, 2160, 60), Some(52));
		assert_eq!(level(7680, 4320, 30), Some(60));
		assert_eq!(level(8192, 4320, 120), Some(62));
		assert_eq!(level(8192, 4320, 300), None);
		// 125 macroblocks fit level 1.1's frame, but a side of 125 needs 8 * MaxFS >= 125^2: level 3.1.
		assert_eq!(level(2000, 16, 1), Some(31));
	}

	#[test]
	fn the_codec_string_names_profile_constraints_and_level() {
		assert_eq!(codec(Profile::Baseline, 1280, 720, 30).as_deref(), Some("avc1.42E01F"));
		assert_eq!(codec(Profile::Main, 1920, 1080, 30).as_deref(), Some("avc1.4D0028"));
		assert_eq!(codec(Profile::High, 3840, 2160, 60).as_deref(), Some("avc1.640034"));
		assert_eq!(codec(Profile::High, 16384, 16384, 60), None);
	}

	#[test]
	fn every_fps_th_picture_is_a_key_frame_at_its_time() {
		let keys: Vec<u64> = (0..12).filter(|&i| is_key(i, 5)).collect();
		assert_eq!(keys, [0, 5, 10]);
		assert_eq!([0, 1, 2, 3].map(|i| timestamp(i, 30)), [0, 33_333, 66_667, 100_000]);
		assert_eq!(timestamp(90_000, 25), 3_600_000_000);
	}

	#[test]
	fn a_time_names_the_picture_there_and_no_other() {
		for fps in [1, 24, 30, 60, 144, 240] {
			for index in [0, 1, 2, 7, 59, 1_000, 216_000] {
				assert_eq!(picture_at(timestamp(index, fps), fps), Some(index), "{index} at {fps} fps");
			}
		}
		assert_eq!(picture_at(33_334, 30), None, "a microsecond late");
		assert_eq!(picture_at(-1, 30), None);
		assert_eq!(picture_at(i64::MAX, 240), None);
	}

	#[test]
	fn i420_is_the_three_planes_in_a_row() {
		let picture = Yuv420 { width: 4, height: 2, y: (0..8).collect(), cb: vec![20, 21], cr: vec![30, 31] };
		let mut out = vec![99; 3];
		pack_i420(&picture, &mut out);
		assert_eq!(out, [0, 1, 2, 3, 4, 5, 6, 7, 20, 21, 30, 31]);
	}

	fn submitted(chunks: &mut Chunks, n: u64) {
		for _ in 0..n {
			chunks.submit();
		}
	}

	/// The drained access units' bytes, sync flags and pictures.
	fn drained(chunks: &mut Chunks) -> Result<Vec<(Vec<u8>, bool, u64)>> {
		let mut got = Vec::new();
		chunks.drain(&mut |unit| {
			got.push((unit.stream.to_vec(), unit.sync, unit.picture));
			Ok(())
		})?;
		Ok(got)
	}

	#[test]
	fn pictures_held_by_the_encoder_do_not_hold_the_next_back() {
		let mut chunks = Chunks::new(2);
		assert_eq!(chunks.submit(), (0, true));
		assert_eq!(chunks.submit(), (1, false));
		submitted(&mut chunks, 40);
		assert!(chunks.ready(0).unwrap(), "42 pictures in the encoder, none out, none waiting for it");
		assert!(!chunks.ready(QUEUED).unwrap());
		chunks.push(0, vec![1], true);
		chunks.push(timestamp(1, 2), vec![2], false);
		assert_eq!(drained(&mut chunks).unwrap(), [(vec![1], true, 0), (vec![2], false, 1)]);
		assert!(chunks.flushed().is_err(), "pictures are still in the encoder");
	}

	#[test]
	fn pictures_out_of_display_order_keep_their_indices() {
		let mut chunks = Chunks::new(60);
		submitted(&mut chunks, 5);
		for index in [0, 4, 2, 1, 3] {
			chunks.push(timestamp(index, 60), vec![index as u8], index == 0);
		}
		let pictures: Vec<u64> = drained(&mut chunks).unwrap().into_iter().map(|(_, _, p)| p).collect();
		assert_eq!(pictures, [0, 4, 2, 1, 3], "decode order, each naming its picture");
		assert!(chunks.flushed().is_ok());
	}

	#[test]
	fn a_picture_given_twice_or_at_an_unknown_time_fails_every_later_call() {
		let mut chunks = Chunks::new(10);
		submitted(&mut chunks, 3);
		chunks.push(0, vec![1], true);
		chunks.push(timestamp(2, 10), vec![3], false);
		assert!(chunks.ready(0).is_ok(), "picture 1 may come later");
		chunks.push(timestamp(2, 10), vec![3], false);
		assert!(chunks.ready(0).is_err());
		assert!(chunks.drain(&mut |_| Ok(())).is_err());
		assert!(chunks.flushed().is_err());

		let mut chunks = Chunks::new(10);
		submitted(&mut chunks, 3);
		chunks.push(0, vec![1], true);
		chunks.push(0, vec![1], true);
		assert!(chunks.check().is_err(), "a picture below every one still out given again");

		for time in [timestamp(3, 10), 50_000, -100_000] {
			let mut chunks = Chunks::new(10);
			submitted(&mut chunks, 3);
			chunks.push(time, vec![1], true);
			assert!(chunks.check().is_err(), "no picture submitted at {time} us");
		}

		let mut chunks = Chunks::new(10);
		chunks.fail("first".into());
		chunks.fail("second".into());
		assert!(matches!(chunks.check(), Err(ExportError::H264(e)) if e == "first"));
	}

	#[test]
	fn a_flush_completes_once_every_picture_came_out() {
		let mut chunks = Chunks::new(1);
		submitted(&mut chunks, 3);
		chunks.push(0, vec![1], true);
		chunks.push(2_000_000, vec![3], true);
		assert!(chunks.flushed().is_err(), "picture 1 is missing");
		chunks.push(1_000_000, vec![2], true);
		assert!(chunks.flushed().is_ok());
	}
}
