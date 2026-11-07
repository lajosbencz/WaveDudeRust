//! Updates from the project's GitHub releases (decision 0011): the release build names the feed and the public
//! key its packages are signed with; a build without them has no updater. A newer release is installed on the
//! user's word where the running program is a package the updater can replace (the Windows installer, the macOS
//! app, a Linux AppImage); a plain Linux binary gets the release's address. Threads: the UI thread; the check and
//! the install run on threads of their own.

use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use cargo_packager_updater::{Config, Update, check_update};
use freya::prelude::*;

use super::app::{Cx, TICK};
use super::dialogs::Modal;

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
	spawn(async move {
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

/// Downloads, verifies and installs `update`; the Windows installer ends this process itself.
fn install(cx: &Cx, update: &Update) {
	let update = update.clone();
	let version = update.version.clone();
	let (tx, rx) = mpsc::channel();
	let spawned = std::thread::Builder::new().name("update-install".into()).spawn(move || {
		let _ = tx.send(update.download_and_install().map_err(|e| e.to_string()));
	});
	if let Err(e) = spawned {
		tracing::error!("the update could not start: {e}");
		return;
	}
	tracing::info!("installing update {version}");
	let cx = cx.clone();
	spawn(async move {
		let Some(result) = receive(rx).await else { return };
		let s = cx.strings();
		match result {
			Ok(()) => {
				tracing::info!("update {version} installed");
				cx.ui.show_modal(Modal::Info(s.format("UpdateInstalledFormat", &[&version])));
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
