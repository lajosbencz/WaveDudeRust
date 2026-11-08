//! Cisco's OpenH264 for MP4 export (decision 0005): the user's choice is the app's `UseOpenH264` setting, in the
//! app's settings file; the binary a file in the codec's folder. Every action prints Cisco's notice, as its
//! licence asks where the user controls the codec.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::ValueEnum;
use wavedude_app_state::settings::{AppDirs, Settings};
use wavedude_export::h264;

/// What to do with the codec.
#[derive(Clone, Copy, ValueEnum)]
pub enum Action {
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

fn dirs() -> Result<AppDirs> {
	AppDirs::platform().context("no user folders on this platform")
}

fn settings_file() -> Result<PathBuf> {
	Ok(dirs()?.settings_file())
}

/// The library an MP4 export loads, per the user's choice in the app's settings file.
pub fn library() -> Result<Option<PathBuf>> {
	let enabled = Settings::read(&settings_file()?)?.use_openh264 == Some(true);
	h264::library(&dirs()?.codec(), enabled, |_, _| {}).map_err(|e| match e {
		wavedude_export::ExportError::H264Missing => anyhow::anyhow!("{e}: run `wavedude-cli openh264 enable`"),
		e => e.into(),
	})
}

fn choose(enabled: bool) -> Result<()> {
	let path = settings_file()?;
	let mut settings = Settings::read(&path)?;
	settings.use_openh264 = Some(enabled);
	settings.save(&path)?;
	Ok(())
}

pub fn run(action: Action) -> Result<()> {
	println!("{}", h264::NOTICE);
	if h264::BUILT_IN {
		println!("compiled into this build; nothing to download");
		return Ok(());
	}
	let dir = dirs()?.codec();
	match action {
		Action::Status => {
			let choice = Settings::read(&settings_file()?)?.use_openh264;
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
			choose(true)?;
			println!("enabled: {}", h264::download(&dir, |_, _| {})?.display());
		}
		Action::Disable => {
			choose(false)?;
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
