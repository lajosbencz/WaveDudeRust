//! Playback (docs/ui/50, Playback, Live follow, Position rules): the position on the timeline in frames, speed, loop,
//! the in/out range and following the run.

use std::time::Instant;

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
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use super::*;

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
	fn following_tracks_the_run_and_a_seek_ends_it() {
		let mut p = Playback { frames: 100, ..Playback::default() };
		p.follow();
		p.tick(Instant::now(), 30, Some(42));
		assert_eq!(p.position, 42);
		p.seek(500);
		assert_eq!((p.position, p.live), (99, false));
	}
}
