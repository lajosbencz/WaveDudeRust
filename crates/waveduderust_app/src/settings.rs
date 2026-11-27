//! The program's user settings (docs/ui/80-settings.md): one sparse JSON text in the platform's store (a file in
//! the config dir, the browser's local storage), values at their default omitted, the first start's preview
//! quality chosen once from the machine.
//!
//! Threads: plain data, owned by the UI thread's `AppState`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use waveduderust::model::Word;
use waveduderust::quality::{MachineFacts, PreviewDefaults, PreviewQuality, PreviewTier};
use waveduderust::run::RamCache;
use waveduderust::unstable::engine::SampleFormat;
use waveduderust::unstable::export::h264::Profile;

use super::history;
use super::live::{WEB_FRAME_SHARE, WEB_RAM_CACHE_SIZES};
use super::platform::Store;
use super::strings::Language;

/// The settings file's key in the store.
pub const FILE_NAME: &str = "settings.json";
/// The settings file's format version, written in every file.
pub const VERSION: u32 = 2;
/// The RAM cache share of version 1, dropped at load.
const DROPPED_PERCENT: &str = "FrameMemoryPercent";
pub const MAX_RECENT: usize = 10;
pub const MAX_EXPORT_HINTS: usize = 20;
/// UI zoom bounds from the menu and from `--zoom=`.
pub const MENU_ZOOM: (f64, f64) = (0.5, 3.0);
pub const SWITCH_ZOOM: (f64, f64) = (0.5, 8.0);
/// The default UI zoom of a touch-first device; 1 elsewhere.
pub const TOUCH_ZOOM: f64 = 2.0;
pub const DITHER_BITS: [u32; 5] = [0, 10, 12, 14, 16];
/// The bounds of the steps that can be undone (decision 0030).
pub const UNDO_STEPS: (u32, u32) = (10, 100_000);

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

	/// [`platform`](Self::platform)'s, else all three in the temp folder's [`APP_FOLDER`].
	pub fn current() -> Self {
		Self::platform().unwrap_or_else(|| Self::under(&std::env::temp_dir().join(APP_FOLDER)))
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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
	/// The CPU engine is the legacy one where the CPU_REV2 engine would run (natively, by default on x86-64 with AVX2
	/// and FMA; docs/spec/30-engine.md).
	pub legacy_cpu_engine: bool,
	/// The browser's RAM cache size: 0 is Auto, else one of [`WEB_RAM_CACHE_SIZES`].
	pub frame_memory_bytes: u64,
	/// 1, 2 or 4.
	pub preview_resolution: u32,
	/// 1, 2 or 4.
	pub preview_frames: u32,
	/// 1 = 8-bit palette index, 2 = f16.
	pub preview_sample_bytes: u32,
	/// One of [`DITHER_BITS`]; 0 is off.
	pub preview_dither_bits: u32,
	/// Defaults to [`TOUCH_ZOOM`] on a touch-first device, else 1.
	pub ui_zoom: f64,
	pub last_export_folder: PathBuf,
	/// An index of [`waveduderust::unstable::export::ExportFormat::ALL`], the run export's last format.
	pub export_filter_index: u32,
	/// A run export writing several files puts them into one ZIP archive; the browser always does.
	pub export_archive: bool,
	pub part_description_expanded: bool,
	/// Newest project first.
	pub export_hints: Vec<ExportHint>,
	/// The window title names the project's full path rather than its file name (decision 0008).
	pub title_full_path: bool,
	/// MP4 export may use Cisco's `OpenH264`, downloaded at the first MP4 export (decision 0005); `None` until the
	/// user answered the launch's question. The CLI shares it.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub use_openh264: Option<bool>,
	/// The H.264 profile of MP4 export; the encoder's best when it lacks it.
	pub h264_profile: Profile,
	/// Look for a new release at launch (GitHub releases); `None` (never set) reads as off.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub check_for_updates: Option<bool>,
	/// The welcome page opens at each launch.
	pub show_startup: bool,
	/// The steps that can be undone, within [`UNDO_STEPS`].
	pub undo_steps: u32,
	/// The primary pointer is a finger, decided at the start; not stored. It sets the defaults of a device.
	#[serde(skip)]
	pub touch_first: bool,
	/// The settings this version does not know (another version's), written back as read.
	#[serde(flatten)]
	pub unknown: Map<String, Value>,
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
			legacy_cpu_engine: false,
			frame_memory_bytes: 0,
			preview_resolution: 1,
			preview_frames: 1,
			preview_sample_bytes: 1,
			preview_dither_bits: 0,
			ui_zoom: 1.0,
			last_export_folder: PathBuf::new(),
			export_filter_index: 0,
			export_archive: false,
			part_description_expanded: true,
			export_hints: Vec::new(),
			title_full_path: false,
			use_openh264: None,
			h264_profile: Profile::Baseline,
			check_for_updates: None,
			show_startup: true,
			undo_steps: history::DEFAULT_LIMIT as u32,
			touch_first: false,
			unknown: Map::new(),
		}
	}
}

/// The outcome of reading the settings file.
#[derive(Debug)]
pub struct Loaded {
	pub settings: Settings,
	/// No file existed: the first start's choice was made and the file written.
	pub first_start: bool,
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
	/// The defaults of a device, touch-first or not.
	pub fn for_device(touch_first: bool) -> Self {
		let ui_zoom = if touch_first { TOUCH_ZOOM } else { Self::default().ui_zoom };
		Self { ui_zoom, touch_first, ..Self::default() }
	}

	/// Brings every value into its valid set.
	pub fn normalize(&mut self) {
		let d = Self::for_device(self.touch_first);
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
		if !WEB_RAM_CACHE_SIZES.contains(&self.frame_memory_bytes) {
			self.frame_memory_bytes = 0;
		}
		self.preview_resolution = PreviewQuality::legal_divisor(self.preview_resolution);
		self.preview_frames = PreviewQuality::legal_divisor(self.preview_frames);
		self.preview_sample_bytes = self.preview_sample_bytes.clamp(1, 2);
		if !DITHER_BITS.contains(&self.preview_dither_bits) {
			self.preview_dither_bits = 0;
		}
		self.ui_zoom =
			if self.ui_zoom.is_finite() { self.ui_zoom.clamp(SWITCH_ZOOM.0, SWITCH_ZOOM.1) } else { d.ui_zoom };
		self.export_hints.truncate(MAX_EXPORT_HINTS);
		if self.export_filter_index as usize >= waveduderust::unstable::export::ExportFormat::ALL.len() {
			self.export_filter_index = 0;
		}
		let undo_steps = self.undo_steps.clamp(UNDO_STEPS.0, UNDO_STEPS.1);
		if undo_steps != self.undo_steps {
			tracing::warn!("the undo steps {} are out of {UNDO_STEPS:?}, {undo_steps} are used", self.undo_steps);
			self.undo_steps = undo_steps;
		}
	}

	/// The sparse JSON text: the version and the values that differ from the device's defaults.
	pub fn to_sparse_json(&self) -> String {
		let current = Self { version: VERSION, ..self.clone() };
		let mut value = serde_json::to_value(current).expect("settings serialise");
		let defaults = serde_json::to_value(Self::for_device(self.touch_first)).expect("settings serialise");
		if let (Value::Object(map), Value::Object(d)) = (&mut value, &defaults) {
			map.retain(|k, v| k == "Version" || d.get(k) != Some(v));
		}
		serde_json::to_string_pretty(&value).expect("settings serialise")
	}

	/// Reads a settings text of a device, the defaults of which fill what it lacks; a file without a version is
	/// version 0, the format before the field. A value that cannot be read is logged with its key and reads as its
	/// default; the others are kept, unknown keys among them. A RAM cache share is logged and dropped.
	pub fn from_json(text: &str, touch_first: bool) -> Result<Self, serde_json::Error> {
		let mut file: Map<String, Value> = serde_json::from_str(text)?;
		if let Some(percent) = file.remove(DROPPED_PERCENT).filter(|v| v.as_f64().is_some_and(|p| p > 0.0)) {
			tracing::warn!(
				"the RAM cache share of {percent} % ({DROPPED_PERCENT}) is dropped: the frames take the available memory"
			);
		}
		let versioned = file.contains_key("Version");
		let Value::Object(mut filled) = serde_json::to_value(Self::for_device(touch_first))? else {
			unreachable!("settings serialise to an object")
		};
		// every value before `key` reads, so a failure is the value of `key`
		for (key, value) in file {
			let default = filled.insert(key.clone(), value);
			if let Err(e) = Self::deserialize(&filled) {
				tracing::warn!("the setting '{key}' cannot be read, its default is used: {e}");
				match default {
					Some(d) => filled.insert(key, d),
					None => filled.remove(&key),
				};
			}
		}
		let mut s = Self::deserialize(&filled)?;
		s.touch_first = touch_first;
		if !versioned {
			s.version = 0;
		}
		if s.version > VERSION {
			tracing::warn!("the settings file is of version {}, newer than {VERSION}", s.version);
		}
		s.normalize();
		Ok(s)
	}

	/// Reads the store's settings for a tool beside the app: the defaults when there are none, an error when they
	/// are unreadable, and never the first start's choice.
	pub fn read(store: &Store, touch_first: bool) -> Result<Self, SettingsError> {
		let error = |detail: String| SettingsError::Read { location: store.location(FILE_NAME), detail };
		match store.read(FILE_NAME) {
			Ok(Some(text)) => Self::from_json(&text, touch_first).map_err(|e| error(e.to_string())),
			Ok(None) => Ok(Self::for_device(touch_first)),
			Err(e) => Err(error(e.to_string())),
		}
	}

	/// Reads the store's settings of a device; with none, makes the first start's choice from `facts` and writes
	/// them at once.
	pub fn load(store: &Store, touch_first: bool, facts: impl FnOnce() -> MachineFacts) -> Loaded {
		let location = || store.location(FILE_NAME);
		match store.read(FILE_NAME) {
			Ok(Some(text)) => match Self::from_json(&text, touch_first) {
				Ok(settings) => Loaded { settings, first_start: false },
				Err(e) => {
					let quoted = waveduderust::io::excerpt(&text, Some((e.line(), e.column())));
					let location = location();
					tracing::error!(
						"the settings file '{location}' could not be read: {e}\n--- the text of '{location}' ---\n{quoted}"
					);
					Loaded { settings: Self::for_device(touch_first), first_start: false }
				}
			},
			Ok(None) => {
				let mut settings = Self::for_device(touch_first);
				let facts = facts();
				settings.apply_first_start(&facts);
				if let Err(e) = settings.save(store) {
					tracing::warn!("{e}");
				}
				Loaded { settings, first_start: true }
			}
			Err(e) => {
				tracing::error!("the settings file '{}' could not be read: {e}", location());
				Loaded { settings: Self::for_device(touch_first), first_start: false }
			}
		}
	}

	/// The first start's preset for the machine (`PreviewDefaults`), logged at Info.
	pub fn apply_first_start(&mut self, facts: &MachineFacts) {
		let tier = PreviewDefaults::tier(facts);
		self.apply_preset(tier);
		let (quality, dither) = (self.preview_quality(), self.preview_dither_bits);
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

	/// Sets the four preview levers to `tier`'s.
	pub fn apply_preset(&mut self, tier: PreviewTier) {
		self.set_preview_quality(PreviewDefaults::quality_of(tier));
		self.preview_dither_bits = PreviewDefaults::dither_bits_of(tier);
	}

	/// The Auto preset: the tier of `facts`; in the browser a picked RAM cache size stands in for the device memory.
	pub fn auto_preset(&self, facts: MachineFacts) -> PreviewTier {
		let picked = if cfg!(target_arch = "wasm32") { self.frame_memory_bytes } else { 0 };
		PreviewDefaults::tier(&preset_facts(facts, picked))
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

	/// The RAM cache size for the store's budget: in the browser Auto or a size, natively Auto.
	pub fn ram_cache(&self) -> RamCache {
		match self.frame_memory_bytes {
			bytes if cfg!(target_arch = "wasm32") && bytes != 0 => RamCache::Bytes(bytes),
			_ => RamCache::Auto,
		}
	}

	/// Sets the RAM cache size; Auto clears the size, a share is no setting.
	pub fn set_ram_cache(&mut self, cache: RamCache) {
		match cache {
			RamCache::Auto => self.frame_memory_bytes = 0,
			RamCache::Bytes(bytes) => self.frame_memory_bytes = bytes,
			RamCache::Percent(percent) => tracing::debug!("the RAM cache share of {percent} % is ignored"),
		}
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

	/// Sets the UI zoom from Settings or the zoom keys (0.5..3).
	pub fn set_menu_zoom(&mut self, zoom: f64) {
		if zoom.is_finite() {
			self.ui_zoom = zoom.clamp(MENU_ZOOM.0, MENU_ZOOM.1);
		}
	}
}

/// `facts` with a picked RAM cache size (0 is none) in place of the memory, of which the size is the
/// [`WEB_FRAME_SHARE`]th part.
fn preset_facts(facts: MachineFacts, picked_bytes: u64) -> MachineFacts {
	match picked_bytes {
		0 => facts,
		bytes => MachineFacts { physical_bytes: bytes.saturating_mul(WEB_FRAME_SHARE), ..facts },
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
		let s = Settings {
			use_gpu: false,
			legacy_cpu_engine: true,
			ui_zoom: 1.25,
			h264_profile: Profile::High,
			undo_steps: 50,
			..Settings::default()
		};
		let v: Value = serde_json::from_str(&s.to_sparse_json()).unwrap();
		assert_eq!(v.as_object().unwrap().len(), 6);
		assert_eq!(v["UndoSteps"], 50);
		assert_eq!(v["UseGpu"], Value::Bool(false));
		assert_eq!(v["LegacyCpuEngine"], Value::Bool(true));
		assert_eq!(v["H264Profile"], "High", "the profile by its name");
		assert_eq!(Settings::from_json("{}", false).unwrap().version, 0, "a file before the field");
		assert_eq!(Settings::from_json(&s.to_sparse_json(), false).unwrap(), s);
	}

	#[test]
	fn the_ram_cache_size_round_trips_auto_clears_it_and_a_share_is_ignored() {
		let mut s = Settings::default();
		s.set_ram_cache(RamCache::Bytes(GIB));
		s.set_ram_cache(RamCache::Percent(40));
		let back = Settings::from_json(&s.to_sparse_json(), false).unwrap();
		assert_eq!(back.frame_memory_bytes, GIB);
		assert_eq!(back.ram_cache(), RamCache::Auto, "natively always Auto");
		s.set_ram_cache(RamCache::Auto);
		assert_eq!((s.frame_memory_bytes, s.ram_cache()), (0, RamCache::Auto));
	}

	#[test]
	fn a_version_1_share_is_dropped_at_load_and_not_written_back() {
		let dir = tempfile::tempdir().unwrap();
		let store = Store::in_dir(dir.path().to_owned());
		let v1 = serde_json::json!({ "Version": 1, "FrameMemoryPercent": 40, "UseGpu": false });
		std::fs::write(dir.path().join(FILE_NAME), v1.to_string()).unwrap();
		let loaded = Settings::load(&store, false, || panic!("not a first start")).settings;
		assert_eq!(loaded.version, 1);
		assert!(loaded.unknown.is_empty(), "{:?}", loaded.unknown);
		assert!(!loaded.use_gpu);
		assert!(!loaded.legacy_cpu_engine, "a file before the setting");
		loaded.save(&store).unwrap();
		let written: Value = serde_json::from_str(&store.read(FILE_NAME).unwrap().unwrap()).unwrap();
		assert_eq!(written, serde_json::json!({ "Version": 2, "UseGpu": false }));
		let auto = Settings::from_json(r#"{"Version": 1, "FrameMemoryPercent": 0}"#, false).unwrap();
		assert!(auto.unknown.is_empty(), "Auto is dropped too");
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
		assert_eq!(Settings::load(&store, false, crate::platform::machine_facts).settings, Settings::default());
	}

	#[test]
	fn normalize_clamps_each_value_to_its_bounds_or_the_devices_default() {
		type Set = fn(&mut Settings);
		let rows: [(&str, Set, Set); 12] = [
			("browser frame memory", |s| s.frame_memory_bytes = 3 << 30, |s| s.frame_memory_bytes = 0),
			("preview resolution", |s| s.preview_resolution = 3, |s| s.preview_resolution = 2),
			("preview frames", |s| s.preview_frames = 9, |s| s.preview_frames = 4),
			("sample bytes", |s| s.preview_sample_bytes = 7, |s| s.preview_sample_bytes = 2),
			("dither bits", |s| s.preview_dither_bits = 11, |s| s.preview_dither_bits = DITHER_BITS[0]),
			("zoom", |s| s.ui_zoom = 20.0, |s| s.ui_zoom = SWITCH_ZOOM.1),
			("zoom no number", |s| s.ui_zoom = f64::NAN, |_| {}),
			("language", |s| s.language = "xx".into(), |s| s.language.clear()),
			("window left", |s| s.window_left = Some(f64::NAN), |s| s.window_left = None),
			("export filter", |s| s.export_filter_index = 99, |s| s.export_filter_index = 0),
			("undo steps low", |s| s.undo_steps = 0, |s| s.undo_steps = UNDO_STEPS.0),
			("undo steps high", |s| s.undo_steps = u32::MAX, |s| s.undo_steps = UNDO_STEPS.1),
		];
		for touch_first in [false, true] {
			for (name, bad, want) in rows {
				let (mut got, mut expected) = (Settings::for_device(touch_first), Settings::for_device(touch_first));
				bad(&mut got);
				want(&mut expected);
				got.normalize();
				assert_eq!(got, expected, "{name}, touch first {touch_first}");
			}
		}
	}

	#[test]
	fn each_preset_sets_the_four_levers() {
		let levers =
			|s: &Settings| (s.preview_resolution, s.preview_frames, s.preview_sample_bytes, s.preview_dither_bits);
		let rows =
			[(PreviewTier::Low, (2, 2, 1, 0)), (PreviewTier::Medium, (1, 1, 1, 0)), (PreviewTier::High, (1, 1, 2, 16))];
		for (tier, want) in rows {
			let mut s =
				Settings { preview_resolution: 4, preview_frames: 4, preview_dither_bits: 12, ..Settings::default() };
			s.apply_preset(tier);
			assert_eq!(levers(&s), want, "{tier:?}");
			assert_eq!(s.preview_quality(), PreviewDefaults::quality_of(tier), "{tier:?}");
		}
	}

	#[test]
	fn a_picked_size_stands_in_for_the_memory() {
		let machine = facts(64, 16);
		assert_eq!(preset_facts(machine, 0), machine, "Auto keeps the machine");
		let picked = preset_facts(machine, 256 << 20);
		assert_eq!((picked.physical_bytes, picked.threads), (2 * GIB, 16));
		assert_eq!(PreviewDefaults::tier(&picked), PreviewTier::Low);
		assert_eq!(PreviewDefaults::tier(&preset_facts(facts(8, 16), 2 * GIB)), PreviewTier::Medium);
		let s = Settings { frame_memory_bytes: 256 << 20, ..Settings::default() };
		assert_eq!(s.auto_preset(machine), PreviewTier::High, "natively the size is unused");
	}

	#[test]
	fn first_start_tiers() {
		let mut high = Settings::default();
		high.apply_first_start(&facts(64, 16));
		assert_eq!((high.preview_sample_bytes, high.preview_dither_bits, high.preview_resolution), (2, 16, 1));
		let mut low = Settings::default();
		low.apply_first_start(&facts(8, 16));
		assert_eq!((low.preview_resolution, low.preview_frames, low.preview_sample_bytes), (2, 2, 1));
		let mut medium = Settings::default();
		medium.apply_first_start(&facts(20, 16));
		assert_eq!(medium.preview_quality(), PreviewQuality::DEFAULT);
	}

	fn stores_zoom(s: &Settings) -> bool {
		let stored: Value = serde_json::from_str(&s.to_sparse_json()).unwrap();
		stored.get("UiZoom").is_some()
	}

	#[test]
	fn the_ui_zoom_defaults_by_device_and_only_a_differing_zoom_is_stored() {
		assert!(Settings::for_device(true).ui_zoom > Settings::for_device(false).ui_zoom);
		assert!((MENU_ZOOM.0..=MENU_ZOOM.1).contains(&TOUCH_ZOOM));
		let dir = tempfile::tempdir().unwrap();
		for touch_first in [false, true] {
			let default = Settings::for_device(touch_first);
			let unzoomed = Settings::from_json("{}", touch_first).unwrap();
			assert_eq!(
				(unzoomed.ui_zoom, unzoomed.touch_first),
				(default.ui_zoom, touch_first),
				"a file without a zoom"
			);
			assert!(!stores_zoom(&default), "a default is not written");
			let default_zoom = default.ui_zoom;
			let other = Settings::for_device(!touch_first).ui_zoom;
			let chosen = Settings { ui_zoom: other, ..default };
			assert!(stores_zoom(&chosen), "a zoom that differs from the device's default is");
			assert_eq!(Settings::from_json(&chosen.to_sparse_json(), touch_first).unwrap(), chosen);

			let store = Store::in_dir(dir.path().join(format!("touch-first-{touch_first}")));
			let first = Settings::load(&store, touch_first, || facts(20, 16));
			assert!(first.first_start);
			assert_eq!(first.settings.ui_zoom, Settings::for_device(touch_first).ui_zoom, "the first start");
			let existing = Settings::load(&store, touch_first, || panic!("not a first start"));
			assert_eq!(existing.settings.ui_zoom, default_zoom, "a file without a zoom");
			assert!(existing.settings.touch_first == touch_first);
			chosen.save(&store).unwrap();
			let again = Settings::load(&store, touch_first, || panic!("not a first start"));
			assert_eq!(again.settings.ui_zoom, other, "an explicit zoom is kept");
		}
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
	fn an_unreadable_file_is_kept_out_and_no_first_start() {
		let dir = tempfile::tempdir().unwrap();
		let store = Store::in_dir(dir.path().to_owned());
		std::fs::write(dir.path().join(FILE_NAME), "{ not json").unwrap();
		let broken = Settings::load(&store, false, || panic!("an unreadable file is not a first start"));
		assert!(!broken.first_start);
		assert_eq!(broken.settings, Settings::default());
	}

	#[test]
	fn an_unreadable_value_reads_as_its_default_and_keeps_the_others() {
		for touch_first in [false, true] {
			let text = r#"{"Version": 2, "UiZoom": 1.5, "Theme": "Neon", "UseGpu": "yes", "UiZoomTypo": 3, "AutoRerun": false}"#;
			let s = Settings::from_json(text, touch_first).unwrap();
			let expected = Settings {
				ui_zoom: 1.5,
				auto_rerun: false,
				unknown: Map::from_iter([("UiZoomTypo".to_owned(), Value::from(3))]),
				..Settings::for_device(touch_first)
			};
			assert_eq!(s, expected, "touch first {touch_first}");
		}
	}

	#[test]
	fn unknown_settings_survive_a_save() {
		let dir = tempfile::tempdir().unwrap();
		let store = Store::in_dir(dir.path().to_owned());
		let newer = serde_json::json!({ "Version": 3, "UseGpu": false, "Future": { "Kind": "Mode", "Level": [1, 2] } });
		std::fs::write(dir.path().join(FILE_NAME), newer.to_string()).unwrap();
		let mut loaded = Settings::load(&store, false, || panic!("not a first start")).settings;
		assert!(!loaded.use_gpu);
		loaded.auto_rerun = false;
		loaded.save(&store).unwrap();
		let written: Value = serde_json::from_str(&store.read(FILE_NAME).unwrap().unwrap()).unwrap();
		assert_eq!(written["Future"], newer["Future"]);
		assert_eq!((written["UseGpu"].clone(), written["AutoRerun"].clone()), (Value::Bool(false), Value::Bool(false)));
	}

	#[test]
	fn a_tool_reads_defaults_without_a_file_and_refuses_a_broken_one() {
		let dir = tempfile::tempdir().unwrap();
		let store = Store::in_dir(dir.path().to_owned());
		let path = dir.path().join(FILE_NAME);
		assert_eq!(Settings::read(&store, false).unwrap(), Settings::default());
		assert!(!path.exists(), "a tool makes no first start");
		let s = Settings { use_openh264: Some(false), ..Settings::default() };
		s.save(&store).unwrap();
		assert_eq!(Settings::read(&store, false).unwrap().use_openh264, Some(false));
		std::fs::write(&path, "{").unwrap();
		assert!(Settings::read(&store, false).is_err());
	}
}
