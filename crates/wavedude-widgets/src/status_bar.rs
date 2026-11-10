//! The status bar: the latest log line (left, colouring the bar for warnings and errors), the run status
//! (right), busy text and a progress bar (far right), a resize grip in the corner.

use std::time::{Duration, Instant};

use freya::engine::prelude::SkRect;
use freya::prelude::*;

use crate::message_list::LogLevel;
use crate::paint::{DeviceGrid, fill, stroke};
use crate::theme::use_wave_theme;
use crate::tooltip::Tip;
use crate::util::{TestIdExt, wide_tooltip};

/// The bar's colouring.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StatusLevel {
	#[default]
	Info,
	Warn,
	Error,
}

/// The left text and the colouring it brings.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusLine {
	pub text: String,
	pub level: StatusLevel,
}

impl Default for StatusLine {
	fn default() -> Self {
		Self { text: "Ready".into(), level: StatusLevel::Info }
	}
}

impl StatusLine {
	/// Takes a log line: Info and up replace the text; a warning or error colours the bar until the next Info.
	/// Returns whether the line changed.
	pub fn apply(&mut self, level: LogLevel, text: &str) -> bool {
		let level = match level {
			LogLevel::Trace | LogLevel::Debug => return false,
			LogLevel::Info => StatusLevel::Info,
			LogLevel::Warn => StatusLevel::Warn,
			LogLevel::Error | LogLevel::Fatal => StatusLevel::Error,
		};
		let changed = self.text != text || self.level != level;
		self.text = text.to_owned();
		self.level = level;
		changed
	}
}

/// Progress of the busy work.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Progress {
	/// 0..1.
	Determinate(f32),
	Indeterminate,
}

const SWEEP: Duration = Duration::from_millis(1500);
const CHUNK: f32 = 0.3;

/// The filled span of a progress bar `width` wide, in the bar's logical pixels; an indeterminate chunk of
/// 30 % sweeps from beyond the left end to beyond the right one every 1.5 s and is clipped to the bar.
pub fn progress_span(progress: Progress, width: f32, elapsed: Duration) -> (f32, f32) {
	match progress {
		Progress::Determinate(f) => (0., width * f.clamp(0., 1.)),
		Progress::Indeterminate => {
			let t = (elapsed.as_secs_f64() / SWEEP.as_secs_f64()).fract() as f32;
			let start = (t * (1. + CHUNK) - CHUNK) * width;
			(start.max(0.), (start + CHUNK * width).min(width))
		}
	}
}

/// Whether a press at `now` completes a double click after the press at `previous`.
pub fn is_double_click(previous: Option<Instant>, now: Instant) -> bool {
	previous.is_some_and(|p| now.saturating_duration_since(p) <= Duration::from_millis(500))
}

const BAR_WIDTH: f32 = 120.;
const HEIGHT: f32 = 22.;
/// The resize grip's side; the bar keeps it clear on its right.
const GRIP: f32 = 16.;
const ITEM_PADDING: (f32, f32) = (2., 4.);

/// The bar. `busy` shows its text and progress bar; nothing on the far right when `None`.
#[derive(Clone, PartialEq)]
pub struct StatusBar {
	line: StatusLine,
	right: String,
	busy: Option<(String, Progress)>,
	tooltip: Option<String>,
	busy_tooltip: Option<String>,
	on_double_click: Option<EventHandler<()>>,
}

impl StatusBar {
	pub fn new(line: StatusLine) -> Self {
		Self { line, right: String::new(), busy: None, tooltip: None, busy_tooltip: None, on_double_click: None }
	}

	/// The run status.
	pub fn right(mut self, text: impl Into<String>) -> Self {
		self.right = text.into();
		self
	}

	pub fn busy(mut self, busy: Option<(String, Progress)>) -> Self {
		self.busy = busy;
		self
	}

	pub fn tooltip(mut self, text: impl Into<String>) -> Self {
		self.tooltip = Some(text.into());
		self
	}

	/// The tooltip of the busy text and its bar.
	pub fn busy_tooltip(mut self, text: Option<String>) -> Self {
		self.busy_tooltip = text;
		self
	}

	pub fn on_double_click(mut self, handler: impl Into<EventHandler<()>>) -> Self {
		self.on_double_click = Some(handler.into());
		self
	}
}

impl Component for StatusBar {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let mut last_press = use_state(|| None::<Instant>);
		let background = match self.line.level {
			StatusLevel::Info => theme.status_background,
			StatusLevel::Warn => theme.status_warn,
			StatusLevel::Error => theme.status_error,
		};
		let fg = theme.status_text;
		let on_double = self.on_double_click.clone();
		let left =
			label().text(self.line.text.clone()).max_lines(1).text_overflow(TextOverflow::Ellipsis).width(Size::fill());
		let left: Element = match &self.tooltip {
			Some(t) => wide_tooltip(t.clone(), left),
			None => left.into_element(),
		};
		let busy_tip = self.busy_tooltip.clone();
		let busy = self.busy.clone().map(|(text, progress)| {
			let view = rect()
				.horizontal()
				.cross_align(Alignment::center())
				.padding(ITEM_PADDING)
				.child(label().text(text).max_lines(1))
				.child(rect().margin((0., 8., 0., 6.)).child(ProgressBarView { progress, color: fg }));
			match busy_tip.clone() {
				Some(t) => Tip::new(t).child(view).into_element(),
				None => view.into_element(),
			}
		});
		rect()
			.test_id("StatusBar")
			.width(Size::fill())
			.min_height(Size::px(HEIGHT))
			.horizontal()
			.content(Content::flex())
			.cross_align(Alignment::center())
			.padding((0., GRIP, 0., 4.))
			.background(background)
			.color(fg)
			.font_size(theme.font_size)
			.on_pointer_down(move |e: Event<PointerEventData>| {
				if e.button() != Some(MouseButton::Left) {
					return;
				}
				let now = Instant::now();
				if is_double_click(*last_press.peek(), now) {
					last_press.set(None);
					if let Some(h) = &on_double {
						h.call(());
					}
				} else {
					last_press.set(Some(now));
				}
			})
			.child(rect().width(Size::flex(1.)).padding(ITEM_PADDING).child(left))
			.maybe_child(
				(!self.right.is_empty())
					.then(|| rect().padding(ITEM_PADDING).child(label().text(self.right.clone()).max_lines(1))),
			)
			.maybe_child(busy)
			// absolute positions start inside the padding, which keeps the grip's corner clear
			.child(rect().position(Position::new_absolute().right(-GRIP).bottom(0.)).child(ResizeGrip { color: fg }))
	}
}

#[derive(Clone, PartialEq)]
struct ProgressBarView {
	progress: Progress,
	color: Color,
}

impl Component for ProgressBarView {
	fn render(&self) -> impl IntoElement {
		let start = use_hook(Instant::now);
		let mut tick = use_state(|| 0u64);
		let indeterminate = self.progress == Progress::Indeterminate;
		let mut running = use_state(|| false);
		if indeterminate && !*running.peek() {
			running.set(true);
			spawn(async move {
				while *running.peek() {
					timer(Duration::from_millis(16)).await;
					*tick.write() += 1;
				}
			});
		} else if !indeterminate && *running.peek() {
			running.set(false);
		}
		let _ = tick.read();
		let (progress, color, elapsed) = (self.progress, self.color, start.elapsed());
		rect().width(Size::px(BAR_WIDTH)).height(Size::px(12.)).background(RenderCallback::new(
			move |ctx: &mut FillRenderContext| {
				let grid = DeviceGrid::from_ctx(ctx);
				let RenderTarget::Background { canvas, .. } = &mut ctx.target else { return };
				let (x0, x1) = progress_span(progress, BAR_WIDTH, elapsed);
				grid.enter(canvas);
				let (l, t, r, b) = (grid.x(0.), grid.y(0.), grid.x(BAR_WIDTH), grid.y(12.));
				let line = grid.hairline();
				if x1 > x0 {
					canvas.draw_rect(SkRect::new(grid.x(x0), t, grid.x(x1), b), &fill(color));
				}
				canvas.draw_rect(
					SkRect::new(l + line / 2., t + line / 2., r - line / 2., b - line / 2.),
					&stroke(color, line),
				);
				canvas.restore();
			},
		))
	}
}

#[derive(Clone, PartialEq)]
struct ResizeGrip {
	color: Color,
}

impl Component for ResizeGrip {
	fn render(&self) -> impl IntoElement {
		let color = self.color;
		rect().width(Size::px(GRIP)).height(Size::px(GRIP)).background(RenderCallback::new(
			move |ctx: &mut FillRenderContext| {
				let grid = DeviceGrid::from_ctx(ctx);
				let RenderTarget::Background { canvas, .. } = &mut ctx.target else { return };
				grid.enter(canvas);
				let dot = grid.width(2.);
				// a 10 px triangle of 2 px dots, 3 px off the corner
				for (cx, cy) in [(8., 0.), (8., 4.), (8., 8.), (4., 4.), (4., 8.), (0., 8.)] {
					let (x, y) = (grid.x(3. + cx).round(), grid.y(3. + cy).round());
					canvas.draw_rect(SkRect::new(x, y, x + dot, y + dot), &fill(color));
				}
				canvas.restore();
			},
		))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn line_follows_info_and_up() {
		let mut s = StatusLine::default();
		assert!(!s.apply(LogLevel::Debug, "x"));
		assert_eq!(s.text, "Ready");
		assert!(s.apply(LogLevel::Warn, "careful"));
		assert_eq!(s.level, StatusLevel::Warn);
		assert!(s.apply(LogLevel::Fatal, "dead"));
		assert_eq!(s.level, StatusLevel::Error);
		assert!(s.apply(LogLevel::Info, "Saved"));
		assert_eq!((s.text.as_str(), s.level), ("Saved", StatusLevel::Info));
	}

	#[test]
	fn progress_spans() {
		assert_eq!(progress_span(Progress::Determinate(0.5), 120., Duration::ZERO), (0., 60.));
		assert_eq!(progress_span(Progress::Determinate(2.), 120., Duration::ZERO), (0., 120.));
		let (a, b) = progress_span(Progress::Indeterminate, 100., Duration::ZERO);
		assert_eq!((a, b), (0., 0.));
		let (a, b) = progress_span(Progress::Indeterminate, 100., SWEEP / 2);
		assert!((a - 35.).abs() < 1e-3 && (b - 65.).abs() < 1e-3);
	}

	#[test]
	fn double_click_window() {
		let t = Instant::now();
		assert!(!is_double_click(None, t));
		assert!(is_double_click(Some(t), t + Duration::from_millis(400)));
		assert!(!is_double_click(Some(t), t + Duration::from_millis(600)));
	}
}
