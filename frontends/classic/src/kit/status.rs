//! The status bar (docs/ui/90, 21) on egui_mineral's status model: [`StatusItem`]s from the start and from the end,
//! placed by its strip, the bar in a [`Tone`], and the window's resize grip while the window can resize.

use std::sync::Arc;

use eframe::egui::{
	self, Color32, FontId, Galley, Id, Margin, Rect, Response, Role, Sense, Stroke, StrokeKind, TextStyle, Ui, Vec2,
	WidgetInfo,
};
use eframe::egui::{pos2, vec2};
use egui_mineral::i18n::Direction;
use egui_mineral::kit::strip;
use egui_mineral::layout::Fraction;
use egui_mineral::runtime::Core;
use egui_mineral::status::{StatusItem, StatusLead, StatusProgress};
use egui_mineral::tone::Tone;
use egui_mineral::window;

use super::Palette;

const HEIGHT: f32 = 22.;
/// The right keeps the resize grip clear.
const PADDING: Margin = Margin { left: 4, right: 16, top: 0, bottom: 0 };
const ITEM_PADDING: Vec2 = Vec2::new(4., 2.);
const GRIP: f32 = 16.;
/// The grip's dots stand off the corner by this, past the core's inset.
const GRIP_INSET: f32 = 2.;
const BAR: Vec2 = Vec2::new(120., 12.);
const BAR_GAP: f32 = 6.;
/// An indeterminate bar's chunk: its share of the bar and the seconds of one sweep.
const CHUNK: f32 = 0.3;
const SWEEP: f64 = 1.5;

/// The warning tone: Next's warning bar.
pub(crate) fn warning(palette: &Palette) -> Tone {
	Tone::Custom { container: palette.status_warn, content: palette.status_text }
}

/// What [`StatusBar::show`] got.
pub(crate) struct StatusBarResponse<A> {
	/// The bar, sensing clicks.
	pub response: Response,
	/// The action item clicked.
	pub clicked: Option<A>,
}

/// A bar at the window's bottom.
pub(crate) struct StatusBar<A> {
	id: &'static str,
	tone: Tone,
	items: Vec<(StatusItem, bool, Option<A>)>,
	grip: bool,
}

impl<A: Clone> StatusBar<A> {
	pub(crate) fn new(id: &'static str) -> Self {
		Self { id, tone: Tone::Default, items: Vec::new(), grip: false }
	}

	pub(crate) fn tone(mut self, tone: Tone) -> Self {
		self.tone = tone;
		self
	}

	/// The window's resize grip in the end corner, hidden while the window cannot resize.
	pub(crate) fn grip(mut self, grip: bool) -> Self {
		self.grip = grip;
		self
	}

	/// An item after the start's ones.
	pub(crate) fn item(mut self, item: StatusItem) -> Self {
		self.items.push((item, false, None));
		self
	}

	/// An item before the end's ones.
	pub(crate) fn trailing(mut self, item: StatusItem) -> Self {
		self.items.push((item, true, None));
		self
	}

	/// [`Self::trailing`] as a flat button reporting `value`.
	pub(crate) fn trailing_action(mut self, value: A, item: StatusItem) -> Self {
		self.items.push((item, true, Some(value)));
		self
	}

	/// The bar as the bottom panel of `ui`.
	pub(crate) fn show(self, ui: &mut Ui) -> StatusBarResponse<A> {
		let palette = Palette::of(ui.ctx().theme());
		let (fill, ink) = colors(self.tone, palette);
		egui::Panel::bottom(self.id)
			.exact_size(HEIGHT)
			.resizable(false)
			.show_separator_line(false)
			.frame(egui::Frame::new().fill(fill))
			.show(ui, |ui| self.contents(ui, palette, ink))
			.inner
	}

	fn contents(self, ui: &mut Ui, palette: &Palette, ink: Color32) -> StatusBarResponse<A> {
		let bar = ui.max_rect();
		let id = Id::unique(self.id);
		let response = ui.interact(bar, id, Sense::click());
		let inner = bar - PADDING;
		let font = TextStyle::Body.resolve(ui.style());
		let mut galleys: Vec<Arc<Galley>> =
			self.items.iter().map(|(item, ..)| line(ui, item.text(), &font, ink, f32::INFINITY)).collect();
		let chrome = |item: &StatusItem| 2. * ITEM_PADDING.x + item.lead().map_or(0., |_| BAR.x + BAR_GAP);
		let measured: Vec<strip::Item> = self
			.items
			.iter()
			.zip(&galleys)
			.map(|((item, at_end, _), g)| {
				strip::Item::new(chrome(item) + g.size().x)
					.chrome(chrome(item))
					.at_end(*at_end)
					.elide(item.elision().is_some())
			})
			.collect();
		let mut clicked = None;
		for (i, placed) in strip::place(inner.width(), &measured).into_iter().enumerate() {
			let (Some(p), Some((item, at_end, value))) = (placed, self.items.get(i)) else { continue };
			let x = if *at_end { inner.right() - p.offset - p.width } else { inner.left() + p.offset };
			let rect = Rect::from_x_y_ranges(x..=x + p.width, bar.y_range());
			if let Some(room) = p.elided_room {
				galleys[i] = line(ui, item.text(), &font, ink, room);
			}
			let sense = if value.is_some() { Sense::click() } else { Sense::hover() };
			// a button keeps its id while items before it come and go
			let key = if value.is_some() { id.with(item.text()) } else { id.with(i) };
			let r = ui.interact(rect, key, sense);
			let role = if value.is_some() { Role::Button } else { Role::Label };
			r.widget_info(|| WidgetInfo::labeled(role, true, item.text()));
			if value.is_some() && r.hovered() {
				ui.painter().rect_filled(rect, 0., ui.visuals().widgets.hovered.weak_bg_fill);
			}
			let mut at = rect.left() + ITEM_PADDING.x;
			if let Some(StatusLead::Progress(StatusProgress::Linear(share))) = item.lead() {
				let track = Rect::from_min_size(pos2(at, rect.center().y - BAR.y / 2.), BAR);
				progress_bar(ui, track, share, ink);
				at += BAR.x + BAR_GAP;
			}
			let galley = Arc::clone(&galleys[i]);
			ui.painter().galley(pos2(at, rect.center().y - galley.size().y / 2.), galley, ink);
			let r = match item.tooltip_text() {
				Some(tip) => r.on_hover_text(tip),
				None => r,
			};
			if r.clicked() {
				clicked.clone_from(value);
			}
		}
		if self.grip && window::can_resize(ui.ctx()) {
			grip(ui, bar, id.with("grip"), palette.status_text);
		}
		StatusBarResponse { response, clicked }
	}
}

/// The first line of `text` within `width`, cut with an ellipsis.
fn line(ui: &Ui, text: &str, font: &FontId, ink: Color32, width: f32) -> Arc<Galley> {
	let mut job = egui::text::LayoutJob::simple(text.to_owned(), font.clone(), ink, f32::INFINITY);
	job.wrap = egui::text::TextWrapping::truncate_at_width(width);
	ui.painter().layout_job(job)
}

/// The bar's fill and ink in `tone`.
fn colors(tone: Tone, palette: &Palette) -> (Color32, Color32) {
	match tone {
		Tone::Error => (palette.status_error, palette.status_text),
		Tone::Custom { container, content } => (container, content),
		_ => (palette.status, palette.status_text),
	}
}

/// The window's resize grip in `bar`'s end corner.
fn grip(ui: &Ui, bar: Rect, id: Id, ink: Color32) {
	let dir = Core::of(ui.ctx()).map_or(Direction::Ltr, |core| core.i18n().direction());
	let corner = match dir {
		Direction::Rtl => Rect::from_min_size(bar.left_bottom() - vec2(0., GRIP), Vec2::splat(GRIP)),
		_ => Rect::from_min_max(bar.max - Vec2::splat(GRIP), bar.max),
	};
	window::resize_handle(ui, corner, id, window::end_corner(dir));
	for c in window::grip_dots(corner.shrink(GRIP_INSET), dir) {
		ui.painter().rect_filled(Rect::from_center_size(c, Vec2::splat(2. * window::GRIP_DOT)), 0., ink);
	}
}

/// A bar filled to `share`, or a chunk sweeping through it while `None`.
fn progress_bar(ui: &Ui, track: Rect, share: Option<Fraction>, ink: Color32) {
	let painter = ui.painter();
	let fill = match share {
		Some(share) => Rect::from_min_size(track.min, vec2(share.get() * track.width(), track.height())),
		None => {
			let t = (ui.input(|i| i.time) / SWEEP).fract() as f32;
			let start = track.left() + (t * (1. + CHUNK) - CHUNK) * track.width();
			Rect::from_x_y_ranges(
				start.max(track.left())..=(start + CHUNK * track.width()).min(track.right()),
				track.y_range(),
			)
		}
	};
	if fill.width() > 0. {
		painter.rect_filled(fill, 0., ink);
	}
	painter.rect_stroke(track, 0., Stroke::new(1., ink), StrokeKind::Inside);
}
