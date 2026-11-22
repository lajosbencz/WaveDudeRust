//! The modal questions and messages as data (docs/ui/70, Message window): one shows at a time, the others wait in
//! turn; a frontend draws the shown one and closes it once answered.

use std::collections::VecDeque;

use crate::Event;
#[cfg(not(target_arch = "wasm32"))]
use crate::export::session::Target as ExportTarget;
use crate::export::session::{ExportForm, Step};
#[cfg(not(target_arch = "wasm32"))]
use crate::jobs::Job;
use crate::memory_warning::MemoryWarning;
use crate::strings::Strings;

/// A modal and what it shows.
pub enum Modal {
	/// "Save the changes to {name}?", formatted.
	AskSave(String),
	/// A notice of the updater, formatted.
	#[cfg(not(target_arch = "wasm32"))]
	Info(String),
	/// One line; the detail is in the log.
	Error(String),
	/// The crash report an earlier session left, formatted, and the text Copy puts on the clipboard: natively the
	/// report's path, in the browser the report itself.
	Crash {
		text: String,
		copy: String,
	},
	/// A question whose yes answers `yes`; No is the default when yes `destroys` something.
	Confirm {
		text: String,
		yes: Reply,
		destroys: bool,
	},
	Export(ExportForm),
	/// A completed export's file, downloaded when the user clicks Download: a browser may drop a download a page
	/// starts without the user's input (decision 0025).
	#[cfg(target_arch = "wasm32")]
	Download(crate::platform::Download),
	MemoryWarning(MemoryWarning),
	/// "Load the file?", formatted, about the scene image file that differs from the embedded one.
	SceneImage(String),
	/// The key table: each group's title and its (label, key text) rows.
	Shortcuts(Vec<(String, Vec<(String, String)>)>),
	/// A secondary window of a frontend that shows it as a modal.
	Window(Window),
}

/// A secondary window; what it shows is the frontend's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
	About,
	Examples,
	Welcome,
	Settings,
}

impl Modal {
	pub fn ask_save(s: &Strings, name: &str) -> Self {
		Self::AskSave(s.format("SaveChangesFormat", &[&name]))
	}

	/// `report` as [`crate::platform::take_crash_report`] gives it.
	pub fn crash(s: &Strings, report: String) -> Self {
		Self::Crash { text: s.format("CrashReportFormat", &[&report]), copy: report }
	}

	/// A question the exit waits for.
	fn asks(&self) -> bool {
		matches!(self, Self::AskSave(_) | Self::Confirm { .. })
	}
}

/// The update a question offers, carried with it so racing checks cannot cross.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
pub struct Offered(pub Box<cargo_packager_updater::Update>);

#[cfg(not(target_arch = "wasm32"))]
impl PartialEq for Offered {
	fn eq(&self, other: &Self) -> bool {
		self.0.version == other.0.version && self.0.download_url == other.0.download_url
	}
}

/// What an answered modal asks the window for.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
	Send(Event),
	Export(Step),
	/// Downloads the update the question offered.
	#[cfg(not(target_arch = "wasm32"))]
	DownloadUpdate(Offered),
	/// The export dialog's save dialog of a box; the modal stays.
	#[cfg(not(target_arch = "wasm32"))]
	Browse(ExportTarget),
	/// Puts `text` on the clipboard and tells so with the string `note`; the modal stays.
	Copy {
		text: String,
		note: &'static str,
	},
	/// Uploads the scene image the project names but lacks.
	#[cfg(target_arch = "wasm32")]
	LocateSceneImage,
}

/// The modal shown, first, and the ones waiting.
#[derive(Default)]
pub struct Modals {
	queue: VecDeque<Modal>,
	/// Another modal shows, or none, since [`Self::take_fresh`].
	fresh: bool,
}

impl Modals {
	pub fn is_open(&self) -> bool {
		!self.queue.is_empty()
	}

	/// Shows `modal`, or queues it after the shown one; a memory warning shown or waiting is not asked again.
	pub fn show(&mut self, modal: Modal) {
		let warning = |m: &Modal| matches!(m, Modal::MemoryWarning(_));
		if warning(&modal) && self.queue.iter().any(warning) {
			return;
		}
		self.fresh |= self.queue.is_empty();
		self.queue.push_back(modal);
	}

	/// The modal shown.
	pub fn shown_mut(&mut self) -> Option<&mut Modal> {
		self.queue.front_mut()
	}

	/// Whether another modal shows, or none, since the last call.
	pub fn take_fresh(&mut self) -> bool {
		std::mem::take(&mut self.fresh)
	}

	/// Closes the modal shown; the next one waiting shows.
	pub fn close(&mut self) {
		self.fresh |= self.queue.pop_front().is_some();
	}

	/// Drops every modal, shown or waiting, but the questions whose answer the exit needs.
	pub fn drop_for_exit(&mut self) {
		self.retain(Modal::asks);
	}

	fn retain(&mut self, keep: impl Fn(&Modal) -> bool) {
		self.fresh |= self.queue.front().is_some_and(|m| !keep(m));
		self.queue.retain(keep);
	}

	/// The browse dialog of the shown export dialog's box `target`.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn browse(&self, target: ExportTarget) -> Option<Job> {
		match self.queue.front()? {
			Modal::Export(form) => {
				let (spec, start) = form.browse(target);
				Some(Job::Browse { target, spec, start })
			}
			_ => None,
		}
	}

	/// A browse dialog's answer, into the shown export dialog's box.
	#[cfg(not(target_arch = "wasm32"))]
	pub fn browsed(&mut self, target: ExportTarget, path: &std::path::Path) {
		if let Some(Modal::Export(form)) = self.queue.front_mut() {
			*form.text_mut(target) = path.display().to_string();
		}
	}

	/// Asks the scene image question `text`, or words the one shown or waiting so: the offer may have changed.
	pub fn ask_scene_image(&mut self, text: String) {
		match self.queue.iter_mut().find(|m| matches!(m, Modal::SceneImage(_))) {
			Some(Modal::SceneImage(asked)) => *asked = text,
			_ => self.show(Modal::SceneImage(text)),
		}
	}

	/// Drops the scene image question: the offer it asked about is gone.
	pub fn drop_scene_image(&mut self) {
		self.retain(|m| !matches!(m, Modal::SceneImage(_)));
	}

	/// Drops the memory warning, answered elsewhere or gone.
	pub fn drop_memory_warning(&mut self) {
		self.retain(|m| !matches!(m, Modal::MemoryWarning(_)));
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn texts(modals: &Modals) -> Vec<&str> {
		modals
			.queue
			.iter()
			.map(|m| match m {
				Modal::AskSave(t) | Modal::Error(t) | Modal::Crash { text: t, .. } | Modal::Confirm { text: t, .. } => {
					t.as_str()
				}
				_ => "",
			})
			.collect()
	}

	#[test]
	fn the_exit_keeps_only_its_questions_in_their_order() {
		let mut modals = Modals::default();
		modals.show(Modal::Error("a".into()));
		modals.show(Modal::AskSave("b".into()));
		modals.show(Modal::Crash { text: "c".into(), copy: "d".into() });
		modals.show(Modal::Confirm { text: "e".into(), yes: Reply::Send(Event::Run), destroys: false });
		modals.show(Modal::AskSave("f".into()));
		modals.drop_for_exit();
		assert_eq!(texts(&modals), ["b", "e", "f"]);
	}

	#[test]
	fn the_exit_drops_a_lone_message() {
		let mut modals = Modals::default();
		modals.show(Modal::Error("a".into()));
		modals.drop_for_exit();
		assert!(!modals.is_open());
	}
}
