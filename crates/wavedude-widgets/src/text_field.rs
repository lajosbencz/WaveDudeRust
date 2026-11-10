//! Single-line text editor on `freya-edit`: the field inside the text and number boxes, with a select-all
//! on focus that Freya's `Input` cannot offer.

use freya::{
	elements::paragraph::{ParagraphCursorExt, ParagraphHolderInner},
	prelude::*,
	text_edit::*,
	torin::prelude::{Area, Point2D, Vector2D},
};

use crate::{theme::use_wave_theme, util::is_named};

/// WPF's text padding 3,2 inside the field's 1 px border.
pub(crate) const FIELD_PADDING: (f32, f32) = (2., 3.);

/// A borderless single-line editor for inside [`crate::text_box::field_frame`].
#[derive(Clone, PartialEq)]
pub struct TextField {
	value: WritableSetter<String>,
	enabled: bool,
	select_all_on_focus: bool,
	width: Size,
	id: Option<AccessibilityId>,
	on_pre_key_down: Option<Callback<Event<KeyboardEventData>, bool>>,
}

impl TextField {
	/// `value` receives the text after every edit.
	pub fn new(value: impl Into<WritableSetter<String>>) -> Self {
		Self {
			value: value.into(),
			enabled: true,
			select_all_on_focus: false,
			width: Size::px(150.),
			id: None,
			on_pre_key_down: None,
		}
	}

	pub fn enabled(mut self, enabled: bool) -> Self {
		self.enabled = enabled;
		self
	}

	/// Gaining focus selects the whole text.
	pub fn select_all_on_focus(mut self, select_all: bool) -> Self {
		self.select_all_on_focus = select_all;
		self
	}

	pub fn width(mut self, width: impl Into<Size>) -> Self {
		self.width = width.into();
		self
	}

	pub fn a11y_id(mut self, id: impl Into<AccessibilityId>) -> Self {
		self.id = Some(id.into());
		self
	}

	/// Called first for each key press: `false` skips the field's own handling. By default the field takes
	/// every key but Tab, Enter and Esc and stops it from bubbling.
	pub fn on_pre_key_down(mut self, handler: impl Into<Callback<Event<KeyboardEventData>, bool>>) -> Self {
		self.on_pre_key_down = Some(handler.into());
		self
	}
}

impl Component for TextField {
	fn render(&self) -> impl IntoElement {
		let id = use_hook(|| self.id.unwrap_or_else(AccessibilityId::new_unique));
		let focus = use_focus(id);
		let focused = focus().is_focused();
		let holder = use_state(ParagraphHolder::default);
		let mut area = use_state(Area::default);
		let mut viewport = use_state(Area::default);
		let mut scroll = use_scroll_controller(ScrollConfig::default);
		let mut editable = use_editable(|| self.value.read().to_string(), EditableConfig::new);
		let mut dragging = use_state(|| false);
		let mut hovered = use_state(|| false);
		let mut was_focused = use_state(|| false);
		// the focusing click's press arrives after the focus change and must not undo the select-all
		let mut fresh = use_state(|| false);
		let mut value = self.value.clone();
		let theme = use_wave_theme();
		let enabled = self.enabled;

		let (mut movement, caret) = use_cursor_blink(focused, theme.text);

		if *value.read() != editable.editor().read().committed_text() {
			let mut editor = editable.editor_mut().write();
			editor.clear_preedit();
			editor.set(&value.read());
			editor.editor_history_mut().clear();
			editor.clear_selection();
		}

		if *was_focused.peek() != focused {
			was_focused.set(focused);
			if focused && self.select_all_on_focus {
				let mut editor = editable.editor_mut().write();
				let end = editor.len_utf16_cu();
				if end > 0 {
					editor.set_selection((0, end));
					fresh.set(true);
				}
			}
		}

		let mut follow_caret = move || {
			if !id.is_focused() {
				return;
			}
			let holder = holder.peek();
			let holder = holder.0.borrow();
			let Some(ParagraphHolderInner { paragraph, scale_factor, .. }) = holder.as_ref() else {
				return;
			};
			let view = viewport();
			if view.width() == 0. {
				return;
			}
			let editor = editable.editor().peek();
			let rect = paragraph.cursor_rect(&editor.rope().to_string(), editor.cursor_pos(), TextAlign::default());
			let caret = Point2D::new(rect.left, rect.top) / (*scale_factor as f32);
			let visible = caret + Vector2D::new(FIELD_PADDING.1, FIELD_PADDING.0);
			let start = view.min_x() - area.peek().min_x();
			if visible.x < start {
				scroll.scroll_to_x(-caret.x as i32);
			} else if visible.x > start + view.width() {
				scroll.scroll_to_x(-(visible.x + FIELD_PADDING.1 - view.width()) as i32);
			}
		};

		let on_ime_preedit = move |e: Event<ImePreeditEventData>| {
			let mut editor = editable.editor_mut().write();
			if e.data().text.is_empty() {
				editor.clear_preedit();
			} else {
				editor.set_preedit(&e.data().text);
			}
		};

		let pre_key_down = self.on_pre_key_down.clone().unwrap_or_else(|| {
			Callback::new(|e: Event<KeyboardEventData>| {
				let passes = [NamedKey::Tab, NamedKey::Enter, NamedKey::Escape].iter().any(|k| is_named(&e, *k));
				if !passes {
					e.stop_propagation();
					e.prevent_default();
				}
				!is_named(&e, NamedKey::Tab)
			})
		});
		let on_key_down = move |e: Event<KeyboardEventData>| {
			let key = e.key.clone();
			let modifiers = e.modifiers;
			if !pre_key_down.call(e) {
				return;
			}
			match &key {
				Key::Named(NamedKey::Enter | NamedKey::Tab) => {}
				Key::Named(NamedKey::Escape) => id.request_unfocus(),
				_ => {
					movement.reset();
					fresh.set_if_modified(false);
					let before = editable.editor().peek().editor_history().version;
					editable.process_event(EditableEvent::KeyDown {
						key: &key,
						modifiers,
						editor_line: Some(EditorLine::SingleParagraph),
						holder: Some(&holder.read()),
					});
					let text = editable.editor().read().committed_text();
					if *value.peek() != text {
						value.set(text);
					}
					if editable.editor().peek().editor_history().version == before {
						follow_caret();
					}
				}
			}
		};
		let on_key_up = move |e: Event<KeyboardEventData>| {
			e.stop_propagation();
			editable.process_event(EditableEvent::KeyUp { key: &e.key });
		};

		let on_focus_press = move |e: Event<FocusPressEventData>| {
			e.stop_propagation();
			e.prevent_default();
			dragging.set_if_modified(true);
			movement.reset();
			if !std::mem::take(&mut *fresh.write()) {
				editable.process_event(EditableEvent::Down {
					location: e.element_location(),
					editor_line: EditorLine::SingleParagraph,
					holder: &holder.read(),
				});
			}
			id.request_focus();
		};
		let on_pointer_press = move |e: Event<PointerEventData>| {
			e.stop_propagation();
			e.prevent_default();
			if *hovered.peek() || id.is_focused() {
				editable.process_event(EditableEvent::Release);
			}
			if id.is_focused() {
				dragging.set_if_modified(false);
			}
		};
		let on_global_pointer_move = move |e: Event<PointerEventData>| {
			if id.is_focused() && *dragging.read() {
				let origin = area.read().to_f64().min();
				editable.process_event(EditableEvent::Move {
					location: (e.global_location() - origin).to_point(),
					editor_line: EditorLine::SingleParagraph,
					holder: &holder.read(),
				});
				follow_caret();
			}
		};
		let on_global_pointer_up = move |_: Event<PointerEventData>| {
			if *hovered.peek() || id.is_focused() {
				editable.process_event(EditableEvent::Release);
			}
			if id.is_focused() {
				if *dragging.read() {
					dragging.set(false);
				} else {
					id.request_unfocus();
				}
			}
		};
		let on_paragraph_sized = move |e: Event<SizedEventData>| {
			let view = viewport();
			if view.width() > 0. {
				scroll.apply_layout(e.area.size, view.size);
			}
			let resized = area.peek().size != e.area.size;
			area.set_if_modified(e.area);
			if resized {
				follow_caret();
			}
		};

		let (cursor_index, selection) = if enabled && focused {
			let editor = editable.editor().read();
			(Some(editor.cursor_pos()), editor.get_visible_selection(EditorLine::SingleParagraph))
		} else {
			(None, None)
		};
		let text_color = if enabled { theme.text } else { theme.text_disabled };

		let editor = editable.editor().read();
		let paragraph = paragraph()
			.holder(holder.read().clone())
			.on_sized(on_paragraph_sized)
			.min_width(Size::percent(100.))
			.maybe(enabled, |el| el.on_focus_press(on_focus_press))
			.padding(FIELD_PADDING)
			.cursor_index(cursor_index)
			.cursor_color(caret)
			.highlight_color(theme.selection)
			.color(text_color)
			.max_lines(1)
			.highlights(selection.map(|h| vec![h]));
		let paragraph = if editor.has_preedit() {
			let (before, preedit, after) = editor.preedit_text_segments();
			paragraph.span(before).span(Span::new(preedit).text_decoration(TextDecoration::Underline)).span(after)
		} else {
			paragraph.span(editor.rope().to_string())
		};

		rect()
			.a11y_id(id)
			.a11y_enabled(enabled)
			.a11y_focusable(enabled)
			.a11y_alt(self.value.read().to_string())
			.a11y_role(AccessibilityRole::TextInput)
			.maybe(enabled, |el| {
				el.on_key_up(on_key_up)
					.on_key_down(on_key_down)
					.on_pointer_press(on_pointer_press)
					.on_ime_preedit(on_ime_preedit)
					.on_global_pointer_up(on_global_pointer_up)
					.on_global_pointer_move(on_global_pointer_move)
			})
			.on_pointer_enter(move |_| hovered.set_if_modified(true))
			.on_pointer_leave(move |_| hovered.set_if_modified(false))
			.cursor(if enabled { CursorIcon::Text } else { CursorIcon::NotAllowed })
			.width(self.width.clone())
			.content(Content::Flex)
			.direction(Direction::Horizontal)
			.cross_align(Alignment::center())
			.child(
				ScrollView::new_controlled(scroll)
					.width(Size::flex(1.))
					.height(Size::Inner)
					.direction(Direction::Horizontal)
					.show_scrollbar(false)
					.on_sized(move |e: Event<SizedEventData>| viewport.set_if_modified(e.area))
					.child(paragraph),
			)
	}
}
