//! The app's side of Cisco's `OpenH264` (decision 0005): the user's choice is the `UseOpenH264` setting, the
//! binary a file in the app's codec folder (`AppDirs::codec`), both shared with the CLI, downloaded from Cisco here.
//! Threads: the UI thread; an export's worker downloads.

use std::io::Read;
use std::path::{Path, PathBuf};

use waveduderust::unstable::export::{ExportError, Result, h264};

/// MP4 export may run: compiled in, or enabled by the user.
pub fn enabled(choice: Option<bool>) -> bool {
	h264::BUILT_IN || choice == Some(true)
}

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

/// The library an MP4 export loads: `None` when `OpenH264` is compiled in, else Cisco's binary in `dir`,
/// downloaded now when it is missing (`on_chunk` as in [`download`]). Fails with `ExportError::H264Missing`
/// unless `enabled`, the user's choice.
pub fn library(dir: &Path, enabled: bool, on_chunk: impl FnMut(usize, Option<u64>)) -> Result<Option<PathBuf>> {
	if h264::BUILT_IN {
		return Ok(None);
	}
	if !enabled {
		return Err(ExportError::H264Missing);
	}
	download(dir, on_chunk).map(Some)
}

/// The downloaded binary in `dir`, if there.
pub fn installed(dir: &Path) -> Option<PathBuf> {
	file_name().map(|n| dir.join(n)).filter(|p| p.is_file())
}

/// Downloads Cisco's binary into `dir` (made when missing) unless it is there, and returns its path. The file
/// is checked against the known SHA-256 of Cisco's builds before it takes its name. `on_chunk(bytes, total)`
/// follows the download, `total` the compressed size when the server names it.
pub fn download(dir: &Path, mut on_chunk: impl FnMut(usize, Option<u64>)) -> Result<PathBuf> {
	if let Some(path) = installed(dir) {
		return Ok(path);
	}
	let name =
		file_name().ok_or_else(|| ExportError::OpenH264("Cisco publishes no OpenH264 for this platform".into()))?;
	let url = format!("{URL}{name}.bz2");
	tracing::info!(%url, "downloading OpenH264");
	let failed = |e: &dyn std::fmt::Display| ExportError::OpenH264(format!("downloading {url}: {e}"));
	let response = reqwest::blocking::get(&url).and_then(reqwest::blocking::Response::error_for_status);
	let response = response.map_err(|e| failed(&e))?;
	let total = response.content_length();
	let mut reader = response.take(MAX_DOWNLOAD);
	let mut packed = Vec::new();
	let mut chunk = [0; 64 << 10];
	loop {
		let n = reader.read(&mut chunk).map_err(|e| failed(&e))?;
		if n == 0 {
			break;
		}
		packed.extend_from_slice(&chunk[..n]);
		on_chunk(n, total);
	}
	let mut bytes = Vec::new();
	bzip2::read::BzDecoder::new(packed.as_slice()).read_to_end(&mut bytes).map_err(|e| failed(&e))?;
	std::fs::create_dir_all(dir)?;
	let path = dir.join(name);
	install(&bytes, &dir.join(format!("{name}.part")), &path)?;
	tracing::info!(path = %path.display(), "OpenH264 installed");
	Ok(path)
}

/// Removes its file on drop; a file already gone is fine.
struct Part<'a>(&'a Path);

impl Drop for Part<'_> {
	fn drop(&mut self) {
		if let Err(e) = std::fs::remove_file(self.0)
			&& e.kind() != std::io::ErrorKind::NotFound
		{
			tracing::warn!(path = %self.0.display(), "removing the partial OpenH264 download failed: {e}");
		}
	}
}

/// Writes `bytes` to `part`, checks them as a binary and renames `part` to `path`; `part` is gone on every exit.
fn install(bytes: &[u8], part: &Path, path: &Path) -> Result<()> {
	let _part = Part(part);
	std::fs::write(part, bytes)?;
	h264::check_library(part)?;
	std::fs::rename(part, path)?;
	Ok(())
}

/// Deletes the downloaded binary from `dir`, if there.
pub fn remove(dir: &Path) -> Result<()> {
	if let Some(path) = installed(dir) {
		std::fs::remove_file(path)?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_platform_names_a_binary_of_the_accepted_release() {
		if let Some(name) = file_name() {
			assert!(name.contains(h264::VERSION));
		}
	}

	#[test]
	fn a_refused_binary_leaves_no_part_file() {
		let dir = tempfile::tempdir().unwrap();
		let (part, path) = (dir.path().join("lib.part"), dir.path().join("lib"));
		assert!(matches!(install(b"not a library", &part, &path), Err(ExportError::OpenH264(_))));
		assert!(!part.exists() && !path.exists());
	}

	#[test]
	fn a_failed_write_is_an_error_and_leaves_nothing() {
		let dir = tempfile::tempdir().unwrap();
		let part = dir.path().join("missing").join("lib.part");
		assert!(matches!(install(b"x", &part, &dir.path().join("lib")), Err(ExportError::Io(_))));
		assert!(!part.exists());
	}
}

#[cfg(test)]
mod ignored_network {
	use super::*;

	#[test]
	#[ignore = "network: downloads Cisco's OpenH264; cargo test -p waveduderust_platform -- --ignored ignored_network::"]
	fn ciscos_binary_downloads_checks_and_deletes() {
		if h264::BUILT_IN {
			return;
		}
		let dir = tempfile::tempdir().unwrap();
		let library = download(dir.path(), |_, _| {}).unwrap();
		assert_eq!(installed(dir.path()), Some(library.clone()));
		h264::check_library(&library).unwrap();
		remove(dir.path()).unwrap();
		assert_eq!(installed(dir.path()), None);
	}
}
