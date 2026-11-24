//! The Settings window's data (docs/ui/70, Settings; decision 0027): each change applies and is saved at once.

use waveduderust::quality::MachineFacts;
use waveduderust::quality::PreviewTier;
use waveduderust_platform::settings::{Settings, Theme};
use waveduderust_platform::store::Store;

use crate::ui::{ColorTheme, Language, Levers, NumberSpec, PreviewQuality, Scale, SettingsData};

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
		undo_steps: i32::try_from(s.undo_steps).unwrap_or(i32::MAX),
		preview_resolution: scale(s.preview_resolution),
		preview_frames: scale(s.preview_frames),
		use_gpu: s.use_gpu,
		auto_rerun: s.auto_rerun,
		warn_ram: s.warn_when_ram_insufficient,
		check_updates: s.check_for_updates == Some(true),
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
	let int = |n: u32| i32::try_from(n).unwrap_or(i32::MAX);
	Levers {
		resolution: int(settings.preview_resolution),
		frames: int(settings.preview_frames),
		bits: if settings.preview_sample_bytes >= 2 { 16 } else { 8 },
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

fn save(settings: &Settings, store: Option<&Store>) {
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
