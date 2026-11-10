//! The About dialog (docs/ui/70): name, version, credits, the license notice and text, Cisco's OpenH264 notice and
//! license (unless OpenH264 is compiled in), the system lines.

use freya::markdown::MarkdownViewerThemePartialExt as Md;
use freya::prelude::*;
use wavedude_export::h264;
use wavedude_widgets::{Theme, Tip, use_wave_theme};

use super::app::{Cx, Ui};
use super::dialogs::{button, button_row, frame};

const LICENSE: &str = include_str!("../../../../LICENSE.md");
const ICON: &[u8] = include_bytes!("../../assets/icon.png");
/// The copyright notice of this program (GPL-3.0 section 5 d); names are not translated.
const COPYRIGHT: &str = "Copyright (C) 2026 Lajos Bencz";
/// Names and works as the source headers give them; English in every language.
const CREDITS: [&str; 3] = [
	"Alexander Graef, author of WaveDude Next, Copyright (C) 2026",
	"Jeroen Vleggaar, Huygens Optics, author of Wave Dude 2D, Copyright (C) 2026",
	"0x23 (Diffraction Limited), author of WaveSimulator2D",
];

/// Markdown in the app's theme.
pub fn markdown(text: impl Into<std::borrow::Cow<'static, str>>, theme: &Theme) -> MarkdownViewer {
	let v = MarkdownViewer::new(text).width(Size::fill());
	let v = Md::color(v, theme.text);
	let v = Md::color_link(v, theme.accent);
	let v = Md::color_code(v, theme.text);
	let v = Md::background_code(v, theme.control_background);
	let v = Md::background_blockquote(v, theme.control_background);
	let v = Md::border_blockquote(v, theme.border);
	let v = Md::background_divider(v, theme.hairline);
	let v = Md::heading_h1(v, theme.font_size * 2.);
	let v = Md::heading_h2(v, theme.font_size * 1.6);
	let v = Md::heading_h3(v, theme.font_size * 1.3);
	let v = Md::heading_h4(v, theme.font_size * 1.15);
	let v = Md::paragraph_size(v, theme.font_size);
	Md::code_font_size(v, theme.font_size)
}

/// A license text in a scrolled box.
fn license_view(text: impl IntoElement) -> ScrollView {
	ScrollView::new()
		.width(Size::fill())
		.height(Size::px(260.))
		.child(rect().padding(4.).width(Size::fill()).child(text))
}

/// The MP4 encoder as the export would load it.
fn h264_encoder(s: &wavedude_app_state::strings::Strings, choice: Option<bool>) -> String {
	if h264::BUILT_IN {
		s.get("AboutH264BuiltIn").to_owned()
	} else if super::codec::enabled(choice) {
		format!("Cisco's OpenH264 {}", h264::VERSION)
	} else {
		s.get("AboutH264Off").to_owned()
	}
}

/// The system lines of the bug-report box.
pub fn system_lines(cx: &Cx) -> String {
	let s = cx.strings();
	let (gpu, openh264, engine) =
		cx.shared.peek(|a| (a.settings.use_gpu, a.settings.use_openh264, a.live.engine_name().to_string()));
	let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
	[
		format!("{}: {} {}", s.get("AboutProductLine"), s.get("Title"), env!("CARGO_PKG_VERSION")),
		format!("{}: Rust, Freya", s.get("AboutRuntime")),
		format!("{}: {} {}", s.get("AboutOperatingSystem"), std::env::consts::OS, std::env::consts::ARCH),
		format!("{}: {cores} {}", s.get("AboutProcessor"), s.get("AboutCores")),
		format!("{}: {}", s.get("AboutGpuSetting"), s.get(if gpu { "AboutOn" } else { "AboutOff" })),
		format!("{}: {engine}", s.get("AboutEngine")),
		format!("{}: {}", s.get("AboutH264Encoder"), h264_encoder(&s, openh264)),
	]
	.join("\n")
}

/// The About dialog.
#[derive(PartialEq)]
pub struct AboutDialog;

impl Component for AboutDialog {
	fn render(&self) -> impl IntoElement {
		let cx = Cx::get();
		let theme = use_wave_theme();
		let s = cx.strings();
		let mut shown = use_state(|| false);
		let mut cisco = use_state(|| false);
		let ui: Ui = cx.ui;
		let lines = system_lines(&cx);
		let copy = {
			let lines = lines.clone();
			move |_: Event<PressEventData>| {
				if Clipboard::set(lines.clone()).is_ok() {
					tracing::info!("{}", s.get("AboutCopied"));
				}
			}
		};
		let heading = |t: &str| label().text(t.to_owned()).font_weight(FontWeight::BOLD);
		let mut body = frame(600.)
			.on_global_key_down(move |e: Event<KeyboardEventData>| {
				if matches!(e.key, Key::Named(NamedKey::Escape | NamedKey::Enter)) {
					e.stop_propagation();
					ui.close_modal();
				}
			})
			.child(
				rect()
					.horizontal()
					.spacing(12.)
					.cross_align(Alignment::center())
					.child(ImageViewer::new(("about-icon", ICON)).width(Size::px(64.)).height(Size::px(64.)))
					.child(
						rect()
							.spacing(2.)
							.child(
								label()
									.text(s.get("Title").to_owned())
									.font_size(theme.font_size * 1.6)
									.font_weight(FontWeight::BOLD),
							)
							.child(label().text(s.format("AboutVersionFormat", &[&env!("CARGO_PKG_VERSION")])))
							.child(label().text(s.get("AboutTagline").to_owned()).color(theme.text_dim))
							.child(label().text(COPYRIGHT)),
					),
			)
			.child(heading(s.get("AboutCredits")))
			.child(rect().spacing(2.).children(CREDITS.map(|c| label().text(c).into_element())))
			.child(heading(s.get("AboutLicense")))
			.child(paragraph().width(Size::fill()).span(Span::new(s.get("AboutLicenseNotice").to_owned())))
			.child(button(
				s.get(if *shown.read() { "AboutHideLicense" } else { "AboutShowLicense" }),
				true,
				move |_| {
					let v = !*shown.peek();
					shown.set(v);
				},
			));
		if *shown.read() {
			body = body.child(license_view(markdown(LICENSE, &theme)));
		}
		if !h264::BUILT_IN {
			body = body.child(heading("OpenH264")).child(label().text(h264::NOTICE)).child(button(
				s.get(if *cisco.read() { "AboutHideOpenH264License" } else { "AboutShowOpenH264License" }),
				true,
				move |_| {
					let v = !*cisco.peek();
					cisco.set(v);
				},
			));
			if *cisco.read() {
				body = body.child(license_view(paragraph().width(Size::fill()).span(Span::new(h264::LICENSE))));
			}
		}
		body.child(heading(s.get("AboutSystem")))
			.child(
				rect()
					.width(Size::fill())
					.padding(6.)
					.background(theme.input_background)
					.border(Border::new().fill(theme.input_border).width(1.).alignment(BorderAlignment::Inner))
					.child(paragraph().width(Size::fill()).span(Span::new(lines))),
			)
			.child(button_row(vec![
				Tip::new(s.get("TipAboutCopy")).child(button(s.get("AboutCopy"), true, copy)).into_element(),
				button(s.get("AboutClose"), true, move |_| ui.close_modal()),
			]))
			.into_element()
	}
}
