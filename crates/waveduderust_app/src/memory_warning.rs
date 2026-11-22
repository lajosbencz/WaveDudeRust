//! The memory warning's model (docs/ui/60, Memory warning): a run's frames exceed the RAM cache. The need and the
//! budget, four levers pre-ticked by the model's proposal (greyed where they gain nothing), the ticks' result and
//! "Do not warn me again"; Run applies the ticks, Cancel keeps the frames.

use crate::Event;
use waveduderust::run::Levers;

use crate::App;
use crate::settings_table::bytes_text;

/// The dialog's state: the ticks and the texts of the warning it asks about.
pub struct MemoryWarning {
	pub levers: Levers,
	pub enabled: Levers,
	pub mute: bool,
	pub text: String,
	pub cache: String,
	/// What the ticks give, renewed when they change.
	pub result: String,
	/// The usage bar's label for each choice of ticks open to the user, so the bar keeps its width.
	pub usages: Vec<String>,
}

impl MemoryWarning {
	/// The dialog for the warning `app` asks; `None` when the warning is gone already.
	pub fn of(app: &App) -> Option<Self> {
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

	/// What the ticks give; `None` once the warning is gone.
	pub fn result_of(&self, app: &App) -> Option<String> {
		let (s, l) = (&app.strings, self.levers);
		let w = app.live.warning()?;
		Some(if w.fits(l) {
			s.format("MemoryWarningFitsFormat", &[&bytes_text(w.bytes_of(l))])
		} else {
			s.format("MemoryWarningSpillsFormat", &[&bytes_text(w.bytes_of(l)), &bytes_text(w.spill_of(l))])
		})
	}

	/// The answer of Run (`run`) or Cancel.
	pub fn answer(&self, run: bool) -> Event {
		if run { Event::MemoryWarningRun { levers: self.levers, mute: self.mute } } else { Event::MemoryWarningCancel }
	}
}

/// The usage bar's label.
pub fn usage(need: u64, budget: u64) -> String {
	format!("{} / {}", bytes_text(need), bytes_text(budget))
}
