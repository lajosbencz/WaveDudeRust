//! The Settings window's rows (docs/ui/70, Settings; decision 0027): each change applies and is saved at once.

use waveduderust::quality::MachineFacts;
use waveduderust::quality::PreviewTier;
use waveduderust_platform::settings::{Settings, Theme};
use waveduderust_platform::store::Store;

use crate::ui::{PropKind, PropRow};

pub(crate) const SECTIONS: [&str; 4] = ["General", "Preview and memory", "Simulation", "Updates"];

/// A row: its section, label, choices (empty for a check box or number), and how it reads and writes the setting.
struct Row {
	section: usize,
	label: &'static str,
	choices: &'static [&'static str],
	int: bool,
	get: fn(&Settings) -> String,
	set: fn(&mut Settings, &str),
}

const SCALES: [&str; 3] = ["Full", "Half", "Quarter"];

fn scale_name(divisor: u32) -> String {
	SCALES[divisor.trailing_zeros().min(2) as usize].into()
}

fn scale_divisor(name: &str) -> u32 {
	1 << SCALES.iter().position(|s| *s == name).unwrap_or(0)
}

fn flag(b: bool) -> String {
	b.to_string()
}

const ROWS: &[Row] = &[
	Row {
		section: 0,
		label: "Theme",
		choices: &["System", "Light", "Dark"],
		int: false,
		get: |s| format!("{:?}", s.theme),
		set: |s, v| {
			s.theme = match v {
				"Light" => Theme::Light,
				"Dark" => Theme::Dark,
				_ => Theme::System,
			}
		},
	},
	Row {
		section: 0,
		label: "Full path in the title",
		choices: &[],
		int: false,
		get: |s| flag(s.title_full_path),
		set: |s, v| s.title_full_path = v == "true",
	},
	Row {
		section: 0,
		label: "Show the welcome page at each launch",
		choices: &[],
		int: false,
		get: |s| flag(s.show_startup),
		set: |s, v| s.show_startup = v == "true",
	},
	Row {
		section: 0,
		label: "Undo steps",
		choices: &[],
		int: true,
		get: |s| s.undo_steps.to_string(),
		set: |s, v| {
			if let Ok(n) = v.trim().parse::<u32>() {
				s.undo_steps = n.clamp(10, 100_000);
			}
		},
	},
	Row {
		section: 1,
		label: "Preview resolution",
		choices: &SCALES,
		int: false,
		get: |s| scale_name(s.preview_resolution),
		set: |s, v| s.preview_resolution = scale_divisor(v),
	},
	Row {
		section: 1,
		label: "Preview frames",
		choices: &SCALES,
		int: false,
		get: |s| scale_name(s.preview_frames),
		set: |s, v| s.preview_frames = scale_divisor(v),
	},
	Row {
		section: 1,
		label: "Use the GPU",
		choices: &[],
		int: false,
		get: |s| flag(s.use_gpu),
		set: |s, v| s.use_gpu = v == "true",
	},
	Row {
		section: 2,
		label: "Re-run automatically after edits",
		choices: &[],
		int: false,
		get: |s| flag(s.auto_rerun),
		set: |s, v| s.auto_rerun = v == "true",
	},
	Row {
		section: 2,
		label: "Warn when the RAM cache is too small",
		choices: &[],
		int: false,
		get: |s| flag(s.warn_when_ram_insufficient),
		set: |s, v| s.warn_when_ram_insufficient = v == "true",
	},
	Row {
		section: 3,
		label: "Check for updates at start",
		choices: &[],
		int: false,
		get: |s| flag(s.check_for_updates == Some(true)),
		set: |s, v| s.check_for_updates = Some(v == "true"),
	},
];

fn in_section(section: usize) -> impl Iterator<Item = &'static Row> {
	ROWS.iter().filter(move |r| r.section == section)
}

/// The rows of `section`.
pub(crate) fn rows(settings: &Settings, section: usize) -> Vec<PropRow> {
	in_section(section)
		.map(|r| {
			let value = (r.get)(settings);
			let kind = if !r.choices.is_empty() {
				PropKind::Choice
			} else if r.int {
				PropKind::Int
			} else {
				PropKind::Bool
			};
			let choices: Vec<slint::SharedString> = r.choices.iter().map(|c| (*c).into()).collect();
			PropRow {
				label: r.label.into(),
				kind,
				current: r.choices.iter().position(|c| *c == value).map_or(-1, |i| i as i32),
				value: value.into(),
				choices: slint::ModelRc::from(choices.as_slice()),
			}
		})
		.collect()
}

/// Sets row `row` of `section` and saves the settings.
pub(crate) fn set_row(settings: &mut Settings, store: Option<&Store>, section: usize, row: usize, value: &str) {
	if let Some(r) = in_section(section).nth(row) {
		(r.set)(settings, value);
		settings.normalize();
		save(settings, store);
	}
}

/// The preview levers as they are, e.g. "Full resolution, every frame, 8-bit samples".
pub(crate) fn levers(settings: &Settings) -> String {
	let resolution = match settings.preview_resolution {
		1 => "Full resolution".to_owned(),
		d => format!("1/{d} resolution"),
	};
	let frames = match settings.preview_frames {
		1 => "every frame".to_owned(),
		2 => "every 2nd frame".to_owned(),
		d => format!("every {d}th frame"),
	};
	format!("{resolution}, {frames}, {}-bit samples", if settings.preview_sample_bytes >= 2 { 16 } else { 8 })
}

/// Applies preset `index`: Low, Medium, High, then Auto from `facts`; saves the settings.
pub(crate) fn preset(settings: &mut Settings, store: Option<&Store>, index: usize, facts: MachineFacts) {
	let tier = PreviewTier::ALL.get(index).copied().unwrap_or_else(|| settings.auto_preset(facts));
	settings.apply_preset(tier);
	save(settings, store);
}

fn save(settings: &Settings, store: Option<&Store>) {
	if let Some(Err(e)) = store.map(|s| settings.save(s)) {
		tracing::error!("{e}");
	}
}

/// The theme as the window's `Theme.mode`: 0 follows the system, 1 light, 2 dark.
pub(crate) fn theme_mode(settings: &Settings) -> i32 {
	match settings.theme {
		Theme::Light => 1,
		Theme::Dark => 2,
		_ => 0,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_choice_row_reads_and_writes_its_setting() {
		let mut s = Settings::default();
		set_row(&mut s, None, 0, 0, "Dark");
		assert_eq!(s.theme, Theme::Dark);
		let rows = rows(&s, 1);
		set_row(&mut s, None, 1, 0, "Half");
		assert_eq!((rows[0].kind, s.preview_resolution), (PropKind::Choice, 2));
	}
}
