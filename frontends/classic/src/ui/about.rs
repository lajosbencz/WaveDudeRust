//! The About dialog (docs/ui/70): name, version, credits with a link to WaveDude Next, the license notice and text,
//! Cisco's `OpenH264` notice and license where MP4 export downloads it, and the system lines for a bug report.

use eframe::egui::{self, Frame, RichText, ScrollArea, Stroke, Ui, vec2};
use egui_commonmark::CommonMarkCache;
use waveduderust_app::App;
use waveduderust_app::about::{COPYRIGHT, CREDITS, ICON, LICENSE, NEXT_REPOSITORY, system_lines};
use waveduderust_app::platform::{Opened, open};
use waveduderust_app::strings::Strings;

use super::markdown::{self, Piece, pieces};
use super::modal::Answer;
use crate::kit::dialog::{self, Choice, buttons, mnemonic_pressed};
use crate::kit::{FONT_SIZE, Palette};

pub(super) const WIDTH: f32 = 600.;
const LICENSE_HEIGHT: f32 = 260.;
/// The share of the window's height a license box takes at most, so the buttons stay on a short screen.
const LICENSE_SHARE: f32 = 0.3;
const SYSTEM_HEIGHT: f32 = 160.;
const LICENSE_LEAST: f32 = 120.;

/// The dialog's state.
pub(super) struct About {
	system: String,
	/// The license's text, while it shows.
	license: Option<Vec<Piece>>,
	/// Why the last link could not be opened.
	link_error: Option<String>,
	#[cfg(not(target_arch = "wasm32"))]
	openh264: bool,
	markdown: CommonMarkCache,
	/// What a license box gives up so the window fits the screen.
	shrink: f32,
}

impl About {
	pub(super) fn new(app: &App) -> Self {
		Self {
			system: system_lines(app),
			license: None,
			link_error: None,
			#[cfg(not(target_arch = "wasm32"))]
			openh264: false,
			markdown: CommonMarkCache::default(),
			shrink: 0.,
		}
	}

	pub(super) fn ui(&mut self, ui: &mut Ui, fresh: bool, app: &App) -> Option<Answer> {
		let s = &app.strings;
		let palette = Palette::of(ui.ctx().theme());
		let screen = ui.ctx().content_rect();
		let license_height = (LICENSE_HEIGHT.min(screen.height() * LICENSE_SHARE) - self.shrink).max(LICENSE_LEAST);
		ui.horizontal(|ui| {
			ui.add(egui::Image::from_bytes("bytes://about-icon.png", ICON).fit_to_exact_size(vec2(64., 64.)));
			ui.vertical(|ui| {
				ui.spacing_mut().item_spacing.y = 2.;
				ui.label(
					RichText::new(s.get("Title"))
						.size(FONT_SIZE * 1.6)
						.family(waveduderust::unstable::view::fonts::bold()),
				);
				ui.label(s.format("AboutVersionFormat", &[&env!("CARGO_PKG_VERSION")]));
				ui.label(RichText::new(s.get("AboutTagline")).color(palette.text_dim));
				ui.label(COPYRIGHT);
			});
		});
		heading(ui, s.get("AboutCredits"));
		ui.vertical(|ui| {
			ui.spacing_mut().item_spacing.y = 2.;
			let [next, rest @ ..] = CREDITS;
			ui.horizontal(|ui| {
				ui.label(next);
				let link = RichText::new(s.get("AboutNextLink")).small();
				if ui.link(link).on_hover_text(NEXT_REPOSITORY).clicked() {
					self.link_error = open(Opened::Url(NEXT_REPOSITORY)).err().map(|e| {
						tracing::error!("{NEXT_REPOSITORY} could not be opened: {e}");
						format!("{} {e}", s.format("OpenError", &[&NEXT_REPOSITORY]))
					});
				}
			});
			for credit in rest {
				ui.label(credit);
			}
			if let Some(error) = &self.link_error {
				ui.label(RichText::new(error).small().color(palette.level_error));
			}
		});
		heading(ui, s.get("AboutLicense"));
		ui.add(egui::Label::new(s.get("AboutLicenseNotice")).wrap());
		let mut shown = self.license.is_some();
		if toggle(ui, s, &mut shown, "AboutShowLicense", "AboutHideLicense") {
			let (cache, license) = (&mut self.markdown, self.license.get_or_insert_with(|| pieces(LICENSE)));
			text_box(ui, palette, "AboutLicenseText", license_height, |ui| markdown::show(ui, cache, license));
		} else {
			self.license = None;
		}
		#[cfg(not(target_arch = "wasm32"))]
		if !waveduderust::unstable::export::h264::BUILT_IN {
			heading(ui, "OpenH264");
			ui.label(waveduderust::unstable::export::h264::NOTICE);
			if toggle(ui, s, &mut self.openh264, "AboutShowOpenH264License", "AboutHideOpenH264License") {
				text_box(ui, palette, "AboutOpenH264Text", license_height, |ui| {
					ui.add(egui::Label::new(waveduderust::unstable::export::h264::LICENSE).wrap());
				});
			}
		}
		heading(ui, s.get("AboutSystem"));
		text_box(ui, palette, "AboutSystemText", SYSTEM_HEIGHT, |ui| {
			ui.add(egui::Label::new(self.system.as_str()).wrap());
		});
		let copy = Choice { tip: Some(s.get("TipAboutCopy")), ..Choice::plain(s.get("AboutCopy")) };
		let choices = [copy, Choice::both(s.get("AboutClose"))];
		let pressed = buttons(ui, fresh, &choices);
		let bottom = ui.min_rect().bottom() + dialog::PADDING + crate::kit::BORDER;
		self.shrink = (self.shrink + bottom - screen.bottom()).max(0.);
		match pressed? {
			0 => {
				ui.ctx().copy_text(self.system.clone());
				tracing::info!("{}", s.get("AboutCopied"));
				None
			}
			_ => Some(None),
		}
	}
}

fn heading(ui: &mut Ui, text: &str) {
	ui.label(RichText::new(text).family(waveduderust::unstable::view::fonts::bold()));
}

/// A button that shows (`on`) or hides a text, named by `show` or `hide`, pressed by its mnemonic too; whether the
/// text shows.
fn toggle(ui: &mut Ui, s: &Strings, on: &mut bool, show: &str, hide: &str) -> bool {
	let text = s.get(if *on { hide } else { show });
	if dialog::button(ui, text, true).clicked() || mnemonic_pressed(ui, text) {
		*on = !*on;
	}
	*on
}

/// A bordered box of the input colours, scrolled beyond `height`.
fn text_box(ui: &mut Ui, palette: &Palette, id: &str, height: f32, content: impl FnOnce(&mut Ui)) {
	Frame::new().fill(palette.input_background).stroke(Stroke::new(1., palette.border)).inner_margin(6.).show(
		ui,
		|ui| {
			ScrollArea::vertical().id_salt(id).max_height(height).auto_shrink([false, true]).show(ui, content);
		},
	);
}
