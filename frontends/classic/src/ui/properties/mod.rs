//! The Properties pane (docs/ui/30): the form of the current row, drawn from the project in place, every edit sent
//! as a typed `Command::Edit` through the model's field accessors (`Probe::BOX_WIDTH`), its sections on the kit's
//! form grid ([`crate::kit::form`]); typed text waits in the pending edit ([`super::edit`]); Enter on a tree row or an
//! Add puts the focus into the form's first box.

mod forms;

use std::hash::{Hash, Hasher};

use eframe::egui::{
	self, Align, Color32, FontId, Id, Layout, Rect, Response, Role, ScrollArea, Sense, Stroke, StrokeKind, Ui,
	UiBuilder, Vec2, WidgetInfo, pos2, vec2,
};
use waveduderust::model::{ItemKind, Stop};
use waveduderust_app::App;
use waveduderust_app::Event;
use waveduderust_app::form::{self, Out, Request};
use waveduderust_app::number::NumberSpec;
use waveduderust_app::properties::{Readouts, readouts};
use waveduderust_app::selection::Row;
use waveduderust_app::strings::{Language, Strings};
use waveduderust_egui_preview::gradient_editor::gradient_mesh;

use super::edit;
use super::icons::{self, Icon};
use super::number_box::number_box;
use super::text_box::{self, Kind};
use crate::kit::form::{self as kit_form, Grid, Look};
use crate::kit::menu::{self, Entry};
use crate::kit::{FONT_SIZE, FORM, INPUT_HEIGHT, Palette};

const CHECK_MARK: f32 = 14.;
const CHECK_GAP: f32 = 6.;
/// A scheme's swatch in a picker.
const SWATCH: Vec2 = Vec2::new(56., 12.);
/// Width a drop-down button adds to its widest word: padding, gap and arrow.
const COMBO_CHROME: f32 = 56.;
const COMBO_PADDING: f32 = 5.;
/// A glyph button: a 16 px icon and 6 px padding each side, a row high.
const GLYPH_BUTTON: Vec2 = Vec2::new(28., 23.);
const BUTTON_PADDING: Vec2 = Vec2::new(11., 4.);
const BUTTON_HEIGHT: f32 = 23.;

/// The pane's state between frames.
#[derive(Default)]
pub(super) struct PropertiesPane {
	/// Enter on a tree row or an Add asked for the first editor's focus.
	focus: bool,
	/// The read-outs, and the document, selection and settings revisions they were made at.
	readouts: Option<((u64, u64, u64), Readouts)>,
}

impl PropertiesPane {
	/// The form's first box takes the focus when the form shows next.
	pub(super) fn focus_form(&mut self) {
		self.focus = true;
	}

	/// The pane in `ui`'s room; the events it asks the state for go to `out`.
	pub(super) fn ui(&mut self, ui: &mut Ui, app: &App, out: &mut Vec<Event>) -> Option<Request> {
		let palette = Palette::of(ui.ctx().theme());
		let pane = ui.max_rect();
		ui.painter().rect_filled(pane, 0., palette.tool_window_background);
		let s = app.strings;
		let project = &app.document.project;
		let row = app.selection.current().filter(|r| match r {
			Row::Item(i) => i.exists(project),
			_ => true,
		});
		let Some(row) = row else {
			ui.put(pane, egui::Label::new(egui::RichText::new(s.get("NoSelection")).color(palette.text_dim)));
			return None;
		};
		let revisions = app.revisions();
		let key = (revisions.document, revisions.selection, revisions.settings);
		if self.readouts.as_ref().is_none_or(|(k, _)| *k != key) {
			self.readouts = Some((key, readouts(app, row)));
		}
		let r = self.readouts.as_ref().map(|(_, r)| r).expect("made above");
		let mut asked = None;
		ScrollArea::vertical().auto_shrink(false).id_salt("PropertiesScroll").show(ui, |ui| {
			let inner = ui.available_rect_before_wrap().shrink(FORM.padding);
			let mut ui = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)));
			let ui = &mut ui;
			style(ui, palette);
			let base = Id::unique("Properties");
			let mut p = Pane {
				ui,
				cx: Cx {
					s,
					app,
					base,
					out,
					asked: &mut asked,
					focus: &mut self.focus,
					claimed: false,
					comma: s.language() != Language::English,
				},
			};
			match row {
				Row::Folder(kind) => folder(&mut p, kind),
				row => {
					if let Some(model) = form::build(form::Cx { s, app, r }, row) {
						forms::draw(&mut p, &model);
					}
				}
			}
			let used = p.ui.min_rect();
			let bottom = used.max.y + FORM.padding;
			ui.allocate_rect(Rect::from_min_max(inner.min, pos2(inner.max.x, bottom)), Sense::hover());
		});
		asked
	}
}

/// The form's look: buttons as the dialogs', no fading of disabled rows (they grey themselves).
pub(super) fn style(ui: &mut Ui, palette: &Palette) {
	ui.spacing_mut().item_spacing = vec2(0., 0.);
	ui.spacing_mut().button_padding = BUTTON_PADDING;
	let v = ui.visuals_mut();
	v.disabled_alpha = 1.;
	let w = &mut v.widgets;
	w.inactive.weak_bg_fill = palette.button_background;
	w.inactive.bg_stroke = Stroke::new(1., palette.button_border);
	w.hovered.weak_bg_fill = palette.button_hover;
	w.hovered.bg_stroke = Stroke::new(1., palette.button_border);
	w.active.weak_bg_fill = palette.accent;
	w.active.bg_stroke = Stroke::new(1., palette.button_border);
}

/// Puts `widget` at the left of `rect`, centred across, leaving the cursor where it is.
fn put_left(ui: &mut Ui, rect: Rect, widget: impl egui::Widget) -> Response {
	ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center))).add(widget)
}

/// A folder's name and its count, "No entries", "1 entry", "N entries", in grey (Next's CollectionView.xaml:31-32).
fn folder(p: &mut Pane<'_>, kind: ItemKind) {
	let s = p.cx.s;
	let key = super::tree::folder_of(kind).0;
	let text = match kind.len(&p.cx.app.document.project) {
		0 => s.get("ItemCountZero").to_owned(),
		1 => s.get("ItemCountOne").to_owned(),
		n => s.format("ItemCountMany", &[&n]),
	};
	kit_form::heading(p.ui, s.get(key), FORM.folder_after);
	kit_form::value(p.ui, &text);
}

/// The pane's content: its `Ui` and what the editors share.
pub(super) struct Pane<'a> {
	ui: &'a mut Ui,
	pub(super) cx: Cx<'a>,
}

/// A section's rows on the kit's grid.
pub(super) struct Form<'f, 'a> {
	grid: Grid<'f>,
	pub(super) cx: &'f mut Cx<'a>,
	/// The next row is enabled.
	enabled: bool,
}

/// What the editors of a form share.
pub(super) struct Cx<'a> {
	pub(super) s: Strings,
	pub(super) app: &'a App,
	base: Id,
	pub(super) out: &'a mut Vec<Event>,
	asked: &'a mut Option<Request>,
	/// The pending request for the first editor's focus.
	focus: &'a mut bool,
	/// The first editor was placed in this pass.
	claimed: bool,
	/// Numbers show a decimal comma.
	comma: bool,
}

impl Cx<'_> {
	pub(super) fn ask(&mut self, request: Request) {
		*self.asked = Some(request);
	}

	pub(super) fn send(&mut self, event: Event) {
		self.out.push(event);
	}

	/// An id unique in the form for `salt`.
	pub(super) fn id(&self, salt: impl Hash + std::fmt::Debug) -> Id {
		self.base.with(salt)
	}

	/// Whether the editor `id` is the form's first; it takes the focus when that was asked for.
	fn claim(&mut self, ui: &Ui, id: Id) {
		if !std::mem::replace(&mut self.claimed, true) && std::mem::take(self.focus) {
			ui.memory_mut(|m| m.request_focus(id));
		}
	}

	/// A number box of `width` (the rest of the row when none), the first editor when `first`; `event` makes the
	/// event of a value.
	#[expect(clippy::too_many_arguments, reason = "a box's parts, each its own")]
	pub(super) fn number(
		&mut self,
		ui: &mut Ui,
		id: Id,
		value: f64,
		spec: NumberSpec,
		blank: bool,
		width: Option<f32>,
		label: &str,
		first: bool,
		event: impl Fn(f64) -> Event,
	) -> Response {
		let spec = if self.comma { spec.with_decimal_separator(',') } else { spec };
		let (rect, _) =
			ui.allocate_exact_size(vec2(width.unwrap_or(ui.available_width()), INPUT_HEIGHT), Sense::hover());
		if first {
			self.claim(ui, id);
		}
		let edited = number_box(ui, rect, id, value, &spec, blank, label);
		if let Some(v) = edited.applied {
			self.out.push(event(v));
		}
		edited.offer(ui.ctx(), id, event);
		edited.response
	}

	/// A text box filling the row, the first editor when `first`.
	#[expect(clippy::too_many_arguments, reason = "a box's parts, each its own")]
	pub(super) fn text(
		&mut self,
		ui: &mut Ui,
		id: Id,
		value: &str,
		kind: Kind,
		label: &str,
		first: bool,
		event: impl Fn(String) -> Option<Event>,
	) -> Response {
		if first {
			self.claim(ui, id);
		}
		let edited = text_box::text_box(ui, id, value, ui.available_width(), kind, label);
		if let Some(e) = edited.applied.and_then(&event) {
			self.out.push(e);
		}
		if edited.response.has_focus() || edited.response.lost_focus() {
			edit::offer(ui.ctx(), id, edited.typed.and_then(event));
		}
		edited.response
	}

	/// Sends or asks for what an editor returned.
	pub(super) fn apply(&mut self, outs: Vec<Out>) {
		for out in outs {
			match out {
				Out::Send(event) => self.send(event),
				Out::Ask(request) => self.ask(request),
			}
		}
	}
}

impl<'a> Pane<'a> {
	/// A section: its heading, then its rows on a grid of their own.
	pub(super) fn section(&mut self, title: &str, rows: impl FnOnce(&mut Form<'_, 'a>)) {
		kit_form::heading(self.ui, title, FORM.heading_after);
		let id = self.cx.id(("Section", title));
		let mut f = Form { grid: Grid::begin(self.ui, id), cx: &mut self.cx, enabled: true };
		rows(&mut f);
		f.grid.end();
	}

	/// The parts' description header, the kind's text under it while open.
	pub(super) fn description(&mut self, title: &str, tip: &str, text: &str, open: bool) {
		let response = kit_form::disclosure(self.ui, self.cx.base.with("Description"), title, tip, open);
		if response.clicked() {
			self.cx.ask(Request::DescriptionOpen(!open));
		}
		if open {
			kit_form::description(self.ui, text);
		}
	}
}

impl<'a> Form<'_, 'a> {
	pub(super) fn ui(&mut self) -> &mut Ui {
		self.grid.ui()
	}

	/// The next row is greyed and its editor disabled unless `enabled`.
	pub(super) fn on(&mut self, enabled: bool) -> &mut Self {
		self.enabled = enabled;
		self
	}

	/// A row: its label in the label column, `editor` in the rest, enabled as set by [`Self::on`].
	pub(super) fn row_of<R>(&mut self, row: kit_form::Row<'_>, editor: impl FnOnce(&mut Ui, &mut Cx<'a>) -> R) -> R {
		let enabled = std::mem::replace(&mut self.enabled, true);
		let cx = &mut *self.cx;
		self.grid.row(kit_form::Row { enabled, ..row }, |ui| editor(ui, cx))
	}

	pub(super) fn full<R>(&mut self, hint: &str, editor: impl FnOnce(&mut Ui, &mut Cx<'a>) -> R) -> R {
		self.row_of(kit_form::Row::new(None, hint), editor)
	}

	/// `rows` leaving `width` free on their right; that room, as high as the rows.
	pub(super) fn beside(&mut self, width: f32, rows: impl FnOnce(&mut Self)) -> Rect {
		let (top, right) = (self.ui().cursor().top(), self.ui().max_rect().right());
		self.grid.trail(width);
		rows(self);
		self.grid.trail(-width);
		Rect::from_min_max(pos2(right - width, top), pos2(right, self.ui().cursor().top()))
	}

	/// A row of `text` read as `look`; it wraps.
	pub(super) fn readout(&mut self, label: Option<&str>, text: &str, hint: &str, look: Look) {
		let enabled = std::mem::replace(&mut self.enabled, true);
		self.grid.text(kit_form::Row { enabled, ..kit_form::Row::new(label, hint) }, text, look);
	}
}

/// A check box with its text, `id` its focus; the state it is set to.
pub(super) fn check_box(ui: &mut Ui, id: Id, text: &str, on: bool) -> Option<bool> {
	let palette = Palette::of(ui.ctx().theme());
	let enabled = ui.is_enabled();
	let ink = if enabled { palette.text } else { palette.text_disabled };
	let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(FONT_SIZE), ink);
	let size = vec2(CHECK_MARK + CHECK_GAP + galley.size().x, CHECK_MARK.max(galley.size().y));
	let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
	let response = ui.interact(rect, id, Sense::click());
	response.widget_info(|| WidgetInfo::selected(Role::CheckBox, enabled, on, text));
	let mark = Rect::from_min_size(pos2(rect.left(), rect.center().y - CHECK_MARK / 2.), Vec2::splat(CHECK_MARK));
	// the disabled fill is the pane's own colour, which the input border equals in the dark theme
	let (fill, border) = if enabled {
		(palette.input_background, if response.has_focus() { palette.accent } else { palette.border })
	} else {
		(palette.control_background, palette.hairline)
	};
	ui.painter().rect(mark, 0., fill, Stroke::new(1., border), StrokeKind::Inside);
	if on {
		icons::paint(ui.painter(), Rect::from_center_size(mark.center(), Vec2::splat(16.)), icons::CHECK, ink);
	}
	let at = pos2(mark.right() + CHECK_GAP, rect.center().y - galley.size().y / 2.);
	ui.painter().galley(at, galley, ink);
	response.clicked().then_some(!on)
}

/// What a drop-down offers: items by index, the word shown closed, and action rows after the items.
pub(super) struct Choices<'c> {
	pub(super) len: usize,
	pub(super) current: Option<usize>,
	pub(super) shown: &'c str,
	pub(super) word: &'c dyn Fn(usize) -> std::borrow::Cow<'c, str>,
	/// Item `i`'s gradient swatch, if it has one.
	pub(super) swatch: Option<&'c dyn Fn(usize) -> Option<&'c [Stop]>>,
	/// Why item `i` cannot be picked, greyed with this as its tooltip.
	pub(super) disabled: Option<&'c dyn Fn(usize) -> Option<&'c str>>,
	/// Italic rows after the items; picking one gives `len` plus its index.
	pub(super) actions: &'c [&'c str],
}

/// A drop-down button filling the row, never narrower than its widest word; the index picked.
pub(super) fn combo(ui: &mut Ui, id: Id, label: &str, c: &Choices<'_>) -> Option<usize> {
	let palette = Palette::of(ui.ctx().theme());
	let enabled = ui.is_enabled();
	let swatch_of = |i: usize| c.swatch.and_then(|f| f(i));
	let natural = widest_word(ui, id, c) + COMBO_CHROME;
	let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(natural), INPUT_HEIGHT), Sense::hover());
	let response = ui.interact(rect, id, Sense::click());
	response.widget_info(|| WidgetInfo::labeled(Role::ComboBox, enabled, label));
	let (fill, border) = if enabled && response.hovered() {
		(palette.combo_hover, palette.input_hover_border)
	} else {
		(palette.input_background, palette.combo_border)
	};
	let border = if enabled && response.has_focus() { palette.accent } else { border };
	ui.painter().rect(rect, 0., fill, Stroke::new(1., border), StrokeKind::Inside);
	let ink = if enabled { palette.text } else { palette.text_disabled };
	let mut x = rect.left() + COMBO_PADDING;
	if let Some(stops) = c.current.and_then(swatch_of) {
		let at = Rect::from_min_size(pos2(x, rect.center().y - SWATCH.y / 2.), SWATCH);
		paint_swatch(ui, at, stops, palette.border);
		x = at.right() + 6.;
	}
	let arrow = Rect::from_center_size(pos2(rect.right() - COMBO_PADDING - 8., rect.center().y), Vec2::splat(16.));
	let word = Rect::from_min_max(pos2(x, rect.top()), pos2(arrow.left(), rect.bottom()));
	let text = egui::Label::new(egui::RichText::new(c.shown).color(ink)).truncate().selectable(false);
	put_left(ui, word, text);
	icons::paint(ui.painter(), arrow, icons::DROPDOWN, ink);
	let mut picked = None;
	menu::below(&response, rect.width(), |m| {
		for i in 0..c.len {
			let word = (c.word)(i);
			let painted;
			let swatch = match swatch_of(i) {
				Some(stops) => {
					painted = move |ui: &Ui, r: Rect| paint_swatch(ui, r, stops, palette.border);
					Some((SWATCH, &painted as &dyn Fn(&Ui, Rect)))
				}
				None => None,
			};
			let reason = c.disabled.and_then(|f| f(i));
			let mut row = Entry::plain(&word).check(c.current == Some(i)).swatch(swatch).enabled(reason.is_none());
			if let Some(reason) = &reason {
				row = row.tip(reason);
			}
			if m.item(row).picked {
				picked = Some(i);
			}
		}
		for (k, action) in c.actions.iter().enumerate() {
			if m.item(Entry::plain(action).check(false).italic()).picked {
				picked = Some(c.len + k);
			}
		}
	});
	picked
}

/// The widest word of `c`, measured when its words change.
fn widest_word(ui: &Ui, id: Id, c: &Choices<'_>) -> f32 {
	let mut h = std::hash::DefaultHasher::new();
	for i in 0..c.len {
		(c.word)(i).hash(&mut h);
	}
	let (key, words) = (id.with("widest"), h.finish());
	if let Some((_, w)) = ui.data(|d| d.get_temp::<(u64, f32)>(key)).filter(|(of, _)| *of == words) {
		return w;
	}
	let font = FontId::proportional(FONT_SIZE);
	let w = (0..c.len)
		.map(|i| ui.painter().layout_no_wrap((c.word)(i).into_owned(), font.clone(), Color32::WHITE).size().x)
		.fold(0., f32::max);
	ui.data_mut(|d| d.insert_temp(key, (words, w)));
	w
}

/// A scheme's gradient at `rect`, thinly outlined.
pub(super) fn paint_swatch(ui: &Ui, rect: Rect, stops: &[Stop], border: Color32) {
	ui.painter().add(egui::Shape::mesh(gradient_mesh(rect, stops)));
	ui.painter().rect_stroke(rect, 0., Stroke::new(0.5, border), StrokeKind::Outside);
}

/// A button with `text`, as the dialogs'.
pub(super) fn button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
	let text = waveduderust_app::strings::strip_mnemonic(text);
	ui.add_enabled(enabled, egui::Button::new(text).min_size(vec2(0., BUTTON_HEIGHT)))
}

/// A compact button showing `icon` (`flat`: no frame at rest), `tip` its tooltip and name.
pub(super) fn glyph_button(ui: &mut Ui, id: Id, icon: Icon, color: Option<Color32>, flat: bool, tip: &str) -> Response {
	let palette = Palette::of(ui.ctx().theme());
	let enabled = ui.is_enabled();
	let (rect, _) = ui.allocate_exact_size(GLYPH_BUTTON, Sense::hover());
	let response = ui.interact(rect, id, Sense::click()).on_hover_text(tip);
	response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, tip));
	let pressed = enabled && response.is_pointer_button_down_on();
	let (fill, border) = if pressed {
		(palette.accent, palette.button_border)
	} else if enabled && response.hovered() {
		(palette.button_hover, if flat { Color32::TRANSPARENT } else { palette.button_border })
	} else if flat {
		(Color32::TRANSPARENT, Color32::TRANSPARENT)
	} else {
		(palette.button_background, palette.button_border)
	};
	let border = if enabled && response.has_focus() { palette.accent } else { border };
	ui.painter().rect(rect, 0., fill, Stroke::new(1., border), StrokeKind::Inside);
	let ink = if !enabled {
		palette.text_disabled
	} else if pressed {
		Color32::WHITE
	} else {
		color.unwrap_or(palette.text)
	};
	icons::paint(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(16.)), icon, ink);
	response
}

/// A word in the text colour `dim` or not, `width` wide (its own when none), left aligned in the row.
pub(super) fn caption(ui: &mut Ui, text: &str, width: Option<f32>, dim: bool) {
	let palette = Palette::of(ui.ctx().theme());
	let ink = if dim { palette.text_dim } else { palette.text };
	let label = egui::Label::new(egui::RichText::new(text).color(ink)).truncate().selectable(false);
	match width {
		Some(w) => {
			let (rect, _) = ui.allocate_exact_size(vec2(w, INPUT_HEIGHT), Sense::hover());
			put_left(ui, rect, label);
		}
		None => {
			ui.add(label);
		}
	}
}
