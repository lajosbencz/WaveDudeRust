//! The status bar: the newest Info-or-up message on the left, the run's line (while a run exists) and the busy work
//! with its progress on the right; a double-click opens the Messages pane.

use freya::prelude::*;
use wavedude_widgets::{Progress, StatusBar};

use super::app::Cx;
use super::shared::Topics;
use super::window;

#[derive(Clone, PartialEq)]
struct Right {
	run: String,
	busy: Option<(String, Progress)>,
	tip: String,
	busy_tip: Option<String>,
}

fn right(cx: &Cx) -> Right {
	let download = cx.ui.download.read().clone();
	let exporting = cx.ui.export.read().is_some();
	let export = download.or_else(|| cx.ui.export.read().as_ref().map(|e| (e.text.clone(), e.progress())));
	let topics = Topics::PLAYBACK | Topics::PROGRESS | Topics::WORK | Topics::RUN | Topics::SETTINGS;
	cx.shared.read(topics, |a| {
		let s = &a.strings;
		let busy = export.or_else(|| {
			a.live.computing_frame().map(|f| (s.format("StatusComputingFormat", &[&f]), Progress::Indeterminate))
		});
		Right {
			run: a.run_status(),
			busy,
			tip: s.get("TipStatusBar").to_owned(),
			busy_tip: exporting.then(|| s.get("TipExportCancel").to_owned()),
		}
	})
}

/// The status bar under the dock host.
#[derive(PartialEq)]
pub struct StatusArea;

impl Component for StatusArea {
	fn render(&self) -> impl IntoElement {
		let cx = Cx::get();
		let r = {
			let cx = cx.clone();
			use_memo(move || right(&cx))
		};
		let r = r.read().clone();
		let line = cx.ui.status.read().clone();
		let ui = cx.ui;
		StatusBar::new(line)
			.right(r.run)
			.busy(r.busy)
			.tooltip(r.tip)
			.busy_tooltip(r.busy_tip)
			.on_double_click(move |()| window::show_messages(&ui))
	}
}
