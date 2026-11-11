//! Text editor on `freya-edit`: the field inside the text and number boxes, with a select-all on focus that
//! Freya's `Input` cannot offer. Multi-line mode lays out one `paragraph` per line.

use std::ops::Range;

use freya::{
	elements::paragraph::{ParagraphCursorExt, ParagraphHolderInner},
	prelude::*,
	text_edit::*,
	torin::prelude::{Area, CursorPoint, Point2D, Vector2D},
};

use crate::{theme::use_wave_theme, util::is_named};

/// WPF's text padding 3,2 inside the field's 1 px border.
pub(crate) const FIELD_PADDING: (f32, f32) = (2., 3.);

/// A borderless editor for inside `field_frame`.
#[derive(Clone, PartialEq)]
pub struct TextField {
	value: WritableSetter<String>,
	enabled: bool,
	select_all_on_focus: bool,
	multiline: bool,
	placeholder: Option<String>,
	width: Size,
	height: Size,
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
			multiline: false,
			placeholder: None,
			width: Size::px(150.),
			height: Size::Inner,
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

	/// Enter inserts a newline, long lines wrap and the text scrolls vertically.
	pub fn multiline(mut self, multiline: bool) -> Self {
		self.multiline = multiline;
		self
	}

	pub fn width(mut self, width: impl Into<Size>) -> Self {
		self.width = width.into();
		self
	}

	/// A multi-line field higher than its text scrolls; [`Size::Inner`] grows with the text.
	pub fn height(mut self, height: impl Into<Size>) -> Self {
		self.height = height.into();
		self
	}

	pub fn a11y_id(mut self, id: impl Into<AccessibilityId>) -> Self {
		self.id = Some(id.into());
		self
	}

	/// Grey text shown while the field is empty.
	pub fn placeholder(mut self, text: impl Into<String>) -> Self {
		self.placeholder = Some(text.into());
		self
	}

	/// Called first for each key press: `false` skips the field's own handling. By default the field takes
	/// every key but Tab, Esc and a single-line field's Enter and stops it from bubbling.
	pub fn on_pre_key_down(mut self, handler: impl Into<Callback<Event<KeyboardEventData>, bool>>) -> Self {
		self.on_pre_key_down = Some(handler.into());
		self
	}
}

/// How the text maps to `paragraph` elements: all in one, or one per line.
#[derive(Clone, Copy, PartialEq)]
struct Lines {
	multiline: bool,
}

impl Lines {
	fn count(self, editor: &RopeEditor) -> usize {
		if self.multiline { editor.len_lines() } else { 1 }
	}

	fn editor_line(self, line: usize) -> EditorLine {
		if self.multiline { EditorLine::Paragraph(line) } else { EditorLine::SingleParagraph }
	}

	fn caret_line(self, editor: &RopeEditor) -> usize {
		if self.multiline { editor.cursor_row() } else { 0 }
	}

	/// Chars shown by `line`'s paragraph: its text without the line break.
	fn chars(self, editor: &RopeEditor, line: usize) -> Range<usize> {
		let rope = editor.rope();
		if !self.multiline {
			return 0..rope.len_chars();
		}
		let start = rope.line_to_char(line);
		let text = rope.line(line);
		let line_break = (0..text.len_chars())
			.rev()
			.map(|i| text.char(i))
			.take_while(|c| matches!(c, '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'))
			.count();
		start..start + text.len_chars() - line_break
	}
}

/// A laid-out line: its paragraph for hit testing and measuring, and where it is.
#[derive(Clone, Default)]
struct LineView {
	holder: ParagraphHolder,
	area: Area,
}

impl Component for TextField {
	fn render(&self) -> impl IntoElement {
		let id = use_hook(|| self.id.unwrap_or_else(AccessibilityId::new_unique));
		let focus = use_focus(id);
		let focused = focus().is_focused();
		let mut views = use_state(Vec::<LineView>::new);
		let mut content = use_state(Area::default);
		let mut viewport = use_state(Area::default);
		let mut scroll = use_scroll_controller(ScrollConfig::default);
		let mut editable = use_editable(|| self.value.read().to_string(), EditableConfig::new);
		let mut dragging = use_state(|| false);
		let mut hovered = use_state(|| false);
		let mut was_focused = use_state(|| false);
		let select_all_on_focus = self.select_all_on_focus;
		// the focusing click's press arrives after the focus change and must not undo the select-all
		let mut fresh = use_state(|| false);
		let mut value = self.value.clone();
		let theme = use_wave_theme();
		let enabled = self.enabled;
		let lines = Lines { multiline: self.multiline };

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
			if focused && select_all_on_focus {
				let mut editor = editable.editor_mut().write();
				let end = editor.len_utf16_cu();
				if end > 0 {
					editor.set_selection((0, end));
					fresh.set(true);
				}
			}
		}

		let count = lines.count(&editable.editor().peek());
		if views.peek().len() != count {
			views.write().resize_with(count, LineView::default);
		}

		let mut follow_caret = move || {
			if !id.is_focused() {
				return;
			}
			let view = viewport();
			if view.width() == 0. {
				return;
			}
			let editor = editable.editor().peek();
			let line = lines.caret_line(&editor);
			let views = views.peek();
			let Some(LineView { holder, area }) = views.get(line) else {
				return;
			};
			let holder = holder.0.borrow();
			let Some(ParagraphHolderInner { paragraph, scale_factor, padding }) = holder.as_ref() else {
				return;
			};
			let chars = lines.chars(&editor, line);
			let first = editor.rope().char_to_utf16_cu(chars.start);
			let text = editor.rope().slice(chars).to_string();
			let rect = paragraph.cursor_rect(&text, editor.cursor_pos() - first, TextAlign::default());
			let scale = *scale_factor as f32;
			let offset = area.min() - content.peek().min();
			let caret = Point2D::new(rect.left, rect.top) / scale + offset;
			let visible = caret + Vector2D::new(padding.left(), padding.top());
			let bottom = rect.bottom / scale + offset.y + padding.top();
			let start = view.min() - content.peek().min();
			if visible.x < start.x {
				scroll.scroll_to_x(-caret.x as i32);
			} else if visible.x > start.x + view.width() {
				scroll.scroll_to_x(-(visible.x + padding.right() - view.width()) as i32);
			}
			if visible.y < start.y {
				scroll.scroll_to_y(-caret.y as i32);
			} else if bottom > start.y + view.height() {
				scroll.scroll_to_y(-(bottom + padding.bottom() - view.height()) as i32);
			}
		};

		// the line under a window position, clamped to the first and last, and the position inside it
		let hit = move |global: CursorPoint| {
			let views = views.peek();
			let line =
				views.iter().position(|v| global.y < f64::from(v.area.max_y())).unwrap_or(views.len().checked_sub(1)?);
			let view = &views[line];
			Some((lines.editor_line(line), (global - view.area.to_f64().min()).to_point(), view.holder.clone()))
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
			Callback::new(move |e: Event<KeyboardEventData>| {
				let passes = [NamedKey::Tab, NamedKey::Escape].iter().any(|k| is_named(&e, *k))
					|| (is_named(&e, NamedKey::Enter) && !lines.multiline);
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
				Key::Named(NamedKey::Tab) => {}
				Key::Named(NamedKey::Enter) if !lines.multiline => {}
				Key::Named(NamedKey::Escape) => id.request_unfocus(),
				_ => {
					movement.reset();
					// a key can arrive before the render that saw the focus: select here then, same as that render does
					if select_all_on_focus && !*was_focused.peek() {
						was_focused.set(true);
						let mut editor = editable.editor_mut().write();
						let end = editor.len_utf16_cu();
						if end > 0 {
							editor.set_selection((0, end));
						}
					}
					fresh.set_if_modified(false);
					let before = editable.editor().peek().editor_history().version;
					let line = lines.caret_line(&editable.editor().peek());
					editable.process_event(EditableEvent::KeyDown {
						key: &key,
						modifiers,
						editor_line: Some(lines.editor_line(line)),
						holder: views.peek().get(line).map(|v| &v.holder),
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
			// the focusing click can beat the render that selects on focus: select here then instead of placing the caret
			if select_all_on_focus && !*was_focused.peek() {
				was_focused.set(true);
				let mut editor = editable.editor_mut().write();
				let end = editor.len_utf16_cu();
				if end > 0 {
					editor.set_selection((0, end));
				}
				fresh.set(false);
			} else if !std::mem::take(&mut *fresh.write())
				&& let Some((editor_line, location, holder)) = hit(e.global_location())
			{
				editable.process_event(EditableEvent::Down { location, editor_line, holder: &holder });
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
			if id.is_focused()
				&& *dragging.read()
				&& let Some((editor_line, location, holder)) = hit(e.global_location())
			{
				editable.process_event(EditableEvent::Move { location, editor_line, holder: &holder });
				follow_caret();
			}
		};
		// a press outside the focused field: its release unfocuses (a press that focuses the field, a spin
		// button's say, is not one; the focus lands between the press and the release)
		let mut pressed_outside = use_state(|| false);
		let on_global_pointer_down = move |_: Event<PointerEventData>| {
			pressed_outside.set_if_modified(id.is_focused() && !*hovered.peek());
		};
		let on_global_pointer_up = move |_: Event<PointerEventData>| {
			if *hovered.peek() || id.is_focused() {
				editable.process_event(EditableEvent::Release);
			}
			if id.is_focused() {
				if *dragging.read() {
					dragging.set(false);
				} else if std::mem::take(&mut *pressed_outside.write()) {
					id.request_unfocus();
				}
			}
		};
		let on_content_sized = move |e: Event<SizedEventData>| {
			let resized = content.peek().size != e.area.size;
			content.set_if_modified(e.area);
			if resized {
				follow_caret();
			}
		};
		let on_line_sized = move |line: usize| {
			move |e: Event<SizedEventData>| {
				let Some(old) = views.peek().get(line).map(|v| v.area) else {
					return;
				};
				if old != e.area {
					views.write()[line].area = e.area;
					if old.size != e.area.size && line == lines.caret_line(&editable.editor().peek()) {
						follow_caret();
					}
				}
			}
		};

		let text_color = if enabled { theme.text } else { theme.text_disabled };
		let editor = editable.editor().read();
		let active = enabled && focused;
		let caret_line = lines.caret_line(&editor);
		let preedit = editor.has_preedit().then(|| {
			let (before, preedit, _) = editor.preedit_text_segments();
			let start = before.chars().count();
			start..start + preedit.chars().count()
		});
		let (pad_y, pad_x) = FIELD_PADDING;
		let shown = views.peek();
		let paragraphs = shown.iter().enumerate().map(|(i, view)| {
			let chars = lines.chars(&editor, i);
			let rope = editor.rope();
			let first = rope.char_to_utf16_cu(chars.start);
			let cursor_index = (active && i == caret_line).then(|| editor.cursor_pos() - first);
			let selection = if active { editor.get_visible_selection(lines.editor_line(i)) } else { None };
			let padding =
				Gaps::new(if i == 0 { pad_y } else { 0. }, pad_x, if i + 1 == count { pad_y } else { 0. }, pad_x);
			let paragraph = paragraph()
				.holder(view.holder.clone())
				.on_sized(on_line_sized(i))
				.padding(padding)
				.cursor_index(cursor_index)
				.cursor_color(caret)
				.highlight_color(theme.selection)
				.color(text_color)
				.highlights(selection.map(|h| vec![h]))
				.map(lines.multiline.then_some(()), |el, ()| el.width(Size::percent(100.)))
				.maybe(!lines.multiline, |el| el.max_lines(1));
			let span = |range: Range<usize>| rope.slice(range).to_string();
			match &preedit {
				Some(p) => {
					let (from, to) = (p.start.clamp(chars.start, chars.end), p.end.clamp(chars.start, chars.end));
					paragraph
						.span(span(chars.start..from))
						.span(Span::new(span(from..to)).text_decoration(TextDecoration::Underline))
						.span(span(to..chars.end))
				}
				None => paragraph.span(span(chars)),
			}
		});
		let placeholder = self.placeholder.as_ref().filter(|_| editor.rope().len_chars() == 0).map(|text| {
			rect()
				.position(Position::new_absolute().left(pad_x).top(pad_y))
				.interactive(Interactive::No)
				.child(label().color(theme.text_disabled).max_lines(1).text(text.clone()))
		});
		let content = rect()
			.on_sized(on_content_sized)
			.maybe(enabled, |el| el.on_focus_press(on_focus_press))
			.map(lines.multiline.then_some(()), |el, ()| el.width(Size::percent(100.)))
			.min_width(Size::percent(100.))
			.children(paragraphs)
			.maybe_child(placeholder);

		rect()
			.a11y_id(id)
			.a11y_enabled(enabled)
			.a11y_focusable(enabled)
			.a11y_alt(self.value.read().to_string())
			.a11y_role(if lines.multiline {
				AccessibilityRole::MultilineTextInput
			} else {
				AccessibilityRole::TextInput
			})
			.maybe(enabled, |el| {
				el.on_key_up(on_key_up)
					.on_key_down(on_key_down)
					.on_pointer_press(on_pointer_press)
					.on_ime_preedit(on_ime_preedit)
					.on_global_pointer_down(on_global_pointer_down)
					.on_global_pointer_up(on_global_pointer_up)
					.on_global_pointer_move(on_global_pointer_move)
			})
			.on_pointer_enter(move |_| hovered.set_if_modified(true))
			.on_pointer_leave(move |_| hovered.set_if_modified(false))
			.cursor(if enabled { CursorIcon::Text } else { CursorIcon::NotAllowed })
			.width(self.width.clone())
			.height(self.height.clone())
			.content(Content::Flex)
			.direction(Direction::Horizontal)
			.cross_align(Alignment::center())
			.child(
				ScrollView::new_controlled(scroll)
					.width(Size::flex(1.))
					.height(if lines.multiline && self.height != Size::Inner { Size::fill() } else { Size::Inner })
					.direction(if lines.multiline { Direction::Vertical } else { Direction::Horizontal })
					.show_scrollbar(lines.multiline)
					.on_sized(move |e: Event<SizedEventData>| viewport.set_if_modified(e.area))
					.child(content),
			)
	}
}

#[cfg(test)]
mod tests {
	use freya::prelude::*;
	use freya_testing::prelude::*;

	use super::TextField;

	fn field(text: &'static str, height: Size) -> Element {
		crate::theme::use_init_wave_theme(crate::theme::Theme::dark);
		let mut value = use_state(|| text.to_string());
		rect()
			.child(
				TextField::new(WritableSetter::new(value, move |t: String| value.set(t)))
					.multiline(true)
					.width(Size::px(200.))
					.height(height),
			)
			.child(label().text(format!("value={:?}", value.read())))
			.into()
	}

	fn app() -> Element {
		field("ab\ncd", Size::Inner)
	}

	fn shown(test: &TestingRunner) -> String {
		test.find(|_, e| Label::try_downcast(e).filter(|l| l.text.starts_with("value=")).map(|l| l.text.to_string()))
			.unwrap_or_default()
	}

	fn lines(test: &TestingRunner) -> Vec<String> {
		test.find_many(|_, e| Paragraph::try_downcast(e).map(|p| p.spans.iter().map(|s| s.text.as_ref()).collect()))
	}

	fn key(test: &mut TestingRunner, key: NamedKey, modifiers: Modifiers) {
		test.send_event(PlatformEvent::Keyboard {
			name: KeyboardEventName::KeyDown,
			key: Key::Named(key),
			code: Code::Unidentified,
			modifiers,
		});
		test.sync_and_update();
	}

	#[test]
	fn multi_line_shows_a_paragraph_per_line_and_enter_inserts_a_newline() {
		let mut test = launch_test(app);
		test.sync_and_update();
		assert_eq!(lines(&test), ["ab", "cd"]);
		let second = test.find_many(|n, e| Paragraph::try_downcast(e).map(|_| n.layout().area)).pop().unwrap();
		test.click_cursor((190., second.center().y as f64));
		key(&mut test, NamedKey::Enter, Modifiers::empty());
		test.write_text("e");
		test.sync_and_update();
		assert_eq!(shown(&test), r#"value="ab\ncd\ne""#);
		assert_eq!(lines(&test), ["ab", "cd", "e"]);
	}

	#[test]
	fn a_selection_spans_lines() {
		let mut test = launch_test(app);
		test.sync_and_update();
		let second = test.find_many(|n, e| Paragraph::try_downcast(e).map(|_| n.layout().area)).pop().unwrap();
		test.click_cursor((190., second.center().y as f64));
		key(&mut test, NamedKey::ArrowUp, Modifiers::SHIFT);
		test.write_text("X");
		test.sync_and_update();
		assert_eq!(shown(&test), r#"value="abX""#);
	}

	#[test]
	fn the_caret_scrolls_into_view_vertically() {
		let mut test = launch_test(|| field("1\n2\n3\n4\n5\n6", Size::px(40.)));
		test.sync_and_update();
		let areas = |test: &TestingRunner| test.find_many(|n, e| Paragraph::try_downcast(e).map(|_| n.layout().area));
		assert!(areas(&test).last().unwrap().min_y() > 40., "the last line starts out of view");
		test.click_cursor((100., 8.));
		key(&mut test, NamedKey::End, Modifiers::ctrl_or_meta());
		test.sync_and_update();
		let last = *areas(&test).last().unwrap();
		assert!(last.max_y() <= 40.5 && last.min_y() >= 0., "the caret's line is in view: {last:?}");
	}
}
