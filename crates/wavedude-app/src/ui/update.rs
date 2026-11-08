//! Updates from the project's GitHub releases (decision 0011): the release build names the feed and the public
//! key its packages are signed with; a build without them has no updater. A newer release is installed on the
//! user's word where the running program is a package the updater can replace (the Windows installer, the macOS
//! app, a Linux AppImage); a plain Linux binary gets the release's address. The package is downloaded and verified
//! while the program runs, then installed by `main` after a restart that goes through the unsaved-edits question
//! like any exit ([`Pending`]). Threads: the UI thread; the check and the download run on threads of their own.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use cargo_packager_updater::{Config, Update, check_update};
use freya::prelude::*;
use wavedude_app_state::{Event as AppEvent, Guarded};

use super::app::{Cx, TICK};
use super::dialogs::Modal;
use super::downloads;

/// The release feed (`latest.json` of the latest GitHub release) and the minisign public key, set at build time;
/// an empty value counts as unset, as a workflow passes an unset variable.
const ENDPOINT: Option<&str> = set(option_env!("WAVEDUDE_UPDATE_ENDPOINT"));
const PUBKEY: Option<&str> = set(option_env!("WAVEDUDE_UPDATE_PUBKEY"));

const fn set(value: Option<&'static str>) -> Option<&'static str> {
	match value {
		Some(v) if !v.is_empty() => Some(v),
		_ => None,
	}
}

/// This build can look for updates.
pub const AVAILABLE: bool = ENDPOINT.is_some() && PUBKEY.is_some();

/// The updater can replace the running program: everywhere but a plain Linux binary (an AppImage sets `APPIMAGE`).
fn installable() -> bool {
	!cfg!(target_os = "linux") || std::env::var_os("APPIMAGE").is_some()
}

/// The release page beside the feed: `.../releases/latest/download/latest.json` gives `.../releases/latest`.
fn release_page() -> &'static str {
	ENDPOINT.and_then(|e| e.split("/download/").next()).unwrap_or_default()
}

fn config() -> Result<Config, String> {
	let (Some(endpoint), Some(pubkey)) = (ENDPOINT, PUBKEY) else {
		return Err("this build has no updater".into());
	};
	Ok(Config {
		endpoints: vec![endpoint.parse().map_err(|e| format!("the update feed '{endpoint}': {e}"))?],
		pubkey: pubkey.into(),
		..Default::default()
	})
}

fn current() -> semver::Version {
	env!("CARGO_PKG_VERSION").parse().expect("the package version is semver")
}

/// Looks for a newer release; `quiet` (the launch's check) says nothing when there is none or the check fails.
pub fn check(cx: &Cx, quiet: bool) {
	if !AVAILABLE {
		return;
	}
	let (tx, rx) = mpsc::channel();
	let spawned = std::thread::Builder::new().name("update-check".into()).spawn(move || {
		let result = config().and_then(|c| check_update(current(), c).map_err(|e| e.to_string()));
		let _ = tx.send(result);
	});
	if let Err(e) = spawned {
		tracing::error!("the update check could not start: {e}");
		return;
	}
	let cx = cx.clone();
	spawn_in_window(async move {
		let Some(result) = receive(rx).await else { return };
		let s = cx.strings();
		match result {
			Ok(Some(update)) => {
				tracing::info!("update {} available (this is {})", update.version, current());
				if installable() {
					let text = s.format("UpdateAvailableFormat", &[&update.version, &current()]);
					let update = Rc::new(update);
					let install_cx = cx.clone();
					cx.ui.show_modal(Modal::Confirm { text, yes: Rc::new(move || install(&install_cx, &update)) });
				} else {
					let text = s.format("UpdateDownloadFormat", &[&update.version, &current(), &release_page()]);
					cx.ui.show_modal(Modal::Info(text));
				}
			}
			Ok(None) => {
				tracing::info!("no update: {} is the latest release", current());
				if !quiet {
					cx.ui.show_modal(Modal::Info(s.get("UpdateCurrent").to_owned()));
				}
			}
			Err(e) if quiet => tracing::warn!("the update check failed: {e}"),
			Err(e) => {
				tracing::error!("the update check failed: {e}");
				cx.ui.show_modal(Modal::Error(format!("{}\n{e}", s.get("UpdateFailed"))));
			}
		}
	});
}

/// A verified package waiting for the restart that installs it, shared by the window and `main`.
#[derive(Clone, Default)]
pub struct Pending(Rc<RefCell<Option<Ready>>>);

struct Ready {
	update: Update,
	bytes: Vec<u8>,
	/// The restart was confirmed: the exit is the restart's.
	restart: bool,
}

impl Pending {
	/// Marks the package for installing at this exit; false when there is none.
	pub fn confirm_restart(&self) -> bool {
		self.0.borrow_mut().as_mut().map(|r| r.restart = true).is_some()
	}

	/// After the window closed: installs a package whose restart was confirmed and starts the new version. The
	/// Windows installer ends this process itself and starts the program when done.
	pub fn install(&self) {
		let Some(ready) = self.0.borrow_mut().take().filter(|r| r.restart) else { return };
		let version = ready.update.version.clone();
		tracing::info!("installing update {version}");
		if let Err(e) = ready.update.install(ready.bytes) {
			tracing::error!("the update {version} could not be installed: {e}");
			return;
		}
		tracing::info!("update {version} installed");
		let program = std::env::var_os("APPIMAGE").map(Into::into).map_or_else(std::env::current_exe, Ok);
		if let Err(e) = program.and_then(|p| std::process::Command::new(p).spawn()) {
			tracing::error!("the new version could not be started: {e}");
		}
	}
}

/// Downloads and verifies `update` with its progress in the status bar, then asks to restart into it.
fn install(cx: &Cx, update: &Update) {
	let update = update.clone();
	let version = update.version.clone();
	let download = downloads::start(cx, cx.strings().format("StatusDownloadingUpdateFormat", &[&version]));
	let (tx, rx) = mpsc::channel();
	let spawned = std::thread::Builder::new().name("update-download".into()).spawn(move || {
		let bytes = update.download_extended(|bytes, total| download.chunk(bytes, total), || {});
		let _ = tx.send(bytes.map(|b| (update, b)).map_err(|e| e.to_string()));
	});
	if let Err(e) = spawned {
		tracing::error!("the update download could not start: {e}");
		return;
	}
	tracing::info!("downloading update {version}");
	let cx = cx.clone();
	// Not `spawn`: that task would end with the confirm dialog, which closes before the download does.
	spawn_in_window(async move {
		let Some(result) = receive(rx).await else { return };
		let s = cx.strings();
		match result {
			Ok((update, bytes)) => {
				tracing::info!("update {version} downloaded and verified");
				*cx.info.update.0.borrow_mut() = Some(Ready { update, bytes, restart: false });
				let restart_cx = cx.clone();
				let yes = Rc::new(move || restart_cx.send(AppEvent::Guarded(Guarded::Restart)));
				cx.ui.show_modal(Modal::Confirm { text: s.format("UpdateReadyFormat", &[&version]), yes });
			}
			Err(e) => {
				tracing::error!("the update failed: {e}");
				cx.ui.show_modal(Modal::Error(format!("{}\n{e}", s.get("UpdateFailed"))));
			}
		}
	});
}

/// The worker's one message, polled every tick; `None` when the worker ended without one.
async fn receive<T>(rx: Receiver<T>) -> Option<T> {
	loop {
		match rx.try_recv() {
			Ok(v) => return Some(v),
			Err(TryRecvError::Empty) => timer(TICK).await,
			Err(TryRecvError::Disconnected) => return None,
		}
	}
}
