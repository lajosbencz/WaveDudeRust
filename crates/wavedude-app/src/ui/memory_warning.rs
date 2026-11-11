//! The memory warning (a run's frames exceed the RAM cache): the need and the budget, four levers pre-ticked
//! by the model's proposal, a live line with the ticks' result, and "Do not warn me again".

use freya::prelude::*;
use wavedude_core::run::Levers;
use wavedude_widgets::use_wave_theme;

use wavedude_app_state::Event as AppEvent;

use super::app::Cx;
use super::dialogs::{button, button_row, frame};
use super::properties::check_box;

/// Bytes as "x.y GiB" or "x MiB", with a decimal point.
pub fn bytes_text(bytes: u64) -> String {
	const GIB: f64 = 1024. * 1024. * 1024.;
	let b = bytes as f64;
	if b >= GIB { format!("{:.1} GiB", b / GIB) } else { format!("{:.0} MiB", b / (1024. * 1024.)) }
}

#[derive(Clone, PartialEq)]
struct Facts {
	enabled: Levers,
	proposal: Levers,
	need: u64,
	budget: u64,
	auto_budget: u64,
}

fn facts(cx: &Cx) -> Option<Facts> {
	cx.shared.peek(|a| {
		a.live.warning().map(|w| Facts {
			enabled: w.enabled(),
			proposal: w.proposal(),
			need: w.bytes_of(Levers::default()),
			budget: w.budget,
			auto_budget: w.auto_budget,
		})
	})
}

/// The memory warning.
#[derive(PartialEq)]
pub struct MemoryWarningDialog;

impl Component for MemoryWarningDialog {
	fn render(&self) -> impl IntoElement {
		let cx = Cx::get();
		let theme = use_wave_theme();
		let s = cx.strings();
		let facts = use_hook({
			let cx = cx.clone();
			move || facts(&cx)
		});
		let initial = facts.as_ref().map_or_else(Levers::default, |f| f.proposal);
		let levers = use_state(move || initial);
		let mute = use_state(|| false);
		// the live controller dropped the warning before it showed (a new project, an export)
		let gone = facts.is_none();
		let ui = cx.ui;
		use_side_effect(move || {
			if gone {
				ui.close_modal();
			}
		});
		let Some(f) = facts else {
			return rect().into_element();
		};
		let l = *levers.read();
		let (result_bytes, fits, spill) =
			cx.shared.peek(|a| a.live.warning().map_or((0, true, 0), |w| (w.bytes_of(l), w.fits(l), w.spill_of(l))));
		let line = if fits {
			s.format("MemoryWarningFitsFormat", &[&bytes_text(result_bytes)])
		} else {
			s.format("MemoryWarningSpillsFormat", &[&bytes_text(result_bytes), &bytes_text(spill)])
		};
		let lever = |text: String, on: bool, enabled: bool, set: fn(&mut Levers, bool)| {
			if enabled {
				check_box(text, on, move |v| {
					let mut levers = levers;
					set(&mut levers.write(), v);
				})
				.into_element()
			} else {
				rect().opacity(0.45).child(check_box(text, on, |_| {})).into_element()
			}
		};
		let run = {
			let cx = cx.clone();
			move |_: Event<PressEventData>| {
				cx.ui.close_modal();
				cx.send(AppEvent::MemoryWarningRun { levers: *levers.peek(), mute: *mute.peek() });
			}
		};
		let cancel = {
			let cx = cx.clone();
			move || {
				cx.ui.close_modal();
				cx.send(AppEvent::MemoryWarningCancel);
			}
		};
		let cancel_key = cancel.clone();
		frame(520.)
			.on_global_key_down(move |e: Event<KeyboardEventData>| {
				if e.key == Key::Named(NamedKey::Escape) {
					e.stop_propagation();
					cancel_key();
				}
			})
			.child(
				paragraph().width(Size::fill()).span(Span::new(
					s.format("MemoryWarningTextFormat", &[&bytes_text(f.need), &bytes_text(f.budget)]),
				)),
			)
			.child(label().text(s.get("MemoryWarningChoices").to_owned()))
			.child(lever(
				s.format("MemoryWarningCacheFormat", &[&bytes_text(f.auto_budget)]),
				l.cache_auto,
				f.enabled.cache_auto,
				|l, v| l.cache_auto = v,
			))
			.child(lever(s.get("MemoryWarningResolution").into(), l.resolution, f.enabled.resolution, |l, v| {
				l.resolution = v;
			}))
			.child(lever(s.get("MemoryWarningFrames").into(), l.frames, f.enabled.frames, |l, v| l.frames = v))
			.child(lever(s.get("MemoryWarningSamples").into(), l.samples, f.enabled.samples, |l, v| l.samples = v))
			.child(paragraph().width(Size::fill()).color(theme.text_dim).span(Span::new(line)))
			.child(check_box(s.get("MemoryWarningMute").into(), *mute.read(), move |v| {
				let mut mute = mute;
				mute.set(v);
			}))
			.child(button_row(vec![
				button(s.get("DialogRun"), true, run),
				button(s.get("DialogCancel"), true, move |_| cancel()),
			]))
			.into_element()
	}
}
