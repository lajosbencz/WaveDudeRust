//! The About dialog (docs/ui/70): name, version, credits, the license notice and text, Cisco's `OpenH264` notice and
//! license where MP4 export downloads it, and the system lines for a bug report.

use eframe::egui::{self, Frame, RichText, ScrollArea, Stroke, Ui, vec2};
use egui_commonmark::CommonMarkCache;
use wavedude_app_state::strings::Strings;

use super::markdown::{self, Piece, pieces};
use super::modal::{Choice, Reply, buttons, mnemonic_pressed};
use super::theme::{FONT_SIZE, Palette};
use crate::App;

const WIDTH: f32 = 600.;
const LICENSE: &str = include_str!("../../../../LICENSE.md");
const ICON: &[u8] = include_bytes!("../../assets/icons/64x64.png");
/// The copyright notice of this program (GPL-3.0 section 5 d); names are not translated.
const COPYRIGHT: &str = "Copyright (C) 2026 Lajos Bencz";
/// Names and works as the source headers give them; English in every language.
const CREDITS: [&str; 3] = [
	"Alexander Graef, author of WaveDude Next, Copyright (C) 2026",
	"Jeroen Vleggaar, Huygens Optics, author of Wave Dude 2D, Copyright (C) 2026",
	"0x23 (Diffraction Limited), author of WaveSimulator2D",
];
const LICENSE_HEIGHT: f32 = 260.;
const SYSTEM_HEIGHT: f32 = 160.;

/// The MP4 encoder as the export would load it.
#[cfg(not(target_arch = "wasm32"))]
fn h264_encoder(s: &Strings, choice: Option<bool>) -> String {
	use wavedude_export::h264;
	if h264::BUILT_IN {
		s.get("AboutH264BuiltIn").to_owned()
	} else if crate::codec::enabled(choice) {
		format!("Cisco's OpenH264 {}", h264::VERSION)
	} else {
		s.get("AboutH264Off").to_owned()
	}
}

/// The system lines of the bug-report box.
fn system_lines(app: &App) -> String {
	let s = &app.strings;
	let threads = wavedude_app_state::platform::machine_facts().threads;
	let gpu = s.get(if app.settings.use_gpu { "AboutOn" } else { "AboutOff" });
	#[cfg(not(target_arch = "wasm32"))]
	let h264 = Some(format!("{}: {}", s.get("AboutH264Encoder"), h264_encoder(s, app.settings.use_openh264)));
	#[cfg(target_arch = "wasm32")]
	let h264 = None;
	let lines = [
		format!("{}: {} {}", s.get("AboutProductLine"), s.get("Title"), env!("CARGO_PKG_VERSION")),
		format!("{}: Rust, egui", s.get("AboutRuntime")),
		format!("{}: {} {}", s.get("AboutOperatingSystem"), std::env::consts::OS, std::env::consts::ARCH),
		format!("{}: {threads} {}", s.get("AboutProcessor"), s.get("AboutCores")),
		format!("{}: {gpu}", s.get("AboutGpuSetting")),
		format!("{}: {}", s.get("AboutEngine"), app.live.engine().text(s)),
	];
	lines.into_iter().chain(h264).collect::<Vec<_>>().join("\n")
}

/// The dialog's state.
pub(super) struct About {
	system: String,
	/// The license's text, while it shows.
	license: Option<Vec<Piece>>,
	#[cfg(not(target_arch = "wasm32"))]
	openh264: bool,
	markdown: CommonMarkCache,
}

impl About {
	pub(super) fn new(app: &App) -> Self {
		Self {
			system: system_lines(app),
			license: None,
			#[cfg(not(target_arch = "wasm32"))]
			openh264: false,
			markdown: CommonMarkCache::default(),
		}
	}

	pub(super) fn ui(&mut self, ui: &mut Ui, fresh: bool, app: &App) -> Option<Reply> {
		let s = &app.strings;
		let palette = Palette::of(ui.ctx().theme());
		ui.set_width(WIDTH);
		ui.horizontal(|ui| {
			ui.add(egui::Image::from_bytes("bytes://about-icon.png", ICON).fit_to_exact_size(vec2(64., 64.)));
			ui.vertical(|ui| {
				ui.spacing_mut().item_spacing.y = 2.;
				ui.label(RichText::new(s.get("Title")).size(FONT_SIZE * 1.6).family(wavedude_view::fonts::bold()));
				ui.label(s.format("AboutVersionFormat", &[&env!("CARGO_PKG_VERSION")]));
				ui.label(RichText::new(s.get("AboutTagline")).color(palette.text_dim));
				ui.label(COPYRIGHT);
			});
		});
		heading(ui, s.get("AboutCredits"));
		ui.vertical(|ui| {
			ui.spacing_mut().item_spacing.y = 2.;
			for credit in CREDITS {
				ui.label(credit);
			}
		});
		heading(ui, s.get("AboutLicense"));
		ui.add(egui::Label::new(s.get("AboutLicenseNotice")).wrap());
		let mut shown = self.license.is_some();
		if toggle(ui, s, &mut shown, "AboutShowLicense", "AboutHideLicense") {
			let (cache, license) = (&mut self.markdown, self.license.get_or_insert_with(|| pieces(LICENSE)));
			text_box(ui, palette, "AboutLicenseText", LICENSE_HEIGHT, |ui| markdown::show(ui, cache, license));
		} else {
			self.license = None;
		}
		#[cfg(not(target_arch = "wasm32"))]
		if !wavedude_export::h264::BUILT_IN {
			heading(ui, "OpenH264");
			ui.label(wavedude_export::h264::NOTICE);
			if toggle(ui, s, &mut self.openh264, "AboutShowOpenH264License", "AboutHideOpenH264License") {
				text_box(ui, palette, "AboutOpenH264Text", LICENSE_HEIGHT, |ui| {
					ui.add(egui::Label::new(wavedude_export::h264::LICENSE).wrap());
				});
			}
		}
		heading(ui, s.get("AboutSystem"));
		text_box(ui, palette, "AboutSystemText", SYSTEM_HEIGHT, |ui| {
			ui.add(egui::Label::new(self.system.as_str()).wrap());
		});
		let copy = Choice { tip: Some(s.get("TipAboutCopy")), ..Choice::plain(s.get("AboutCopy")) };
		let choices = [copy, Choice::both(s.get("AboutClose"))];
		match buttons(ui, fresh, &choices)? {
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
	ui.label(RichText::new(text).family(wavedude_view::fonts::bold()));
}

/// A button that shows (`on`) or hides a text, named by `show` or `hide`, pressed by its mnemonic too; whether the
/// text shows.
fn toggle(ui: &mut Ui, s: &Strings, on: &mut bool, show: &str, hide: &str) -> bool {
	let text = s.get(if *on { hide } else { show });
	if super::modal::button(ui, text, true).clicked() || mnemonic_pressed(ui, text) {
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
