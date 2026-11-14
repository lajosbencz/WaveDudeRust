//! The program's user settings (docs/ui/80-settings.md): one sparse JSON text in the platform's store (a file in
//! the config dir, the browser's local storage), values at their default omitted, the first start's preview
//! quality chosen once from the machine.
//!
//! Threads: plain data, owned by the UI thread's `AppState`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wavedude_core::engine::SampleFormat;
use wavedude_core::model::Word;
use wavedude_core::quality::{MachineFacts, PreviewDefaults, PreviewQuality};

use super::platform::Store;
use super::strings::Language;

/// The settings file's key in the store.
pub const FILE_NAME: &str = "settings.json";
/// The settings file's format version, written in every file.
pub const VERSION: u32 = 1;
pub const MAX_RECENT: usize = 10;
pub const MAX_EXPORT_HINTS: usize = 20;
/// UI zoom bounds from the menu and from `--zoom=`.
pub const MENU_ZOOM: (f64, f64) = (0.5, 3.0);
pub const SWITCH_ZOOM: (f64, f64) = (0.5, 8.0);
pub const DITHER_BITS: [u32; 5] = [0, 10, 12, 14, 16];

/// The user's colour scheme folder in the store.
pub const SCHEME_FOLDER: &str = "ColorSchemes";

/// The name of the app's folders in the platform's config, cache and data dirs.
pub const APP_FOLDER: &str = "WaveDudeRust";

/// The app's folders: settings and log in the config dir, the frame store's spill files in the cache dir,
/// downloaded codecs in the data dir. The CLI uses the same.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppDirs {
	pub config: PathBuf,
	pub cache: PathBuf,
	pub data: PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl AppDirs {
	/// The platform's folders for [`APP_FOLDER`]; `None` when the platform names no home.
	pub fn platform() -> Option<Self> {
		let dirs = directories::ProjectDirs::from("", "", APP_FOLDER)?;
		Some(Self {
			config: dirs.config_dir().to_owned(),
			cache: dirs.cache_dir().to_owned(),
			data: dirs.data_dir().to_owned(),
		})
	}

	/// All three in one folder: tests, and a platform without a home.
	pub fn under(dir: &Path) -> Self {
		Self { config: dir.join("config"), cache: dir.join("cache"), data: dir.join("data") }
	}

	/// Cisco's `OpenH264` binary (decision 0005).
	pub fn codec(&self) -> PathBuf {
		self.data.join("openh264")
	}

	/// The settings, the user's colour schemes, the logs and the crash report.
	pub fn store(&self) -> Store {
		Store::in_dir(self.config.clone())
	}
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowState {
	#[default]
	Normal,
	Maximized,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Word)]
#[fields(name = "AppTheme")]
pub enum Theme {
	#[default]
	System,
	Light,
	Dark,
}

/// The pictures side by side (Horizontal) or stacked (Vertical).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
	Horizontal,
	#[default]
	Vertical,
}

/// Where the last exports of a project went, per kind; restored when the project is opened.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct ExportHint {
	pub project: PathBuf,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub run: Option<PathBuf>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub still: Option<PathBuf>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub probe: Option<PathBuf>,
}

/// Every user setting. Sizes and positions in unzoomed DIP; `None` positions let the system place the window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Settings {
	/// The format version of the file it was read from; [`VERSION`] when written.
	pub version: u32,
	pub window_width: f64,
	pub window_height: f64,
	pub window_left: Option<f64>,
	pub window_top: Option<f64>,
	pub window_state: WindowState,
	/// The dock layout, opaque to this layer; `None` is the default arrangement.
	pub layout: Option<String>,
	pub theme: Theme,
	/// `""` follows the system; else `en`, `de`, `fr`, `nl`.
	pub language: String,
	/// Newest first.
	pub recent_files: Vec<PathBuf>,
	pub preview_orientation: Orientation,
	pub auto_rerun: bool,
	pub warn_when_ram_insufficient: bool,
	pub use_gpu: bool,
	/// 0 or less is Auto, else a share of the physical memory, 10..100.
	pub frame_memory_percent: i32,
	/// 1, 2 or 4.
	pub preview_resolution: u32,
	/// 1, 2 or 4.
	pub preview_frames: u32,
	/// 1 = 8-bit palette index, 2 = f16.
	pub preview_sample_bytes: u32,
	/// One of [`DITHER_BITS`]; 0 is off.
	pub preview_dither_bits: u32,
	pub ui_zoom: f64,
	pub last_export_folder: PathBuf,
	/// An index of [`wavedude_export::ExportFormat::ALL`], the run export's last format.
	pub export_filter_index: u32,
	pub part_description_expanded: bool,
	/// Newest project first.
	pub export_hints: Vec<ExportHint>,
	/// The window title names the project's full path rather than its file name (decision 0008).
	pub title_full_path: bool,
	/// MP4 export may use Cisco's `OpenH264`, downloaded at the first MP4 export (decision 0005); `None` until the
	/// user answered the launch's question. The CLI shares it.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub use_openh264: Option<bool>,
	/// Look for a new release at launch (GitHub releases); `None` until the user answered the startup page.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub check_for_updates: Option<bool>,
	/// The startup page opens at each launch.
	pub show_startup: bool,
}

impl Default for Settings {
	fn default() -> Self {
		Self {
			version: VERSION,
			window_width: 1280.0,
			window_height: 800.0,
			window_left: None,
			window_top: None,
			window_state: WindowState::Normal,
			layout: None,
			theme: Theme::System,
			language: String::new(),
			recent_files: Vec::new(),
			preview_orientation: Orientation::Vertical,
			auto_rerun: true,
			warn_when_ram_insufficient: true,
			use_gpu: true,
			frame_memory_percent: 0,
			preview_resolution: 1,
			preview_frames: 1,
			preview_sample_bytes: 1,
			preview_dither_bits: 0,
			ui_zoom: 1.0,
			last_export_folder: PathBuf::new(),
			export_filter_index: 0,
			part_description_expanded: true,
			export_hints: Vec::new(),
			title_full_path: false,
			use_openh264: None,
			check_for_updates: None,
			show_startup: true,
		}
	}
}

/// The outcome of reading the settings file.
#[derive(Debug)]
pub struct Loaded {
	pub settings: Settings,
	/// No file existed: the first start's choice was made and the file written.
	pub first_start: bool,
	/// The file existed but could not be read: logged with its text, defaults used, not a first start.
	pub unreadable: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
	#[error("cannot read the settings file '{location}': {detail}")]
	Read { location: String, detail: String },
	#[error("cannot write the settings file '{location}': {source}")]
	Write {
		location: String,
		#[source]
		source: std::io::Error,
	},
}

impl Settings {
	/// Brings every value into its valid set.
	pub fn normalize(&mut self) {
		let d = Self::default();
		if !self.window_width.is_finite() || self.window_width < 100.0 {
			self.window_width = d.window_width;
		}
		if !self.window_height.is_finite() || self.window_height < 100.0 {
			self.window_height = d.window_height;
		}
		self.window_left = self.window_left.filter(|v| v.is_finite());
		self.window_top = self.window_top.filter(|v| v.is_finite());
		if !self.language.is_empty() && Language::from_code(&self.language).is_none() {
			self.language.clear();
		}
		self.recent_files.truncate(MAX_RECENT);
		self.frame_memory_percent =
			if self.frame_memory_percent <= 0 { 0 } else { self.frame_memory_percent.clamp(10, 100) };
		self.preview_resolution = PreviewQuality::legal_divisor(self.preview_resolution);
		self.preview_frames = PreviewQuality::legal_divisor(self.preview_frames);
		self.preview_sample_bytes = self.preview_sample_bytes.clamp(1, 2);
		if !DITHER_BITS.contains(&self.preview_dither_bits) {
			self.preview_dither_bits = 0;
		}
		self.ui_zoom = if self.ui_zoom.is_finite() { self.ui_zoom.clamp(SWITCH_ZOOM.0, SWITCH_ZOOM.1) } else { 1.0 };
		self.export_hints.truncate(MAX_EXPORT_HINTS);
		if self.export_filter_index as usize >= wavedude_export::ExportFormat::ALL.len() {
			self.export_filter_index = 0;
		}
	}

	/// The sparse JSON text: the version and the values that differ from the defaults.
	pub fn to_sparse_json(&self) -> String {
		let current = Self { version: VERSION, ..self.clone() };
		let mut value = serde_json::to_value(current).expect("settings serialise");
		let defaults = serde_json::to_value(Self::default()).expect("settings serialise");
		if let (Value::Object(map), Value::Object(d)) = (&mut value, &defaults) {
			map.retain(|k, v| k == "Version" || d.get(k) != Some(v));
		}
		serde_json::to_string_pretty(&value).expect("settings serialise")
	}

	/// Reads a settings text; a file without a version is version 0, the format before the field.
	pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
		let value: Value = serde_json::from_str(text)?;
		let versioned = value.get("Version").is_some();
		let mut s: Self = serde_json::from_value(value)?;
		if !versioned {
			s.version = 0;
		}
		if s.version > VERSION {
			tracing::warn!(
				"the settings file is of version {}, newer than {VERSION}; unknown values are dropped",
				s.version
			);
		}
		s.normalize();
		Ok(s)
	}

	/// Reads the store's settings for a tool beside the app: the defaults when there are none, an error when they
	/// are unreadable, and never the first start's choice.
	pub fn read(store: &Store) -> Result<Self, SettingsError> {
		let error = |detail: String| SettingsError::Read { location: store.location(FILE_NAME), detail };
		match store.read(FILE_NAME) {
			Ok(Some(text)) => Self::from_json(&text).map_err(|e| error(e.to_string())),
			Ok(None) => Ok(Self::default()),
			Err(e) => Err(error(e.to_string())),
		}
	}

	/// Reads the store's settings; with none, makes the first start's choice from `facts` and writes them at once.
	pub fn load(store: &Store, facts: impl FnOnce() -> MachineFacts) -> Loaded {
		match store.read(FILE_NAME) {
			Ok(Some(text)) => match Self::from_json(&text) {
				Ok(settings) => Loaded { settings, first_start: false, unreadable: None },
				Err(e) => {
					let at = Some((e.line(), e.column()));
					let quoted = wavedude_core::io::excerpt(&text, at);
					let location = store.location(FILE_NAME);
					tracing::error!(
						"the settings file '{location}' could not be read: {e}\n--- the text of '{location}' ---\n{quoted}"
					);
					Loaded { settings: Self::default(), first_start: false, unreadable: Some(e.to_string()) }
				}
			},
			Ok(None) => {
				let mut settings = Self::default();
				let facts = facts();
				settings.apply_first_start(&facts);
				if let Err(e) = settings.save(store) {
					tracing::warn!("{e}");
				}
				Loaded { settings, first_start: true, unreadable: None }
			}
			Err(e) => {
				tracing::error!("the settings file '{}' could not be read: {e}", store.location(FILE_NAME));
				Loaded { settings: Self::default(), first_start: false, unreadable: Some(e.to_string()) }
			}
		}
	}

	/// The first start's preview quality and dithering for the machine (`PreviewDefaults`), logged at Info.
	pub fn apply_first_start(&mut self, facts: &MachineFacts) {
		let tier = PreviewDefaults::tier(facts);
		let (quality, dither) = PreviewDefaults::tier_of(facts);
		self.set_preview_quality(quality);
		self.preview_dither_bits = dither;
		tracing::info!(
			"first start: {:.1} GiB memory, {} threads, GPU {}, cache drive seek penalty {}: {tier:?} preview \
			 (resolution 1/{}, every {}. frame, {}-bit samples, dithering {})",
			facts.physical_bytes as f64 / (1u64 << 30) as f64,
			facts.threads,
			facts.has_gpu.map_or("unknown", |g| if g { "yes" } else { "no" }),
			facts.cache_drive_seek_penalty.map_or("unknown", |s| if s { "yes" } else { "no" }),
			quality.resolution_divisor,
			quality.frame_divisor,
			if quality.format == SampleFormat::Half { 16 } else { 8 },
			dither
		);
	}

	pub fn save(&self, store: &Store) -> Result<(), SettingsError> {
		store
			.write(FILE_NAME, &self.to_sparse_json())
			.map_err(|source| SettingsError::Write { location: store.location(FILE_NAME), source })
	}

	/// The preview quality of a preview run (the intensity plane as its root); valid once normalised.
	pub fn preview_quality(&self) -> PreviewQuality {
		let format = if self.preview_sample_bytes >= 2 { SampleFormat::Half } else { SampleFormat::Index };
		let q = PreviewQuality::new(self.preview_resolution, self.preview_frames, format);
		debug_assert!(q.is_valid(), "the settings are not normalised");
		q
	}

	pub fn set_preview_quality(&mut self, q: PreviewQuality) {
		let q = q.validated();
		self.preview_resolution = q.resolution_divisor;
		self.preview_frames = q.frame_divisor;
		self.preview_sample_bytes = if q.format == SampleFormat::Half { 2 } else { 1 };
	}

	/// The RAM cache share for the store's budget; 0 is Auto.
	pub fn frame_memory_share(&self) -> u32 {
		self.frame_memory_percent.max(0) as u32
	}

	/// Puts `path` at the top of the recent files, at most [`MAX_RECENT`].
	pub fn push_recent(&mut self, path: &Path) {
		self.recent_files.retain(|p| p != path);
		self.recent_files.insert(0, path.to_owned());
		self.recent_files.truncate(MAX_RECENT);
	}

	pub fn export_hint(&self, project: &Path) -> Option<&ExportHint> {
		self.export_hints.iter().find(|h| h.project == project)
	}

	/// Records a project's hint as the newest; hints of project files that no longer exist are dropped.
	pub fn set_export_hint(&mut self, hint: ExportHint) {
		self.export_hints.retain(|h| h.project != hint.project && h.project.exists());
		self.export_hints.insert(0, hint);
		self.export_hints.truncate(MAX_EXPORT_HINTS);
	}

	/// Sets the UI zoom from the View menu (0.5..3).
	pub fn set_menu_zoom(&mut self, zoom: f64) {
		if zoom.is_finite() {
			self.ui_zoom = zoom.clamp(MENU_ZOOM.0, MENU_ZOOM.1);
		}
	}
}

/// The `--zoom=<factor>` switch of a command line, 0.5..8.
pub fn zoom_switch<'a>(args: impl IntoIterator<Item = &'a str>) -> Option<f64> {
	args.into_iter()
		.filter_map(|a| a.strip_prefix("--zoom="))
		.filter_map(|v| v.parse::<f64>().ok())
		.find(|z| z.is_finite())
		.map(|z| z.clamp(SWITCH_ZOOM.0, SWITCH_ZOOM.1))
}

#[cfg(test)]
mod tests {
	use super::*;

	const GIB: u64 = 1 << 30;

	fn facts(gib: u64, threads: u32) -> MachineFacts {
		MachineFacts { physical_bytes: gib * GIB, threads, has_gpu: Some(true), cache_drive_seek_penalty: Some(false) }
	}

	#[test]
	fn defaults_write_the_version_alone() {
		let v: Value = serde_json::from_str(&Settings::default().to_sparse_json()).unwrap();
		assert_eq!(v, serde_json::json!({ "Version": VERSION }));
		let s = Settings { use_gpu: false, ui_zoom: 1.25, ..Settings::default() };
		let v: Value = serde_json::from_str(&s.to_sparse_json()).unwrap();
		assert_eq!(v.as_object().unwrap().len(), 3);
		assert_eq!(v["UseGpu"], Value::Bool(false));
		assert_eq!(Settings::from_json("{}").unwrap().version, 0, "a file before the field");
		assert_eq!(Settings::from_json(&s.to_sparse_json()).unwrap(), s);
	}

	#[test]
	fn save_replaces_the_file_whole() {
		let dir = tempfile::tempdir().unwrap();
		let store = Store::in_dir(dir.path().join("sub"));
		let s = Settings { use_gpu: false, ..Settings::default() };
		s.save(&store).unwrap();
		Settings::default().save(&store).unwrap();
		let names: Vec<_> =
			std::fs::read_dir(dir.path().join("sub")).unwrap().map(|e| e.unwrap().file_name()).collect();
		assert_eq!(names, [FILE_NAME], "no temporary file is left");
		assert_eq!(Settings::load(&store, wavedude_core::run::machine_facts).settings, Settings::default());
	}

	#[test]
	fn normalize_clamps() {
		let mut s = Settings {
			frame_memory_percent: 5,
			preview_resolution: 3,
			preview_frames: 9,
			preview_sample_bytes: 7,
			preview_dither_bits: 11,
			ui_zoom: 20.0,
			language: "xx".into(),
			window_left: Some(f64::NAN),
			export_filter_index: 99,
			..Settings::default()
		};
		s.normalize();
		assert_eq!(s.export_filter_index, 0);
		assert_eq!(
			(s.frame_memory_percent, s.preview_resolution, s.preview_frames, s.preview_sample_bytes),
			(10, 2, 4, 2)
		);
		assert_eq!((s.preview_dither_bits, s.ui_zoom, s.language.as_str(), s.window_left), (0, 8.0, "", None));
	}

	#[test]
	fn first_start_tiers() {
		let mut high = Settings::default();
		high.apply_first_start(&facts(64, 16));
		assert_eq!((high.preview_sample_bytes, high.preview_dither_bits, high.preview_resolution), (2, 16, 1));
		let mut low = Settings::default();
		low.apply_first_start(&facts(8, 16));
		assert_eq!((low.preview_resolution, low.preview_frames, low.preview_sample_bytes), (2, 2, 1));
		let mut middle = Settings::default();
		middle.apply_first_start(&facts(20, 16));
		assert_eq!(middle.preview_quality(), PreviewQuality::DEFAULT);
	}

	#[test]
	fn recent_and_hints() {
		let mut s = Settings::default();
		for i in 0..12 {
			s.push_recent(Path::new(&format!("/p/{i}.wdproj")));
		}
		s.push_recent(Path::new("/p/5.wdproj"));
		assert_eq!(s.recent_files.len(), MAX_RECENT);
		assert_eq!(s.recent_files[0], Path::new("/p/5.wdproj"));
		assert_eq!(zoom_switch(["a", "--zoom=12"]), Some(8.0));
		assert_eq!(zoom_switch(["--zoom=x"]), None);
	}

	#[test]
	fn a_tool_reads_defaults_without_a_file_and_refuses_a_broken_one() {
		let dir = tempfile::tempdir().unwrap();
		let store = Store::in_dir(dir.path().to_owned());
		let path = dir.path().join(FILE_NAME);
		assert_eq!(Settings::read(&store).unwrap(), Settings::default());
		assert!(!path.exists(), "a tool makes no first start");
		let s = Settings { use_openh264: Some(false), ..Settings::default() };
		s.save(&store).unwrap();
		assert_eq!(Settings::read(&store).unwrap().use_openh264, Some(false));
		std::fs::write(&path, "{").unwrap();
		assert!(Settings::read(&store).is_err());
	}
}
