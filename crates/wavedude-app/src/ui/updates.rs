//! The updater's questions and notices (docs/ui/70, Updates): the check and the download run on threads of their
//! own, their answers are taken on a later frame; a downloaded package waits in [`Pending`] for the restart.

use cargo_packager_updater::Update;
use eframe::egui;
use wavedude_app_state::strings::Strings;
use wavedude_app_state::{Event, Guarded};

use super::Boot;
use super::inbox::Inbox;
use super::modal::{Modal, Modals, Then};
use crate::downloads;
use crate::update::{self, Pending};

enum Answer {
	/// A check's result; `quiet` for the launch's check, which says nothing unless there is an update.
	Checked {
		result: Result<Option<Update>, String>,
		quiet: bool,
	},
	Downloaded(Result<(Update, Vec<u8>), String>),
}

/// The threads' answers and the package waiting for the restart.
pub(super) struct Updates {
	answers: Inbox<Answer>,
	pending: Pending,
}

impl Updates {
	pub(super) fn new(boot: &Boot) -> Self {
		Self { answers: Inbox::default(), pending: boot.update.clone() }
	}

	/// Looks for a newer release; `quiet` (the launch's check) says nothing when there is none or the check fails.
	pub(super) fn check(&self, ctx: &egui::Context, quiet: bool) {
		if update::AVAILABLE {
			self.answers.spawn(
				ctx,
				"update-check",
				move |_| Some(Answer::Checked { result: update::check(), quiet }),
				move |e| Some(Answer::Checked { result: Err(e), quiet }),
			);
		}
	}

	/// Downloads and verifies `update` with its progress in the status bar.
	pub(super) fn download(&self, ctx: &egui::Context, s: &Strings, update: Update) {
		let version = update.version.clone();
		tracing::info!("downloading update {version}");
		let download = downloads::start(s.format("StatusDownloadingUpdateFormat", &[&version]));
		self.answers.spawn(
			ctx,
			"update-download",
			move |_| Some(Answer::Downloaded(update::download(update, |bytes, total| download.chunk(bytes, total)))),
			|e| Some(Answer::Downloaded(Err(e))),
		);
	}

	/// Marks the downloaded package for installing at this exit.
	pub(super) fn restart(&self) {
		if !self.pending.confirm_restart() {
			tracing::warn!("a restart without a downloaded update");
		}
	}

	/// The modals of the answers that arrived.
	pub(super) fn take(&self, s: &Strings, modals: &mut Modals) {
		for answer in self.answers.try_iter() {
			let current = update::current();
			match answer {
				Answer::Checked { result: Ok(Some(u)), .. } => {
					tracing::info!("update {} available (this is {current})", u.version);
					modals.show(if update::installable() {
						let text = s.format("UpdateAvailableFormat", &[&u.version, &current]);
						Modal::Confirm { text, yes: Then::Download(Box::new(u)), destroys: false }
					} else {
						let mut text =
							s.format("UpdateDownloadFormat", &[&u.version, &current, &update::release_page()]);
						if cfg!(target_os = "macos") {
							text = format!("{text}\n{}", s.get("UpdateMoveHint"));
						}
						Modal::Info(text)
					});
				}
				Answer::Checked { result: Ok(None), quiet } => {
					tracing::info!("no update: {current} is the latest release");
					if !quiet {
						modals.show(Modal::Info(s.get("UpdateCurrent").to_owned()));
					}
				}
				Answer::Checked { result: Err(e), quiet: true } => tracing::warn!("the update check failed: {e}"),
				Answer::Checked { result: Err(e), quiet: false } | Answer::Downloaded(Err(e)) => {
					tracing::error!("the update failed: {e}");
					modals.show(Modal::Error(format!("{}\n{e}", s.get("UpdateFailed"))));
				}
				Answer::Downloaded(Ok((u, bytes))) => {
					let version = u.version.clone();
					tracing::info!("update {version} downloaded and verified");
					self.pending.ready(u, bytes);
					let text = s.format("UpdateReadyFormat", &[&version]);
					modals.show(Modal::Confirm {
						text,
						yes: Then::Send(Event::Guarded(Guarded::Restart)),
						destroys: false,
					});
				}
			}
		}
	}
}
