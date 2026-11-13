//! The status bar: the newest Info-or-up log line on the left (a warning or an error tints the bar), the run's line
//! while a run exists, the busy work with its progress and the frame cache on the right. A double-click asks for the
//! Messages pane.

use eframe::egui::{self, Color32, Frame, Label, Margin, Rect, Sense, Stroke, StrokeKind, Vec2};
use tracing::Level;
use wavedude_app_state::live::FrameCache;
use wavedude_app_state::log::LogRing;
use wavedude_app_state::strings::Language;

use super::memory_warning::bytes_text;
use crate::App;

const HEIGHT: f32 = 22.;
const BACKGROUND: Color32 = Color32::from_rgb(0x00, 0x7A, 0xCC);
const WARN: Color32 = Color32::from_rgb(0xCA, 0x51, 0x00);
pub(super) const ERROR: Color32 = Color32::from_rgb(0xB4, 0x21, 0x1D);
const TEXT: Color32 = Color32::WHITE;
/// Left, right, top, bottom; the right keeps the resize grip clear.
const PADDING: Margin = Margin { left: 4, right: 16, top: 0, bottom: 0 };
const ITEM_PADDING: Margin = Margin::symmetric(4, 2);
#[cfg(not(target_arch = "wasm32"))]
const GRIP: f32 = 16.;
const BAR: Vec2 = Vec2::new(120., 12.);
/// An indeterminate bar's chunk: its share of the bar and the seconds of one sweep.
const CHUNK: f32 = 0.3;
const SWEEP: f64 = 1.5;

/// The frame cache's text and tooltip, formatted again only when its figures or the language change.
#[derive(Default)]
pub(super) struct CacheLine {
	of: Option<(FrameCache, Language)>,
	text: String,
	tip: String,
}

impl CacheLine {
	pub(super) fn update(&mut self, app: &App) {
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

/// The bar at the window's bottom, `export` the running export's text and share done; whether it was double-clicked.
pub(super) fn show(
	ui: &mut egui::Ui,
	app: &App,
	log: &LogRing,
	export: Option<(&str, Option<f32>)>,
	cache: &CacheLine,
) -> bool {
	let s = &app.strings;
	let line = log.status();
	let fill = match line.as_ref().map(|l| l.level) {
		Some(Level::ERROR) => ERROR,
		Some(Level::WARN) => WARN,
		_ => BACKGROUND,
	};
	let text = line.map_or_else(|| s.get("Ready").to_owned(), |l| l.message);
	#[cfg(not(target_arch = "wasm32"))]
	let download = crate::downloads::shown();
	#[cfg(target_arch = "wasm32")]
	let download = None;
	// the share done, `None` while unknown
	let busy = download
		.or_else(|| export.map(|(text, share)| (text.to_owned(), share)))
		.or_else(|| app.live.computing_frame().map(|f| (s.format("StatusComputingFormat", &[&f]), None)));
	let busy_tip = export.is_some().then(|| s.get("TipExportCancel"));
	let run = app.run_status();
	let frame = Frame::new().fill(fill).inner_margin(PADDING);
	let bar = egui::Panel::bottom("status_bar")
		.exact_size(HEIGHT)
		.resizable(false)
		.show_separator_line(false)
		.frame(frame)
		.show(ui, |ui| {
			let clicked =
				ui.interact(ui.max_rect(), ui.unique_id().with("double_click"), Sense::click()).double_clicked();
			ui.visuals_mut().override_text_color = Some(TEXT);
			ui.spacing_mut().item_spacing.x = 0.;
			ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
				if cache.of.is_some() {
					item(ui, |ui| ui.add(Label::new(&cache.text).selectable(false))).on_hover_text(&cache.tip);
				}
				if let Some((busy, share)) = busy {
					let shown = item(ui, |ui| {
						match share {
							Some(share) => progress_bar(ui, share),
							None => indeterminate_bar(ui),
						}
						ui.add_space(6.);
						ui.add(Label::new(busy).selectable(false));
					});
					if let Some(tip) = busy_tip {
						shown.on_hover_text(tip);
					}
				}
				if !run.is_empty() {
					item(ui, |ui| ui.add(Label::new(run).selectable(false)));
				}
				ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
					item(ui, |ui| {
						ui.add(Label::new(text).selectable(false).truncate()).on_hover_text(s.get("TipStatusBar"));
					});
				});
			});
			clicked
		});
	#[cfg(not(target_arch = "wasm32"))]
	resize_grip(ui, bar.response.rect);
	bar.inner
}

/// One of the bar's texts with its padding.
fn item<R>(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui) -> R) -> egui::Response {
	Frame::new().inner_margin(ITEM_PADDING).show(ui, content).response
}

/// The window's resize grip in the bar's bottom right corner: a 10 px triangle of 2 px dots, 3 px off the corner.
#[cfg(not(target_arch = "wasm32"))]
fn resize_grip(ui: &egui::Ui, bar: Rect) {
	let rect = Rect::from_min_max(bar.max - Vec2::splat(GRIP), bar.max);
	let response = ui.interact(rect, ui.unique_id().with("resize_grip"), Sense::drag());
	if response.drag_started() {
		ui.send_viewport_cmd(egui::ViewportCommand::BeginResize(egui::viewport::ResizeDirection::SouthEast));
	}
	response.on_hover_cursor(egui::CursorIcon::ResizeSouthEast);
	let origin = rect.min + Vec2::splat(GRIP - 3. - 10.);
	for (x, y) in [(8., 0.), (8., 4.), (8., 8.), (4., 4.), (4., 8.), (0., 8.)] {
		let dot = Rect::from_min_size(origin + Vec2::new(x, y), Vec2::splat(2.));
		ui.painter().rect_filled(dot, 0., TEXT);
	}
}

/// A bar filled to `share`.
fn progress_bar(ui: &mut egui::Ui, share: f32) {
	let (rect, _) = ui.allocate_exact_size(BAR, Sense::hover());
	let painter = ui.painter();
	painter.rect_filled(Rect::from_min_size(rect.min, Vec2::new(share * rect.width(), rect.height())), 0., TEXT);
	painter.rect_stroke(rect, 0., Stroke::new(1., TEXT), StrokeKind::Inside);
}

/// A bar a chunk sweeps through, left to right.
fn indeterminate_bar(ui: &mut egui::Ui) {
	let (rect, _) = ui.allocate_exact_size(BAR, Sense::hover());
	let t = (ui.input(|i| i.time) / SWEEP).fract() as f32;
	let start = rect.left() + (t * (1. + CHUNK) - CHUNK) * rect.width();
	let chunk = Rect::from_x_y_ranges(
		start.max(rect.left())..=(start + CHUNK * rect.width()).min(rect.right()),
		rect.y_range(),
	);
	let painter = ui.painter();
	if chunk.width() > 0. {
		painter.rect_filled(chunk, 0., TEXT);
	}
	painter.rect_stroke(rect, 0., Stroke::new(1., TEXT), StrokeKind::Inside);
}
