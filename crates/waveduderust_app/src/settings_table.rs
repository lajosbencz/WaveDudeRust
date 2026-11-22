//! The user settings as the Settings window and the welcome page list them (docs/ui/70, Settings; decision 0027): the
//! rows by section, which builds show them, the search over their labels, the levers' choices, the zoom steps and the
//! welcome page's lever line. No toolkit: each window draws a row's control for its [`Setting`].

use crate::settings::{DITHER_BITS, Settings};
use crate::strings::{Language, Strings, strip_mnemonic};
use waveduderust::unstable::engine::SampleFormat;

use crate::App;
use crate::menus::ZoomStep;

/// The UI zooms: factor and string key.
pub const ZOOMS: [(f64, &str); 8] = [
	(0.5, "MenuViewUiZoom50"),
	(0.75, "MenuViewUiZoom75"),
	(1.0, "MenuViewUiZoom100"),
	(1.25, "MenuViewUiZoom125"),
	(1.5, "MenuViewUiZoom150"),
	(2.0, "MenuViewUiZoom200"),
	(2.5, "MenuViewUiZoom250"),
	(3.0, "MenuViewUiZoom300"),
];

/// The preview's resolution lever: divisor and string key.
pub const RESOLUTIONS: [(u32, &str); 3] =
	[(1, "MenuViewResolutionFull"), (2, "MenuViewResolutionHalf"), (4, "MenuViewResolutionQuarter")];

/// The preview's frames lever: divisor and string key.
pub const FRAME_STEPS: [(u32, &str); 3] =
	[(1, "MenuViewFramesFull"), (2, "MenuViewFramesHalf"), (4, "MenuViewFramesQuarter")];

/// The preview's samples lever: format and string key.
pub const SAMPLES: [(SampleFormat, &str); 2] =
	[(SampleFormat::Half, "MenuViewSamples16"), (SampleFormat::Index, "MenuViewSamples8")];

/// The preview's dither lever: bits and string key.
pub const DITHERS: [(u32, &str); DITHER_BITS.len()] = [
	(DITHER_BITS[0], "MenuViewDitherOff"),
	(DITHER_BITS[1], "MenuViewDither10"),
	(DITHER_BITS[2], "MenuViewDither12"),
	(DITHER_BITS[3], "MenuViewDither14"),
	(DITHER_BITS[4], "MenuViewDither16"),
];

/// The UI zoom a zoom key sets from `st`'s, if it changes: the next of [`ZOOMS`] up or down, or the device's default.
pub fn zoom_step(st: &Settings, step: ZoomStep) -> Option<f64> {
	let now = st.ui_zoom;
	let zooms = ZOOMS.iter().map(|&(z, _)| z);
	let zoom = match step {
		ZoomStep::In => zooms.filter(|&z| z > now + 1e-6).reduce(f64::min),
		ZoomStep::Out => zooms.filter(|&z| z < now - 1e-6).reduce(f64::max),
		ZoomStep::Reset => Some(Settings::for_device(st.touch_first).ui_zoom),
	};
	zoom.filter(|&z| (z - now).abs() > 1e-6)
}

/// Bytes as "x.y GiB" or "x MiB", with a decimal point.
pub fn bytes_text(bytes: u64) -> String {
	const MIB: f64 = 1024. * 1024.;
	const GIB: f64 = 1024. * MIB;
	let b = bytes as f64;
	if b >= GIB { format!("{:.1} GiB", b / GIB) } else { format!("{:.0} MiB", b / MIB) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
	General,
	Preview,
	Simulation,
	Export,
	Updates,
}

impl Section {
	pub const ALL: [Self; 5] = [Self::General, Self::Preview, Self::Simulation, Self::Export, Self::Updates];

	pub const fn title_key(self) -> &'static str {
		match self {
			Self::General => "SettingsGeneral",
			Self::Preview => "SettingsPreview",
			Self::Simulation => "MenuSimulation",
			Self::Export => "SettingsExport",
			Self::Updates => "StartupUpdates",
		}
	}
}

/// What the build offers that rows depend on.
#[derive(Clone, Copy, Debug)]
pub struct Build {
	pub web: bool,
	/// `OpenH264` is compiled in.
	pub codec_built_in: bool,
	pub updater: bool,
}

/// This build.
#[cfg(not(target_arch = "wasm32"))]
pub const BUILD: Build = Build {
	web: false,
	codec_built_in: waveduderust::unstable::export::h264::BUILT_IN,
	updater: crate::update::AVAILABLE,
};
/// This build.
#[cfg(target_arch = "wasm32")]
pub const BUILD: Build = Build { web: true, codec_built_in: false, updater: false };

/// Which builds show a row.
#[derive(Clone, Copy, Debug)]
pub enum Need {
	Always,
	Native,
	/// Natively, while `OpenH264` is downloaded from Cisco.
	CodecDownload,
	Updater,
}

impl Need {
	pub fn met(self, b: Build) -> bool {
		match self {
			Self::Always => true,
			Self::Native => !b.web,
			Self::CodecDownload => !b.web && !b.codec_built_in,
			Self::Updater => !b.web && b.updater,
		}
	}
}

/// The setting a row changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
	Theme,
	Language,
	Zoom,
	TitleFullPath,
	ShowStartup,
	UndoSteps,
	Ram,
	Preset,
	Resolution,
	Frames,
	Samples,
	Dither,
	UseGpu,
	AutoRerun,
	WarnMemory,
	OpenH264,
	H264Profile,
	Archive,
	CheckAtLaunch,
	CheckNow,
}

/// A row: its section, label key, the builds showing it and its setting.
#[derive(Clone, Copy, Debug)]
pub struct Row {
	pub section: Section,
	pub label: &'static str,
	pub need: Need,
	pub setting: Setting,
}

const fn row(section: Section, label: &'static str, need: Need, setting: Setting) -> Row {
	Row { section, label, need, setting }
}

pub const ROWS: [Row; 20] = [
	row(Section::General, "MenuViewTheme", Need::Always, Setting::Theme),
	row(Section::General, "MenuViewLanguage", Need::Always, Setting::Language),
	row(Section::General, "MenuViewUiZoom", Need::Always, Setting::Zoom),
	row(Section::General, "SettingsTitleFullPath", Need::Always, Setting::TitleFullPath),
	row(Section::General, "SettingsShowStartup", Need::Always, Setting::ShowStartup),
	row(Section::General, "SettingsUndoSteps", Need::Always, Setting::UndoSteps),
	row(Section::Preview, "SettingsRam", Need::Always, Setting::Ram),
	row(Section::Preview, "SettingsPreset", Need::Always, Setting::Preset),
	row(Section::Preview, "MenuViewResolution", Need::Always, Setting::Resolution),
	row(Section::Preview, "MenuViewFrames", Need::Always, Setting::Frames),
	row(Section::Preview, "MenuViewSamples", Need::Always, Setting::Samples),
	row(Section::Preview, "MenuViewDither", Need::Always, Setting::Dither),
	row(Section::Preview, "MenuSimulationUseGpu", Need::Always, Setting::UseGpu),
	row(Section::Simulation, "MenuSimulationAutoRerun", Need::Always, Setting::AutoRerun),
	row(Section::Simulation, "MenuSimulationWarnMemory", Need::Always, Setting::WarnMemory),
	row(Section::Export, "ExportUseOpenH264", Need::CodecDownload, Setting::OpenH264),
	row(Section::Export, "ExportH264Profile", Need::Always, Setting::H264Profile),
	row(Section::Export, "SettingsExportArchive", Need::Native, Setting::Archive),
	row(Section::Updates, "SettingsCheckForUpdates", Need::Updater, Setting::CheckAtLaunch),
	row(Section::Updates, "SettingsCheckNow", Need::Updater, Setting::CheckNow),
];

/// The indices into [`ROWS`] that `build` shows.
pub fn shown_rows(build: Build) -> impl Iterator<Item = usize> {
	(0..ROWS.len()).filter(move |&i| ROWS[i].need.met(build))
}

/// The sections with a row `build` shows.
pub fn shown_sections(build: Build) -> Vec<Section> {
	Section::ALL.into_iter().filter(|&sec| shown_rows(build).any(|i| ROWS[i].section == sec)).collect()
}

/// Whether `query`, trimmed and in any case, is part of one of `texts`; an empty query is part of every text.
pub fn matches(query: &str, texts: &[&str]) -> bool {
	let query = query.trim().to_lowercase();
	query.is_empty() || texts.iter().any(|t| t.to_lowercase().contains(&query))
}

/// The indices into [`ROWS`] shown by `build` that `query` matches: the label in `s` or in English, or the section's
/// name in `s`.
pub fn found(query: &str, s: &Strings, build: Build) -> Vec<usize> {
	let en = Strings::new(Language::English);
	shown_rows(build)
		.filter(|&i| {
			let row = &ROWS[i];
			let texts = [s.get(row.label), en.get(row.label), s.get(row.section.title_key())].map(strip_mnemonic);
			matches(query, &texts.each_ref().map(String::as_str))
		})
		.collect()
}

/// The welcome page opens at this launch.
pub fn welcome_due(app: &App) -> bool {
	app.first_start || app.settings.show_startup
}

/// The lever line's words: resolution divisor and string key.
const LINE_RESOLUTIONS: [(u32, &str); 3] =
	[(1, "StartupResolution1"), (2, "StartupResolution2"), (4, "StartupResolution4")];

/// The lever line's words: frame divisor and string key.
const LINE_FRAME_STEPS: [(u32, &str); 3] = [(1, "StartupFrames1"), (2, "StartupFrames2"), (4, "StartupFrames4")];

/// The levers as they are, e.g. "Full resolution, every frame, 8-bit samples".
pub fn levers_line(s: &Strings, st: &Settings) -> String {
	let q = st.preview_quality();
	let word =
		|table: &[(u32, &'static str)], v: u32| table.iter().find(|&&(d, _)| d == v).map(|&(_, k)| s.get(k).to_owned());
	let samples = match q.format {
		SampleFormat::Half => "StartupSamples16",
		SampleFormat::Index => "StartupSamples8",
	};
	let dither = (q.format == SampleFormat::Half && st.preview_dither_bits > 0)
		.then(|| s.format("StartupDitherFormat", &[&st.preview_dither_bits]));
	[
		word(&LINE_RESOLUTIONS, q.resolution_divisor),
		word(&LINE_FRAME_STEPS, q.frame_divisor),
		Some(s.get(samples).to_owned()),
		dither,
	]
	.into_iter()
	.flatten()
	.collect::<Vec<_>>()
	.join(", ")
}

#[cfg(test)]
mod tests {
	use crate::settings::TOUCH_ZOOM;
	use waveduderust::quality::PreviewTier;

	use super::*;

	const NATIVE: Build = Build { web: false, codec_built_in: false, updater: true };

	fn labels(rows: &[usize]) -> Vec<&'static str> {
		rows.iter().map(|&i| ROWS[i].label).collect()
	}

	#[test]
	fn a_query_matches_in_any_case_and_an_empty_one_everything() {
		assert!(matches("", &["Theme"]) && matches("  ", &[]));
		assert!(matches(" gPu ", &["Use the GPU"]));
		assert!(matches("lang", &["Sprache", "Language"]));
		assert!(!matches("zoom", &["Theme", "General"]));
	}

	#[test]
	fn the_zoom_keys_step_through_the_zooms_and_reset_to_the_devices() {
		let at = |ui_zoom, touch_first| Settings { ui_zoom, ..Settings::for_device(touch_first) };
		assert_eq!(zoom_step(&at(1.0, false), ZoomStep::In), Some(1.25));
		assert_eq!(zoom_step(&at(1.0, false), ZoomStep::Out), Some(0.75));
		assert_eq!(zoom_step(&at(1.1, false), ZoomStep::Out), Some(1.0), "off the table");
		assert_eq!(zoom_step(&at(3.0, false), ZoomStep::In), None);
		assert_eq!(zoom_step(&at(0.5, false), ZoomStep::Out), None);
		assert_eq!(zoom_step(&at(5.0, false), ZoomStep::Out), Some(3.0), "a --zoom= beyond the table");
		assert_eq!(zoom_step(&at(1.5, false), ZoomStep::Reset), Some(1.0));
		assert_eq!(zoom_step(&at(1.0, true), ZoomStep::Reset), Some(TOUCH_ZOOM));
		assert_eq!(zoom_step(&at(1.0, false), ZoomStep::Reset), None);
	}

	#[test]
	fn rows_follow_the_build() {
		let shown = |b: Build| labels(&shown_rows(b).collect::<Vec<_>>());
		let web = Build { web: true, codec_built_in: false, updater: false };
		for label in ["ExportUseOpenH264", "SettingsExportArchive", "SettingsCheckForUpdates", "SettingsCheckNow"] {
			assert!(!shown(web).contains(&label), "{label} in the browser");
			assert!(shown(NATIVE).contains(&label), "{label} natively");
		}
		assert!(!shown(Build { codec_built_in: true, ..NATIVE }).contains(&"ExportUseOpenH264"));
		assert!(!shown_sections(web).contains(&Section::Updates));
		assert!(!shown_sections(Build { updater: false, ..NATIVE }).contains(&Section::Updates));
		assert_eq!(shown_sections(NATIVE), Section::ALL);
	}

	#[test]
	fn the_search_reads_labels_in_both_languages_and_section_names() {
		let (en, de) = (Strings::new(Language::English), Strings::new(Language::German));
		assert_eq!(labels(&found("gpu", &en, NATIVE)), ["MenuSimulationUseGpu"]);
		assert_eq!(labels(&found("undo", &en, NATIVE)), ["SettingsUndoSteps"]);
		assert_eq!(labels(&found("undo", &de, NATIVE)), ["SettingsUndoSteps"], "English in German");
		assert_eq!(labels(&found("preview res", &en, NATIVE)), ["MenuViewResolution"], "mnemonics are no text");
		assert_eq!(labels(&found("sprache", &de, NATIVE)), ["MenuViewLanguage"]);
		assert_eq!(labels(&found("language", &de, NATIVE)), ["MenuViewLanguage"], "English in German");
		assert_eq!(labels(&found("simulation", &en, NATIVE)), ["MenuSimulationAutoRerun", "MenuSimulationWarnMemory"]);
		assert!(found("no such setting", &en, NATIVE).is_empty());
		assert_eq!(found("", &en, NATIVE).len(), ROWS.len());
	}

	#[test]
	fn every_label_is_in_every_language() {
		for language in Language::ALL {
			let keys: Vec<_> = crate::strings::keys(language).collect();
			for key in ROWS.iter().map(|r| r.label).chain(Section::ALL.map(Section::title_key)) {
				assert!(keys.contains(&key), "{key} in {language:?}");
			}
		}
	}

	fn at(tier: PreviewTier) -> Settings {
		let mut st = Settings::for_device(false);
		st.apply_preset(tier);
		st
	}

	#[test]
	fn the_line_names_the_levers() {
		let s = Strings::new(Language::English);
		assert_eq!(levers_line(&s, &at(PreviewTier::Medium)), "Full resolution, every frame, 8-bit samples");
		assert_eq!(levers_line(&s, &at(PreviewTier::Low)), "Half resolution, every second frame, 8-bit samples");
		assert_eq!(
			levers_line(&s, &at(PreviewTier::High)),
			"Full resolution, every frame, 16-bit samples, dithering to 16 bits"
		);
	}

	#[test]
	fn every_line_word_is_in_every_language() {
		let words = LINE_RESOLUTIONS.iter().chain(&LINE_FRAME_STEPS).map(|&(_, k)| k);
		let keys: Vec<_> = words.chain(["StartupSamples16", "StartupSamples8", "StartupDitherFormat"]).collect();
		for language in Language::ALL {
			let have: Vec<_> = crate::strings::keys(language).collect();
			for key in &keys {
				assert!(have.contains(key), "{key} in {language:?}");
			}
		}
	}
}
