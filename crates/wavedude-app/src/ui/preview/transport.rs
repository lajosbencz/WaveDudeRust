//! The timeline and the transport bar (docs/ui/50-timeline-transport.md): Run/Stop, Pause run, Play, Pause, the
//! frame steps, the export range, Loop, Live, the readout with frame and step, Speed, the layer toggles and the
//! split radios. Every control sends an [`Event`] or a [`Command`]; the state decides.
//!
//! Threads: the UI thread.

use std::borrow::Cow;
use std::rc::Rc;

use freya::prelude::*;
use serde_json::json;
use wavedude_widgets::{
	Callback, Icon, IconView, NumberBox, NumberSpec, TestIdExt, Timeline, TimelineEvent, TimelineModel, icon::icons,
	use_wave_theme,
};

use crate::ui::shared::{App, Shared, Topics};
use wavedude_app_state::Event as AppEvent;
use wavedude_app_state::commands::{Command, Target};
use wavedude_app_state::playback::{Playback, SPEED_PRESETS, SPEED_RANGE};
use wavedude_app_state::settings::Orientation;

const fn drawn(stroke: &'static str, fill: &'static str) -> Icon {
	Icon::Path { stroke: Cow::Borrowed(stroke), fill: Some(Cow::Borrowed(fill)) }
}

const RING: &str = "M7.5 1 A6.5 6.5 0 1 1 7.49 1 Z";
const RUN: Icon = drawn(RING, "M6 4.5 L11 7.5 L6 10.5 Z");
const STOP: Icon = drawn(RING, "M5 5 L10 5 L10 10 L5 10 Z");
const PAUSE_RUN: Icon = drawn(RING, "M5 4.5 L6.8 4.5 L6.8 10.5 L5 10.5 Z M8.2 4.5 L10 4.5 L10 10.5 L8.2 10.5 Z");
const LIVE: Icon = drawn(RING, "M7.5 5 A2.5 2.5 0 1 1 7.49 5 Z");
const PREVIOUS: Icon = Icon::filled("M3 2 L4.5 2 L4.5 13 L3 13 Z M12 2 L5.5 7.5 L12 13 Z");
const NEXT: Icon = Icon::filled("M10.5 2 L12 2 L12 13 L10.5 13 Z M3 2 L9.5 7.5 L3 13 Z");
const IN_POINT: Icon = Icon::path("M6.5 2.5 L3.5 2.5 L3.5 12.5 L6.5 12.5 M7.5 7.5 L12.5 7.5");
const OUT_POINT: Icon = Icon::path("M8.5 2.5 L11.5 2.5 L11.5 12.5 L8.5 12.5 M2.5 7.5 L7.5 7.5");
const CLEAR: Icon = Icon::path("M2.5 2.5 L2.5 12.5 M12.5 2.5 L12.5 12.5 M5.5 5.5 L9.5 9.5 M9.5 5.5 L5.5 9.5");
const LOOP: Icon = Icon::path(
	"M3.5 9.5 L3.5 4.5 L11.5 4.5 M9.5 2.5 L11.5 4.5 L9.5 6.5 M11.5 5.5 L11.5 10.5 L3.5 10.5 M5.5 8.5 L3.5 10.5 L5.5 12.5",
);
const WAVE: Icon =
	Icon::path("M0.5 7.5 C2.5 2.5 4.5 2.5 5.5 7.5 C6.5 12.5 8.5 12.5 9.5 7.5 C10.5 2.5 12.5 2.5 14.5 7.5");
const SUN: Icon = Icon::path(
	"M7.5 4.5 A3 3 0 1 1 7.49 4.5 Z M7.5 0.5 L7.5 2.5 M7.5 12.5 L7.5 14.5 M0.5 7.5 L2.5 7.5 M12.5 7.5 L14.5 7.5 M2.5 2.5 L3.5 3.5 M11.5 11.5 L12.5 12.5 M12.5 2.5 L11.5 3.5 M3.5 11.5 L2.5 12.5",
);
const SIDE_BY_SIDE: Icon =
	Icon::path("M0.5 3.5 L6.5 3.5 L6.5 11.5 L0.5 11.5 Z M8.5 3.5 L14.5 3.5 L14.5 11.5 L8.5 11.5 Z");
const STACKED: Icon = Icon::path("M2.5 0.5 L12.5 0.5 L12.5 6.5 L2.5 6.5 Z M2.5 8.5 L12.5 8.5 L12.5 14.5 L2.5 14.5 Z");

/// A 23 px tool button: an icon, a tooltip, pressed while checked, greyed and half transparent while disabled,
/// an accent border under keyboard focus; Enter or Space presses it.
#[derive(Clone, PartialEq)]
pub struct ToolButton {
	icon: Icon,
	tooltip: String,
	enabled: bool,
	checked: bool,
	on_press: Callback<()>,
	test_id: String,
}

impl ToolButton {
	pub fn new(icon: Icon, tooltip: impl Into<String>, on_press: impl Into<Callback<()>>) -> Self {
		Self {
			icon,
			tooltip: tooltip.into(),
			enabled: true,
			checked: false,
			on_press: on_press.into(),
			test_id: String::new(),
		}
	}

	pub fn enabled(mut self, enabled: bool) -> Self {
		self.enabled = enabled;
		self
	}

	pub fn checked(mut self, checked: bool) -> Self {
		self.checked = checked;
		self
	}

	pub fn test_id(mut self, id: impl Into<String>) -> Self {
		self.test_id = id.into();
		self
	}
}

impl Component for ToolButton {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let id = use_a11y();
		let focus = use_focus(id);
		let mut hover = use_state(|| false);
		let enabled = self.enabled;
		let press = self.on_press.clone();
		let key_press = self.on_press.clone();
		let background = if self.checked {
			theme.pressed_background
		} else if *hover.read() && enabled {
			theme.hover_background
		} else {
			Color::TRANSPARENT
		};
		let border = (focus() == Focus::Keyboard)
			.then(|| Border::new().fill(theme.accent).width(1.).alignment(BorderAlignment::Inner));
		let color = if enabled { theme.text } else { theme.text_disabled };
		let button = rect()
			.a11y_id(id)
			.a11y_focusable(enabled)
			.a11y_role(AccessibilityRole::Button)
			.test_id(self.test_id.clone())
			.width(Size::px(theme.row_height))
			.height(Size::px(theme.row_height))
			.center()
			.background(background)
			.border(border)
			.opacity(if enabled { 1. } else { 0.5 })
			.on_pointer_enter(move |_| hover.set(true))
			.on_pointer_leave(move |_| hover.set(false))
			.on_pointer_press(move |_| {
				if enabled {
					press.call(());
				}
			})
			.on_key_down(move |e: Event<KeyboardEventData>| {
				let activates =
					matches!(&e.key, Key::Named(NamedKey::Enter)) || matches!(&e.key, Key::Character(c) if c == " ");
				if activates && enabled {
					e.stop_propagation();
					e.prevent_default();
					key_press.call(());
				}
			})
			.child(IconView::new(self.icon.clone()).color(color));
		TooltipContainer::new(Tooltip::new_text(self.tooltip.clone())).child(button)
	}
}

/// The timeline's model from the playback state: frames, position, the range in frames, the frames made.
pub fn timeline_model(playback: &Playback, available: Rc<[(u32, u32)]>, live: bool) -> TimelineModel {
	let (from, to) = playback.range_frames();
	let (in_step, out_step) = playback.range_steps();
	TimelineModel {
		frames: playback.frame_count(),
		position: playback.position(),
		in_point: (in_step != 0).then_some(from),
		out_point: (out_step != 0).then_some(to),
		available,
		bounded: live,
	}
}

fn set_display(shared: &Shared, path: &str, value: serde_json::Value) {
	shared.send(AppEvent::Command(Command::Set { target: Target::Display, path: path.into(), value }));
}

/// Play or pause playback.
pub fn toggle_playback(shared: &Shared) {
	let playing = shared.peek(|app| app.live.playback.is_playing());
	shared.send(if playing { AppEvent::PausePlayback } else { AppEvent::Play });
}

/// The timeline and the transport bar; the timeline alone follows the run's progress.
#[derive(PartialEq)]
pub struct Transport;

impl Component for Transport {
	fn render(&self) -> impl IntoElement {
		rect().width(Size::fill()).child(TimelineBar).child(TransportBar)
	}
}

/// The timeline bound to the playback state and the frames made.
#[derive(PartialEq)]
struct TimelineBar;

impl Component for TimelineBar {
	fn render(&self) -> impl IntoElement {
		let shared = Shared::get();
		shared.subscribe(Topics::DOCUMENT | Topics::PLAYBACK | Topics::PROGRESS | Topics::RUN | Topics::SETTINGS);
		timeline(&shared).into_element()
	}
}

/// The buttons, the readout and the speed.
#[derive(PartialEq)]
struct TransportBar;

impl Component for TransportBar {
	fn render(&self) -> impl IntoElement {
		let shared = Shared::get();
		shared.subscribe(Topics::DOCUMENT | Topics::PLAYBACK | Topics::RUN | Topics::SETTINGS);
		transport_bar(&shared).into_element()
	}
}

fn timeline(shared: &Shared) -> impl IntoElement {
	let (model, tip, every) = shared.peek(|app: &App| {
		let live = app.live.is_live();
		let every = app.document.project.simulation.visualize_every;
		let model = timeline_model(&app.live.playback, app.live.present_spans().into(), live);
		(model, app.strings.get("TipPreviewTimeline").to_owned(), every)
	});
	let last = model.last();
	let shared = shared.clone();
	rect().width(Size::fill()).test_id("PreviewTimeline").child(
		Timeline::new(model, move |e: TimelineEvent| match e {
			TimelineEvent::Seek(f) => {
				shared.send(AppEvent::Scrub(f));
			}
			TimelineEvent::SetIn(f) => set_display(&shared, "export_from", json!(f * every)),
			TimelineEvent::SetOut(f) => set_display(&shared, "export_to", json!(if f >= last { 0 } else { f * every })),
			TimelineEvent::PlayPause => toggle_playback(&shared),
		})
		.tooltip(tip),
	)
}

/// The transport bar.
fn transport_bar(shared: &Shared) -> impl IntoElement {
	let theme = use_wave_theme();
	let view = shared.peek(|app: &App| {
		let s = &app.strings;
		let p = &app.live.playback;
		let d = &app.document.project.display;
		let paused = app.live.run_paused_by_user();
		TransportView {
			active: app.live.run_is_active(),
			paused,
			playing: p.is_playing(),
			has_range: p.has_range(),
			looping: p.looping(),
			live: app.live.is_live(),
			speed: p.speed(),
			frames: p.readout().frames_text(),
			step: format!("{} {}", s.get("LabelPreviewStep"), p.readout().step),
			show_field: d.field.preview,
			show_intensity: d.intensity.preview,
			side_by_side: app.settings.preview_orientation == Orientation::Horizontal,
			tips: [
				"TipPreviewRun",
				"TipPreviewStop",
				if paused { "TipPreviewResumeRun" } else { "TipPreviewPauseRun" },
				"TipPreviewPlay",
				"TipPreviewPause",
				"TipPreviewStepBack",
				"TipPreviewStepForward",
				"TipPreviewSetIn",
				"TipPreviewSetOut",
				"TipPreviewClearRange",
				"TipPreviewLoop",
				"TipPreviewLive",
				"TipPreviewFrame",
				"TipPreviewStep",
				"TipPreviewSpeed",
				"TipPreviewShowField",
				"TipPreviewShowIntensity",
				"TipPreviewSideBySide",
				"TipPreviewStacked",
				"LabelPreviewSpeed",
			]
			.map(|k| s.get(k).to_owned()),
		}
	});
	let tip = |i: usize| view.tips[i].clone();
	let send = |event: AppEvent| {
		let shared = shared.clone();
		move |_| {
			shared.send(event.clone());
		}
	};
	let run = if view.active {
		ToolButton::new(STOP, tip(1), send(AppEvent::StopRun)).test_id("PreviewStop")
	} else {
		ToolButton::new(RUN, tip(0), send(AppEvent::Run)).test_id("PreviewRun")
	};
	let toggle = |path: &'static str, on: bool| {
		let shared = shared.clone();
		move |_| set_display(&shared, path, json!(!on))
	};
	let orient = |o: Orientation| {
		let shared = shared.clone();
		move |_| {
			shared.send(AppEvent::SetPreviewOrientation(o));
		}
	};
	let speed_shared = shared.clone();
	let divider = || rect().width(Size::px(1.)).height(Size::px(16.)).background(theme.hairline);
	rect()
		.horizontal()
		.width(Size::fill())
		.height(Size::px(theme.row_height + 4.))
		.cross_align(Alignment::center())
		.padding((2., 4., 2., 4.))
		.spacing(2.)
		.content(Content::flex())
		.background(theme.tool_window_background)
		.color(theme.text)
		.font_size(theme.font_size)
		.child(run)
		.child(
			ToolButton::new(PAUSE_RUN, tip(2), send(AppEvent::TogglePauseRun))
				.enabled(view.active)
				.checked(view.paused)
				.test_id("PreviewPauseRun"),
		)
		.child(divider())
		.child(ToolButton::new(icons::PLAY, tip(3), send(AppEvent::Play)).enabled(!view.playing).test_id("PreviewPlay"))
		.child(
			ToolButton::new(icons::PAUSE, tip(4), send(AppEvent::PausePlayback))
				.enabled(view.playing)
				.test_id("PreviewPause"),
		)
		.child(ToolButton::new(PREVIOUS, tip(5), send(AppEvent::StepFrames(-1))).test_id("PreviewStepBack"))
		.child(ToolButton::new(NEXT, tip(6), send(AppEvent::StepFrames(1))).test_id("PreviewStepForward"))
		.child(ToolButton::new(IN_POINT, tip(7), send(AppEvent::SetInPoint)).test_id("PreviewSetIn"))
		.child(ToolButton::new(OUT_POINT, tip(8), send(AppEvent::SetOutPoint)).test_id("PreviewSetOut"))
		.child(
			ToolButton::new(CLEAR, tip(9), send(AppEvent::ClearRange))
				.enabled(view.has_range)
				.test_id("PreviewClearRange"),
		)
		.child(
			ToolButton::new(LOOP, tip(10), send(AppEvent::SetLoop(!view.looping)))
				.checked(view.looping)
				.test_id("PreviewLoop"),
		)
		.child(
			ToolButton::new(LIVE, tip(11), send(AppEvent::SetLive(!view.live)))
				.enabled(view.active)
				.checked(view.live)
				.test_id("PreviewLive"),
		)
		.child(
			rect()
				.horizontal()
				.cross_align(Alignment::center())
				.spacing(6.)
				.padding((0., 6., 0., 6.))
				.child(
					TooltipContainer::new(Tooltip::new_text(tip(12)))
						.child(label().text(view.frames).test_id("PreviewFrame")),
				)
				.child(
					TooltipContainer::new(Tooltip::new_text(tip(13)))
						.child(label().text(view.step).color(theme.text_dim).test_id("PreviewStep")),
				),
		)
		.child(rect().width(Size::flex(1.)))
		.child(label().text(tip(19)).color(theme.text_dim))
		.child(
			NumberBox::new(
				WritableSetter::new(view.speed, move |v: f64| {
					speed_shared.send(AppEvent::SetSpeed(v));
				}),
				NumberSpec::linear(SPEED_RANGE.0, SPEED_RANGE.1, 0.25).with_decimals(2),
			)
			.width(Size::px(64.))
			.tooltip(tip(14))
			.test_id("PreviewSpeed"),
		)
		.child(SpeedPresets { speed: view.speed, tooltip: tip(14) })
		.child(divider())
		.child(
			ToolButton::new(WAVE, tip(15), toggle("field.preview", view.show_field))
				.checked(view.show_field)
				.test_id("PreviewShowField"),
		)
		.child(
			ToolButton::new(SUN, tip(16), toggle("intensity.preview", view.show_intensity))
				.checked(view.show_intensity)
				.test_id("PreviewShowIntensity"),
		)
		.child(divider())
		.child(
			ToolButton::new(SIDE_BY_SIDE, tip(17), orient(Orientation::Horizontal))
				.checked(view.side_by_side)
				.test_id("PreviewSideBySide"),
		)
		.child(
			ToolButton::new(STACKED, tip(18), orient(Orientation::Vertical))
				.checked(!view.side_by_side)
				.test_id("PreviewStacked"),
		)
}

/// The drop-down beside the speed box: the preset factors, the current one ticked.
#[derive(Clone, PartialEq)]
struct SpeedPresets {
	speed: f64,
	tooltip: String,
}

impl Component for SpeedPresets {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let ui = consume_context::<crate::ui::app::Ui>();
		let shared = Shared::get();
		let mut at = use_state(|| (0., 0.));
		let speed = self.speed;
		let open = move |_: Event<PressEventData>| {
			let entries: Vec<wavedude_widgets::MenuEntry<f64>> = SPEED_PRESETS
				.iter()
				.map(|&p| {
					wavedude_widgets::MenuItem::with_action(format!("speed:{p}"), format!("{p}x"), p)
						.check(wavedude_widgets::Check::Radio((speed - p).abs() < 1e-9))
						.into()
				})
				.collect();
			let shared = shared.clone();
			ui.open_menu(*at.peek(), wavedude_widgets::MenuModel::new(entries), move |p: f64| {
				shared.send(AppEvent::SetSpeed(p));
			});
		};
		TooltipContainer::new(Tooltip::new_text(self.tooltip.clone())).child(
			rect()
				.on_sized(move |e: Event<SizedEventData>| {
					at.set((f64::from(e.area.min_x()), f64::from(e.area.max_y())));
				})
				.test_id("PreviewSpeedPresets")
				.child(
					Button::new()
						.flat()
						.compact()
						.on_press(open)
						.child(IconView::new(icons::DROPDOWN).color(theme.text)),
				),
		)
	}
}

struct TransportView {
	active: bool,
	paused: bool,
	playing: bool,
	has_range: bool,
	looping: bool,
	live: bool,
	speed: f64,
	frames: String,
	step: String,
	show_field: bool,
	show_intensity: bool,
	side_by_side: bool,
	tips: [String; 20],
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_timeline_shows_the_range_and_the_frames_made() {
		let mut p = Playback::default();
		p.set_timeline(100, 4, 30, 40, 200);
		p.set_position(20);
		let m = timeline_model(&p, Rc::from([(0, 30)]), true);
		assert_eq!((m.frames, m.position, m.in_point, m.out_point), (100, 20, Some(10), Some(50)));
		assert_eq!((&m.available[..], m.bounded), (&[(0, 30)][..], true));
		p.clear_range();
		let m = timeline_model(&p, Rc::from([]), false);
		assert_eq!((m.in_point, m.out_point, m.available.len()), (None, None, 0));
	}
}
