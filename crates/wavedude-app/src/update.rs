//! Updates from the project's GitHub releases (decision 0011): the release build names the feed and the public
//! key its packages are signed with; a build without them has no updater. A newer release is installed on the
//! user's word where the running program is a package the updater can replace (the Windows installer, the macOS
//! app, a Linux `AppImage`); a plain Linux binary gets the release's address. The package is downloaded and verified
//! while the program runs, then installed by `main` after a restart that goes through the unsaved-edits question
//! like any exit ([`Pending`]). Threads: the UI thread; the check and the download run on threads of their own.

use anyhow::Context;
use std::cell::RefCell;
use std::rc::Rc;

use cargo_packager_updater::{Config, Update, check_update};
use wavedude_app_state::platform::LogGuard;

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

/// The updater can replace the running program: everywhere but a plain Linux binary (an `AppImage` sets `APPIMAGE`)
/// and a macOS app that cannot swap itself out, see [`movable`].
pub fn installable() -> bool {
	if cfg!(target_os = "macos") {
		return movable();
	}
	!cfg!(target_os = "linux") || std::env::var_os("APPIMAGE").is_some()
}

/// The macOS updater renames the running app into a temp dir backup, which fails across volumes: a translocated app,
/// one on a mounted image or on another disk than the temp dir.
#[cfg(target_os = "macos")]
fn movable() -> bool {
	use std::os::unix::fs::MetadataExt;
	let volume = |p: &std::path::Path| p.metadata().map(|m| m.dev()).ok();
	std::env::current_exe()
		.ok()
		.and_then(|exe| volume(&exe))
		.is_some_and(|dev| Some(dev) == volume(&std::env::temp_dir()))
}

#[cfg(not(target_os = "macos"))]
fn movable() -> bool {
	true
}

/// The release page beside the feed: `.../releases/latest/download/latest.json` gives `.../releases/latest`.
pub fn release_page() -> &'static str {
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

/// This build's version.
pub fn current() -> semver::Version {
	env!("CARGO_PKG_VERSION").parse().expect("the package version is semver")
}

/// Asks the release feed for a newer release; blocks, so it runs on a thread of its own.
pub fn check() -> Result<Option<Update>, String> {
	config().and_then(|c| check_update(current(), c).map_err(|e| e.to_string()))
}

/// Downloads and verifies `update`, reporting `(bytes, total)` as they arrive; blocks like [`check`].
pub fn download(update: Update, on_chunk: impl Fn(usize, Option<u64>)) -> Result<(Update, Vec<u8>), String> {
	let bytes = update.download_extended(on_chunk, || {}).map_err(|e| e.to_string())?;
	Ok((update, bytes))
}

/// A verified package waiting for the restart that installs it, shared by the views and `main`.
#[derive(Clone, Default)]
pub struct Pending(Rc<RefCell<Option<Ready>>>);

struct Ready {
	update: Update,
	bytes: Vec<u8>,
	/// The restart was confirmed: the exit is the restart's.
	restart: bool,
}

impl Pending {
	/// Holds a downloaded and verified package until the restart is confirmed.
	pub fn ready(&self, update: Update, bytes: Vec<u8>) {
		*self.0.borrow_mut() = Some(Ready { update, bytes, restart: false });
	}

	/// Marks the package for installing at this exit; false when there is none.
	pub fn confirm_restart(&self) -> bool {
		self.0.borrow_mut().as_mut().map(|r| r.restart = true).is_some()
	}

	/// After the window closed: installs a package whose restart was confirmed and starts the new version, or the old
	/// one again when the install fails. The Windows installer ends this process itself and starts the program when
	/// done; `log` is dropped before it, as the exit skips destructors and would lose the writer's last lines.
	pub fn install(&self, log: LogGuard) -> anyhow::Result<()> {
		let Some(ready) = self.0.borrow_mut().take().filter(|r| r.restart) else { return Ok(()) };
		let version = ready.update.version.clone();
		tracing::info!("installing update {version}");
		if cfg!(windows) {
			drop(log);
		}
		let installed = ready.update.install(ready.bytes);
		let program = std::env::var_os("APPIMAGE").map(Into::into).map_or_else(std::env::current_exe, Ok);
		let started = program.and_then(|p| std::process::Command::new(p).spawn());
		if let Err(e) = installed {
			tracing::error!("the update {version} could not be installed: {e}");
			if let Err(e) = started {
				tracing::error!("the program could not be started again: {e}");
			}
			anyhow::bail!("the update {version} could not be installed: {e}");
		}
		tracing::info!("update {version} installed");
		started.with_context(|| format!("the update {version} is installed, but it could not be started"))?;
		Ok(())
	}
}
