//! The startup page: shown at the first start, and while a question it asks is unanswered. The preview quality
//! (four levers, and three tier buttons that set them; pre-set from the machine), Cisco's OpenH264 for MP4
//! export (decision 0005) and the launch's update check (decision 0011). Continue saves every answer.

use freya::prelude::*;
use wavedude_core::engine::SampleFormat;
use wavedude_core::run::{PreviewDefaults, PreviewQuality, PreviewTier};
use wavedude_export::h264;
use wavedude_widgets::use_wave_theme;

use wavedude_app_state::Event as AppEvent;
use wavedude_app_state::strings::{Strings, strip_mnemonic};

use super::app::Cx;
use super::dialogs::{button, button_row, frame};
use super::properties::{check_box, combo_box};
use super::{codec, update};

/// The page has a question to ask.
pub fn due(cx: &Cx) -> bool {
	cx.shared.peek(|a| {
		a.first_start
			|| codec::unasked(a.settings.use_openh264)
			|| (update::AVAILABLE && a.settings.check_for_updates.is_none())
	})
}

const DIVISORS: [(u32, &str); 3] = [(1, "Full"), (2, "Half"), (4, "Quarter")];
const DITHER: [(u32, &str); 5] = [(0, "Off"), (10, "10"), (12, "12"), (14, "14"), (16, "16")];

/// A labelled drop-down of `(value, string key)` pairs.
fn lever(
	cx: &Cx,
	s: &Strings,
	label_key: &str,
	current: u32,
	items: Vec<(u32, String)>,
	test: &str,
	pick: impl Fn(u32) + 'static,
) -> Rect {
	let items = items.into_iter().map(|(v, k)| (v.to_string(), strip_mnemonic(s.get(&k)))).collect();
	rect()
		.horizontal()
		.width(Size::fill())
		.cross_align(Alignment::center())
		.child(rect().width(Size::px(180.)).child(label().text(strip_mnemonic(s.get(label_key)))))
		.child(combo_box(cx, current.to_string(), items, test.into(), move |v: String| {
			pick(v.parse().expect("a lever's value is a number"))
		}))
}

/// The startup page.
#[derive(PartialEq)]
pub struct StartupPage;

impl Component for StartupPage {
	fn render(&self) -> impl IntoElement {
		let cx = Cx::get();
		let theme = use_wave_theme();
		let s = cx.strings();
		let settings = cx.shared.peek(|a| a.settings.clone());
		let mut quality = use_state(|| settings.preview_quality());
		let mut dither = use_state(|| settings.preview_dither_bits);
		let openh264 = use_state(|| settings.use_openh264 == Some(true));
		let updates = use_state(|| settings.check_for_updates.unwrap_or(true));
		let q = *quality.read();
		let half = q.format == SampleFormat::Half;

		let heading = |key: &str| label().text(s.get(key).to_owned()).font_weight(FontWeight::BOLD);
		let text = |key: &str| paragraph().width(Size::fill()).span(Span::new(s.get(key).to_owned()));
		let tier = |key: &str, tier: PreviewTier| {
			button(s.get(key), true, move |_| {
				quality.set(PreviewDefaults::quality_of(tier));
				dither.set(PreviewDefaults::dither_bits_of(tier));
			})
		};
		let set = move |f: fn(&mut PreviewQuality, u32)| {
			move |v: u32| {
				let (mut quality, mut dither) = (quality, dither);
				let mut next = *quality.peek();
				f(&mut next, v);
				quality.set(next.validated());
				if next.format == SampleFormat::Index {
					dither.set(0);
				}
			}
		};
		let divisors = |prefix: &str| DIVISORS.iter().map(|&(v, k)| (v, format!("{prefix}{k}"))).collect::<Vec<_>>();

		let mut body = frame(560.)
			.child(
				label()
					.text(s.get("StartupTitle").to_owned())
					.font_weight(FontWeight::BOLD)
					.font_size(theme.font_size * 1.4),
			)
			.child(heading("StartupPreview"))
			.child(text("StartupPreviewText"))
			.child(rect().horizontal().spacing(8.).children([
				tier("StartupTierLow", PreviewTier::Low),
				tier("StartupTierStandard", PreviewTier::Middle),
				tier("StartupTierHigh", PreviewTier::High),
			]))
			.child(lever(
				&cx,
				&s,
				"MenuViewResolution",
				q.resolution_divisor,
				divisors("MenuViewResolution"),
				"StartupResolution",
				set(|q, v| q.resolution_divisor = v),
			))
			.child(lever(
				&cx,
				&s,
				"MenuViewFrames",
				q.frame_divisor,
				divisors("MenuViewFrames"),
				"StartupFrames",
				set(|q, v| q.frame_divisor = v),
			))
			.child(lever(
				&cx,
				&s,
				"MenuViewSamples",
				if half { 2 } else { 1 },
				vec![(2, "MenuViewSamples16".into()), (1, "MenuViewSamples8".into())],
				"StartupSamples",
				set(|q, v| q.format = if v == 2 { SampleFormat::Half } else { SampleFormat::Index }),
			))
			.maybe_child(half.then(|| {
				let items = DITHER.iter().map(|&(v, k)| (v, format!("MenuViewDither{k}"))).collect();
				lever(&cx, &s, "MenuViewDither", *dither.read(), items, "StartupDither", move |v| {
					let mut dither = dither;
					dither.set(v)
				})
			}));
		if !h264::BUILT_IN {
			body = body
				.child(heading("StartupCodec"))
				.child(text("StartupCodecText"))
				.child(label().text(h264::NOTICE).color(theme.text_dim))
				.child(check_box(s.get("ExportUseOpenH264").to_owned(), *openh264.read(), move |on| {
					let mut openh264 = openh264;
					openh264.set(on)
				}));
		}
		if update::AVAILABLE {
			body = body.child(heading("StartupUpdates")).child(check_box(
				s.get("StartupCheckForUpdates").to_owned(),
				*updates.read(),
				move |on| {
					let mut updates = updates;
					updates.set(on)
				},
			));
		}
		let done = {
			let cx = cx.clone();
			move || {
				cx.ui.close_modal();
				cx.send(AppEvent::SetPreviewQuality(*quality.peek()));
				cx.send(AppEvent::SetDitherBits(*dither.peek()));
				if !h264::BUILT_IN {
					codec::set(&cx, *openh264.peek());
				}
				if update::AVAILABLE {
					let on = *updates.peek();
					cx.send(AppEvent::SetCheckForUpdates(on));
					if on {
						update::check(&cx, true);
					}
				}
			}
		};
		let key_done = done.clone();
		body.on_global_key_down(move |e: Event<KeyboardEventData>| {
			if matches!(e.key, Key::Named(NamedKey::Enter | NamedKey::Escape)) {
				e.stop_propagation();
				key_done();
			}
		})
		.child(button_row(vec![button(s.get("StartupContinue"), true, move |_| done())]))
		.into_element()
	}
}
