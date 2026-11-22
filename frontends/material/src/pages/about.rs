//! The About page (docs/ui/70, About): name, version, credits, the licence notice, Cisco's OpenH264 notice and the
//! system lines with Copy.

use eframe::egui::{self, Align, Layout, ScrollArea, vec2};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use egui_mineral_material::prelude::{ButtonVariant, Card, CardVariant, PushButton, TypeScale, icons};
use waveduderust_app::about::{COPYRIGHT, CREDITS, ICON, LICENSE};

use waveduderust_app::strings::{Strings, strip_mnemonic};

use crate::shell::Intent;
use crate::{App, Mui, form};

const ICON_SIZE: f32 = 64.;
const GAP: f32 = 16.;
/// The most a licence text takes before it scrolls.
const TEXT_HEIGHT: f32 = 260.;

/// What the page keeps between passes: which licence texts show.
#[derive(Default)]
pub(super) struct State {
	license: bool,
	#[cfg(not(target_arch = "wasm32"))]
	openh264: bool,
	markdown: CommonMarkCache,
}

pub(super) fn show(ui: &mut Mui<'_>, app: &App, system: &str, state: &mut State, out: &mut Vec<Intent>) {
	let s = &app.strings;
	let rt = ui.rt();
	ui.with_layout(Layout::left_to_right(Align::Min).with_main_wrap(true), |ui| {
		let icon = egui::Image::from_bytes("bytes://about-icon.png", ICON);
		ui.add(icon.fit_to_exact_size(vec2(ICON_SIZE, ICON_SIZE)));
		ui.vertical(|ui| {
			let ui = &mut rt.ui(ui);
			form::text(ui, s.get("Title"), TypeScale::HeadlineSmall, false);
			form::text(ui, &s.format("AboutVersionFormat", &[&env!("CARGO_PKG_VERSION")]), TypeScale::BodyLarge, false);
			form::text(ui, s.get("AboutTagline"), TypeScale::BodyMedium, true);
			form::text(ui, COPYRIGHT, TypeScale::BodyMedium, true);
		});
	});
	ui.add_space(GAP);
	section(ui, "about-credits", s.get("AboutCredits"), |ui| {
		for credit in CREDITS {
			form::text(ui, credit, TypeScale::BodyMedium, false);
		}
	});
	section(ui, "about-license", s.get("AboutLicense"), |ui| {
		form::text(ui, s.get("AboutLicenseNotice"), TypeScale::BodyMedium, false);
		if toggle(ui, "about-license-toggle", s, &mut state.license, "AboutShowLicense", "AboutHideLicense") {
			text_box(ui, "about-license-text", |ui| {
				CommonMarkViewer::new().show(ui, &mut state.markdown, LICENSE);
			});
		}
	});
	#[cfg(not(target_arch = "wasm32"))]
	if !waveduderust::unstable::export::h264::BUILT_IN {
		section(ui, "about-openh264", "OpenH264", |ui| {
			form::text(ui, waveduderust::unstable::export::h264::NOTICE, TypeScale::BodyMedium, false);
			let on = &mut state.openh264;
			if toggle(ui, "about-openh264-toggle", s, on, "AboutShowOpenH264License", "AboutHideOpenH264License") {
				text_box(ui, "about-openh264-text", |ui| {
					ui.add(egui::Label::new(waveduderust::unstable::export::h264::LICENSE).wrap());
				});
			}
		});
	}
	section(ui, "about-system", s.get("AboutSystem"), |ui| {
		form::text(ui, system, TypeScale::BodySmall, false);
		ui.add_space(GAP / 2.);
		let copy = PushButton::new("about-copy", s.get("AboutCopy"))
			.icon(icons::ICON_CONTENT_COPY)
			.variant(ButtonVariant::Tonal);
		if ui.mineral(copy).clicked() {
			out.push(Intent::Copy { text: system.to_owned(), note: "AboutCopied" });
		}
	});
}

/// A titled card over the page's width.
fn section(ui: &mut Mui<'_>, id: &'static str, title: &str, content: impl FnOnce(&mut Mui<'_>)) {
	Card::new(id, title).variant(CardVariant::Outlined).show(ui, |ui| {
		let w = ui.available_width();
		ui.set_width(w);
		form::heading(ui, title);
		content(ui);
	});
	ui.add_space(GAP);
}

/// A text button flipping `on`, named by `show` or `hide`; whether the text shows.
fn toggle(ui: &mut Mui<'_>, id: &'static str, s: &Strings, on: &mut bool, show: &str, hide: &str) -> bool {
	let button = PushButton::new(id, strip_mnemonic(s.get(if *on { hide } else { show }))).variant(ButtonVariant::Text);
	if ui.mineral(button).clicked() {
		*on = !*on;
	}
	*on
}

/// A scrolling box for a long text, as wide as the page.
fn text_box(ui: &mut Mui<'_>, id: &'static str, content: impl FnOnce(&mut egui::Ui)) {
	ScrollArea::vertical().id_salt(id).max_height(TEXT_HEIGHT).auto_shrink([false, true]).show(ui, content);
}
