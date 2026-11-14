//! The memory warning (docs/ui/60): a run's frames exceed the RAM cache. The need and the budget, four levers
//! pre-ticked by the model's proposal (greyed where they gain nothing), a bar and a line with the ticks' result, and
//! "Do not warn me again"; Run applies the ticks, Cancel keeps the frames.

use eframe::egui::{self, Id, RichText, Ui};
use wavedude_app_state::Event;
use wavedude_app_state::strings::Strings;
use wavedude_core::run::Levers;

use super::modal::{Choice, Reply, Then, buttons};
use super::properties::check_box;
use super::theme::Palette;
use crate::App;

const WIDTH: f32 = 520.;

/// Bytes as "x.y GiB" or "x MiB", with a decimal point.
pub(super) fn bytes_text(bytes: u64) -> String {
	const MIB: f64 = 1024. * 1024.;
	const GIB: f64 = 1024. * MIB;
	let b = bytes as f64;
	if b >= GIB { format!("{:.1} GiB", b / GIB) } else { format!("{:.0} MiB", b / MIB) }
}

/// The dialog's state: the ticks and the texts of the warning it asks about.
pub(super) struct MemoryWarning {
	levers: Levers,
	enabled: Levers,
	mute: bool,
	text: String,
	cache: String,
	/// What the ticks give, renewed when they change.
	result: String,
	/// The usage bar's label for each choice of ticks open to the user, so the bar keeps its width.
	usages: Vec<String>,
}

impl MemoryWarning {
	/// The dialog for the warning `app` asks; `None` when the warning is gone already.
	pub(super) fn of(app: &App) -> Option<Self> {
		let s = &app.strings;
		let w = app.live.warning()?;
		let levers = w.proposal();
		let mut dialog = Self {
			levers,
			enabled: w.enabled(),
			mute: false,
			text: s.format(
				"MemoryWarningTextFormat",
				&[&bytes_text(w.bytes_of(Levers::default())), &bytes_text(w.budget)],
			),
			cache: s.format("MemoryWarningCacheFormat", &[&bytes_text(w.auto_budget)]),
			result: String::new(),
			usages: Vec::new(),
		};
		let open = |l: Levers| {
			let fixed = |enabled: bool, on: bool, proposed: bool| enabled || on == proposed;
			let e = dialog.enabled;
			fixed(e.cache_auto, l.cache_auto, levers.cache_auto)
				&& fixed(e.resolution, l.resolution, levers.resolution)
				&& fixed(e.frames, l.frames, levers.frames)
				&& fixed(e.samples, l.samples, levers.samples)
		};
		dialog.usages = (0..16u8)
			.map(|bits| Levers {
				cache_auto: bits & 1 != 0,
				resolution: bits & 2 != 0,
				frames: bits & 4 != 0,
				samples: bits & 8 != 0,
			})
			.filter(|&l| open(l))
			.map(|l| usage(w.bytes_of(l), w.budget_of(l)))
			.collect();
		dialog.result = dialog.result_of(app)?;
		Some(dialog)
	}

	fn result_of(&self, app: &App) -> Option<String> {
		let (s, l) = (&app.strings, self.levers);
		let w = app.live.warning()?;
		Some(if w.fits(l) {
			s.format("MemoryWarningFitsFormat", &[&bytes_text(w.bytes_of(l))])
		} else {
			s.format("MemoryWarningSpillsFormat", &[&bytes_text(w.bytes_of(l)), &bytes_text(w.spill_of(l))])
		})
	}

	pub(super) fn ui(&mut self, ui: &mut Ui, fresh: bool, app: &App) -> Option<Reply> {
		if app.live.warning().is_none() {
			// dropped before it was answered
			return Some(None);
		}
		let s: &Strings = &app.strings;
		let dim = Palette::of(ui.ctx().theme()).text_dim;
		ui.set_width(WIDTH);
		ui.add(egui::Label::new(self.text.as_str()).wrap());
		ui.label(s.get("MemoryWarningChoices"));
		let (enabled, levers) = (self.enabled, &mut self.levers);
		let rows: [(&str, bool, &mut bool); 4] = [
			(&self.cache, enabled.cache_auto, &mut levers.cache_auto),
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
		if changed && let Some(result) = self.result_of(app) {
			self.result = result;
		}
		if let Some(w) = app.live.warning() {
			usage_bar(ui, w.bytes_of(self.levers), w.budget_of(self.levers), &self.usages);
		}
		ui.add(egui::Label::new(RichText::new(self.result.as_str()).color(dim)).wrap());
		if let Some(v) = check_box(ui, Id::unique("MemoryWarningMute"), s.get("MemoryWarningMute"), self.mute) {
			self.mute = v;
		}
		let choices = [Choice::default(s.get("DialogRun")), Choice::cancel(s.get("DialogCancel"))];
		buttons(ui, fresh, &choices).map(|i| {
			Some(Then::Send(if i == 0 {
				Event::MemoryWarningRun { levers: self.levers, mute: self.mute }
			} else {
				Event::MemoryWarningCancel
			}))
		})
	}
}

/// The usage bar's label.
fn usage(need: u64, budget: u64) -> String {
	format!("{} / {}", bytes_text(need), bytes_text(budget))
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

#[cfg(test)]
mod tests {
	use super::bytes_text;

	#[test]
	fn bytes_read_in_mib_below_a_gib() {
		assert_eq!(bytes_text(512 << 20), "512 MiB");
		assert_eq!(bytes_text(3 << 29), "1.5 GiB");
	}
}
