//! The text box of docs/ui/30 (Text box): typed text applies on Enter (single line, the text then selected) or
//! when the focus leaves, Esc puts the value back and keeps the key only when something was typed; a live box
//! applies every keystroke. The box selects its text when it gains the focus.

use eframe::egui::text::{CCursor, CCursorRange};
use eframe::egui::{self, EventFilter, Id, Key, Modifiers, Rect, Response, Role, Stroke, StrokeKind, Ui, WidgetInfo};

use super::edit;
use super::theme::{INPUT_HEIGHT, Palette};

/// Selects the `chars` characters of the text edit `id`.
pub(super) fn select_all(ui: &Ui, id: Id, chars: usize) {
	if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
		state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(chars))));
		state.store(ui.ctx(), id);
	}
}

/// While `typed`, Esc reaches the focused box `id` in the next pass, which reverts it, instead of taking its focus.
pub(super) fn hold_escape(ui: &Ui, id: Id, typed: bool) {
	let filter = EventFilter { horizontal_arrows: true, vertical_arrows: true, tab: false, escape: typed };
	ui.memory_mut(|m| m.set_focus_lock_filter(id, filter));
}

/// How a text box applies its text.
#[derive(Clone, Copy, Default)]
pub(super) struct Kind {
	/// Enter is a newline; the text wraps.
	pub(super) multiline: bool,
	/// Every keystroke applies.
	pub(super) live: bool,
}

/// What a text box did in a frame.
pub(super) struct TextEdited {
	pub(super) response: Response,
	/// The text it applied.
	pub(super) applied: Option<String>,
	/// The text its typing would apply, for [`edit::offer`].
	pub(super) typed: Option<String>,
}

/// The box `id` showing `value` across `width`, as `kind` says; `label` names it for assistive technology.
/// Disabled with its `Ui`.
pub(super) fn text_box(ui: &mut Ui, id: Id, value: &str, width: f32, kind: Kind, label: &str) -> TextEdited {
	let palette = Palette::of(ui.ctx().theme());
	let enabled = ui.is_enabled();
	let focused = ui.memory(|m| m.has_focus(id));
	let typed_id = edit::typed_id(id);
	let stored = if focused && !kind.live { ui.data(|d| d.get_temp::<String>(typed_id)) } else { None };
	let mut text = stored.unwrap_or_else(|| value.to_owned());
	let mut enter = false;
	if focused {
		let typed = text != value;
		ui.input_mut(|i| {
			if !kind.multiline {
				enter = i.consume_key(Modifiers::NONE, Key::Enter);
			}
			if typed && !kind.live && i.consume_key(Modifiers::NONE, Key::Escape) {
				text = value.to_owned();
			}
		});
	}
	let ink = if enabled { palette.text } else { palette.text_disabled };
	let edit =
		if kind.multiline { egui::TextEdit::multiline(&mut text) } else { egui::TextEdit::singleline(&mut text) };
	let edit = edit
		.id(id)
		.frame(egui::Frame::NONE)
		.margin(egui::Margin::symmetric(3, 3))
		.text_color(ink)
		.return_key(None)
		.min_size(egui::vec2(width, INPUT_HEIGHT))
		.desired_width(width - 6.)
		.desired_rows(1)
		.align(if kind.multiline { egui::Align2::LEFT_TOP } else { egui::Align2::LEFT_CENTER });
	let (painted, frame) = (ui.painter().add(egui::Shape::Noop), ui.painter().clone());
	let response = ui.add(edit);
	let hovered = enabled && response.hovered();
	let border = if response.has_focus() {
		palette.accent
	} else if hovered {
		palette.input_hover_border
	} else {
		palette.border
	};
	let fill = if enabled { palette.input_background } else { palette.window_background };
	let rect = Rect::from_min_size(response.rect.min, response.rect.size().max(egui::vec2(width, INPUT_HEIGHT)));
	frame.set(painted, egui::epaint::RectShape::new(rect, 0., fill, Stroke::new(1., border), StrokeKind::Inside));
	if response.gained_focus() {
		select_all(ui, id, text.chars().count());
	}
	let typed = text != value;
	let mut applied = None;
	if kind.live {
		applied = typed.then(|| text.clone());
	} else if (enter || response.lost_focus()) && typed {
		applied = Some(text.clone());
	}
	if enter {
		select_all(ui, id, text.chars().count());
	}
	hold_escape(ui, id, typed && !kind.live && applied.is_none());
	if response.has_focus() && !kind.live && applied.is_none() {
		ui.data_mut(|d| d.insert_temp(typed_id, text.clone()));
	} else if response.lost_focus() || applied.is_some() {
		ui.data_mut(|d| d.remove::<String>(typed_id));
	}
	response.widget_info(|| {
		let role = if kind.multiline { Role::MultilineTextInput } else { Role::TextInput };
		WidgetInfo::labeled(role, enabled, label)
	});
	let typed = (response.has_focus() && !kind.live && typed && applied.is_none()).then_some(text);
	TextEdited { response, applied, typed }
}
