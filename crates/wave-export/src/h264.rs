//! The H.264 encoder behind MP4 export (decision 0005): Cisco's prebuilt OpenH264, which Cisco licenses
//! under the AVC patent pool only when it is downloaded from Cisco to the user's device, the user can
//! enable and disable it, [`NOTICE`] is shown where they do, and [`LICENSE`] where licensing information
//! is. [`download`] fetches it into a folder, [`installed`] finds it there, and the MP4 sink loads it at
//! run time. A build with the `openh264-source` feature compiles OpenH264 in instead ([`BUILT_IN`]).

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::{ExportError, Result};

/// The OpenH264 release whose binaries the `openh264` crate accepts (it checks their SHA-256).
pub const VERSION: &str = "2.6.0";
/// The text Cisco requires where the user controls the codec's use.
pub const NOTICE: &str = "OpenH264 Video Codec provided by Cisco Systems, Inc.";
/// Cisco's licence of its binary, to be shown with the program's licensing information.
pub const LICENSE: &str = include_str!("../assets/OpenH264-BINARY_LICENSE.txt");
/// OpenH264 is compiled into this build: no download, no Cisco terms.
pub const BUILT_IN: bool = cfg!(feature = "openh264-source");

const URL: &str = "https://ciscobinary.openh264.org/";
/// Cisco's compressed binaries are about 1 MB.
const MAX_DOWNLOAD: u64 = 16 << 20;

/// Cisco's binary for this platform, `None` where Cisco publishes none.
pub fn file_name() -> Option<&'static str> {
	Some(match (std::env::consts::OS, std::env::consts::ARCH) {
		("linux", "x86_64") => "libopenh264-2.6.0-linux64.8.so",
		("linux", "aarch64") => "libopenh264-2.6.0-linux-arm64.8.so",
		("macos", "x86_64") => "libopenh264-2.6.0-mac-x64.dylib",
		("macos", "aarch64") => "libopenh264-2.6.0-mac-arm64.dylib",
		("windows", "x86_64") => "openh264-2.6.0-win64.dll",
		("windows", "aarch64") => "openh264-2.6.0-win-arm64.dll",
		_ => return None,
	})
}

/// The user's folder for the codec, `None` when the platform names no home.
pub fn default_dir() -> Option<PathBuf> {
	directories::ProjectDirs::from("", "", "WaveDude").map(|d| d.data_dir().join("openh264"))
}

/// The downloaded binary in `dir`, if there.
pub fn installed(dir: &Path) -> Option<PathBuf> {
	file_name().map(|n| dir.join(n)).filter(|p| p.is_file())
}

/// Downloads Cisco's binary into `dir` (made when missing) unless it is there, and returns its path. The file
/// is checked against the known SHA-256 of Cisco's builds before it takes its name.
pub fn download(dir: &Path) -> Result<PathBuf> {
	if let Some(path) = installed(dir) {
		return Ok(path);
	}
	let name = file_name().ok_or_else(|| ExportError::Codec("Cisco publishes no OpenH264 for this platform".into()))?;
	let url = format!("{URL}{name}.bz2");
	tracing::info!(%url, "downloading OpenH264");
	let failed = |e: &dyn std::fmt::Display| ExportError::Codec(format!("downloading {url}: {e}"));
	let mut response = ureq::get(&url).call().map_err(|e| failed(&e))?;
	let packed = response.body_mut().with_config().limit(MAX_DOWNLOAD).read_to_vec().map_err(|e| failed(&e))?;
	let mut bytes = Vec::new();
	bzip2::read::BzDecoder::new(packed.as_slice()).read_to_end(&mut bytes).map_err(|e| failed(&e))?;
	std::fs::create_dir_all(dir)?;
	let part = dir.join(format!("{name}.part"));
	std::fs::write(&part, &bytes)?;
	let checked = openh264::OpenH264API::from_blob_path(&part);
	if let Err(e) = checked {
		std::fs::remove_file(&part)?;
		return Err(ExportError::Codec(format!("the downloaded OpenH264 was refused: {e}")));
	}
	drop(checked);
	let path = dir.join(name);
	std::fs::rename(&part, &path)?;
	tracing::info!(path = %path.display(), "OpenH264 installed");
	Ok(path)
}

/// Removes the downloaded binary from `dir`, if there.
pub fn remove(dir: &Path) -> Result<()> {
	if let Some(path) = installed(dir) {
		std::fs::remove_file(path)?;
	}
	Ok(())
}

/// The encoder library: the compiled-in one, else Cisco's binary at `library`.
pub(crate) fn api(library: Option<&Path>) -> Result<openh264::OpenH264API> {
	#[cfg(feature = "openh264-source")]
	{
		let _ = library;
		Ok(openh264::OpenH264API::from_source())
	}
	#[cfg(not(feature = "openh264-source"))]
	{
		let path = library.ok_or(ExportError::H264Missing)?;
		openh264::OpenH264API::from_blob_path(path).map_err(|e| ExportError::Codec(e.to_string()))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_platform_names_a_binary_of_the_accepted_release() {
		if let Some(name) = file_name() {
			assert!(name.contains(VERSION));
		}
		assert!(LICENSE.contains(NOTICE));
	}
}
