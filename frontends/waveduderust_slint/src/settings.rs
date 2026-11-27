//! The Settings window's data (docs/ui/70, Settings; decision 0027): each change applies and is saved at once.

use waveduderust::quality::MachineFacts;
use waveduderust::quality::PreviewTier;
use waveduderust_platform::settings::{Settings, Theme, WEB_RAM_CACHE_SIZES};
use waveduderust_platform::store::Store;

use crate::models;
use crate::ui::{ColorTheme, Language, Levers, NumberSpec, PreviewQuality, RamBar, Scale, SettingsData};

fn scale(divisor: u32) -> Scale {
	match divisor {
		1 => Scale::Full,
		2 => Scale::Half,
		_ => Scale::Quarter,
	}
}

fn divisor(scale: Scale) -> u32 {
	match scale {
		Scale::Full => 1,
		Scale::Half => 2,
		Scale::Quarter => 4,
	}
}

/// The settings as the window shows them.
pub(crate) fn data(s: &Settings) -> SettingsData {
	SettingsData {
		theme: match s.theme {
			Theme::System => ColorTheme::System,
			Theme::Light => ColorTheme::Light,
			Theme::Dark => ColorTheme::Dark,
		},
		language: match s.language.as_str() {
			"en" => Language::En,
			"de" => Language::De,
			"fr" => Language::Fr,
			"nl" => Language::Nl,
			_ => Language::System,
		},
		title_full_path: s.title_full_path,
		show_startup: s.show_startup,
		undo_steps: models::count_to_ui(s.undo_steps),
		preview_resolution: scale(s.preview_resolution),
		preview_frames: scale(s.preview_frames),
		preview_index: s.preview_sample_bytes < 2,
		preview_dither: [0, 10, 12, 14, 16]
			.iter()
			.position(|&b| b == s.preview_dither_bits)
			.and_then(|i| i32::try_from(i).ok())
			.unwrap_or(0),
		ram_auto: web_ram_auto(),
		ram_sizes: web_ram_sizes(),
		ram_size: WEB_RAM_CACHE_SIZES
			.iter()
			.position(|&b| b == s.frame_memory_bytes)
			.and_then(|i| i32::try_from(i + 1).ok())
			.unwrap_or(0),
		use_gpu: s.use_gpu,
		auto_rerun: s.auto_rerun,
		warn_ram: s.warn_when_ram_insufficient,
		check_updates: s.check_for_updates == Some(true),
	}
}

/// The browser's Auto RAM cache size; empty natively, where the cache has no choice.
fn web_ram_auto() -> slint::SharedString {
	#[cfg(target_arch = "wasm32")]
	return crate::memory::size(crate::web::BUDGET).into();
	#[cfg(not(target_arch = "wasm32"))]
	slint::SharedString::default()
}

/// The browser's fixed RAM cache sizes; none natively.
fn web_ram_sizes() -> slint::ModelRc<slint::SharedString> {
	if cfg!(target_arch = "wasm32") {
		let sizes: Vec<slint::SharedString> =
			WEB_RAM_CACHE_SIZES.iter().map(|&b| crate::memory::size(b).into()).collect();
		slint::ModelRc::from(sizes.as_slice())
	} else {
		slint::ModelRc::default()
	}
}

/// The memory bar: `available` free (unknown in the browser) of `physical`, and the RAM cache.
pub(crate) fn ram_bar(available: Option<u64>, physical: u64, cache: u64) -> RamBar {
	let share = |bytes: u64| if physical == 0 { 0.0 } else { (bytes as f64 / physical as f64).min(1.0) as f32 };
	RamBar {
		available: available.map_or_else(String::new, crate::memory::size).into(),
		physical: crate::memory::size(physical).into(),
		cache: crate::memory::size(cache).into(),
		used: available.map_or(-1.0, |a| 1.0 - share(a)),
		cache_share: share(cache),
	}
}

/// Writes the window's edit into `settings` and saves them.
pub(crate) fn apply(d: &SettingsData, settings: &mut Settings, store: Option<&Store>) {
	let shown = data(settings);
	settings.theme = match d.theme {
		ColorTheme::System => Theme::System,
		ColorTheme::Light => Theme::Light,
		ColorTheme::Dark => Theme::Dark,
	};
	settings.language = match d.language {
		Language::System => "",
		Language::En => "en",
		Language::De => "de",
		Language::Fr => "fr",
		Language::Nl => "nl",
	}
	.into();
	settings.title_full_path = d.title_full_path;
	settings.show_startup = d.show_startup;
	settings.undo_steps = u32::try_from(d.undo_steps).unwrap_or_default();
	settings.preview_resolution = divisor(d.preview_resolution);
	settings.preview_frames = divisor(d.preview_frames);
	settings.preview_sample_bytes = if d.preview_index { 1 } else { 2 };
	settings.preview_dither_bits = [0, 10, 12, 14, 16][models::index_from_ui(d.preview_dither).unwrap_or(0).min(4)];
	// the browser's row alone sets it
	if !d.ram_auto.is_empty() {
		settings.frame_memory_bytes =
			models::index_from_ui(d.ram_size - 1).and_then(|i| WEB_RAM_CACHE_SIZES.get(i).copied()).unwrap_or(0);
	}
	settings.use_gpu = d.use_gpu;
	settings.auto_rerun = d.auto_rerun;
	settings.warn_when_ram_insufficient = d.warn_ram;
	// unset stays unset until the user picks
	if d.check_updates != shown.check_updates {
		settings.check_for_updates = Some(d.check_updates);
	}
	settings.normalize();
	save(settings, store);
}

/// The undo steps' box.
pub(crate) fn undo_spec() -> NumberSpec {
	let (min, max) = waveduderust_platform::settings::UNDO_STEPS;
	crate::number::whole(min as f32, max as f32)
}

/// The preview levers as they are.
pub(crate) fn levers(settings: &Settings) -> Levers {
	Levers {
		resolution: models::count_to_ui(settings.preview_resolution),
		frames: models::count_to_ui(settings.preview_frames),
		bits: if settings.preview_sample_bytes >= 2 { 16 } else { 8 },
		preset: models::index_to_ui(
			settings.preset().and_then(|tier| PreviewTier::ALL.iter().position(|&t| t == tier)),
		),
	}
}

/// The language `settings` pick: theirs, else the system's when a table has it, else English.
pub(crate) fn language(settings: &Settings) -> String {
	if !settings.language.is_empty() {
		return settings.language.clone();
	}
	let system = sys_locale::get_locale().unwrap_or_default();
	let base = system.split(['-', '_']).next().unwrap_or_default().to_lowercase();
	if waveduderust_platform::settings::LANGUAGES.contains(&base.as_str()) { base } else { "en".into() }
}

/// Applies preset `index`: Low, Medium, High, then Auto from `facts`; saves the settings.
pub(crate) fn preset(settings: &mut Settings, store: Option<&Store>, quality: PreviewQuality, facts: MachineFacts) {
	let tier = match quality {
		PreviewQuality::Low => PreviewTier::Low,
		PreviewQuality::Medium => PreviewTier::Medium,
		PreviewQuality::High => PreviewTier::High,
		PreviewQuality::Auto => settings.auto_preset(facts),
	};
	settings.apply_preset(tier);
	save(settings, store);
}

/// Saves `settings` to `store`; a failure is logged.
pub(crate) fn save(settings: &Settings, store: Option<&Store>) {
	if let Some(Err(e)) = store.map(|s| settings.save(s)) {
		tracing::error!("{e}");
	}
}

/// The colour scheme the theme forces on the std-widgets; `None` for the desktop's.
pub(crate) fn color_scheme(settings: &Settings) -> Option<slint::language::ColorScheme> {
	match settings.theme {
		Theme::Light => Some(slint::language::ColorScheme::Light),
		Theme::Dark => Some(slint::language::ColorScheme::Dark),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_memory_bar_shares_the_physical_memory() {
		let gib = 1u64 << 30;
		let bar = ram_bar(Some(12 * gib), 16 * gib, 8 * gib);
		assert_eq!((bar.used, bar.cache_share), (0.25, 0.5));
		let unknown = ram_bar(None, 0, gib);
		assert_eq!((unknown.used, unknown.cache_share, unknown.available.as_str()), (-1.0, 0.0, ""));
	}

	#[test]
	fn the_window_data_reads_and_writes_the_settings() {
		let mut s = Settings::default();
		let mut d = data(&s);
		d.theme = ColorTheme::Dark;
		d.language = Language::Fr;
		d.preview_resolution = Scale::Half;
		apply(&d, &mut s, None);
		assert_eq!((s.theme, s.language.as_str(), s.preview_resolution), (Theme::Dark, "fr", 2));
		assert_eq!(s.check_for_updates, Settings::default().check_for_updates);
		assert_eq!(data(&s), d);
	}
}
