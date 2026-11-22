//! The inspector (DESIGN.md, Outline and inspector): a header with the current item's name and actions, then the form
//! of [`waveduderust_app::form`] as an `Accordion` panel per section. No selection shows the Project form.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::text_edit::TextEditState;
use eframe::egui::{self, Align, Color32, Layout, Response, Sense, Shape, Stroke, StrokeKind};
use egui_mineral::expand::Expansion;
use egui_mineral::number::ScrubRate;
use egui_mineral::overlay::Dismiss;
use egui_mineral::widget::SizeExt as _;
use egui_mineral_material::prelude::{
	Accordion, AccordionPanel, AccordionVariant, ButtonVariant, CheckButton, ColorPicker, IconButton, InlineEdit,
	LeadingPaint, MaterialExt as _, Menu, NumberField, PlainTooltip, Popover, PushButton, Rt, Select, SelectOption,
	TextField, TextFieldVariant, TypeScale, icons,
};
use waveduderust::model::{ItemRef, Rgb, Stop};
use waveduderust_app::Event;
use waveduderust_app::form::{self, Glyph, Out, RowKind};
use waveduderust_app::menus::{MenuAction, On};
use waveduderust_app::outline::{enabled_of, folder_key};
use waveduderust_app::properties::{Readouts, readouts};
use waveduderust_app::selection::Row;
use waveduderust_app::strings::{Strings, strip_mnemonic};
use waveduderust_egui_preview::gradient_editor::{GradientColors, gradient_editor, gradient_mesh};

use super::number;
use crate::form as rows;
use crate::menus::{self, row_icon};
use crate::shell::Intent;
use crate::{App, Mui, Salt};

/// The label column's share of a row.
const LABEL_SHARE: f32 = 0.4;
const GAP: f32 = 8.;
/// A row's controls beside its fields: a button's footprint.
const BUTTON_W: f32 = 36.;
/// A grid row's height: the fields'.
const FIELD_H: f32 = 40.;
/// A multi-line text box's lines, and the lines it grows by before it scrolls.
const MULTILINE_ROWS: usize = 3;
const MULTILINE_GROW: usize = 7;
/// The sections' titles.
const TITLE: TypeScale = TypeScale::TitleSmall;
/// A colour stop's swatch.
const SWATCH: egui::Vec2 = egui::Vec2::new(40., 32.);
/// A scheme's swatch in a picker.
const PICKER_SWATCH: egui::Vec2 = egui::Vec2::new(40., 12.);

/// What takes the focus when the form shows next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
	/// The form's first editor, when it can take the focus.
	First,
	/// The item's name.
	Name,
}

/// The inspector's state between passes.
#[derive(Default)]
pub(super) struct Inspector {
	focus: Option<Focus>,
	/// The name's editor is open.
	naming: bool,
	/// The header's overflow menu is open.
	more_open: bool,
	/// The read-outs, and the document, selection and settings revisions they were made at.
	readouts: Option<((u64, u64, u64), Row, Readouts)>,
	/// The open panels of each form, by its section titles.
	panels: HashMap<u64, Expansion<usize>>,
}

/// What the rows of a pass share.
struct Cx<'a> {
	s: Strings,
	out: &'a mut Vec<Event>,
	intents: &'a mut Vec<Intent>,
	focus: &'a mut Option<Focus>,
	naming: &'a mut bool,
	more_open: &'a mut bool,
	/// The rows the header shows.
	skip: [Option<form::Key>; 2],
	/// The first editor was placed.
	placed: bool,
	/// Title of the section being drawn.
	section: &'static str,
}

impl Cx<'_> {
	fn apply(&mut self, outs: Vec<Out>) {
		for out in outs {
			match out {
				Out::Send(event) => self.out.push(event),
				Out::Ask(request) => self.intents.push(Intent::Ask(request)),
			}
		}
	}

	/// Whether the editor placed now takes the focus asked for the form's first.
	fn first(&mut self) -> bool {
		!std::mem::replace(&mut self.placed, true) && self.focus.take_if(|f| *f == Focus::First).is_some()
	}
}

impl Inspector {
	/// The form's first editor takes the focus when the form shows next.
	pub(super) fn focus_first(&mut self) {
		self.focus = Some(Focus::First);
	}

	/// The current item's name opens for editing when the form shows next.
	pub(super) fn focus_name(&mut self) {
		self.focus = Some(Focus::Name);
	}

	pub(super) fn show(&mut self, ui: &mut Mui<'_>, app: &App, out: &mut Vec<Event>, intents: &mut Vec<Intent>) {
		let project = &app.document.project;
		let rt = ui.rt();
		let row = app.selection.current().filter(|r| !matches!(r, Row::Item(i) if !i.exists(project)));
		let row = row.unwrap_or(Row::Project);
		let item = match row {
			Row::Item(i) => Some(i),
			_ => None,
		};
		let skip = [item.map(form::name_key), item.and_then(form::enabled_key)];
		let mut cx = Cx {
			s: app.strings,
			out,
			intents,
			focus: &mut self.focus,
			naming: &mut self.naming,
			more_open: &mut self.more_open,
			skip,
			placed: false,
			section: "",
		};
		if let Row::Folder(kind) = row {
			header(ui, app, &mut cx, row, None);
			let s = &app.strings;
			let text = match kind.len(project) {
				0 => s.get("ItemCountZero").to_owned(),
				1 => s.get("ItemCountOne").to_owned(),
				n => s.format("ItemCountMany", &[&n]),
			};
			rows::text(ui, &text, TypeScale::BodyMedium, true);
			*cx.focus = None;
			return;
		}
		let revisions = app.revisions();
		let key = (revisions.document, revisions.selection, revisions.settings);
		if self.readouts.as_ref().is_none_or(|(k, at, _)| *k != key || *at != row) {
			self.readouts = Some((key, row, readouts(app, row)));
		}
		let r = &self.readouts.as_ref().expect("made above").2;
		let model = form::build(form::Cx { s: app.strings, app, r }, row);
		let name = model.as_ref().and_then(|m| {
			m.rows().find_map(|r| match &r.kind {
				RowKind::Text(t) if Some(t.key) == skip[0] => Some(t),
				_ => None,
			})
		});
		header(ui, app, &mut cx, row, name);
		let Some(model) = model else { return };
		if let Some(d) = &model.description {
			let mut open = Expansion::default();
			if d.open {
				open.expand(0);
			}
			let r =
				Accordion::new("inspector-description", &mut open).variant(AccordionVariant::Flat).show(ui, |acc| {
					acc.panel(0, AccordionPanel::new(d.title).title_style(TITLE), |ui| {
						rows::text(ui, d.text, TypeScale::BodyMedium, true);
					});
				});
			if r.toggled.is_some() {
				cx.intents.push(Intent::Ask(form::Request::DescriptionOpen(!d.open)));
			}
		}
		// the header's rows leave General empty for some kinds
		let sections: Vec<_> = model.sections.iter().filter(|s| s.rows.iter().any(|r| !row_skipped(&cx, r))).collect();
		let titles = {
			let mut h = std::hash::DefaultHasher::new();
			for s in &sections {
				s.title.hash(&mut h);
			}
			h.finish()
		};
		let salt = Salt(row);
		let panels = self.panels.entry(titles).or_insert_with(|| {
			let mut e = Expansion::default();
			for i in 0..sections.len() {
				e.expand(i);
			}
			e
		});
		egui::ScrollArea::vertical().id_salt("inspector-scroll").auto_shrink([false, true]).show(ui, |ui| {
			let accordion = Accordion::new(("inspector-form", salt), panels).variant(AccordionVariant::Flat);
			accordion.show(&mut rt.ui(ui), |acc| {
				for (i, section) in sections.iter().enumerate() {
					acc.panel(i, AccordionPanel::new(section.title).title_style(TITLE), |ui| {
						cx.section = section.title;
						body(ui, &mut cx, &section.rows);
					});
				}
			});
		});
		// a focus asked for an editor the form lacks lapses
		if cx.placed {
			self.focus = None;
		}
	}
}

/// The header: the row's icon and title, for an item its name to rename in place ("`<name>` and n more" for several
/// items, not editable), and Enabled, Duplicate, Remove and the overflow menu.
fn header(ui: &mut Mui<'_>, app: &App, cx: &mut Cx<'_>, row: Row, name: Option<&form::Text<'_>>) {
	let s = &app.strings;
	let project = &app.document.project;
	let rt = ui.rt();
	let several = app.selection.items().len() > 1;
	let title = match row {
		Row::Project => s.get("TreeProject").to_owned(),
		Row::Scene => s.get("TreeScene").to_owned(),
		Row::Simulation => s.get("TreeSimulation").to_owned(),
		Row::Display => s.get("TreeDisplay").to_owned(),
		Row::Folder(kind) => s.get(folder_key(kind)).to_owned(),
		Row::Item(i) => {
			let name = i.name(project).unwrap_or_default();
			match app.selection.items().len() {
				0 | 1 => name.to_owned(),
				n => s.format("InspectorMore", &[&name, &(n - 1)]),
			}
		}
	};
	// the name is the form's first editor
	let focus = name.and_then(|_| cx.focus.take());
	cx.placed |= name.is_some();
	// the actions first, so the name takes the width they leave
	egui::Sides::new().height(FIELD_H).shrink_left().show(
		ui,
		|ui| {
			let c = rt.tokens().color;
			ui.add(
				egui::Label::new(icon_text(row_icon(project, row), 20.).color(c.on_surface_variant)).selectable(false),
			);
			let ui = &mut rt.ui(ui);
			match name.filter(|_| !several) {
				Some(t) => {
					let mut value = t.value.to_string();
					*cx.naming |= focus == Some(Focus::Name);
					let edit = InlineEdit::new(("inspector-name", t.key), t.label, &mut value)
						.text_style(TITLE)
						.editing(cx.naming);
					let w = ui.available_width();
					let r = ui.mineral(edit.preferred_width(w));
					if focus == Some(Focus::First) {
						r.response().request_focus();
					}
					if value != t.value
						&& let Some(event) = (t.apply)(value)
					{
						cx.out.push(event);
					}
				}
				None => {
					*cx.naming = false;
					let font = TITLE.font(ui.ctx());
					ui.add(egui::Label::new(egui::RichText::new(&title).font(font).color(c.on_surface)).truncate());
				}
			}
		},
		|ui| {
			if let Row::Item(item) = row {
				actions(&mut rt.ui(ui), app, item, cx.intents, cx.more_open);
			}
		},
	);
}

/// An item's header actions, right to left: More (with Duplicate and Remove) and the Enabled toggle.
fn actions(ui: &mut Mui<'_>, app: &App, item: ItemRef, intents: &mut Vec<Intent>, more_open: &mut bool) {
	let s = &app.strings;
	let project = &app.document.project;
	let rt = ui.rt();
	let more = IconButton::new("inspector-more", icons::ICON_MORE_VERT, s.get("NavMore"));
	let anchor = ui.mineral(more);
	menus::command_menu(app, Menu::new("inspector-more-menu", s.get("NavMore")), &menus::header_menu(app)).show(
		rt,
		anchor.response(),
		more_open,
	);
	if let Some(on) = enabled_of(project, item) {
		let mut now = on;
		let icon = if on { icons::ICON_VISIBILITY } else { icons::ICON_VISIBILITY_OFF };
		let label = strip_mnemonic(s.get("MenuProjectToggleEnabled"));
		ui.mineral(IconButton::toggle("inspector-enabled", icon, label, &mut now));
		if now != on {
			intents.push(Intent::On(MenuAction::ToggleEnabled, On::Current));
		}
	}
}

/// A grid row:  `label` in the label column and `control` in the value column, given its width; no label gives the
/// control the whole row.
fn field_row(ui: &mut Mui<'_>, label: Option<&str>, control: impl FnOnce(&mut Mui<'_>, f32)) {
	let Some(label) = label else {
		let w = ui.available_width();
		return control(ui, w);
	};
	let rt = ui.rt();
	let label_w = (ui.available_width() * LABEL_SHARE).floor();
	let size = egui::vec2(ui.available_width(), FIELD_H);
	// the label centres on the height the control takes, which may exceed FIELD_H (touch-sized boxes)
	let row = ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
		ui.add_space(label_w);
		let ui = &mut rt.ui(ui);
		let w = ui.available_width();
		control(ui, w);
	});
	let rect = row.response.rect;
	let c = rt.tokens().color;
	let font = TypeScale::BodyMedium.font(ui.ctx());
	let text = egui::WidgetText::from(egui::RichText::new(label).font(font).color(c.on_surface_variant));
	let galley = text.into_galley(ui, Some(egui::TextWrapMode::Truncate), label_w - GAP, egui::TextStyle::Body);
	let pos = egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.);
	ui.painter().galley(pos, galley, c.on_surface_variant);
}

/// A section's rows, one grid row each.
fn body(ui: &mut Mui<'_>, cx: &mut Cx<'_>, list: &[form::Row<'_>]) {
	for row in list {
		if row_skipped(cx, row) {
			continue;
		}
		draw(ui, cx, row);
		ui.add_space(GAP / 2.);
	}
}

fn row_skipped(cx: &Cx<'_>, row: &form::Row<'_>) -> bool {
	let key = match &row.kind {
		RowKind::Text(t) => t.key,
		RowKind::Check(c) => c.key,
		_ => return false,
	};
	cx.skip.contains(&Some(key))
}

/// A number field `width` wide, `prefix`ed with its label where it shares a row; a commit sends its event.
fn number(
	ui: &mut Mui<'_>,
	cx: &mut Cx<'_>,
	n: &form::Number<'_>,
	enabled: bool,
	hint: &str,
	width: f32,
	prefix: bool,
) {
	let focus = cx.first();
	let place = |ui: &mut Mui<'_>, field: NumberField<'_>| {
		let field = field.scrub(ScrubRate::Extent).label_visible(false).disabled(!enabled);
		let field = if prefix { field.prefix(n.label) } else { field };
		let field = match n.unit {
			Some(unit) => field.suffix(unit),
			None => field,
		};
		let field = if hint.is_empty() { field } else { field.tooltip(hint) };
		ui.mineral(field.preferred_width(width))
	};
	let (r, change) = number::show(ui, ("inspector-number", n.key), n.label, &n.spec, n.value, n.blank, place);
	if let Some(id) = r.output().field.filter(|_| focus) {
		focus_selected(ui.ctx(), id);
	}
	if change.scrub_began {
		cx.out.push(Event::MouseDown);
	}
	if let Some(v) = change.value {
		cx.out.push((n.apply)(v));
	}
	if change.scrub_ended {
		cx.out.push(Event::MouseUp);
	}
}

/// `n`'s field of `width` under its label as a caption: a part of a row of numbers.
fn captioned(ui: &mut Mui<'_>, cx: &mut Cx<'_>, n: &form::Number<'_>, enabled: bool, hint: &str, width: f32) {
	let rt = ui.rt();
	ui.vertical(|ui| {
		let ui = &mut rt.ui(ui);
		let font = TypeScale::LabelMedium.font(ui.ctx());
		let ink = rt.tokens().color.on_surface_variant;
		ui.add(egui::Label::new(egui::RichText::new(n.label).font(font).color(ink)).selectable(false));
		number(ui, cx, n, enabled, hint, width, false);
	});
}

/// A text field of `width`; `focus` gives it the focus with its text selected.
fn text(ui: &mut Mui<'_>, cx: &mut Cx<'_>, t: &form::Text<'_>, enabled: bool, hint: &str, width: f32) {
	let focus = cx.first();
	let mut value = t.value.to_string();
	let mut field = TextField::new(("inspector-text", t.key), t.label, &mut value)
		.variant(TextFieldVariant::Outlined)
		.label_visible(false)
		.commit_on_submit(!t.live)
		.disabled(!enabled);
	if t.multiline {
		field = field.rows(MULTILINE_ROWS).grow_rows(MULTILINE_GROW).text_style(TypeScale::BodyMedium);
	}
	if !hint.is_empty() {
		field = field.tooltip(hint);
	}
	let r = ui.mineral(field.preferred_width(width));
	if focus {
		focus_selected(ui.ctx(), r.response().id);
	}
	if value != t.value
		&& let Some(event) = (t.apply)(value)
	{
		cx.out.push(event);
	}
}

/// The text edit `id` takes the focus with its text selected; the edit clamps the range to its text.
pub(super) fn focus_selected(ctx: &egui::Context, id: egui::Id) {
	ctx.memory_mut(|m| m.request_focus(id));
	let mut state = TextEditState::load(ctx, id).unwrap_or_default();
	state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(usize::MAX))));
	state.store(ctx, id);
}

/// A read-out: `label` and `text` as a description list.
/// A read-only value in the grid's value column.
fn readout(ui: &mut Mui<'_>, label: Option<&str>, text: &str, hint: &str) {
	let rt = ui.rt();
	field_row(ui, label, |ui, _| {
		let font = TypeScale::BodyMedium.font(ui.ctx());
		let ink = rt.tokens().color.on_surface;
		let r = ui.add(egui::Label::new(egui::RichText::new(text).font(font).color(ink)).truncate());
		tip(rt, &r, hint);
	});
}

fn tip(rt: &Rt, response: &Response, hint: &str) {
	if !hint.is_empty() {
		PlainTooltip::new(hint).show(rt, response);
	}
}

/// A button: text, or the icon of its glyph.
fn button(ui: &mut Mui<'_>, cx: &mut Cx<'_>, b: &form::Button<'_>, enabled: bool, variant: ButtonVariant) {
	let enabled = enabled && b.enabled;
	let clicked = match b.glyph {
		Some(g) => {
			let label = if b.tip.is_empty() { b.text } else { b.tip };
			ui.mineral(IconButton::new(("inspector-button", b.key), glyph_icon(g), label).disabled(!enabled)).clicked()
		}
		None => {
			let button = PushButton::new(("inspector-button", b.key), strip_mnemonic(b.text))
				.variant(variant)
				.disabled(!enabled);
			let r = ui.mineral(button);
			tip(ui.rt(), r.response(), b.tip);
			r.clicked()
		}
	};
	if clicked {
		cx.apply((b.press)());
	}
}

fn glyph_icon(g: Glyph) -> egui_mineral_material::MaterialIcon {
	match g {
		Glyph::Save => icons::ICON_SAVE,
		Glyph::Close => icons::ICON_CLOSE,
		Glyph::Minus => icons::ICON_REMOVE,
		Glyph::Up => icons::ICON_ARROW_UPWARD,
		Glyph::Down => icons::ICON_ARROW_DOWNWARD,
		Glyph::Linked => icons::ICON_LINK,
		Glyph::Parted => icons::ICON_LINK_OFF,
	}
}

/// `c`'s box; `labeled` shows its text beside it, else the grid's label column carries it.
fn check(ui: &mut Mui<'_>, cx: &mut Cx<'_>, c: &form::Check<'_>, enabled: bool, hint: &str, labeled: bool) {
	let focus = cx.first();
	let mut on = c.on;
	let r = ui.mineral(
		CheckButton::new(("inspector-check", c.key), c.text, &mut on).label_visible(labeled).disabled(!enabled),
	);
	tip(ui.rt(), r.response(), hint);
	if focus {
		r.response().request_focus();
	}
	if on != c.on {
		cx.out.extend((c.apply)(on));
	}
}

fn choice(ui: &mut Mui<'_>, cx: &mut Cx<'_>, c: &form::Choice<'_>, enabled: bool, hint: &str, width: f32) {
	let focus = cx.first();
	let options = c.words.iter().enumerate().map(|(i, word)| {
		let option = SelectOption::new(word.as_ref());
		match c.swatches.get(i).copied().flatten() {
			Some(stops) => option.leading(swatch_paint(stops)),
			None => option,
		}
	});
	let actions = c.actions.iter().map(|a| SelectOption::new(strip_mnemonic(a)));
	let mut picked = c.current;
	let select = Select::optional(("inspector-choice", c.key), c.label, &mut picked)
		.items(options.chain(actions).enumerate())
		.variant(TextFieldVariant::Outlined)
		.label_visible(false)
		.disabled(!enabled);
	let select = if hint.is_empty() { select } else { select.tooltip(hint) };
	let r = ui.mineral(select.preferred_width(width));
	if focus {
		r.response().request_focus();
	}
	if picked != c.current
		&& let Some(i) = picked
	{
		cx.apply((c.pick)(i));
	}
}

/// A scheme's gradient as a picker's swatch.
fn swatch_paint(stops: &[Stop]) -> LeadingPaint {
	let stops: Arc<[Stop]> = stops.into();
	LeadingPaint::new(move |painter, slot, _| {
		let rect = egui::Rect::from_center_size(slot.center(), PICKER_SWATCH.min(slot.size()));
		painter.add(Shape::mesh(gradient_mesh(rect, &stops)));
	})
}

/// A colour swatch that opens the picker; Esc in the picker puts the colour back.
fn color(ui: &mut Mui<'_>, cx: &mut Cx<'_>, c: &form::Color<'_>) {
	let rt = ui.rt();
	let rgb = |c: Rgb| Color32::from_rgb(c.r, c.g, c.b);
	let (rect, trigger) = ui.allocate_exact_size(SWATCH, Sense::click());
	let id = ui.make_persistent_id(("inspector-color", c.key));
	let t = ui.tokens();
	ui.painter().rect_filled(rect, 4., rgb(c.value));
	ui.painter().rect_stroke(rect, 4., Stroke::new(1., t.color.outline), StrokeKind::Inside);
	crate::look::focus_ring(ui, &trigger, &t);
	let trigger = trigger.on_hover_cursor(egui::CursorIcon::PointingHand);
	tip(rt, &trigger, c.tip);
	if trigger.clicked() {
		ui.data_mut(|d| d.insert_temp(id, c.value));
	}
	let mut now = rgb(c.value);
	let open_id = id.with("open");
	let mut open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
	// Space and Enter click the focused swatch; Down opens it too, Enter in the open picker keeps and closes unless a field in it has the focus (it commits first).
	let opening = trigger.clicked();
	if trigger.has_focus() && !open && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
		ui.data_mut(|d| d.insert_temp(id, c.value));
		open = true;
	} else if open
		&& !opening
		&& !trigger.has_focus()
		&& !ui.ctx().text_edit_focused()
		&& ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
	{
		open = false;
	}
	let popover = Popover::new(("inspector-color-popover", c.key), c.tip).show(rt, &trigger, &mut open, |ui| {
		ui.mineral(ColorPicker::new(("inspector-picker", c.key), c.tip, &mut now).rgb_fields(true));
	});
	ui.data_mut(|d| d.insert_temp(open_id, open));
	let picked = Rgb::default().with_r(now.r()).with_g(now.g()).with_b(now.b());
	if picked != c.value {
		cx.out.push((c.apply)(picked));
	}
	if popover.dismissed == Some(Dismiss::Escape)
		&& let Some(before) = ui.data(|d| d.get_temp::<Rgb>(id))
		&& before != c.value
	{
		cx.out.push((c.apply)(before));
	}
}

/// `row` of its section as grid rows.
fn draw(ui: &mut Mui<'_>, cx: &mut Cx<'_>, row: &form::Row<'_>) {
	let (on, hint) = (row.enabled, row.hint);
	let rt = ui.rt();
	match &row.kind {
		RowKind::Number(n) => {
			field_row(ui, Some(row.label.unwrap_or(n.label)), |ui, w| number(ui, cx, n, on, hint, w, false));
		}
		RowKind::NumberButton(n, b) => field_row(ui, Some(row.label.unwrap_or(n.label)), |ui, _| {
			// the button first, so the field takes the width it leaves
			ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
				let ui = &mut rt.ui(ui);
				button(ui, cx, b, on, ButtonVariant::Text);
				let w = ui.available_width();
				number(ui, cx, n, on, hint, w, false);
			});
		}),
		RowKind::Check(c) => {
			let labeled = row.label.is_some();
			field_row(ui, Some(row.label.unwrap_or(c.text)), |ui, _| check(ui, cx, c, on, hint, labeled));
		}
		RowKind::Choice(c) => field_row(ui, Some(row.label.unwrap_or(c.label)), |ui, w| choice(ui, cx, c, on, hint, w)),
		RowKind::Text(t) if t.multiline => {
			rows::text(ui, row.label.unwrap_or(t.label), TypeScale::BodyMedium, true);
			let w = ui.available_width();
			text(ui, cx, t, on, hint, w);
		}
		RowKind::Text(t) => field_row(ui, Some(row.label.unwrap_or(t.label)), |ui, w| text(ui, cx, t, on, hint, w)),
		RowKind::Image { name, browse, save, remove } => {
			field_row(ui, row.label, |ui, w| text(ui, cx, name, on, hint, w));
			ui.horizontal(|ui| {
				let ui = &mut rt.ui(ui);
				button(ui, cx, browse, on, ButtonVariant::Outlined);
				button(ui, cx, save, on, ButtonVariant::Outlined);
				button(ui, cx, remove, on, ButtonVariant::Outlined);
			});
		}
		RowKind::Readout(text) | RowKind::Value(text) => readout(ui, row.label, text, hint),
		RowKind::Paragraph(text) => rows::text(ui, text, TypeScale::BodySmall, true),
		RowKind::Buttons(buttons) => {
			ui.horizontal_wrapped(|ui| {
				let ui = &mut rt.ui(ui);
				for b in buttons {
					button(ui, cx, b, on, ButtonVariant::Outlined);
				}
			});
		}
		RowKind::Group(parts) => {
			// the parts need the pane's width; the label goes above unless the section already says it
			if let Some(label) = row.label.filter(|l| *l != cx.section) {
				rows::text(ui, label, TypeScale::BodyMedium, true);
			}
			ui.horizontal(|ui| {
				let ui = &mut rt.ui(ui);
				let n = parts.len().max(1) as f32;
				let w = (ui.available_width() - (n - 1.) * GAP) / n;
				ui.spacing_mut().item_spacing.x = GAP;
				for r in parts {
					if let RowKind::Number(num) = &r.kind {
						captioned(ui, cx, num, on && r.enabled, r.hint, w);
					}
				}
			});
		}
		RowKind::Linked(pair, chain) => {
			ui.horizontal(|ui| {
				let ui = &mut rt.ui(ui);
				let w = (ui.available_width() - BUTTON_W - 3. * GAP) / 2.;
				ui.spacing_mut().item_spacing.x = GAP;
				for r in &**pair {
					if let RowKind::Number(n) = &r.kind {
						captioned(ui, cx, n, r.enabled, r.hint, w);
					}
				}
				let mut linked = chain.glyph == Some(Glyph::Linked);
				let b = IconButton::toggle(
					("inspector-button", chain.key),
					glyph_icon(Glyph::Linked),
					chain.tip,
					&mut linked,
				);
				// level with the fields, under the captions
				let changed = ui.vertical(|ui| {
					ui.add_space(TypeScale::LabelMedium.line_height());
					rt.ui(ui).mineral(b).changed()
				});
				if changed.inner {
					cx.apply((chain.press)());
				}
			});
		}
		RowKind::Points(p) => {
			for item in &p.items {
				ui.horizontal(|ui| {
					let ui = &mut rt.ui(ui);
					let w = (ui.available_width() - BUTTON_W - 3. * GAP) / 2.;
					number(ui, cx, &item.x, on, item.x.hint, w, true);
					number(ui, cx, &item.y, on, item.y.hint, w, true);
					button(ui, cx, &item.remove, on, ButtonVariant::Outlined);
				});
			}
			button(ui, cx, &p.add, on, ButtonVariant::Outlined);
		}
		RowKind::Layers(l) => {
			for (i, item) in l.items.iter().enumerate() {
				ui.horizontal(|ui| {
					let ui = &mut rt.ui(ui);
					rows::text(ui, &(i + 1).to_string(), TypeScale::LabelLarge, true);
					let w = (ui.available_width() - 3. * BUTTON_W - 5. * GAP) / 2.;
					number(ui, cx, &item.thickness, on, item.thickness.hint, w, true);
					number(ui, cx, &item.index, on, item.index.hint, w, true);
					button(ui, cx, &item.up, on && i > 0, ButtonVariant::Outlined);
					button(ui, cx, &item.down, on && i + 1 < l.items.len(), ButtonVariant::Outlined);
					button(ui, cx, &item.remove, on, ButtonVariant::Outlined);
				});
			}
			ui.horizontal(|ui| {
				let ui = &mut rt.ui(ui);
				button(ui, cx, &l.add, on, ButtonVariant::Outlined);
				button(ui, cx, &l.clear, on, ButtonVariant::Outlined);
			});
		}
		RowKind::Nodes(n) => {
			for (k, item) in n.items.iter().enumerate() {
				rows::heading(ui, &cx.s.format("LabelNodeNumber", &[&(k + 1)]));
				for (label, x, y) in &item.lines {
					field_row(ui, Some(label), |ui, w| {
						let w = (w - GAP) / 2.;
						number(ui, cx, x, on, x.hint, w, true);
						number(ui, cx, y, on, y.hint, w, true);
					});
				}
				ui.horizontal(|ui| {
					let ui = &mut rt.ui(ui);
					check(ui, cx, &item.smooth, on, "", true);
					button(ui, cx, &item.remove, on, ButtonVariant::Outlined);
				});
			}
			button(ui, cx, &n.add, on, ButtonVariant::Outlined);
		}
		RowKind::Gradient(bar) => {
			let c = ui.tokens().color;
			let colors = GradientColors { hairline: c.outline_variant, accent: c.primary };
			let id = ui.make_persistent_id(bar.key);
			if let Some(stops) = gradient_editor(ui, id, &bar.gradient, bar.tip, colors) {
				cx.out.push((bar.apply)(stops));
			}
		}
		// the picker has the hex field, so the row has none
		RowKind::Stop(stop) => {
			let label = format!("{} {}", cx.s.get("LabelStop"), stop.ordinal);
			field_row(ui, Some(&label), |ui, w| {
				number(ui, cx, &stop.position, on, stop.position.hint, w - SWATCH.x - BUTTON_W - 2. * GAP, false);
				color(ui, cx, &stop.color);
				button(ui, cx, &stop.remove, on, ButtonVariant::Outlined);
			});
		}
	}
}

/// `icon` as text of `size` points in its own font.
fn icon_text(icon: egui_mineral_material::MaterialIcon, size: f32) -> egui::RichText {
	let glyph = egui_mineral::icon::Icon::from(icon).regular();
	egui::RichText::new(glyph.ch().to_string()).font(glyph.font_id(size))
}
