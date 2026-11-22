//! The modal questions and messages (docs/ui/70, Message window) of [`Modals`]: the shown one drawn as a dialog.

use eframe::egui::{self, ScrollArea};
use egui_mineral::overlay::Dismiss;
use egui_mineral::semantic::KeySalt;
use egui_mineral_material::adaptive::WindowClass;
use egui_mineral_material::prelude::{
	CheckButton, Dialog, DialogAction, Kbd, LinearProgressIndicator, Rt, TypeScale, icons,
};
use waveduderust_app::memory_warning::{MemoryWarning, usage};
use waveduderust_app::modal::{Modal, Modals, Reply};
use waveduderust_app::platform::HAS_PATHS;
#[cfg(not(target_arch = "wasm32"))]
use waveduderust_app::platform::show_in_folder;
use waveduderust_app::strings::{Strings, mnemonic, strip_mnemonic};
use waveduderust_app::{Event, SaveChoice};

use crate::export::{self, Asked};
use crate::{App, Mui, Salt, form};

const SHORTCUTS_H: f32 = 420.;

/// Draws the shown modal; it closes once answered.
pub(crate) fn show(modals: &mut Modals, rt: &Rt, app: &App) -> Option<Reply> {
	let s = &app.strings;
	// the queue's front shows; it leaves the queue once answered
	let mut open = true;
	let action = |key: &str| action(s, key);
	let closed = |d: Option<Dismiss>| d.is_some();
	let (closes, reply) = match modals.shown_mut()? {
		Modal::AskSave(text) => {
			let r = Dialog::new("modal-ask-save", text.as_str())
				.icon(icons::ICON_HELP)
				.action(Salt(SaveChoice::Cancel), action("DialogCancel"))
				.action(Salt(SaveChoice::DontSave), action("DialogDiscard"))
				.action(Salt(SaveChoice::Save), action("DialogSave"))
				.default_action(Salt(SaveChoice::Save))
				.show_modal(rt, &mut open, |_| {});
			let choice = r.clicked.map(|c| c.0).or(r.dismissed.map(|_| SaveChoice::Cancel));
			(choice.is_some(), choice.map(|c| Reply::Send(Event::SaveChoice(c))))
		}
		#[cfg(not(target_arch = "wasm32"))]
		Modal::Info(text) => {
			let r = Dialog::new("modal-info", text.as_str())
				.icon(icons::ICON_INFO)
				.action(YES, action("DialogOk"))
				.default_action(YES)
				.show_modal(rt, &mut open, |_| {});
			(r.clicked.is_some() || closed(r.dismissed), None)
		}
		Modal::Error(text) => {
			let r = Dialog::new("modal-error", text.as_str())
				.icon(icons::ICON_ERROR)
				.action(YES, action("DialogOk"))
				.default_action(YES)
				.show_modal(rt, &mut open, |_| {});
			(r.clicked.is_some() || closed(r.dismissed), None)
		}
		Modal::Crash { text, copy } => {
			let (copy_key, note) =
				if HAS_PATHS { ("CrashCopyPath", "CrashPathCopied") } else { ("CrashCopyReport", "CrashReportCopied") };
			let dialog = Dialog::new("modal-crash", s.get("Title")).icon(icons::ICON_ERROR).supporting(text.as_str());
			let mut dialog = dialog.action(Crash::Copy, action(copy_key));
			if HAS_PATHS {
				dialog = dialog.action(Crash::ShowFolder, action("CrashShowFolder"));
			}
			let r = dialog.action(Crash::Ok, action("DialogOk")).default_action(Crash::Ok).show_modal(
				rt,
				&mut open,
				|_| {},
			);
			match r.clicked {
				Some(Crash::Copy) => (false, Some(Reply::Copy { text: copy.clone(), note })),
				#[cfg(not(target_arch = "wasm32"))]
				Some(Crash::ShowFolder) => {
					show_in_folder(std::path::Path::new(copy));
					(false, None)
				}
				Some(_) => (true, None),
				None => (closed(r.dismissed), None),
			}
		}
		Modal::Confirm { text, yes, .. } => {
			let r = Dialog::new("modal-confirm", text.as_str())
				.icon(icons::ICON_WARNING)
				.action(NO, action("DialogCancel"))
				.action(YES, action("DialogOk"))
				.default_action(NO)
				.show_modal(rt, &mut open, |_| {});
			(r.clicked.is_some() || closed(r.dismissed), (r.clicked == Some(YES)).then(|| yes.clone()))
		}
		Modal::SceneImage(text) => {
			let r = Dialog::new("modal-scene-image", text.as_str())
				.icon(icons::ICON_HELP)
				.action(NO, action("SceneImageKeep"))
				.action(YES, action("SceneImageTake"))
				.default_action(YES)
				.show_modal(rt, &mut open, |_| {});
			let take = r.clicked.or(r.dismissed.map(|_| NO));
			(take.is_some(), take.map(|t| Reply::Send(Event::SceneImageOffer(t))))
		}
		Modal::Export(form) => {
			let ready = form.chosen(app);
			let title = strip_mnemonic(s.get(form.title_key()));
			let ok = action("DialogOk").disabled(ready.is_none());
			let content = |ui: &mut Mui<'_>| export::dialog(ui, form, app);
			// full screen, Cancel is the bar's close button
			let (r, ok) = if WindowClass::of(rt.core()) == WindowClass::Compact {
				let r = Dialog::full_screen("modal-export", title)
					.confirm(YES, ok)
					.enter_confirms(true)
					.show_modal(rt, &mut open, content);
				let ok = r.clicked == Some(YES);
				(r, ok)
			} else {
				let r = Dialog::new("modal-export", title)
					.icon(icons::ICON_IOS_SHARE)
					.action(NO, action("DialogCancel"))
					.action(YES, ok)
					.default_action(YES)
					.show_modal(rt, &mut open, content);
				let ok = r.clicked == Some(YES);
				(r, ok)
			};
			match (r.clicked, r.inner.flatten()) {
				(Some(_), _) if ok => (true, ready.map(Reply::Export)),
				(Some(_), _) => (true, None),
				(None, Some(Asked::Send(event))) => (false, Some(Reply::Send(event))),
				#[cfg(not(target_arch = "wasm32"))]
				(None, Some(Asked::Browse(target))) => (false, Some(Reply::Browse(target))),
				(None, None) => (closed(r.dismissed), None),
			}
		}
		#[cfg(target_arch = "wasm32")]
		Modal::Download(download) => {
			let text = s.format("ExportReadyFormat", &[&download.name()]);
			let r = Dialog::new("modal-download", text)
				.icon(icons::ICON_DOWNLOAD)
				.action(NO, action("DialogClose"))
				.action(YES, action("DialogDownload"))
				.default_action(YES)
				.show_modal(rt, &mut open, |_| {});
			if r.clicked == Some(YES)
				&& let Err(e) = download.start()
			{
				tracing::error!("{} could not be downloaded: {e}", download.name());
			}
			(r.clicked.is_some() || closed(r.dismissed), None)
		}
		Modal::MemoryWarning(w) => {
			if app.live.warning().is_none() {
				(true, None)
			} else {
				let r = Dialog::new("modal-memory", s.get("Title"))
					.icon(icons::ICON_MEMORY)
					.supporting(w.text.as_str())
					.action(NO, action("DialogCancel"))
					.action(YES, action("DialogRun"))
					.default_action(YES)
					.show_modal(rt, &mut open, |ui| memory_warning(ui, w, app));
				let answer = r.clicked.or(r.dismissed.map(|_| NO));
				(answer.is_some(), answer.map(|run| Reply::Send(w.answer(run))))
			}
		}
		// this window shows its secondary windows as pages
		Modal::Window(_) => (true, None),
		Modal::Shortcuts(groups) => {
			let r = Dialog::new("modal-shortcuts", strip_mnemonic(s.get("MenuHelpShortcuts")))
				.icon(icons::ICON_KEYBOARD)
				.action(YES, action("DialogOk"))
				.default_action(YES)
				.show_modal(rt, &mut open, |ui| shortcuts(ui, groups));
			(r.clicked.is_some() || closed(r.dismissed), None)
		}
	};
	if closes {
		modals.close();
	}
	reply
}

/// The levers, the usage bar, what the ticks give and Do not warn me again.
fn memory_warning(ui: &mut Mui<'_>, w: &mut MemoryWarning, app: &App) {
	let s = &app.strings;
	form::text(ui, s.get("MemoryWarningChoices"), TypeScale::BodyMedium, false);
	let (enabled, levers) = (w.enabled, &mut w.levers);
	let rt = ui.rt();
	let rows: [(&str, &str, bool, &mut bool); 4] = [
		("cache", &w.cache, enabled.cache_auto, &mut levers.cache_auto),
		("resolution", s.get("MemoryWarningResolution"), enabled.resolution, &mut levers.resolution),
		("frames", s.get("MemoryWarningFrames"), enabled.frames, &mut levers.frames),
		("samples", s.get("MemoryWarningSamples"), enabled.samples, &mut levers.samples),
	];
	let mut changed = false;
	for (key, text, enabled, on) in rows {
		let check = CheckButton::new(("memory-lever", key), text.to_owned(), on);
		ui.add_enabled_ui(enabled, |ui| changed |= rt.ui(ui).mineral(check).changed());
	}
	if changed && let Some(result) = w.result_of(app) {
		w.result = result;
	}
	if let Some(warning) = app.live.warning() {
		let (need, budget) = (warning.bytes_of(w.levers), warning.budget_of(w.levers));
		let share = if budget == 0 { 1. } else { (need as f64 / budget as f64).min(1.) as f32 };
		ui.mineral(LinearProgressIndicator::determinate("memory-usage", share).label(usage(need, budget)));
	}
	form::text(ui, &w.result, TypeScale::BodyMedium, true);
	ui.mineral(CheckButton::new("memory-mute", s.get("MemoryWarningMute"), &mut w.mute));
}

/// The key table, a heading per group.
fn shortcuts(ui: &mut Mui<'_>, groups: &[(String, Vec<(String, String)>)]) {
	let rt = ui.rt();
	ScrollArea::vertical().max_height(SHORTCUTS_H).show(ui, |ui| {
		let ui = &mut rt.ui(ui);
		for (g, (title, rows)) in groups.iter().enumerate() {
			form::heading(ui, title);
			egui::Grid::new(("shortcuts", g)).num_columns(2).show(ui, |ui| {
				let ui = &mut rt.ui(ui);
				for (i, (label, key)) in rows.iter().enumerate() {
					form::text(ui, label, TypeScale::BodyMedium, false);
					ui.mineral(Kbd::new(("shortcut", g, i), key.split('+').filter(|k| !k.is_empty())));
					ui.end_row();
				}
			});
			ui.add_space(8.);
		}
	});
}

/// The value of a dialog's confirming action.
const YES: bool = true;
/// The value of a dialog's declining action.
const NO: bool = false;

/// The crash dialog's actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, KeySalt)]
enum Crash {
	Copy,
	ShowFolder,
	Ok,
}

impl KeySalt for Salt<SaveChoice> {
	fn write_key(&self, out: &mut String) {
		out.push_str(match self.0 {
			SaveChoice::Save => "save",
			SaveChoice::DontSave => "dont-save",
			SaveChoice::Cancel => "cancel",
		});
	}
}

/// The dialog action of the string `key`, Alt with its marked letter activating it.
fn action(s: &Strings, key: &str) -> DialogAction {
	let text = s.get(key);
	let action = DialogAction::new(strip_mnemonic(text));
	match mnemonic(text).filter(char::is_ascii_alphanumeric) {
		Some(c) => action.mnemonic(c),
		None => action,
	}
}
