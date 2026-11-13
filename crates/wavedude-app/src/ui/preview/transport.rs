//! The transport bar (docs/ui/50-timeline-transport.md, Transport bar): Run/Stop, Pause run, Play, Pause, the frame
//! steps, the export range, Loop, Live, the readout with frame and step, Speed with its presets, the layer toggles
//! and the split buttons. Every control sends an [`Event`]; the state decides.

use std::sync::LazyLock;

use eframe::egui::{
	Align2, Color32, FontId, Id, Popup, Rect, Response, Role, Sense, Stroke, StrokeKind, Ui, WidgetInfo, pos2, vec2,
};
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons::*;
use wavedude_app_state::Event;
use wavedude_app_state::commands::Command;
use wavedude_app_state::playback::{SPEED_PRESETS, SPEED_RANGE};
use wavedude_app_state::settings::Orientation;
use wavedude_core::model::{DisplayLayerEdit, Layer};

use super::take_activation_keys;
use crate::App;
use crate::preview::display_edit;
use crate::ui::menu_bar::check_entry;
use crate::ui::number_box::{NumberSpec, number_box};
use crate::ui::theme::{FONT_SIZE, INPUT_HEIGHT, Palette};

const BUTTON: f32 = 22.;
const ICON_SIZE: f32 = 16.;
/// The bar's padding: horizontal, vertical.
const PADDING: (f32, f32) = (4., 3.);
/// The bar's height with its 1 px top border.
pub(super) const HEIGHT: f32 = 1. + 2. * PADDING.1 + BUTTON;
const DIVIDER: f32 = 1. + 2. * 4.;
const READOUT_MARGIN: f32 = 4.;
/// The margins left and right of "Speed".
const SPEED_LABEL_MARGIN: (f32, f32) = (8., 4.);
const SPEED_WIDTH: f32 = 64.;
const PRESETS_WIDTH: f32 = ICON_SIZE + 4.;

static SPEED: LazyLock<NumberSpec> = LazyLock::new(|| NumberSpec::linear(SPEED_RANGE.0, SPEED_RANGE.1, 0.25));

/// Play when paused, pause when playing.
pub(super) fn play_or_pause(app: &App) -> Event {
	if app.live.playback.is_playing() { Event::PausePlayback } else { Event::Play }
}

/// A tool button's kind: a plain button, a toggle or a radio, the last two with their state.
#[derive(Clone, Copy)]
enum Kind {
	Plain,
	Toggle(bool),
	Radio(bool),
}

/// Lays the bar's items out from left to right in a row of [`BUTTON`] height.
struct Row {
	x: f32,
	top: f32,
}

impl Row {
	fn take(&mut self, width: f32) -> Rect {
		let r = Rect::from_min_size(pos2(self.x, self.top), vec2(width, BUTTON));
		self.x += width;
		r
	}

	fn skip(&mut self, width: f32) {
		self.x += width;
	}
}

/// The bar's state between frames.
#[derive(Default)]
pub(super) struct Transport {
	/// The button the keyboard moves to once it is enabled: Play and Pause hand the focus over.
	refocus: Option<Id>,
}

impl Transport {
	/// The bar in `bar`.
	pub(super) fn ui(&mut self, ui: &mut Ui, bar: Rect, app: &App, out: &mut Vec<Event>) {
		let palette = Palette::of(ui.ctx().theme());
		let s = app.strings;
		let live = &app.live;
		let p = &live.playback;
		let display = &app.document.project.display;
		let scope = ui.scope_id();
		let id = |name: &str| scope.with(name);
		if let Some(id) = self.refocus.take() {
			ui.memory_mut(|m| m.request_focus(id));
		}
		ui.painter().rect_filled(Rect::from_min_max(bar.min, pos2(bar.right(), bar.top() + 1.)), 0., palette.border);
		let mut row = Row { x: bar.left() + PADDING.0, top: bar.top() + 1. + PADDING.1 };
		let button = |ui: &Ui, row: &mut Row, name: &str, icon, tip: &str, enabled, kind| {
			row.skip(1.);
			let r = tool_button(ui, row.take(BUTTON), id(name), icon, tip, enabled, kind);
			row.skip(1.);
			r.clicked()
		};
		let active = live.run_is_active();
		if active {
			if button(ui, &mut row, "PreviewStop", ICON_STOP_CIRCLE, s.get("TipPreviewStop"), true, Kind::Plain) {
				out.push(Event::StopRun);
			}
		} else if button(ui, &mut row, "PreviewRun", ICON_PLAY_CIRCLE, s.get("TipPreviewRun"), true, Kind::Plain) {
			out.push(Event::Run);
		}
		let paused = live.run_paused_by_user();
		let tip = s.get(if paused { "TipPreviewResumeRun" } else { "TipPreviewPauseRun" });
		if button(ui, &mut row, "PreviewPauseRun", ICON_PAUSE_CIRCLE, tip, active, Kind::Toggle(paused)) {
			out.push(Event::TogglePauseRun);
		}
		divider(ui, &mut row, palette.border);
		let playing = p.is_playing();
		let focused = |name: &str| ui.memory(|m| m.has_focus(id(name)));
		let (play_focused, pause_focused) = (focused("PreviewPlay"), focused("PreviewPause"));
		if button(ui, &mut row, "PreviewPlay", ICON_PLAY_ARROW, s.get("TipPreviewPlay"), !playing, Kind::Plain) {
			out.push(Event::Play);
			self.refocus = play_focused.then(|| id("PreviewPause"));
		}
		if button(ui, &mut row, "PreviewPause", ICON_PAUSE, s.get("TipPreviewPause"), playing, Kind::Plain) {
			out.push(Event::PausePlayback);
			self.refocus = pause_focused.then(|| id("PreviewPlay"));
		}
		divider(ui, &mut row, palette.border);
		let steps = [
			("PreviewStepBack", ICON_SKIP_PREVIOUS, "TipPreviewStepBack", Event::StepFrames(-1)),
			("PreviewStepForward", ICON_SKIP_NEXT, "TipPreviewStepForward", Event::StepFrames(1)),
		];
		for (name, icon, tip, event) in steps {
			if button(ui, &mut row, name, icon, s.get(tip), true, Kind::Plain) {
				out.push(event);
			}
		}
		divider(ui, &mut row, palette.border);
		let range = [
			("PreviewSetIn", ICON_LINE_START, "TipPreviewSetIn", true, Event::SetInPoint),
			("PreviewSetOut", ICON_LINE_END, "TipPreviewSetOut", true, Event::SetOutPoint),
			("PreviewClearRange", ICON_CLOSE, "TipPreviewClearRange", p.has_range(), Event::ClearRange),
		];
		for (name, icon, tip, enabled, event) in range {
			if button(ui, &mut row, name, icon, s.get(tip), enabled, Kind::Plain) {
				out.push(event);
			}
		}
		let looping = p.looping();
		if button(ui, &mut row, "PreviewLoop", ICON_REPEAT, s.get("TipPreviewLoop"), true, Kind::Toggle(looping)) {
			out.push(Event::SetLoop(!looping));
		}
		let following = live.is_live();
		let tip = s.get("TipPreviewLive");
		if button(ui, &mut row, "PreviewLive", ICON_RADIO_BUTTON_CHECKED, tip, active, Kind::Toggle(following)) {
			out.push(Event::SetLive(!following));
		}
		divider(ui, &mut row, palette.border);
		let readout = p.readout();
		let step = format!("{} {}", s.get("LabelPreviewStep"), readout.step);
		for (text, tip, color) in
			[(readout.frames_text(), "TipPreviewFrame", palette.text), (step, "TipPreviewStep", palette.text_disabled)]
		{
			row.skip(READOUT_MARGIN);
			label(ui, &mut row, id(tip), text, s.get(tip), color);
			row.skip(READOUT_MARGIN);
		}

		let speed_label = ui.painter().layout_no_wrap(s.get("LabelPreviewSpeed").into(), font(), palette.text);
		let right = SPEED_LABEL_MARGIN.0
			+ speed_label.size().x
			+ SPEED_LABEL_MARGIN.1
			+ SPEED_WIDTH
			+ PRESETS_WIDTH
			+ DIVIDER + 4. * (BUTTON + 2.);
		row.x = row.x.max(bar.right() - PADDING.0 - right);
		row.skip(SPEED_LABEL_MARGIN.0);
		let at = row.take(speed_label.size().x);
		ui.painter().galley(pos2(at.left(), at.center().y - speed_label.size().y / 2.), speed_label, palette.text);
		row.skip(SPEED_LABEL_MARGIN.1);
		let speed = p.speed();
		let tip = s.get("TipPreviewSpeed");
		let at = row.take(SPEED_WIDTH);
		let at = Rect::from_center_size(at.center(), vec2(SPEED_WIDTH, INPUT_HEIGHT));
		let speed_id = id("PreviewSpeed");
		let edited = number_box(ui, at, speed_id, speed, &SPEED, false, Some(s.get("LabelPreviewSpeed")));
		edited.offer(ui.ctx(), speed_id, Event::SetSpeed);
		out.extend(edited.applied.map(Event::SetSpeed));
		edited.response.on_hover_text(tip);
		let presets = ui.interact(row.take(PRESETS_WIDTH), id("PreviewSpeedPresets"), Sense::click());
		let presets = presets.on_hover_text(tip);
		presets.widget_info(|| WidgetInfo::labeled(Role::Button, true, tip));
		paint_button(ui, &presets, ICON_ARROW_DROP_DOWN, true, false, palette);
		take_activation_keys(&presets);
		Popup::menu(&presets).show(|ui| {
			for preset in SPEED_PRESETS {
				if check_entry(ui, &preset.to_string(), (speed - preset).abs() < 1e-9).clicked() {
					out.push(Event::SetSpeed(preset));
				}
			}
		});
		divider(ui, &mut row, palette.border);
		let layers = [
			(Layer::Field, "PreviewShowField", ICON_WAVES, "TipPreviewShowField", display.field.preview),
			(
				Layer::Intensity,
				"PreviewShowIntensity",
				ICON_LIGHT_MODE,
				"TipPreviewShowIntensity",
				display.intensity.preview,
			),
		];
		for (layer, name, icon, tip, on) in layers {
			if button(ui, &mut row, name, icon, s.get(tip), true, Kind::Toggle(on)) {
				out.push(Event::Command(Command::Edit(display_edit(layer, DisplayLayerEdit::Preview(!on)))));
			}
		}
		let side_by_side = app.settings.preview_orientation == Orientation::Horizontal;
		let orientations = [
			(Orientation::Horizontal, "PreviewSideBySide", ICON_VIEW_COLUMN_2, "TipPreviewSideBySide", side_by_side),
			(Orientation::Vertical, "PreviewStacked", ICON_VIEW_AGENDA, "TipPreviewStacked", !side_by_side),
		];
		for (orientation, name, icon, tip, on) in orientations {
			if button(ui, &mut row, name, icon, s.get(tip), true, Kind::Radio(on)) {
				out.push(Event::SetPreviewOrientation(orientation));
			}
		}
	}
}

fn font() -> FontId {
	FontId::proportional(FONT_SIZE)
}

/// A 1 px line 4 px less high than a button, with 4 px margins.
fn divider(ui: &Ui, row: &mut Row, color: Color32) {
	let r = row.take(DIVIDER);
	ui.painter().rect_filled(Rect::from_center_size(r.center(), vec2(1., BUTTON - 4.)), 0., color);
}

/// `text` in `color` with `tip`.
fn label(ui: &Ui, row: &mut Row, id: Id, text: String, tip: &str, color: Color32) {
	let galley = ui.painter().layout_no_wrap(text, font(), color);
	let at = row.take(galley.size().x);
	let response = ui.interact(at, id, Sense::hover()).on_hover_text(tip);
	response.widget_info(|| WidgetInfo::labeled(Role::Label, true, galley.text()));
	ui.painter().galley(pos2(at.left(), at.center().y - galley.size().y / 2.), galley, color);
}

/// A [`BUTTON`] square button showing `icon`, `tip` its tooltip and name; pressed while checked, grey while
/// disabled, an accent border under keyboard focus. Enter or Space presses it when focused.
fn tool_button(ui: &Ui, rect: Rect, id: Id, icon: MaterialIcon, tip: &str, enabled: bool, kind: Kind) -> Response {
	let sense = if enabled { Sense::click() } else { Sense::hover() };
	let response = ui.interact(rect, id, sense).on_hover_text(tip);
	let checked = matches!(kind, Kind::Toggle(true) | Kind::Radio(true));
	response.widget_info(|| match kind {
		Kind::Plain => WidgetInfo::labeled(Role::Button, enabled, tip),
		Kind::Toggle(on) => WidgetInfo::selected(Role::Button, enabled, on, tip),
		Kind::Radio(on) => WidgetInfo::selected(Role::RadioButton, enabled, on, tip),
	});
	paint_button(ui, &response, icon, enabled, checked, Palette::of(ui.ctx().theme()));
	take_activation_keys(&response);
	response
}

fn paint_button(ui: &Ui, response: &Response, icon: MaterialIcon, enabled: bool, checked: bool, palette: &Palette) {
	let painter = ui.painter();
	let rect = response.rect;
	let background = if checked {
		palette.accent
	} else if enabled && response.hovered() {
		palette.hover_background
	} else {
		Color32::TRANSPARENT
	};
	painter.rect_filled(rect, 0., background);
	if response.has_focus() {
		painter.rect_stroke(rect, 0., Stroke::new(1., palette.accent), StrokeKind::Inside);
	}
	let color = if enabled { palette.text } else { palette.text_disabled };
	painter.text(
		rect.center(),
		Align2::CENTER_CENTER,
		icon.codepoint,
		FontId::new(ICON_SIZE, icon.font_family()),
		color,
	);
}
