//! Playback (docs/ui/50, Playback, Live follow, Position rules): the position on the timeline in frames, speed, loop,
//! the in/out range and following the run.

use web_time::Instant;

/// The playback state of the timeline.
#[derive(Debug)]
pub(crate) struct Playback {
	pub(crate) position: u32,
	pub(crate) playing: bool,
	pub(crate) live: bool,
	pub(crate) looping: bool,
	pub(crate) speed: f64,
	/// The in point; `None` is the first frame.
	pub(crate) start: Option<u32>,
	/// The out point; `None` is the last frame.
	pub(crate) end: Option<u32>,
	/// Frames of the planned run.
	pub(crate) frames: u32,
	clock: Option<(Instant, f64)>,
	/// The frames the last tick that moved moved by.
	stride: u32,
}

impl Default for Playback {
	fn default() -> Self {
		Self {
			position: 0,
			playing: false,
			live: false,
			looping: false,
			speed: 1.0,
			start: None,
			end: None,
			frames: 0,
			clock: None,
			stride: 1,
		}
	}
}

impl Playback {
	pub(crate) fn last(&self) -> u32 {
		self.frames.saturating_sub(1)
	}

	/// The in and out points, the whole run when unset.
	pub(crate) fn range(&self) -> (u32, u32) {
		let end = self.end.unwrap_or(self.last()).min(self.last());
		(self.start.unwrap_or(0).min(end), end)
	}

	/// Frame 0, stopped: a new, opened or closed project.
	pub(crate) fn reset(&mut self) {
		*self = Self { speed: self.speed, looping: self.looping, ..Self::default() };
	}

	/// A run the user started: frame 0, following it.
	pub(crate) fn follow(&mut self) {
		self.position = 0;
		self.live = true;
		self.pause();
	}

	/// The user moves the position: Live and playback stop.
	pub(crate) fn seek(&mut self, frame: i64) {
		self.position = u32::try_from(frame.max(0)).unwrap_or(u32::MAX).min(self.last());
		self.live = false;
		self.pause();
	}

	pub(crate) fn play(&mut self) {
		let (start, end) = self.range();
		if self.position >= end || self.position < start {
			self.position = start;
		}
		self.live = false;
		self.playing = true;
		self.clock = Some((Instant::now(), 0.0));
	}

	pub(crate) fn pause(&mut self) {
		self.playing = false;
		self.clock = None;
	}

	pub(crate) fn set_in(&mut self) {
		self.start = Some(self.position);
		if self.end.is_some_and(|e| e < self.position) {
			self.end = None;
		}
	}

	pub(crate) fn set_out(&mut self) {
		self.end = (self.position < self.last()).then_some(self.position);
		if self.start.is_some_and(|s| s > self.position) {
			self.start = None;
		}
	}

	/// Moves the in point (or the out point with `end`) to `frame`, never past the other; at the run's ends it is
	/// unset.
	pub(crate) fn drag_bracket(&mut self, end: bool, frame: i64) {
		let (start, stop) = self.range();
		let frame = u32::try_from(frame.max(0)).unwrap_or(u32::MAX).min(self.last());
		if end {
			let frame = frame.max(start);
			self.end = (frame < self.last()).then_some(frame);
		} else {
			let frame = frame.min(stop);
			self.start = (frame > 0).then_some(frame);
		}
	}

	/// Moves the position by the frames that fell due at `fps` frames a second at speed 1, holding at `made`, the
	/// newest frame the run holds; while following, the position is `made`.
	pub(crate) fn tick(&mut self, now: Instant, fps: u32, made: Option<u32>) {
		if self.live {
			if let Some(made) = made {
				self.position = made;
			}
			return;
		}
		let Some((since, carried)) = self.clock else { return };
		let due = carried + now.duration_since(since).as_secs_f64() * f64::from(fps) * self.speed;
		let whole = due.floor();
		self.clock = Some((now, due - whole));
		if whole >= 1.0 {
			self.stride = whole as u32;
		}
		let (start, end) = self.range();
		let mut next = self.position.saturating_add(whole as u32);
		if next > end {
			if self.looping {
				next = start + (next - end - 1) % (end - start + 1);
			} else {
				next = end;
				self.pause();
			}
		}
		if let Some(made) = made {
			next = next.min(made.max(self.position));
		}
		self.position = next;
	}

	/// The frames playback shows next while playing: the position and up to `count` after it at the stride of the
	/// last tick, wrapped to the in point with Loop on, never past the out point or `made`; none when stopped.
	pub(crate) fn ahead(&self, count: usize, made: Option<u32>) -> Vec<u32> {
		if !self.playing {
			return Vec::new();
		}
		let (start, end) = self.range();
		let mut frames = vec![self.position];
		let mut at = self.position;
		while frames.len() <= count {
			let mut next = at.saturating_add(self.stride);
			if next > end {
				match (self.looping, at < end) {
					(true, _) => next = start + (next - end - 1) % (end - start + 1),
					(false, true) => next = end,
					(false, false) => break,
				}
			}
			if made.is_some_and(|m| next > m) {
				break;
			}
			frames.push(next);
			at = next;
		}
		frames
	}
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use super::*;

	#[test]
	fn a_bracket_stops_at_the_other_and_unsets_at_the_ends() {
		let mut p = Playback { frames: 100, start: Some(20), end: Some(60), ..Playback::default() };
		p.drag_bracket(false, 80);
		assert_eq!(p.start, Some(60));
		p.drag_bracket(false, -5);
		assert_eq!(p.start, None);
		p.drag_bracket(true, 200);
		assert_eq!(p.end, None);
		p.drag_bracket(true, 30);
		assert_eq!(p.end, Some(30));
	}

	fn playing(frames: u32) -> (Playback, Instant) {
		let mut p = Playback { frames, ..Playback::default() };
		p.play();
		let t = p.clock.unwrap().0;
		(p, t)
	}

	#[test]
	fn a_second_at_speed_one_moves_by_the_frame_rate_and_stops_at_the_out_point() {
		let (mut p, t) = playing(100);
		p.tick(t + Duration::from_secs(1), 30, None);
		assert_eq!(p.position, 30);
		p.tick(t + Duration::from_secs(10), 30, None);
		assert_eq!((p.position, p.playing), (99, false));
	}

	#[test]
	fn loop_wraps_into_the_range_and_playback_waits_for_the_run() {
		let (mut p, t) = playing(100);
		p.looping = true;
		p.start = Some(10);
		p.end = Some(19);
		p.position = 18;
		p.tick(t + Duration::from_millis(100), 30, None);
		assert_eq!(p.position, 11);
		let (mut p, t) = playing(100);
		p.tick(t + Duration::from_secs(1), 30, Some(5));
		assert_eq!(p.position, 5);
	}

	#[test]
	fn the_frames_ahead_follow_the_last_stride_wrap_with_loop_and_stop_at_the_out_point_or_the_run() {
		let (mut p, t) = playing(100);
		assert_eq!(p.ahead(3, None), [0, 1, 2, 3], "the position first, at a stride of 1 before a tick");
		p.speed = 4.0;
		p.tick(t + Duration::from_millis(100), 10, None);
		assert_eq!(p.ahead(3, None), [4, 8, 12, 16]);
		assert_eq!(p.ahead(3, Some(13)), [4, 8, 12], "never past the newest frame of the run");
		p.end = Some(10);
		assert_eq!(p.ahead(3, None), [4, 8, 10], "the out point ends playback");
		p.looping = true;
		p.start = Some(2);
		assert_eq!(p.ahead(3, None), [4, 8, 3, 7], "wrapped to the in point");
		p.pause();
		assert!(p.ahead(3, None).is_empty());
	}

	#[test]
	fn following_tracks_the_run_and_a_seek_ends_it() {
		let mut p = Playback { frames: 100, ..Playback::default() };
		p.follow();
		p.tick(Instant::now(), 30, Some(42));
		assert_eq!(p.position, 42);
		p.seek(500);
		assert_eq!((p.position, p.live), (99, false));
	}
}
