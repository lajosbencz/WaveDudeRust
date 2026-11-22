//! The gallery (DESIGN.md, Overlay pages): the start strip (New, Open, the recent projects, the preview quality and
//! whether the gallery shows at launch), a search and topic filter, and a card per example that opens it.

use eframe::egui::text::LayoutJob;
use eframe::egui::{Image, Label, Rect, Sense, TextFormat, vec2};
use egui_mineral::widget::SizeExt as _;
use egui_mineral_material::prelude::{
	ButtonVariant, Card, CardVariant, Chip, EmptyPlaceholder, MaterialExt as _, PushButton, SearchBar, Switch,
	TypeScale, icons,
};
use waveduderust_app::menus::MenuAction;
use waveduderust_app::settings_table::{ROWS, Setting, levers_line};
use waveduderust_app::strings::strip_mnemonic;
use waveduderust_app::{Event, Guarded};
use waveduderust_examples::{Topic, all, matching};

use super::Pages;
use super::settings;
use super::thumbs::Thumbs;
use crate::commands::{Cmd, runs};
use crate::shell::Intent;
use crate::{App, Mui, form};

const GAP: f32 = 16.;
/// The width a card asks for.
const CARD_W: f32 = 280.;
const MAX_COLUMNS: usize = 4;
/// The recent projects the strip shows; File > Recent has them all.
const RECENT: usize = 5;
const DESCRIPTION_ROWS: usize = 3;

/// The examples shown: those of the topic filter (`None`: all) holding `query`, and whether the topic had none, so
/// all topics are shown instead.
fn shown(topic: Option<Topic>, query: &str) -> (Vec<usize>, bool) {
	let everywhere = || {
		let mut found: Vec<usize> = Topic::ALL.iter().flat_map(|&t| matching(t, query)).collect();
		found.sort_unstable();
		found
	};
	match topic {
		None => (everywhere(), false),
		Some(t) => {
			let found = matching(t, query);
			if found.is_empty() && !query.trim().is_empty() {
				let all = everywhere();
				let fell_back = !all.is_empty();
				(if fell_back { all } else { found }, fell_back)
			} else {
				(found, false)
			}
		}
	}
}

/// The grid's columns at `width`.
fn columns(width: f32) -> usize {
	((width / CARD_W) as usize).clamp(1, MAX_COLUMNS)
}

pub(super) fn show(ui: &mut Mui<'_>, app: &App, pages: &mut Pages, out: &mut Vec<Intent>) {
	start_strip(ui, app, pages, out);
	ui.add_space(GAP);
	filter(ui, app, pages);
	ui.add_space(GAP);
	let topic = pages.topic.checked_sub(1).map(|i| Topic::ALL[i]);
	let (found, fell_back) = shown(topic, &pages.examples_query);
	let s = &app.strings;
	if fell_back {
		form::text(ui, s.get("GalleryFellBack"), TypeScale::BodyMedium, true);
		ui.add_space(GAP / 2.);
	}
	if found.is_empty() {
		EmptyPlaceholder::new("gallery-none", s.get("ExamplesNone")).icon(icons::ICON_SEARCH_OFF).show(ui);
	}
	pages.thumbs.poll(ui.ctx());
	let n = columns(ui.available_width());
	let rt = ui.rt();
	for chunk in found.chunks(n) {
		ui.columns(n, |cols| {
			for (col, &i) in cols.iter_mut().zip(chunk) {
				if card(&mut rt.ui(col), app, &mut pages.thumbs, i) {
					out.push(Intent::Send(Event::Guarded(Guarded::OpenExample(i))));
				}
			}
		});
		ui.add_space(GAP / 2.);
	}
}

/// New, Open, the recent projects, the quality card and Show at launch.
fn start_strip(ui: &mut Mui<'_>, app: &App, pages: &Pages, out: &mut Vec<Intent>) {
	let s = &app.strings;
	let rt = ui.rt();
	ui.horizontal_wrapped(|ui| {
		let ui = &mut rt.ui(ui);
		let buttons = [
			(MenuAction::New, "MenuFileNew", icons::ICON_ADD, ButtonVariant::Filled),
			(MenuAction::Open, "MenuFileOpen", icons::ICON_FOLDER_OPEN, ButtonVariant::Tonal),
		];
		for (action, key, icon, variant) in buttons {
			let button = PushButton::new(key, strip_mnemonic(s.get(key))).icon(icon).variant(variant);
			if ui.mineral(button.disabled(!runs(app, Cmd::Act(action)))).clicked() {
				out.push(Intent::Act(action));
			}
		}
		let recent = &app.settings.recent_files;
		if !recent.is_empty() {
			ui.add_space(GAP);
			form::text(ui, &strip_mnemonic(s.get("MenuFileRecent")), TypeScale::LabelLarge, true);
		}
		for (i, path) in recent.iter().enumerate().take(RECENT) {
			let name = path.file_name().map_or_default(|n| n.to_string_lossy().into_owned());
			let chip = Chip::assist(("gallery-recent", i), name).icon(icons::ICON_DESCRIPTION);
			#[cfg(not(target_arch = "wasm32"))]
			let tip = format!("{}\n{}", path.display(), s.get("RecentRevealHint"));
			#[cfg(target_arch = "wasm32")]
			let tip = path.display().to_string();
			let response = ui.mineral(chip).into_response().on_hover_text(tip);
			if response.clicked() {
				out.push(Intent::Act(MenuAction::Recent(i)));
			}
			#[cfg(not(target_arch = "wasm32"))]
			if response.secondary_clicked() {
				waveduderust_app::platform::show_in_folder(path);
			}
		}
	});
	ui.add_space(GAP);
	Card::new("gallery-quality", s.get("StartupPreview")).variant(CardVariant::Outlined).show(ui, |ui| {
		let w = ui.available_width();
		ui.set_width(w);
		form::heading(ui, s.get("StartupPreview"));
		for row in ROWS.iter().filter(|r| matches!(r.setting, Setting::Ram | Setting::Preset)) {
			settings::row(ui, app, pages.machine, row, out);
		}
		form::text(ui, &levers_line(s, &app.settings), TypeScale::BodyMedium, true);
	});
	ui.add_space(GAP / 2.);
	let mut show = app.settings.show_startup;
	if ui.mineral(Switch::new("gallery-show", s.get("StartupShowAtLaunch"), &mut show)).changed() {
		out.push(Intent::Send(Event::SetShowStartup(show)));
	}
}

/// The search and the topic chips, All first.
fn filter(ui: &mut Mui<'_>, app: &App, pages: &mut Pages) {
	let s = &app.strings;
	let rt = ui.rt();
	let search = SearchBar::new("gallery-search", s.get("ExamplesSearchHint"), &mut pages.examples_query)
		.placeholder(s.get("ExamplesSearchHint"));
	let w = ui.available_width();
	ui.mineral(search.preferred_width(w));
	ui.add_space(GAP / 2.);
	ui.horizontal_wrapped(|ui| {
		let ui = &mut rt.ui(ui);
		let names = std::iter::once(s.get("GalleryAll")).chain(Topic::ALL.iter().map(|t| s.get(t.title_key())));
		for (i, name) in names.enumerate() {
			// a topic stays picked: a click on the picked one keeps it
			let mut on = pages.topic == i;
			if ui.mineral(Chip::filter(("gallery-topic", i), name, &mut on)).changed() {
				pages.topic = i;
			}
		}
	});
}

/// Example `i`'s card; whether it was activated.
fn card(ui: &mut Mui<'_>, app: &App, thumbs: &mut Thumbs, i: usize) -> bool {
	let example = &all()[i];
	let c = ui.tokens().color;
	let r = Card::new(("example", i), &example.title).variant(CardVariant::Filled).clickable(true).show(ui, |ui| {
		let w = ui.available_width();
		ui.set_width(w);
		let (slot, _) = ui.allocate_exact_size(vec2(w, w * 9. / 16.), Sense::hover());
		ui.painter().rect_filled(slot, 0., c.surface_container);
		if let Some(tex) = thumbs.get(ui.ctx(), i) {
			let size = tex.size_vec2();
			let fitted = size * (slot.width() / size.x).min(slot.height() / size.y);
			let image = Image::from_texture((tex.id(), size)).fit_to_exact_size(fitted);
			image.paint_at(ui, Rect::from_center_size(slot.center(), fitted));
		}
		form::text(ui, &example.title, TypeScale::TitleMedium, false);
		form::text(ui, app.strings.get(example.topic.title_key()), TypeScale::LabelMedium, true);
		if !example.description.is_empty() {
			let format = TextFormat::simple(TypeScale::BodyMedium.font(ui.ctx()), c.on_surface_variant);
			let mut job = LayoutJob::single_section(example.description.clone(), format);
			job.wrap.max_rows = DESCRIPTION_ROWS;
			ui.add(Label::new(job).selectable(false));
		}
	});
	if !example.description.is_empty() {
		r.response.on_hover_text(&example.description);
	}
	r.clicked
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_grid_has_one_to_four_columns() {
		assert_eq!(columns(100.), 1);
		assert_eq!(columns(2. * CARD_W + 1.), 2);
		assert_eq!(columns(10_000.), MAX_COLUMNS);
	}

	#[test]
	fn all_lists_every_example_in_order() {
		let (found, fell_back) = shown(None, "");
		assert_eq!(found, (0..all().len()).collect::<Vec<_>>());
		assert!(!fell_back);
	}

	#[test]
	fn a_search_without_a_hit_in_its_topic_falls_back_to_all() {
		let other = all().iter().find(|e| e.topic != Topic::Sources).expect("examples beyond Sources");
		let (found, fell_back) = shown(Some(Topic::Sources), &other.title);
		assert!(fell_back || all().iter().any(|e| e.topic == Topic::Sources && e.title == other.title));
		assert!(!found.is_empty());
	}

	#[test]
	fn a_search_without_any_hit_shows_none() {
		assert_eq!(shown(Some(Topic::Lenses), "no example has this title"), (Vec::new(), false));
	}
}
