//! The modals over the window (docs/ui/70): one shows at a time, the others wait in turn; the message window's look
//! and keys (Enter the default button, Esc the cancel button, a mnemonic with or without Alt).

use std::collections::VecDeque;

use eframe::egui::{
	self, Align, Align2, Color32, FontId, Frame, Id, Key, Layout, Modifiers, Response, ScrollArea, Sense, Stroke, Ui,
	Vec2, vec2,
};
use wavedude_app_state::strings::{Strings, mnemonic, strip_mnemonic};
use wavedude_app_state::{Event, SaveChoice};

use super::about::About;
use super::examples::Examples;
use super::export::{ExportDialog, Step};
use super::memory_warning::MemoryWarning;
use super::menu_keys::letter_of;
use super::properties;
use super::startup::Startup;
use super::status_bar::ERROR;
use super::theme::Palette;
use crate::App;

const PADDING: f32 = 16.;
const SPACING: f32 = 12.;
const MESSAGE_WIDTH: f32 = 440.;
const DISC: f32 = 32.;
const BUTTON: Vec2 = Vec2::new(75., 23.);
const BACKDROP: Color32 = Color32::from_black_alpha(110);
/// The crash report's text scrolls beyond this height.
const REPORT_HEIGHT: f32 = 240.;

/// A modal and what it shows.
pub(super) enum Modal {
	/// "Save the changes to {name}?", formatted.
	AskSave(String),
	/// One line; the detail is in the log.
	Error(String),
	/// The crash report an earlier session left, formatted, and the text "Copy" puts on the clipboard: natively the
	/// report's path, in the browser the report itself.
	Crash {
		text: String,
		copy: String,
	},
	/// A notice with OK.
	#[cfg_attr(target_arch = "wasm32", expect(dead_code, reason = "the updater's, which the browser has not"))]
	Info(String),
	/// A question; Yes does `yes`. No is the default when Yes `destroys` something.
	Confirm {
		text: String,
		yes: Then,
		destroys: bool,
	},
	MemoryWarning(MemoryWarning),
	Export(ExportDialog),
	About(About),
	Examples(Examples),
	Startup(Startup),
}

impl Modal {
	pub(super) fn ask_save(s: &Strings, name: &str) -> Self {
		Self::AskSave(s.format("SaveChangesFormat", &[&name]))
	}

	/// `report` as [`wavedude_app_state::platform::take_crash_report`] gives it.
	pub(super) fn crash(s: &Strings, report: String) -> Self {
		Self::Crash { text: s.format("CrashReportFormat", &[&report]), copy: report }
	}

	fn id(&self) -> Id {
		Id::unique(match self {
			Self::AskSave(_) => "AskSave",
			Self::Error(_) => "Error",
			Self::Crash { .. } => "Crash",
			Self::Info(_) => "Info",
			Self::Confirm { .. } => "Confirm",
			Self::MemoryWarning(_) => "MemoryWarning",
			Self::Export(_) => "Export",
			Self::About(_) => "About",
			Self::Examples(_) => "Examples",
			Self::Startup(_) => "Startup",
		})
	}
}

/// What the window does once a modal was answered.
#[derive(Clone)]
pub(super) enum Then {
	Send(Event),
	/// Opens the Examples window.
	Examples,
	/// Each in turn.
	All(Vec<Then>),
	/// Downloads the update and asks to restart into it.
	#[cfg(not(target_arch = "wasm32"))]
	Download(Box<cargo_packager_updater::Update>),
	/// Looks for a newer release, quietly.
	#[cfg(not(target_arch = "wasm32"))]
	CheckForUpdates,
	Export(Step),
	/// Uploads the scene image the project lacks.
	#[cfg(target_arch = "wasm32")]
	LocateSceneImage,
}

/// A modal's answer: it closes, and the window does `Some` [`Then`].
pub(super) type Reply = Option<Then>;

/// The modal shown and the ones waiting.
#[derive(Default)]
pub(super) struct Modals {
	shown: Option<Modal>,
	waiting: VecDeque<Modal>,
	/// The focus before the first modal took it.
	focus: Option<Option<Id>>,
	/// The shown modal has not been drawn yet: its default button takes the focus.
	fresh: bool,
}

impl Modals {
	pub(super) fn is_open(&self) -> bool {
		self.shown.is_some()
	}

	/// Shows `modal`, or queues it after the open one; a memory warning shown or waiting is not asked again.
	pub(super) fn show(&mut self, modal: Modal) {
		let warning = |m: &Modal| matches!(m, Modal::MemoryWarning(_));
		if warning(&modal) && (self.shown.as_ref().is_some_and(warning) || self.waiting.iter().any(warning)) {
			return;
		}
		if self.shown.is_none() {
			self.shown = Some(modal);
			self.fresh = true;
		} else {
			self.waiting.push_back(modal);
		}
	}

	/// Drops the memory warning, shown or waiting: the run it asked about is gone.
	pub(super) fn drop_memory_warning(&mut self, ctx: &egui::Context) {
		self.waiting.retain(|m| !matches!(m, Modal::MemoryWarning(_)));
		if matches!(self.shown, Some(Modal::MemoryWarning(_))) {
			self.close(ctx);
		}
	}

	/// The next waiting modal shows; after the last, the focus goes back.
	fn close(&mut self, ctx: &egui::Context) {
		self.shown = self.waiting.pop_front();
		self.fresh = true;
		if self.shown.is_none()
			&& let Some(focus) = self.focus.take()
		{
			ctx.memory_mut(|m| match (focus, m.focused()) {
				(Some(id), _) => m.request_focus(id),
				(None, Some(now)) => m.surrender_focus(now),
				(None, None) => {}
			});
		}
	}

	/// Draws the shown modal; once answered it closes, events its answer sends go to `out`, and what the window does
	/// then is returned.
	pub(super) fn ui(&mut self, ctx: &egui::Context, app: &App, out: &mut Vec<Event>) -> Option<Then> {
		let modal = self.shown.as_mut()?;
		let fresh = std::mem::take(&mut self.fresh);
		if fresh && self.focus.is_none() {
			self.focus = Some(ctx.memory(egui::Memory::focused));
		}
		let palette = Palette::of(ctx.theme());
		let frame = Frame::new().fill(palette.tool_window_background).stroke(Stroke::new(1., palette.border));
		let s = &app.strings;
		let shown =
			egui::Modal::new(modal.id()).frame(frame.inner_margin(PADDING)).backdrop_color(BACKDROP).show(ctx, |ui| {
				properties::style(ui, palette);
				ui.spacing_mut().item_spacing = vec2(8., SPACING);
				match modal {
					Modal::AskSave(text) => {
						let choices = [
							Choice::default(s.get("DialogSave")),
							Choice::plain(s.get("DialogDiscard")),
							Choice::cancel(s.get("DialogCancel")),
						];
						let answer = [SaveChoice::Save, SaveChoice::DontSave, SaveChoice::Cancel];
						message(ui, fresh, Disc::Question, text, &choices)
							.map(|i| Some(Then::Send(Event::SaveChoice(answer[i]))))
					}
					Modal::Error(text) => {
						message(ui, fresh, Disc::Error, text, &[Choice::both(s.get("DialogOk"))]).map(|_| None)
					}
					Modal::Info(text) => {
						message(ui, fresh, Disc::Info, text, &[Choice::both(s.get("DialogOk"))]).map(|_| None)
					}
					Modal::Confirm { text, yes, destroys } => {
						let (yes_key, no_key) = (s.get("DialogYes"), s.get("DialogNo"));
						let choices = if *destroys {
							[Choice::plain(yes_key), Choice::both(no_key)]
						} else {
							[Choice::default(yes_key), Choice::cancel(no_key)]
						};
						message(ui, fresh, Disc::Question, text, &choices).map(|i| (i == 0).then(|| yes.clone()))
					}
					Modal::Crash { text, copy } => {
						let (copy_key, copied_key) = if wavedude_app_state::platform::HAS_PATHS {
							("CrashCopyPath", "CrashPathCopied")
						} else {
							("CrashCopyReport", "CrashReportCopied")
						};
						let choices = [Choice::plain(s.get(copy_key)), Choice::both(s.get("DialogOk"))];
						match message(ui, fresh, Disc::Error, text, &choices) {
							Some(0) => {
								ui.ctx().copy_text(copy.clone());
								tracing::info!("{}", s.get(copied_key));
								None
							}
							answered => answered.map(|_| None),
						}
					}
					Modal::MemoryWarning(warning) => warning.ui(ui, fresh, app),
					Modal::Export(dialog) => dialog.ui(ui, fresh, app, out),
					Modal::About(about) => about.ui(ui, fresh, app),
					Modal::Examples(examples) => examples.ui(ui, fresh, app),
					Modal::Startup(startup) => startup.ui(ui, fresh, app, out),
				}
			});
		let reply = shown.inner?;
		self.close(ctx);
		reply
	}
}

#[derive(Clone, Copy)]
enum Disc {
	Error,
	Question,
	Info,
}

/// A button of a dialog: its text with the mnemonic, and whether Enter (`default`) or Esc (`cancel`) press it.
#[derive(Clone, Copy)]
pub(super) struct Choice<'a> {
	pub(super) text: &'a str,
	pub(super) default: bool,
	pub(super) cancel: bool,
	pub(super) enabled: bool,
	pub(super) tip: Option<&'a str>,
}

impl<'a> Choice<'a> {
	pub(super) fn plain(text: &'a str) -> Self {
		Self { text, default: false, cancel: false, enabled: true, tip: None }
	}

	pub(super) fn default(text: &'a str) -> Self {
		Self { default: true, ..Self::plain(text) }
	}

	pub(super) fn cancel(text: &'a str) -> Self {
		Self { cancel: true, ..Self::plain(text) }
	}

	/// The only button: Enter and Esc both press it.
	pub(super) fn both(text: &'a str) -> Self {
		Self { default: true, cancel: true, ..Self::plain(text) }
	}
}

/// A message window's body: the disc, `text` wrapped beside it (scrolled when long), `choices` at the bottom right;
/// the index of the one pressed.
fn message(ui: &mut Ui, fresh: bool, disc: Disc, text: &str, choices: &[Choice<'_>]) -> Option<usize> {
	ui.set_width(MESSAGE_WIDTH - 2. * PADDING);
	ui.horizontal_top(|ui| {
		ui.spacing_mut().item_spacing.x = SPACING;
		paint_disc(ui, disc);
		ScrollArea::vertical().max_height(REPORT_HEIGHT).show(ui, |ui| {
			ui.add(egui::Label::new(text).wrap());
		});
	});
	buttons(ui, fresh, choices)
}

fn paint_disc(ui: &mut Ui, disc: Disc) {
	let (fill, glyph) = match disc {
		Disc::Error => (ERROR, "!"),
		Disc::Question => (Palette::of(ui.ctx().theme()).accent, "?"),
		Disc::Info => (Palette::of(ui.ctx().theme()).accent, "i"),
	};
	let (rect, _) = ui.allocate_exact_size(Vec2::splat(DISC), Sense::hover());
	let painter = ui.painter();
	painter.circle_filled(rect.center(), DISC / 2., fill);
	let font = FontId::new(18., wavedude_view::fonts::bold());
	painter.text(rect.center(), Align2::CENTER_CENTER, glyph, font, Color32::WHITE);
}

/// A dialog's button with its mnemonic stripped.
pub(super) fn button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
	ui.add_enabled(enabled, egui::Button::new(strip_mnemonic(text)).min_size(BUTTON))
}

/// The row of `choices` at the bottom right; the index of the one pressed by the pointer or a key. The default one
/// takes the focus when the dialog is `fresh`.
pub(super) fn buttons(ui: &mut Ui, fresh: bool, choices: &[Choice<'_>]) -> Option<usize> {
	let mut pressed = None;
	ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
		for (i, choice) in choices.iter().enumerate().rev() {
			let mut response = button(ui, choice.text, choice.enabled);
			if let Some(tip) = choice.tip {
				response = response.on_hover_text(tip);
			}
			if fresh && choice.default {
				response.request_focus();
			}
			if response.clicked() {
				pressed = Some(i);
			}
		}
	});
	pressed.or_else(|| pressed_key(ui, choices))
}

/// Whether the mnemonic of `text` was typed, with or without Alt, outside a text box; the key is taken.
pub(super) fn mnemonic_pressed(ui: &Ui, text: &str) -> bool {
	pressed_key(ui, &[Choice::plain(text)]).is_some()
}

/// The choice a key picks: Enter, Esc or a mnemonic letter; the key is taken.
fn pressed_key(ui: &Ui, choices: &[Choice<'_>]) -> Option<usize> {
	let typing = ui.ctx().text_edit_focused();
	let pick = |key: Key| -> Option<usize> {
		let find = |f: &dyn Fn(&Choice<'_>) -> bool| choices.iter().position(|c| c.enabled && f(c));
		match key {
			Key::Enter => find(&|c| c.default),
			Key::Escape => find(&|c| c.cancel),
			key if !typing => letter_of(key).and_then(|l| find(&|c| mnemonic(c.text) == Some(l))),
			_ => None,
		}
	};
	let mut picked = None;
	ui.input_mut(|input| {
		input.events.retain(|event| {
			let egui::Event::Key { key, pressed: true, modifiers, .. } = event else { return true };
			if picked.is_some() || !(modifiers.is_none() || *modifiers == Modifiers::ALT) {
				return true;
			}
			picked = pick(*key);
			picked.is_none()
		});
	});
	picked
}
