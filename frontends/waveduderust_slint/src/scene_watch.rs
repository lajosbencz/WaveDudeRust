//! The scene image file watched while its project is open (docs/ui/60, Files and drop): polled, a change taken once
//! the file holds still for 400 ms, a read retried while the writer holds it.

use std::collections::HashSet;
use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use waveduderust::model::Scene;

const SETTLE: Duration = Duration::from_millis(400);

/// Image file bytes and their decoded size.
pub(crate) type Image = (Arc<[u8]>, (u32, u32));

/// What the file asks of the project.
#[derive(Debug, PartialEq)]
pub(crate) enum Event {
	/// Bytes that differ from the embedded image: the question stands.
	Ask,
	/// The question no longer stands.
	Withdraw,
	/// Bytes for a project that embeds no image: taken without asking.
	Take(Arc<[u8]>, (u32, u32)),
}

#[derive(Default)]
pub(crate) struct SceneWatch {
	path: PathBuf,
	/// The file's modification time (hashed) and length when last seen.
	seen: Option<(u64, u64)>,
	/// When the file last changed, until it is read.
	changed: Option<Instant>,
	/// Hashes of bytes kept embedded: not asked about again.
	kept: HashSet<u64>,
	hasher: RandomState,
	/// The bytes asked about, and their size.
	asking: Option<Image>,
	/// The path warned of as not found.
	missing: bool,
	polled: Option<Instant>,
}

impl SceneWatch {
	fn hash(&self, bytes: &[u8]) -> u64 {
		self.hasher.hash_one(bytes)
	}

	/// Watches `scene`'s image file from the project folder `folder`; a new path starts over and is read at once.
	pub(crate) fn tick(&mut self, scene: &Scene, folder: Option<&Path>) -> Option<Event> {
		if self.polled.is_some_and(|at| at.elapsed() < Duration::from_millis(100)) {
			return None;
		}
		self.polled = Some(Instant::now());
		let path = waveduderust::io::resolve_image_path(folder, &scene.image_path);
		if path != self.path {
			let withdraw = self.asking.is_some();
			*self = Self { path, hasher: std::mem::take(&mut self.hasher), ..Self::default() };
			self.changed = Some(Instant::now().checked_sub(SETTLE).unwrap_or_else(Instant::now));
			if withdraw {
				return Some(Event::Withdraw);
			}
		}
		if self.path.as_os_str().is_empty() {
			return None;
		}
		let stat = std::fs::metadata(&self.path).ok().map(|m| (self.hasher.hash_one(m.modified().ok()), m.len()));
		if stat != self.seen {
			let first = self.seen.is_none() && self.changed.is_some();
			self.seen = stat;
			if !first {
				self.changed = Some(Instant::now());
			}
			return None;
		}
		if self.changed.is_none_or(|at| at.elapsed() < SETTLE) {
			return None;
		}
		let Some(bytes) = self.read() else {
			// a missing file changes nothing: a question about it no longer stands
			return if self.missing { self.withdraw() } else { None };
		};
		self.changed = None;
		self.missing = false;
		self.judge(scene, bytes)
	}

	/// The file's bytes; `None` while unreadable (retried), or missing (warned once, then left).
	fn read(&mut self) -> Option<Vec<u8>> {
		match std::fs::read(&self.path) {
			Ok(bytes) => Some(bytes),
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
				if !self.missing {
					tracing::warn!("scene image {} (not found)", self.path.display());
				}
				(self.missing, self.changed) = (true, None);
				None
			}
			Err(_) => None,
		}
	}

	fn judge(&mut self, scene: &Scene, bytes: Vec<u8>) -> Option<Event> {
		let size = match waveduderust::io::image_size(&bytes) {
			Ok(size) => size,
			Err(e) => {
				tracing::warn!("scene image {}: {e}", self.path.display());
				return self.withdraw();
			}
		};
		let Some(embedded) = &scene.image_data else { return Some(Event::Take(bytes.into(), size)) };
		if **embedded == *bytes || self.kept.contains(&self.hash(&bytes)) {
			return self.withdraw();
		}
		self.asking = Some((bytes.into(), size));
		Some(Event::Ask)
	}

	/// A question stands.
	pub(crate) fn asking(&self) -> bool {
		self.asking.is_some()
	}

	fn withdraw(&mut self) -> Option<Event> {
		self.asking.take().map(|_| Event::Withdraw)
	}

	/// The answer to the question: the bytes to embed (Load), or `None` with them kept from asking again.
	pub(crate) fn answer(&mut self, load: bool) -> Option<Image> {
		let (bytes, size) = self.asking.take()?;
		if load {
			return Some((bytes, size));
		}
		let hash = self.hash(&bytes);
		self.kept.insert(hash);
		None
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn png(shade: u8) -> Vec<u8> {
		let mut out = std::io::Cursor::new(Vec::new());
		image::RgbImage::from_pixel(2, 1, image::Rgb([shade, 0, 0]))
			.write_to(&mut out, image::ImageFormat::Png)
			.unwrap();
		out.into_inner()
	}

	#[test]
	fn asks_about_changed_bytes_and_keeps_an_answer() {
		let mut w = SceneWatch::default();
		let scene = Scene::default().with_image("a.png".into()).with_image_data(Some(png(1).into()));
		assert_eq!(w.judge(&scene, png(1)), None);
		assert_eq!(w.judge(&scene, png(2)), Some(Event::Ask));
		assert_eq!(w.answer(false), None);
		assert_eq!(w.judge(&scene, png(2)), None);
		assert_eq!(w.judge(&scene, png(3)), Some(Event::Ask));
		assert_eq!(w.judge(&scene, png(1)), Some(Event::Withdraw));
		assert_eq!(w.judge(&scene, b"junk".to_vec()), None);
		let bare = Scene::default();
		assert!(matches!(w.judge(&bare, png(4)), Some(Event::Take(_, (2, 1)))));
	}
}
