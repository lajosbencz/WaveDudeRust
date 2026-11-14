//! The Properties pane (docs/ui/30): the form of the current row, drawn from the project in place, every edit sent
//! as a typed `Command::Edit` through the model's field accessors (`Probe::BOX_WIDTH`). The rows lay out in two
//! columns, the labels as wide as the widest, the editors filling the rest; typed text waits in the pending edit
//! ([`super::edit`]); Enter on a tree row or an Add puts the focus into the form's first box.

mod forms;

use std::hash::{Hash, Hasher};

use eframe::egui::{
	self, Align, Color32, FontId, Id, Layout, Popup, Rect, Response, Role, ScrollArea, Sense, Stroke, StrokeKind, Ui,
	UiBuilder, Vec2, WidgetInfo, pos2, vec2,
};
use wavedude_app_state::Event;
use wavedude_app_state::commands::{Command, Edit};
use wavedude_app_state::selection::Row;
use wavedude_app_state::strings::{Language, Strings};
use wavedude_core::model::ranges::Range;
use wavedude_core::model::{Edit as _, Field as _, Fields, ItemKind, Nested, Number, Prop, Stop, Variant, Word};

use super::edit;
use super::gradient_editor::gradient_mesh;
use super::icons::{self, Icon};
use super::menu_bar;
use super::menu_keys;
use super::number_box::{NumberSpec, number_box};
use super::text_box::{self, Kind};
use super::theme::{FONT_SIZE, INPUT_HEIGHT, Palette};
use crate::App;
use crate::properties::{Readouts, bounds_of, readouts};

const MIN_LABEL: f32 = 110.;
const LABEL_GAP: f32 = 8.;
const ROW_SPACING: f32 = 4.;
/// WPF's 28 px row pitch with [`ROW_SPACING`].
const ROW_HEIGHT: f32 = 24.;
/// The gap after a section's rows.
const SECTION_GAP: f32 = 10.;
const PADDING: f32 = 8.;
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

/// What the pane asks the window for, beyond the state's events.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Request {
	Dialog(Dialog),
	/// The parts' description shows (the setting `part_description_expanded`).
	DescriptionOpen(bool),
}

/// A platform file dialog the pane asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dialog {
	/// The file picker for the scene image.
	PickSceneImage,
	/// The save dialog for the embedded scene image.
	SaveSceneImage,
	/// The file picker for a colour scheme to add.
	PickScheme,
}

/// The pane's state between frames.
#[derive(Default)]
pub(super) struct PropertiesPane {
	/// Enter on a tree row or an Add asked for the first editor's focus.
	focus: bool,
	/// The label column's width for a row.
	labels: Option<(Row, f32)>,
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
		let label_width = match self.labels {
			Some((at, w)) if at == row => w,
			_ => MIN_LABEL,
		};
		let mut asked = None;
		let mut widest = label_width;
		ScrollArea::vertical().auto_shrink(false).id_salt("PropertiesScroll").show(ui, |ui| {
			let inner = ui.available_rect_before_wrap().shrink(PADDING);
			let mut ui = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)));
			let ui = &mut ui;
			style(ui, palette);
			let base = Id::unique("Properties");
			let mut f = Form {
				ui,
				trailing: 0.,
				cx: Cx {
					s,
					app,
					r,
					base,
					out,
					asked: &mut asked,
					focus: &mut self.focus,
					claimed: false,
					label_width,
					widest: MIN_LABEL,
					enabled: true,
					first_row: true,
					comma: s.language() != Language::English,
				},
			};
			match row {
				Row::Folder(kind) => folder(&mut f, kind),
				row => forms::build(&mut f, row),
			}
			widest = f.cx.widest.ceil();
			let used = f.ui.min_rect();
			ui.allocate_rect(Rect::from_min_max(inner.min, pos2(inner.max.x, used.max.y + PADDING)), Sense::hover());
		});
		if widest != label_width {
			self.labels = Some((row, widest));
			ui.ctx().request_discard("the label column");
		}
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

/// A folder's count: "No entries", "1 entry", "N entries".
fn folder(f: &mut Form<'_>, kind: ItemKind) {
	let s = f.cx.s;
	let key = match kind {
		ItemKind::Source => "TreeSources",
		ItemKind::Part => "TreeParts",
		ItemKind::Probe => "TreeProbes",
		ItemKind::Scheme => "TreeColorSchemes",
	};
	let text = match kind.len(&f.cx.app.document.project) {
		0 => s.get("ItemCountZero").to_owned(),
		1 => s.get("ItemCountOne").to_owned(),
		n => s.format("ItemCountMany", &[&n]),
	};
	f.section(s.get(key), |f| f.readout("", &text, ""));
}

/// The form being drawn: the `Ui` of its rows and what the editors share.
pub(super) struct Form<'a> {
	ui: &'a mut Ui,
	pub(super) cx: Cx<'a>,
	/// Room the rows leave free on their right.
	trailing: f32,
}

/// What the editors of a form share.
pub(super) struct Cx<'a> {
	pub(super) s: Strings,
	pub(super) app: &'a App,
	pub(super) r: &'a Readouts,
	base: Id,
	pub(super) out: &'a mut Vec<Event>,
	asked: &'a mut Option<Request>,
	/// The pending request for the first editor's focus.
	focus: &'a mut bool,
	/// The first editor was placed in this pass.
	claimed: bool,
	label_width: f32,
	/// The widest label of this pass.
	widest: f32,
	/// The next row is enabled.
	enabled: bool,
	/// No row in the section yet.
	first_row: bool,
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
		let edited = number_box(ui, rect, id, value, &spec, blank, Some(label));
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
		event: impl Fn(String) -> Event,
	) -> Response {
		if first {
			self.claim(ui, id);
		}
		let edited = text_box::text_box(ui, id, value, ui.available_width(), kind, label);
		if let Some(t) = edited.applied {
			self.out.push(event(t));
		}
		if edited.response.has_focus() || edited.response.lost_focus() {
			edit::offer(ui.ctx(), id, edited.typed.map(event));
		}
		edited.response
	}
}

impl<'a> Form<'a> {
	/// The next row is greyed and its editor disabled unless `enabled`.
	pub(super) fn on(&mut self, enabled: bool) -> &mut Self {
		self.cx.enabled = enabled;
		self
	}

	/// A section: its name over a rule, then its rows.
	pub(super) fn section(&mut self, title: &str, rows: impl FnOnce(&mut Self)) {
		let palette = Palette::of(self.ui.ctx().theme());
		self.ui.add_space(2.);
		let font = FontId::new(FONT_SIZE, wavedude_view::fonts::bold());
		let galley = self.ui.painter().layout_no_wrap(title.to_owned(), font, palette.text);
		let (rect, _) =
			self.ui.allocate_exact_size(vec2(self.ui.available_width(), galley.size().y + 3.), Sense::hover());
		self.ui.painter().galley(rect.min, galley, palette.text);
		let rule = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 1.), rect.max);
		self.ui.painter().rect_filled(rule, 0., palette.border);
		self.ui.add_space(6.);
		self.cx.first_row = true;
		rows(self);
		self.ui.add_space(SECTION_GAP);
	}

	/// A row: `label` in the label column (greyed when disabled, `hint` its tooltip), `editor` in the rest; none
	/// spans both columns.
	pub(super) fn row_of<R>(
		&mut self,
		label: Option<&str>,
		hint: &str,
		editor: impl FnOnce(&mut Ui, &mut Cx<'a>) -> R,
	) -> R {
		let enabled = std::mem::replace(&mut self.cx.enabled, true);
		if !std::mem::replace(&mut self.cx.first_row, false) {
			self.ui.add_space(ROW_SPACING);
		}
		let palette = Palette::of(self.ui.ctx().theme());
		let top = self.ui.cursor().top();
		let (left, width) = (self.ui.max_rect().left(), self.ui.available_width() - self.trailing);
		let column = if label.is_some() { self.cx.label_width + LABEL_GAP } else { 0. };
		let room = Rect::from_min_size(pos2(left + column, top), vec2(width - column, ROW_HEIGHT));
		let layout = Layout::left_to_right(Align::Center);
		// a child the form does not grow with: an editor wider than its room is clipped by the pane
		let mut ui = self.ui.new_child(UiBuilder::new().max_rect(room).layout(layout));
		if !enabled {
			ui.disable();
		}
		let inner = editor(&mut ui, &mut self.cx);
		let height = ui.min_rect().height().max(ROW_HEIGHT);
		let row = Rect::from_min_size(pos2(left, top), vec2(width, height));
		self.ui.advance_cursor_after_rect(row);
		match label {
			Some(text) => {
				let ink = if enabled { palette.text } else { palette.text_disabled };
				let font = FontId::proportional(FONT_SIZE);
				let natural = self.ui.painter().layout_no_wrap(text.to_owned(), font, ink);
				self.cx.widest = self.cx.widest.max(natural.size().x);
				let cell = Rect::from_min_max(row.min, pos2(left + self.cx.label_width, row.max.y));
				let shown = egui::Label::new(egui::RichText::new(text).color(ink)).truncate().selectable(false);
				let response = put_left(self.ui, cell, shown);
				if !hint.is_empty() {
					response.on_hover_text(hint);
				}
			}
			None if !hint.is_empty() => {
				self.ui.interact(row, self.cx.base.with(("hint", top.to_bits())), Sense::hover()).on_hover_text(hint);
			}
			None => {}
		}
		inner
	}

	/// `rows` leaving `width` free on their right; that room, as high as the rows.
	pub(super) fn beside(&mut self, width: f32, rows: impl FnOnce(&mut Self)) -> Rect {
		let (top, right) = (self.ui.cursor().top(), self.ui.max_rect().right());
		self.trailing += width;
		rows(self);
		self.trailing -= width;
		Rect::from_min_max(pos2(right - width, top), pos2(right, self.ui.cursor().top()))
	}

	pub(super) fn row<R>(&mut self, label: &str, hint: &str, editor: impl FnOnce(&mut Ui, &mut Cx<'a>) -> R) -> R {
		self.row_of(Some(label), hint, editor)
	}

	pub(super) fn full<R>(&mut self, hint: &str, editor: impl FnOnce(&mut Ui, &mut Cx<'a>) -> R) -> R {
		self.row_of(None, hint, editor)
	}

	/// A plain value row; it wraps.
	pub(super) fn readout(&mut self, label: &str, text: &str, hint: &str) {
		let enabled = self.cx.enabled;
		self.row(label, hint, |ui, _| {
			let palette = Palette::of(ui.ctx().theme());
			let ink = if enabled { palette.text } else { palette.text_disabled };
			ui.add(egui::Label::new(egui::RichText::new(text).color(ink)).wrap());
		});
	}

	/// A full-width paragraph.
	pub(super) fn paragraph(&mut self, text: &str) {
		self.full("", |ui, _| {
			ui.add(egui::Label::new(text).wrap());
		});
	}

	/// The parts' description header: a chevron and the title, the kind's text under it while open.
	pub(super) fn description(&mut self, title: &str, tip: &str, text: &str, open: bool) {
		let palette = Palette::of(self.ui.ctx().theme());
		self.ui.add_space(2.);
		let font = FontId::new(FONT_SIZE, wavedude_view::fonts::bold());
		let galley = self.ui.painter().layout_no_wrap(title.to_owned(), font, palette.text);
		let size = vec2(self.ui.available_width(), galley.size().y + 3.);
		let (rect, _) = self.ui.allocate_exact_size(size, Sense::hover());
		let response = self.ui.interact(rect, self.cx.base.with("Description"), Sense::click()).on_hover_text(tip);
		response.widget_info(|| WidgetInfo::selected(Role::Button, true, open, title));
		if response.hovered() {
			self.ui.painter().rect_filled(rect, 0., palette.button_hover);
		}
		let rule = if response.has_focus() { palette.accent } else { palette.border };
		self.ui.painter().rect_filled(Rect::from_min_max(pos2(rect.left(), rect.bottom() - 1.), rect.max), 0., rule);
		let chevron = Rect::from_center_size(pos2(rect.left() + 4., rect.center().y - 1.), Vec2::splat(16.));
		let glyph = if open { icons::CHEVRON_DOWN } else { icons::CHEVRON_RIGHT };
		icons::paint(self.ui, chevron, glyph, palette.text);
		self.ui.painter().galley(pos2(rect.left() + 14., rect.top()), galley, palette.text);
		if response.clicked() {
			self.cx.ask(Request::DescriptionOpen(!open));
		}
		self.ui.add_space(6.);
		if open {
			self.ui.add(egui::Label::new(text).wrap());
			self.ui.add_space(SECTION_GAP);
		}
	}

	/// The item's name box: applied on every keystroke, so the tree follows the typing; renames through the
	/// command that keeps references. `T` is the type whose `Name` label it takes.
	pub(super) fn name<T: Fields>(&mut self, row: Row, value: &str) {
		let Row::Item(target) = row else { unreachable!("only an item has a name") };
		let (s, ty) = (self.cx.s, T::TYPE_NAME);
		let label = s.label(ty, "Name");
		self.row(label, s.hint(ty, "Name"), |ui, cx| {
			let id = cx.id(("Name", target));
			let kind = Kind { live: true, ..Kind::default() };
			cx.text(ui, id, value, kind, label, true, |name| Event::Command(Command::Rename { target, name }));
		});
	}
}

/// The rows of one object: labels and hints are `<TYPE_NAME>.<Member>`'s, the bounds the fields' own; `wrap` makes
/// an edit of the object an edit of the project.
pub(super) struct Scope<'o, T: Fields, W> {
	pub(super) obj: &'o T,
	wrap: W,
	/// Number boxes show no value (a setting that does not apply).
	blank: bool,
}

/// A list field of an object: its items and the means to replace them as a whole.
pub(super) struct List<'o, T: Fields, U, W> {
	pub(super) items: &'o [U],
	prop: Prop<T, Vec<U>>,
	wrap: &'o W,
}

impl<T: Fields, U: Clone, W: Fn(T::Edit) -> Edit> List<'_, T, U, W> {
	/// The event that sets the list to the items as `change` leaves them.
	pub(super) fn set(&self, change: impl FnOnce(&mut Vec<U>)) -> Event {
		let mut items = self.items.to_vec();
		change(&mut items);
		Event::Command(Command::Edit((self.wrap)((self.prop.edit)(items))))
	}

	/// The event that sets `p` of item `index` to `value`.
	pub(super) fn set_item<V>(&self, index: usize, p: Prop<U, V>, value: V) -> Event
	where
		U: Fields,
	{
		self.set(|items| {
			(p.edit)(value).apply(&mut items[index]).expect("an edit of a struct's field applies");
		})
	}

	/// An id for `member` of item `index`.
	pub(super) fn id(&self, cx: &Cx<'_>, index: usize, member: &str) -> Id {
		cx.id(((self.wrap)((self.prop.edit)(Vec::new())).field_ref(), index, member))
	}
}

impl<'o, T: Fields, W: Fn(T::Edit) -> Edit> Scope<'o, T, W> {
	pub(super) fn new(obj: &'o T, wrap: W) -> Self {
		Self { obj, wrap, blank: false }
	}

	pub(super) fn nested<U: Fields>(&self, n: Nested<T, U>) -> Scope<'o, U, impl Fn(U::Edit) -> Edit + '_> {
		let wrap = &self.wrap;
		Scope { obj: (n.get)(self.obj), wrap: move |e| wrap((n.wrap)(e)), blank: self.blank }
	}

	/// The scope of the variant `obj` the object is, for a caller that has matched on it.
	pub(super) fn variant_over<'b, U: Fields>(
		&'b self,
		v: Variant<T, U>,
		obj: &'b U,
	) -> Scope<'b, U, impl Fn(U::Edit) -> Edit + 'b> {
		let wrap = &self.wrap;
		Scope { obj, wrap: move |e| wrap((v.wrap)(e)), blank: self.blank }
	}

	/// The scope of an enum variant or an optional composite; where the object is another variant or absent,
	/// `default` with empty number boxes.
	pub(super) fn variant_or<'b, U: Fields>(
		&'b self,
		v: Variant<T, U>,
		default: &'b U,
	) -> Scope<'b, U, impl Fn(U::Edit) -> Edit + 'b>
	where
		'o: 'b,
	{
		match (v.get)(self.obj) {
			Some(obj) => self.variant_over(v, obj),
			None => Scope { blank: true, ..self.variant_over(v, default) },
		}
	}

	/// The same edits over another value: a design's spacing.
	pub(super) fn over<'b>(&'b self, obj: &'b T) -> Scope<'b, T, impl Fn(T::Edit) -> Edit + 'b> {
		let wrap = &self.wrap;
		Scope { obj, wrap: move |e| wrap(e), blank: self.blank }
	}

	pub(super) fn get<V>(&self, p: Prop<T, V>) -> V {
		(p.get)(self.obj)
	}

	/// The event that sets `p` to `value`.
	pub(super) fn event<V>(&self, p: Prop<T, V>, value: V) -> Event {
		Event::Command(Command::Edit((self.wrap)((p.edit)(value))))
	}

	/// The editor id of `p`, `sample` any value of it.
	pub(super) fn id<V>(&self, cx: &Cx<'_>, p: Prop<T, V>, sample: V) -> Id {
		cx.id((self.wrap)((p.edit)(sample)).field_ref())
	}

	/// The list `p` whose items are `items`.
	pub(super) fn list<'l, U: Clone>(&'l self, p: Prop<T, Vec<U>>, items: &'l [U]) -> List<'l, T, U, W> {
		List { items, prop: p, wrap: &self.wrap }
	}

	pub(super) fn label<V>(&self, s: &Strings, p: &Prop<T, V>) -> &'static str {
		s.label(T::TYPE_NAME, p.field.name())
	}

	pub(super) fn hint<V>(&self, s: &Strings, p: &Prop<T, V>) -> &'static str {
		s.hint(T::TYPE_NAME, p.field.name())
	}

	/// A number bounded by the field's own range.
	pub(super) fn number<V: Number>(&self, f: &mut Form<'_>, p: Prop<T, V>, step: f64) {
		let (min, max) = bounds_of(p);
		self.number_spec(f, p, NumberSpec::linear(min, max, step));
	}

	/// A number bounded by `range`, which the field does not declare (a position shared with the parts).
	pub(super) fn number_in<V: Number>(&self, f: &mut Form<'_>, p: Prop<T, V>, range: Range, step: f64) {
		self.number_spec(f, p, NumberSpec::linear(range.min, range.max, step));
	}

	fn number_spec<V: Number>(&self, f: &mut Form<'_>, p: Prop<T, V>, spec: NumberSpec) {
		let spec = if V::INTEGER { spec.with_decimals(0) } else { spec };
		self.number_with(f, p, spec);
	}

	/// A number with `places` decimals, bounded by the field's range, dragged by `drag` a pixel.
	pub(super) fn decimals<V: Number>(&self, f: &mut Form<'_>, p: Prop<T, V>, drag: f64, places: u32) {
		let (min, max) = bounds_of(p);
		self.number_with(f, p, NumberSpec::linear(min, max, 1.).with_decimals(places).with_drag_step(drag));
	}

	pub(super) fn number_with<V: Number>(&self, f: &mut Form<'_>, p: Prop<T, V>, spec: NumberSpec) {
		let s = f.cx.s;
		let label = self.label(&s, &p);
		f.row(label, self.hint(&s, &p), |ui, cx| self.number_editor(ui, cx, p, spec, label, None));
	}

	/// The box of `p` in `ui`, `width` wide (the rest of the row when none).
	pub(super) fn number_editor<V: Number>(
		&self,
		ui: &mut Ui,
		cx: &mut Cx<'_>,
		p: Prop<T, V>,
		spec: NumberSpec,
		label: &str,
		width: Option<f32>,
	) -> Response {
		let value = self.get(p);
		let id = self.id(cx, p, value);
		let event = |v: f64| self.event(p, V::from_f64(v));
		cx.number(ui, id, value.to_f64(), spec, self.blank, width, label, true, event)
	}

	/// A full-width check box carrying its label.
	pub(super) fn check(&self, f: &mut Form<'_>, p: Prop<T, bool>) {
		self.check_as(f, p, |on| *on, |on| on);
	}

	/// As [`Self::check`] over a field that is not a flag: `on` says whether a value is checked, `of` the value a
	/// state makes.
	pub(super) fn check_as<V>(&self, f: &mut Form<'_>, p: Prop<T, V>, on: fn(&V) -> bool, of: fn(bool) -> V) {
		let s = f.cx.s;
		let value = self.get(p);
		let checked = on(&value);
		let id = self.id(&f.cx, p, value);
		let text = self.label(&s, &p);
		let event = f.full(self.hint(&s, &p), |ui, _| check_box(ui, id, text, checked).map(|v| self.event(p, of(v))));
		f.cx.out.extend(event);
	}

	/// A check box in the editor column, the field's label in the label column and `text` beside the box.
	pub(super) fn check_text(&self, f: &mut Form<'_>, p: Prop<T, bool>, text: &str) {
		let s = f.cx.s;
		let checked = self.get(p);
		let id = self.id(&f.cx, p, checked);
		let event = f.row(self.label(&s, &p), self.hint(&s, &p), |ui, _| {
			check_box(ui, id, text, checked).map(|v| self.event(p, v))
		});
		f.cx.out.extend(event);
	}

	/// An enum combo over every member.
	pub(super) fn combo<V: Word>(&self, f: &mut Form<'_>, p: Prop<T, V>) {
		self.combo_of(f, p, V::MEMBERS);
	}

	/// An enum combo over `members`.
	pub(super) fn combo_of<V: Word>(&self, f: &mut Form<'_>, p: Prop<T, V>, members: &[V]) {
		let s = f.cx.s;
		let current = self.get(p);
		let id = self.id(&f.cx, p, current);
		let label = self.label(&s, &p);
		let word = |i: usize| s.enum_word(V::TYPE_NAME, members[i].name());
		let picked = f.row(label, self.hint(&s, &p), |ui, _| {
			let choices = Choices {
				len: members.len(),
				current: members.iter().position(|m| *m == current),
				shown: s.enum_word(V::TYPE_NAME, current.name()),
				word: &|i| word(i).into(),
				swatch: None,
				disabled: None,
				actions: &[],
			};
			combo(ui, id, label, &choices)
		});
		if let Some(i) = picked.filter(|&i| members[i] != current) {
			f.cx.send(self.event(p, members[i]));
		}
	}

	/// A text box of `p`, whose text is `value`, applied on Enter or focus loss, Esc reverting (`multiline` keeps
	/// Enter as newline).
	pub(super) fn text(&self, f: &mut Form<'_>, p: Prop<T, String>, value: &str, multiline: bool) {
		let s = f.cx.s;
		let label = self.label(&s, &p);
		f.row(label, self.hint(&s, &p), |ui, cx| {
			let id = self.id(cx, p, String::new());
			let kind = Kind { multiline, ..Kind::default() };
			cx.text(ui, id, value, kind, label, true, |t| self.event(p, t));
		});
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
		icons::paint(ui, Rect::from_center_size(mark.center(), Vec2::splat(16.)), icons::CHECK, ink);
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
	icons::paint(ui, arrow, icons::DROPDOWN, ink);
	let popup_id = id.with("menu");
	let mut picked = None;
	let shown = Popup::menu(&response).id(popup_id).width(rect.width()).show(|ui| {
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
			let entry = ui
				.add_enabled_ui(reason.is_none(), |ui| {
					menu_bar::choice_entry(ui, &word, c.current == Some(i), false, swatch)
				})
				.inner;
			if let Some(reason) = reason {
				entry.on_disabled_hover_text(reason);
			} else if entry.clicked() {
				picked = Some(i);
			}
		}
		for (k, action) in c.actions.iter().enumerate() {
			if menu_bar::choice_entry(ui, action, false, true, None).clicked() {
				picked = Some(c.len + k);
			}
		}
	});
	if shown.is_some() {
		menu_keys::menu(ui.ctx(), popup_id, response.id);
	}
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
	let text = wavedude_app_state::strings::strip_mnemonic(text);
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
	icons::paint(ui, Rect::from_center_size(rect.center(), Vec2::splat(16.)), icon, ink);
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
