//! Cisco's OpenH264 for MP4 export (decision 0005): downloaded to the user's folder on `enable`, removed on
//! `disable`. Every action prints Cisco's notice, as its licence asks where the user controls the codec.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::ValueEnum;
use wave_export::h264;

/// What to do with the codec.
#[derive(Clone, Copy, ValueEnum)]
pub enum Action {
	/// Say whether MP4 export can run and where the codec lives.
	Status,
	/// Download Cisco's binary and check it.
	Enable,
	/// Remove the downloaded binary.
	Disable,
	/// Print Cisco's licence of its binary.
	License,
}

fn dir() -> Result<PathBuf> {
	h264::default_dir().context("no user data folder on this platform")
}

/// The library MP4 export loads: `None` for a build with OpenH264 compiled in, else the downloaded binary.
pub fn library() -> Result<Option<PathBuf>> {
	if h264::BUILT_IN {
		return Ok(None);
	}
	h264::installed(&dir()?).map(Some).context("MP4 export needs Cisco's OpenH264: run `wave-cli openh264 enable`")
}

pub fn run(action: Action) -> Result<()> {
	println!("{}", h264::NOTICE);
	if h264::BUILT_IN {
		println!("compiled into this build; nothing to download");
		return Ok(());
	}
	let dir = dir()?;
	match action {
		Action::Status => match h264::installed(&dir) {
			Some(path) => println!("enabled: {}", path.display()),
			None => println!("disabled: `wave-cli openh264 enable` downloads OpenH264 {} from Cisco", h264::VERSION),
		},
		Action::Enable => println!("enabled: {}", h264::download(&dir)?.display()),
		Action::Disable => {
			h264::remove(&dir)?;
			println!("disabled");
		}
		Action::License => print!("{}", h264::LICENSE),
	}
	Ok(())
}
