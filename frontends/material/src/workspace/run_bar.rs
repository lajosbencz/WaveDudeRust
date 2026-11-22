//! The run bar (DESIGN.md, Run bar, timeline, status), in Tab order: Run / Stop, Pause run, Play / Pause, previous,
//! next, the timeline, the read-out, Live, Loop, the speed and the overflow menu. The timeline takes a row of its own above the others.

use eframe::egui::{Align, Key, Label, Layout, Sense, TextFormat, Ui, vec2};
use egui_mineral::edit::EditEvent;
use egui_mineral::kit::paint::tabular_figures;
use egui_mineral::range::RangeModel;
use egui_mineral::semantic::KeySalt;
use egui_mineral::shortcut;
use egui_mineral_material::prelude::{
	ButtonVariant, IconButton, MaterialExt as _, Menu, MenuItem, NumberField, Popover, PushButton, TypeScale, icons,
};
use waveduderust_app::Event;
use waveduderust_app::menus::MenuAction;
use waveduderust_app::playback::{SPEED_PRESETS, SPEED_RANGE};
use waveduderust_app::preview::PaneKey;
use waveduderust_app::strings::strip_mnemonic;
use waveduderust_egui_preview::timeline::{HEIGHT, TimelineColors};

use super::Workspace;
use crate::commands::{Cmd, ViewCommand, key_of, runs};
use crate::{App, Mui};

/// The read-out's width: "99999 / 99999  step 99999" never shifts its neighbours.
const READOUT_W: f32 = 200.;
const PAD: f32 = 8.;

pub(super) fn show(ws: &mut Workspace, ui: &mut Mui<'_>, app: &App, out: &mut Vec<Event>) {
	let rt = ui.rt();
	ui.add_space(PAD / 2.);
	ui.horizontal(|ui| {
		let ui = &mut rt.ui(ui);
		ui.add_space(PAD);
		let w = ui.available_width() - PAD;
		timeline(ws, ui, app, w, out);
	});
	ui.horizontal(|ui| {
		let ui = &mut rt.ui(ui);
		ui.add_space(PAD);
		run_controls(ui, app);
		ui.separator();
		play_controls(ui, app);
		ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
			let ui = &mut rt.ui(ui);
			ui.add_space(PAD);
			overflow(ui, app, &mut ws.run_more_open);
			speed(ws, ui, app, out);
			toggles(ui, app);
			readout(ui, app);
		});
	});
	ui.add_space(PAD / 2.);
}

/// Fires `cmd` as its menu entry and keys would.
fn fire(ui: &Ui, cmd: Cmd) {
	shortcut::fire(ui.ctx(), cmd.id());
}

/// A tooltip with the key of `cmd`.
fn tip(ui: &Ui, text: &str, cmd: Cmd) -> String {
	match key_of(cmd) {
		Some(key) => format!("{text} ({})", shortcut::label(ui.ctx(), &key)),
		None => text.to_owned(),
	}
}

fn run_controls(ui: &mut Mui<'_>, app: &App) {
	let s = &app.strings;
	let active = app.live.run_is_active();
	let (action, icon, key) = if active {
		(MenuAction::Stop, icons::ICON_STOP, "MenuSimulationStop")
	} else {
		(MenuAction::Run, icons::ICON_PLAY_ARROW, "MenuSimulationRun")
	};
	let cmd = Cmd::Act(action);
	let text = strip_mnemonic(s.get(key));
	let button =
		PushButton::new("run-bar-run", &text).icon(icon).variant(ButtonVariant::Filled).disabled(!runs(app, cmd));
	if ui.mineral(button).into_response().on_hover_text(tip(ui, &text, cmd)).clicked() {
		fire(ui, cmd);
	}
	let mut paused = app.live.run_paused_by_user();
	let cmd = Cmd::Act(MenuAction::PauseRun);
	let text = strip_mnemonic(s.get(if paused { "TipPreviewResumeRun" } else { "TipPreviewPauseRun" }));
	let label = tip(ui, &text, cmd);
	let pause =
		IconButton::toggle("run-bar-pause", icons::ICON_PAUSE_CIRCLE, label, &mut paused).disabled(!runs(app, cmd));
	if ui.mineral(pause).changed() {
		fire(ui, cmd);
	}
}

fn play_controls(ui: &mut Mui<'_>, app: &App) {
	let s = &app.strings;
	let playing = app.live.playback.is_playing();
	let (icon, key) =
		if playing { (icons::ICON_PAUSE, "TipPreviewPause") } else { (icons::ICON_PLAY_ARROW, "TipPreviewPlay") };
	let cmd = Cmd::Act(MenuAction::Play);
	if ui.mineral(IconButton::new("run-bar-play", icon, tip(ui, s.get(key), cmd))).clicked() {
		fire(ui, cmd);
	}
	let steps = [
		("run-bar-back", icons::ICON_SKIP_PREVIOUS, "TipPreviewStepBack", PaneKey::StepBack),
		("run-bar-forward", icons::ICON_SKIP_NEXT, "TipPreviewStepForward", PaneKey::StepForward),
	];
	for (id, icon, key, pane) in steps {
		if ui.mineral(IconButton::new(id, icon, s.get(key))).clicked() {
			fire(ui, Cmd::View(ViewCommand::Pane(pane)));
		}
	}
}

/// The timeline, `w` wide.
fn timeline(ws: &mut Workspace, ui: &mut Mui<'_>, app: &App, w: f32, out: &mut Vec<Event>) {
	let (rect, _) = ui.allocate_exact_size(vec2(w, HEIGHT), Sense::hover());
	let c = ui.tokens().color;
	let colors = TimelineColors {
		background: c.surface_container_highest,
		accent: c.primary,
		border: c.outline_variant,
		tick: c.outline,
		label: c.on_surface_variant,
		text: c.on_surface,
	};
	ws.timeline.show(ui, rect, app, &mut ws.pointer_focus, colors, out);
}

/// "frame / last  step N".
fn readout(ui: &mut Mui<'_>, app: &App) {
	let s = &app.strings;
	let r = app.live.playback.readout();
	let text = format!("{}  {} {}", r.frames_text(), s.get("LabelPreviewStep"), r.step);
	let format = TextFormat::simple(TypeScale::LabelLarge.font(ui.ctx()), ui.tokens().color.on_surface_variant);
	let label = Label::new(tabular_figures(ui.ctx(), &text, &format)).selectable(false);
	let h = ui.spacing().interact_size.y;
	ui.allocate_ui_with_layout(vec2(READOUT_W, h), Layout::right_to_left(Align::Center), |ui| {
		ui.add(label).on_hover_text(s.get("TipPreviewFrame"))
	});
}

/// Loop and Live, right to left.
fn toggles(ui: &mut Mui<'_>, app: &App) {
	let s = &app.strings;
	let p = &app.live.playback;
	let toggles = [
		("run-bar-loop", icons::ICON_REPEAT, "TipPreviewLoop", PaneKey::ToggleLoop, p.looping()),
		("run-bar-live", icons::ICON_RADIO_BUTTON_CHECKED, "TipPreviewLive", PaneKey::ToggleLive, app.live.is_live()),
	];
	for (id, icon, key, pane, mut on) in toggles {
		let cmd = Cmd::View(ViewCommand::Pane(pane));
		let button = IconButton::toggle(id, icon, s.get(key), &mut on).disabled(!runs(app, cmd));
		if ui.mineral(button).changed() {
			fire(ui, cmd);
		}
	}
}

/// The speed chip: its presets and Custom, which asks for a value.
fn speed(ws: &mut Workspace, ui: &mut Mui<'_>, app: &App, out: &mut Vec<Event>) {
	let rt = ui.rt();
	let s = &app.strings;
	let now = app.live.playback.speed();
	let button =
		PushButton::new("run-bar-speed", format!("{now}x")).icon(icons::ICON_SPEED).variant(ButtonVariant::Text);
	let anchor = ui.mineral(button).into_response().on_hover_text(s.get("TipPreviewSpeed"));
	let menu = SPEED_PRESETS
		.iter()
		.fold(Menu::new("run-bar-speeds", s.get("LabelPreviewSpeed")).items(None::<(Speed, MenuItem)>), |m, &p| {
			m.item(Speed::of(p), MenuItem::new(format!("{p}x")).selected((p - now).abs() < 1e-9))
		});
	let menu = menu.divider().item(Speed::Custom, s.get("SpeedCustom"));
	match menu.show(rt, &anchor, &mut ws.speeds_open).clicked {
		Some(Speed::Preset(milli)) => out.push(Event::SetSpeed(f64::from(milli) / 1000.0)),
		Some(Speed::Custom) => {
			ws.speed = now;
			ws.speed_open = true;
		}
		None => {}
	}
	let applied = Popover::new("run-bar-speed-custom", s.get("LabelPreviewSpeed")).show_at(
		rt,
		anchor.rect,
		&mut ws.speed_open,
		|ui| {
			let range = RangeModel::new(SPEED_RANGE.0, SPEED_RANGE.1).step(0.25);
			let field = NumberField::new("run-bar-speed-value", s.get("LabelPreviewSpeed"), range, &mut ws.speed)
				.commit_on_submit(true)
				.snap_typed(false)
				.step_buttons(false);
			ui.mineral(field).into_output().edit
		},
	);
	if let Some(edit) = applied.inner {
		match edit {
			Some(EditEvent::Committed) => out.push(Event::SetSpeed(ws.speed)),
			Some(EditEvent::Cancelled) => ws.speed_open = false,
			_ => {}
		}
	}
	if applied.inner.is_some() && ui.ctx().input(|i| i.key_pressed(Key::Enter)) {
		ws.speed_open = false;
	}
}

/// A speed menu entry: a preset in thousandths, or Custom, which asks for a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Speed {
	Preset(u32),
	Custom,
}

impl Speed {
	fn of(speed: f64) -> Self {
		Self::Preset((speed * 1000.0).round() as u32)
	}
}

impl KeySalt for Speed {
	fn write_key(&self, out: &mut String) {
		match self {
			Self::Preset(milli) => ("speed", *milli).write_key(out),
			Self::Custom => out.push_str("speed-custom"),
		}
	}
}

/// Set in, Set out, Clear range and Discard frames.
fn overflow(ui: &mut Mui<'_>, app: &App, open: &mut bool) {
	let s = &app.strings;
	let anchor = ui.mineral(IconButton::new("run-bar-more", icons::ICON_MORE_VERT, s.get("NavMore")));
	let rows = [
		(Cmd::View(ViewCommand::Pane(PaneKey::SetIn)), "MenuSimulationSetIn", icons::ICON_LINE_START),
		(Cmd::View(ViewCommand::Pane(PaneKey::SetOut)), "MenuSimulationSetOut", icons::ICON_LINE_END),
		(Cmd::View(ViewCommand::Pane(PaneKey::ClearRange)), "MenuSimulationClearRange", icons::ICON_CLOSE),
		(Cmd::Act(MenuAction::DiscardFrames), "MenuSimulationDiscardFrames", icons::ICON_DELETE_SWEEP),
	];
	let menu = rows.iter().fold(
		Menu::new("run-bar-overflow", s.get("NavMore")).items(None::<(Cmd, MenuItem)>),
		|m, &(cmd, key, icon)| {
			m.item(
				cmd,
				MenuItem::new(strip_mnemonic(s.get(key))).leading(icon).command(cmd.id()).disabled(!runs(app, cmd)),
			)
		},
	);
	menu.show(ui.rt(), anchor.response(), open);
}
