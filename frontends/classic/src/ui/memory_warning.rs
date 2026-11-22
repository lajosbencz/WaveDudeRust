//! The memory warning (docs/ui/60) as a dialog of this window; the model is [`waveduderust_app::memory_warning`].

use eframe::egui::{self, Id, RichText, Ui};
use waveduderust_app::App;
use waveduderust_app::memory_warning::{MemoryWarning, usage};
use waveduderust_app::strings::Strings;

use super::modal::{Answer, Then};
use super::properties::check_box;
use crate::kit::Palette;
use crate::kit::dialog::{Choice, Disc, buttons, head};

/// The window 500 wide (MemoryWarningWindow.xaml:24).
pub(super) const WIDTH: f32 = 500. - 2. * crate::kit::dialog::PADDING;

/// The memory `warning` in `ui`; `fresh` on its first frame.
pub(super) fn show(warning: &mut MemoryWarning, ui: &mut Ui, fresh: bool, app: &App) -> Option<Answer> {
	if app.live.warning().is_none() {
		// dropped before it was answered
		return Some(None);
	}
	let s: &Strings = &app.strings;
	let dim = Palette::of(ui.ctx().theme()).text_dim;
	head(ui, Disc::Question, &warning.text);
	ui.label(s.get("MemoryWarningChoices"));
	let (enabled, levers) = (warning.enabled, &mut warning.levers);
	let rows: [(&str, bool, &mut bool); 4] = [
		(&warning.cache, enabled.cache_auto, &mut levers.cache_auto),
		(s.get("MemoryWarningResolution"), enabled.resolution, &mut levers.resolution),
		(s.get("MemoryWarningFrames"), enabled.frames, &mut levers.frames),
		(s.get("MemoryWarningSamples"), enabled.samples, &mut levers.samples),
	];
	let mut changed = false;
	for (i, (text, enabled, on)) in rows.into_iter().enumerate() {
		ui.add_enabled_ui(enabled, |ui| {
			if let Some(v) = check_box(ui, Id::unique("MemoryWarningLever").with(i), text, *on) {
				*on = v;
				changed = true;
			}
		});
	}
	if changed && let Some(result) = warning.result_of(app) {
		warning.result = result;
	}
	if let Some(w) = app.live.warning() {
		usage_bar(ui, w.bytes_of(warning.levers), w.budget_of(warning.levers), &warning.usages);
	}
	ui.add(egui::Label::new(RichText::new(warning.result.as_str()).color(dim)).wrap());
	if let Some(v) = check_box(ui, Id::unique("MemoryWarningMute"), s.get("MemoryWarningMute"), warning.mute) {
		warning.mute = v;
	}
	let choices = [Choice::default(s.get("DialogRun")), Choice::cancel(s.get("DialogCancel"))];
	buttons(ui, fresh, &choices).map(|i| Some(Then::send(warning.answer(i == 0))))
}

/// The frames' `need` against the RAM cache's `budget`: the bar is the budget, full and in the warning colour when
/// the need exceeds it. The label takes the width of the widest of `labels`.
fn usage_bar(ui: &mut Ui, need: u64, budget: u64, labels: &[String]) {
	let palette = Palette::of(ui.ctx().theme());
	let fill = if need <= budget { palette.accent } else { palette.level_warn };
	let share = if budget == 0 { 1. } else { (need as f64 / budget as f64).min(1.) as f32 };
	let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
	let font = egui::TextStyle::Body.resolve(ui.style());
	let width = labels
		.iter()
		.map(|l| ui.painter().layout_no_wrap(l.clone(), font.clone(), palette.text).size().x)
		.fold(0., f32::max);
	ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
		ui.add_sized([width, row.y], egui::Label::new(usage(need, budget)));
		ui.add(egui::ProgressBar::new(share).fill(fill));
	});
}
