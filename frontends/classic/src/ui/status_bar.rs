//! The status bar: the newest Info-or-up log line on the left (a warning or an error tones the bar), the run's line
//! while a run exists, the busy work with its progress and the frame cache on the right. A double-click asks for the
//! Messages pane.

use eframe::egui;
use egui_mineral::layout::{Elide, Fraction};
use egui_mineral::status::{StatusItem, StatusProgress};
use egui_mineral::tone::Tone;
use tracing::Level;
use waveduderust_app::App;
use waveduderust_app::log::LogRing;
use waveduderust_app::status::CacheLine;

use crate::kit::Palette;
use crate::kit::status::{self, StatusBar};

/// What the bar was asked for.
#[derive(Default)]
pub(super) struct Asked {
	/// A double-click: the Messages pane.
	pub messages: bool,
	/// The finished export's folder.
	#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
	pub reveal: bool,
}

/// The bar at the window's bottom, `export` the running export's text and share done, `reveal` whether it offers the
/// finished export's folder.
pub(super) fn show(
	ui: &mut egui::Ui,
	app: &App,
	log: &LogRing,
	export: Option<(&str, Option<f32>)>,
	cache: &CacheLine,
	reveal: bool,
) -> Asked {
	let s = &app.strings;
	let palette = Palette::of(ui.ctx().theme());
	let line = log.status();
	let tone = match line.as_ref().map(|l| l.level) {
		Some(Level::ERROR) => Tone::Error,
		Some(Level::WARN) => status::warning(palette),
		_ => Tone::Default,
	};
	let text = line.map_or_else(|| s.get("Ready").to_owned(), |l| l.message);
	let mut bar = StatusBar::new("status_bar")
		.tone(tone)
		.grip(cfg!(not(target_arch = "wasm32")))
		.item(StatusItem::new(text).tooltip(s.get("TipStatusBar")).elide(Elide::End));
	if cache.shown() {
		bar = bar.trailing(StatusItem::new(&cache.text).tooltip(&cache.tip));
	}
	if let Some((busy, share)) = waveduderust_app::status::busy(app, export) {
		let item = StatusItem::new(busy).progress(StatusProgress::Linear(share.map(Fraction::new)));
		bar = bar.trailing(if export.is_some() { item.tooltip(s.get("TipExportCancel")) } else { item });
	}
	if reveal {
		bar = bar.trailing_action((), StatusItem::new(s.get("ShowInFolder")));
	}
	let run = app.run_status();
	if !run.is_empty() {
		bar = bar.trailing(StatusItem::new(run));
	}
	let shown = bar.show(ui);
	Asked { messages: shown.response.double_clicked(), reveal: shown.clicked.is_some() }
}
