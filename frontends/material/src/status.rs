//! The status line (DESIGN.md, Run bar, timeline, status): the busy item (a download, the export or the frame being
//! computed), else the newest Info-or-up log line, tinted for a warning or an error, and the run's status; the
//! frame cache's RAM. A click opens Messages.

use egui_mineral::layout::{Elide, Fraction};
use egui_mineral::semantic::KeySalt;
use egui_mineral::tone::Tone;
use egui_mineral_material::prelude::{MaterialExt as _, StatusBar, StatusItem, StatusProgress, icons};
use tracing::Level;
use waveduderust_app::log::LogRing;
use waveduderust_app::status::CacheLine;

use crate::{App, Mui};

/// The clickable parts of the line; each opens the messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, KeySalt)]
enum Part {
	Line,
	Memory,
	Run,
}

/// `run` shows the run's status, which the top bar shows otherwise; `export` is the export's status
/// ([`waveduderust_app::export::session::Exports::status`]). Whether the line was clicked.
pub(crate) fn show(
	ui: &mut Mui<'_>,
	app: &App,
	log: &LogRing,
	cache: &CacheLine,
	run: bool,
	export: Option<(&str, Option<f32>)>,
) -> bool {
	let s = &app.strings;
	let color = ui.tokens().color;
	let line = log.status();
	let busy = waveduderust_app::status::busy(app, export);
	// a busy item replaces the log line and hides the run's status, stale while it works
	let show_run = run && busy.is_none();
	let first = match busy {
		Some((text, share)) => {
			let item = StatusItem::new(text).progress(StatusProgress::Linear(share.map(Fraction::new)));
			match export.is_some() {
				true => item.tooltip(s.get("TipExportCancel")),
				false => item,
			}
		}
		None => {
			let item = StatusItem::new(line.as_ref().map_or_else(|| s.get("Ready").to_owned(), |l| l.message.clone()));
			match line.map(|l| l.level) {
				Some(Level::ERROR) => item.tone(Tone::Error),
				Some(Level::WARN) => item
					.tone(Tone::Custom { container: color.tertiary_container, content: color.on_tertiary_container }),
				_ => item,
			}
		}
	};
	let mut bar = StatusBar::new("status", s.get("PaneMessages")).action(Part::Line, first.elide(Elide::End));
	if cache.shown() {
		bar = bar.trailing_action(
			Part::Memory,
			StatusItem::new(&cache.text).tabular_figures(true).icon(icons::ICON_MEMORY).tooltip(&cache.tip),
		);
	}
	let run = app.run_status();
	if show_run && !run.is_empty() {
		bar = bar.trailing_action(Part::Run, StatusItem::new(run).tabular_figures(true));
	}
	bar.show(ui).clicked.is_some()
}
