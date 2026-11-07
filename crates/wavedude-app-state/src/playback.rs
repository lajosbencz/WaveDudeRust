//! Playback: the position on the timeline in frames, speed, loop, the in/out points in steps and the band of
//! available frames (docs/ui/50-timeline-transport.md).
//!
//! The app's tick loop advances playback; it measures wall-clock time between ticks, so a late or skipped tick
//! only accumulates. Threads: [`Playback`] lives on the UI thread.

use std::time::Instant;

pub const SPEED_RANGE: (f64, f64) = (0.01, 100.0);
pub const SPEED_PRESETS: [f64; 6] = [0.25, 0.5, 1.0, 1.5, 2.0, 4.0];

/// What the transport bar's readout shows: frame, last frame and the simulation step of the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Readout {
	pub frame: u32,
	pub last: u32,
	pub step: u32,
}

impl Readout {
	/// "frame / last".
	pub fn frames_text(&self) -> String {
		format!("{} / {}", self.frame, self.last)
	}

	/// "step N", shown dimmer beside the frames.
	pub fn step_text(&self) -> String {
		format!("step {}", self.step)
	}
}

/// The frames a run has published, shaded on the timeline (grey while stale).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Available {
	pub frames: u32,
	pub stale: bool,
}

/// The timeline's state. Positions in frames, the in/out points in steps (as the project stores them).
#[derive(Clone, Debug)]
pub struct Playback {
	frame_count: u32,
	visualize_every: u32,
	fps: f64,
	position: u32,
	speed: f64,
	looping: bool,
	in_step: u32,
	out_step: u32,
	playing: bool,
	available: Option<Available>,
	last_tick: Option<Instant>,
	carry: f64,
}

impl Default for Playback {
	fn default() -> Self {
		Self {
			frame_count: 1,
			visualize_every: 1,
			fps: 60.0,
			position: 0,
			speed: 1.0,
			looping: false,
			in_step: 0,
			out_step: 0,
			playing: false,
			available: None,
			last_tick: None,
			carry: 0.0,
		}
	}
}

/// The newest frame there is to play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
	/// More frames follow: playback holds here.
	Hold(u32),
	/// No more frames follow: playback ends here.
	End(u32),
}

impl Playback {
	/// The timeline of a project: its planned frames, steps per frame, video frame rate (speed 1) and export
	/// range in steps, each count at least 1 in a normalised project. The position is kept inside the new timeline.
	pub fn set_timeline(&mut self, frame_count: u32, visualize_every: u32, fps: u32, in_step: u32, out_step: u32) {
		self.frame_count = frame_count;
		self.visualize_every = visualize_every;
		self.fps = f64::from(fps);
		self.in_step = in_step;
		self.out_step = out_step;
		self.position = self.position.min(self.last_frame());
	}

	pub fn frame_count(&self) -> u32 {
		self.frame_count
	}

	pub fn last_frame(&self) -> u32 {
		self.frame_count - 1
	}

	pub fn position(&self) -> u32 {
		self.position
	}

	/// Moves to `frame`, kept on the timeline.
	pub fn set_position(&mut self, frame: u32) {
		self.position = frame.min(self.last_frame());
	}

	/// Moves by `delta` frames (the arrows, PageUp/PageDown, the wheel).
	pub fn step_by(&mut self, delta: i64) {
		self.set_position((self.position as i64 + delta).clamp(0, self.last_frame() as i64) as u32);
	}

	pub fn readout(&self) -> Readout {
		Readout { frame: self.position, last: self.last_frame(), step: self.position * self.visualize_every }
	}

	pub fn speed(&self) -> f64 {
		self.speed
	}

	/// Sets the speed factor, 0.01..100; non-finite is ignored.
	pub fn set_speed(&mut self, speed: f64) {
		if speed.is_finite() {
			self.speed = speed.clamp(SPEED_RANGE.0, SPEED_RANGE.1);
		}
	}

	pub fn looping(&self) -> bool {
		self.looping
	}

	pub fn set_looping(&mut self, looping: bool) {
		self.looping = looping;
	}

	pub fn is_playing(&self) -> bool {
		self.playing
	}

	/// Starts playback at `now`; at or past the out point (or `end`, the last frame there will be) it starts again
	/// from the in point.
	pub fn play(&mut self, now: Instant, end: Option<u32>) {
		let (from, to) = self.range_until(end);
		if self.position >= to || self.position < from {
			self.position = from;
		}
		self.playing = true;
		self.last_tick = Some(now);
		self.carry = 0.0;
	}

	pub fn pause(&mut self) {
		self.playing = false;
		self.last_tick = None;
		self.carry = 0.0;
	}

	/// Advances by the frames that fell due since the last tick, at `fps * speed` frames a second. At a
	/// [`Limit::Hold`] playback holds and drops what accumulated, so it does not jump once the run catches up. At
	/// the out point or a [`Limit::End`] it stops, or wraps to the in point with Loop on. Returns whether the
	/// position moved.
	pub fn advance(&mut self, now: Instant, limit: Option<Limit>) -> bool {
		if !self.playing {
			return false;
		}
		let last = self.last_tick.replace(now).unwrap_or(now);
		self.carry += now.saturating_duration_since(last).as_secs_f64() * self.fps * self.speed;
		let due = self.carry.floor();
		if due < 1.0 {
			return false;
		}
		self.carry -= due;
		let end = match limit {
			Some(Limit::End(end)) => Some(end),
			_ => None,
		};
		let (from, to) = self.range_until(end);
		let mut target = self.position as u64 + due as u64;
		if target > to as u64 {
			if self.looping {
				let span = (to - from + 1) as u64;
				target = from as u64 + (target - to as u64 - 1) % span;
			} else {
				target = to as u64;
				self.pause();
			}
		}
		let mut target = target as u32;
		if let Some(Limit::Hold(limit)) = limit
			&& target > limit
		{
			target = limit.max(self.position);
			self.carry = 0.0;
		}
		let moved = target != self.position;
		self.position = target;
		moved
	}

	/// The in and out points as frames; the whole timeline when unset.
	pub fn range_frames(&self) -> (u32, u32) {
		let last = self.last_frame();
		let to = if self.out_step == 0 { last } else { (self.out_step / self.visualize_every).min(last) };
		let from = (self.in_step / self.visualize_every).min(to);
		(from, to)
	}

	/// [`range_frames`](Self::range_frames) ending at `end` at the latest.
	fn range_until(&self, end: Option<u32>) -> (u32, u32) {
		let (from, to) = self.range_frames();
		let to = end.map_or(to, |end| to.min(end));
		(from.min(to), to)
	}

	/// The in and out points in steps (0 out = to the end), for the project's Display.
	pub fn range_steps(&self) -> (u32, u32) {
		(self.in_step, self.out_step)
	}

	pub fn has_range(&self) -> bool {
		self.in_step != 0 || self.out_step != 0
	}

	/// Starts the export range at the position; an in point past the out point clears the out point.
	pub fn set_in_here(&mut self) {
		self.in_step = self.position * self.visualize_every;
		if self.out_step != 0 && self.in_step > self.out_step {
			self.out_step = 0;
		}
	}

	/// Ends the export range at the position (the last frame is stored as 0); an out point before the in point
	/// clears the in point.
	pub fn set_out_here(&mut self) {
		self.out_step = if self.position >= self.last_frame() { 0 } else { self.position * self.visualize_every };
		if self.out_step != 0 && self.out_step < self.in_step {
			self.in_step = 0;
		}
	}

	pub fn clear_range(&mut self) {
		self.in_step = 0;
		self.out_step = 0;
	}

	/// Home: the in point (the start when unset).
	pub fn home(&mut self) {
		self.position = self.range_frames().0;
	}

	/// End: the out point (the end when unset).
	pub fn end(&mut self) {
		self.position = self.range_frames().1;
	}

	pub fn available(&self) -> Option<Available> {
		self.available
	}

	pub fn set_available(&mut self, available: Option<Available>) {
		self.available = available;
	}
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use super::*;

	fn timeline(frames: u32) -> Playback {
		let mut p = Playback::default();
		p.set_timeline(frames, 2, 60, 0, 0);
		p
	}

	/// Plays for `seconds` in ticks of 1/60 s.
	fn play_for(p: &mut Playback, start: Instant, seconds: f64, limit: Option<Limit>) -> Instant {
		p.play(start, None);
		let ticks = (seconds * 60.0).round() as u32;
		let mut now = start;
		for i in 1..=ticks {
			now = start + Duration::from_secs_f64(i as f64 / 60.0);
			p.advance(now, limit);
		}
		now
	}

	#[test]
	fn plays_at_fps_times_speed() {
		let start = Instant::now();
		let mut p = timeline(10_000);
		play_for(&mut p, start, 2.0, None);
		assert!((119..=121).contains(&p.position()), "{}", p.position());
		let mut p = timeline(10_000);
		p.set_speed(4.0);
		play_for(&mut p, start, 2.0, None);
		assert!((479..=481).contains(&p.position()), "{}", p.position());
	}

	#[test]
	fn a_late_tick_catches_up() {
		let start = Instant::now();
		let mut p = timeline(10_000);
		p.play(start, None);
		assert!(p.advance(start + Duration::from_millis(500), None));
		assert_eq!(p.position(), 30);
	}

	#[test]
	fn stops_or_wraps_at_the_out_point() {
		let start = Instant::now();
		let mut p = timeline(100);
		play_for(&mut p, start, 3.0, None);
		assert_eq!((p.position(), p.is_playing()), (99, false));
		let mut p = timeline(100);
		p.set_looping(true);
		p.set_position(10);
		p.set_in_here();
		p.set_position(19);
		p.set_out_here();
		assert_eq!(p.range_frames(), (10, 19));
		assert_eq!(p.range_steps(), (20, 38));
		play_for(&mut p, start, 1.0, None);
		assert!(p.is_playing() && (10..=19).contains(&p.position()));
	}

	#[test]
	fn holds_at_the_newest_frame() {
		let start = Instant::now();
		let mut p = timeline(10_000);
		let now = play_for(&mut p, start, 1.0, Some(Limit::Hold(5)));
		assert_eq!((p.position(), p.is_playing()), (5, true));
		p.advance(now + Duration::from_millis(17), Some(Limit::Hold(50)));
		assert_eq!(p.position(), 6, "no jump accumulated while holding");
	}

	#[test]
	fn stops_or_wraps_at_the_end_of_a_partial_run() {
		let start = Instant::now();
		let mut p = timeline(10_000);
		play_for(&mut p, start, 1.0, Some(Limit::End(5)));
		assert_eq!((p.position(), p.is_playing()), (5, false));
		p.play(start, Some(5));
		assert_eq!(p.position(), 0, "play at the end starts over");
		let mut p = timeline(10_000);
		p.set_looping(true);
		play_for(&mut p, start, 1.0, Some(Limit::End(5)));
		assert!(p.is_playing() && p.position() <= 5);
	}

	#[test]
	fn range_rules_and_readout() {
		let mut p = timeline(100);
		p.set_position(50);
		p.set_out_here();
		p.set_position(60);
		p.set_in_here();
		assert_eq!(p.range_steps(), (120, 0), "an in point past the out point clears it");
		p.set_position(99);
		p.set_out_here();
		assert_eq!(p.range_steps().1, 0, "the last frame is stored as 0");
		p.set_position(10);
		p.set_out_here();
		assert_eq!(p.range_steps(), (0, 20), "an out point before the in point clears it");
		p.set_position(7);
		assert_eq!(p.readout(), Readout { frame: 7, last: 99, step: 14 });
		assert_eq!(p.readout().frames_text(), "7 / 99");
		p.set_speed(1000.0);
		assert_eq!(p.speed(), 100.0);
	}
}
