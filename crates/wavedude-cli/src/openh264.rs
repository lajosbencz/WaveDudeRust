//! Cisco's `OpenH264` for MP4 export (decision 0005): the user's choice is the app's `UseOpenH264` setting, in the
//! app's settings file; the binary a file in the codec's folder. Every action prints Cisco's notice, as its
//! licence asks where the user controls the codec.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::ValueEnum;
use wavedude_app_state::platform::{Store, touch_first};
use wavedude_app_state::settings::{AppDirs, Settings};
use wavedude_export::h264;

/// What to do with the codec.
#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum Action {
	/// Say whether MP4 export is enabled and whether the codec is downloaded.
	Status,
	/// Enable MP4 export and download Cisco's binary.
	Enable,
	/// Disable MP4 export; the downloaded binary stays.
	Disable,
	/// Delete the downloaded binary; an enabled codec is downloaded again at the next MP4 export.
	Delete,
	/// Print Cisco's licence of its binary.
	License,
}

fn known(dirs: Option<&AppDirs>) -> Result<&AppDirs> {
	dirs.context("no user folders on this platform: pass --config-dir")
}

fn store(dirs: Option<&AppDirs>) -> Result<Store> {
	Ok(known(dirs)?.store())
}

/// The library an MP4 export loads, per the user's choice in the settings file of the app's folders `dirs`.
pub(crate) fn library(dirs: Option<&AppDirs>) -> Result<Option<PathBuf>> {
	let enabled = Settings::read(&store(dirs)?, touch_first())?.use_openh264 == Some(true);
	h264::library(&known(dirs)?.codec(), enabled, |_, _| {}).map_err(|e| match e {
		wavedude_export::ExportError::H264Missing => anyhow::anyhow!("{e}: run `wavedude-cli openh264 enable`"),
		e => e.into(),
	})
}

fn choose(enabled: bool, dirs: Option<&AppDirs>) -> Result<()> {
	let store = store(dirs)?;
	let mut settings = Settings::read(&store, touch_first())?;
	settings.use_openh264 = Some(enabled);
	settings.save(&store)?;
	Ok(())
}

pub(crate) fn run(action: Action, dirs: Option<&AppDirs>) -> Result<()> {
	println!("{}", h264::NOTICE);
	if h264::BUILT_IN {
		println!("compiled into this build; nothing to download");
		return Ok(());
	}
	let dir = known(dirs)?.codec();
	match action {
		Action::Status => {
			let choice = Settings::read(&store(dirs)?, touch_first())?.use_openh264;
			println!(
				"{}",
				match choice {
					Some(true) => "enabled",
					Some(false) => "disabled",
					None => "not chosen yet (disabled)",
				}
			);
			match h264::installed(&dir) {
				Some(path) => println!("downloaded: {}", path.display()),
				None => println!("not downloaded"),
			}
		}
		Action::Enable => {
			choose(true, dirs)?;
			println!("enabled: {}", h264::download(&dir, |_, _| {})?.display());
		}
		Action::Disable => {
			choose(false, dirs)?;
			println!("disabled");
		}
		Action::Delete => {
			h264::delete(&dir)?;
			println!("deleted");
		}
		Action::License => print!("{}", h264::LICENSE),
	}
	Ok(())
}
