//! The status bar's texts that are no widget's (docs/ui/70, Status bar).

use crate::live::FrameCache;
use crate::strings::Language;

use crate::App;
use crate::settings_table::bytes_text;

/// The busy item's text and share done (`None` while unknown): a download, else the export (`export`, as
/// `crate::export::session::Exports::status` gives it), else the frame being computed.
pub fn busy(app: &App, export: Option<(&str, Option<f32>)>) -> Option<(String, Option<f32>)> {
	#[cfg(not(target_arch = "wasm32"))]
	let download = crate::downloads::shown();
	#[cfg(target_arch = "wasm32")]
	let download = None;
	download
		.or_else(|| export.map(|(text, share)| (text.to_owned(), share)))
		.or_else(|| app.live.computing_frame().map(|f| (app.strings.format("StatusComputingFormat", &[&f]), None)))
}

/// The frame cache's text and tooltip, formatted again only when its figures or the language change.
#[derive(Default)]
pub struct CacheLine {
	of: Option<(FrameCache, Language)>,
	pub text: String,
	pub tip: String,
}

impl CacheLine {
	/// A run's frame cache exists, so the line shows.
	pub fn shown(&self) -> bool {
		self.of.is_some()
	}

	pub fn update(&mut self, app: &App) {
		let of = app.live.frame_cache().map(|c| (c, app.strings.language()));
		if of == self.of {
			return;
		}
		self.of = of;
		let Some((c, _)) = of else { return };
		let s = &app.strings;
		let ram = bytes_text(c.in_memory as u64 * c.frame_bytes);
		let budget = bytes_text(c.budget);
		self.text = match c.spilled {
			0 => s.format("StatusFrameCacheFormat", &[&ram, &budget]),
			n => s.format("StatusFrameCacheSpillFormat", &[&ram, &budget, &bytes_text(n as u64 * c.frame_bytes)]),
		};
		let spill = if cfg!(target_arch = "wasm32") { "FrameCacheSpillBrowser" } else { "FrameCacheSpillDisk" };
		self.tip = s.format(
			"TipStatusFrameCacheFormat",
			&[&s.grouped(c.in_memory as u64), &s.grouped(c.spilled as u64), &s.get(spill), &budget],
		);
	}
}
