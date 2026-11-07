//! The status bar: the newest Info-or-up message on the left, the busy work with its progress, the run's line
//! and the frame/step readout on the right; a double-click opens the Messages pane.

use freya::prelude::*;
use wavedude_widgets::{Progress, StatusBar};

use super::app::Cx;
use super::shared::Topics;
use super::window;

#[derive(Clone, PartialEq)]
struct Right {
	run: String,
	readout: String,
	busy: Option<(String, Progress)>,
	tip: String,
}

fn right(cx: &Cx) -> Right {
	let export = cx.ui.export.read().as_ref().map(|e| (e.text.clone(), e.progress()));
	cx.shared.read(Topics::PLAYBACK | Topics::RUN | Topics::SETTINGS, |a| {
		let s = &a.strings;
		let r = a.live.playback.readout();
		let busy = export.or_else(|| {
			a.live.computing_frame().map(|f| (s.format("StatusComputingFormat", &[&f]), Progress::Indeterminate))
		});
		let busy = busy.or_else(|| {
			a.live
				.run_status()
				.filter(|(state, ..)| !state.is_finished())
				.map(|(_, done, total, _)| (String::new(), Progress::Determinate(done as f32 / total as f32)))
		});
		Right {
			run: a.run_status(),
			readout: format!("{}  {} {}", r.frames_text(), s.get("LabelPreviewStep"), r.step),
			busy,
			tip: s.get("TipStatusBar").to_owned(),
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
		let text = if r.run.is_empty() { r.readout } else { format!("{}    {}", r.run, r.readout) };
		let ui = cx.ui;
		StatusBar::new(line)
			.right(text)
			.busy(r.busy)
			.tooltip(r.tip)
			.on_double_click(move |_| window::show_messages(&ui))
	}
}
