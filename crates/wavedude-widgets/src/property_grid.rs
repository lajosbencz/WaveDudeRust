//! Property form: two columns (labels as wide as the widest, editors filling the rest), full-width rows,
//! section headers that collapse, the parts' description header, hints as tooltips.

use std::rc::Rc;

use freya::prelude::*;

use crate::icon::{IconView, icons};
use crate::theme::{UiFontExt, use_wave_theme};
use crate::tooltip::Tip;
use crate::util::{TestIdExt, focus_border};

/// Width of the label column: the widest label, at least `minimum`.
pub fn label_column_width(widths: impl IntoIterator<Item = f32>, minimum: f32) -> f32 {
	widths.into_iter().fold(minimum, f32::max).ceil()
}

/// The widest label seen by the rows of one grid, shared through context.
#[derive(Clone, Copy, PartialEq)]
struct LabelColumn(State<f32>);

const MIN_LABEL: f32 = 110.;
const LABEL_GAP: f32 = 8.;
const ROW_SPACING: f32 = 4.;
/// WPF's 28 px row pitch with [`ROW_SPACING`].
const ROW_HEIGHT: f32 = 24.;
/// The gap after a section's rows.
const SECTION_GAP: f32 = 10.;
const BOTTOM_RULE: BorderWidth = BorderWidth { top: 0., right: 0., bottom: 1., left: 0. };

/// A vertically scrolling form; rows inside share the label column.
#[derive(Clone, PartialEq, Default)]
pub struct PropertyGrid {
	children: Vec<Element>,
	key: DiffKey,
}

impl PropertyGrid {
	pub fn new() -> Self {
		Self::default()
	}
}

impl ChildrenExt for PropertyGrid {
	fn children_mut(&mut self) -> &mut Vec<Element> {
		&mut self.children
	}
}

impl KeyExt for PropertyGrid {
	fn key_mut(&mut self) -> &mut DiffKey {
		&mut self.key
	}
}

impl Component for PropertyGrid {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		use_provide_context(|| LabelColumn(State::create(MIN_LABEL)));
		ScrollView::new().expanded().child(
			rect()
				.width(Size::fill())
				.padding(8.)
				.background(theme.tool_window_background)
				.color(theme.text)
				.ui_font(&theme)
				.children(self.children.clone()),
		)
	}

	fn render_key(&self) -> DiffKey {
		self.key.clone().or(self.default_key())
	}
}

/// A row's editor: built once, or built for the row's enabled state when it is rendered.
#[derive(Clone)]
enum RowEditor {
	Fixed(Element),
	Switchable(Rc<dyn Fn(bool) -> Element>),
}

impl PartialEq for RowEditor {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(Self::Fixed(a), Self::Fixed(b)) => a == b,
			(Self::Switchable(a), Self::Switchable(b)) => Rc::ptr_eq(a, b),
			_ => false,
		}
	}
}

/// A label and its editor; `full_width` spans both columns (check boxes carrying their own label).
#[derive(Clone, PartialEq)]
pub struct PropertyRow {
	label: String,
	editor: RowEditor,
	hint: Option<String>,
	enabled: bool,
	full_width: bool,
	test_id: Option<String>,
}

impl PropertyRow {
	pub fn new(label: impl Into<String>, editor: impl IntoElement) -> Self {
		Self::with_editor(label, RowEditor::Fixed(editor.into_element()))
	}

	/// A row whose editor is built for its enabled state, so [`enabled`](Self::enabled) disables the editor too.
	pub fn switchable(label: impl Into<String>, editor: impl Fn(bool) -> Element + 'static) -> Self {
		Self::with_editor(label, RowEditor::Switchable(Rc::new(editor)))
	}

	fn with_editor(label: impl Into<String>, editor: RowEditor) -> Self {
		Self { label: label.into(), editor, hint: None, enabled: true, full_width: false, test_id: None }
	}

	pub fn hint(mut self, hint: impl Into<String>) -> Self {
		self.hint = Some(hint.into());
		self
	}

	/// A disabled row greys its label and disables a switchable editor; a fixed one is disabled by the caller.
	pub fn enabled(mut self, enabled: bool) -> Self {
		self.enabled = enabled;
		self
	}

	pub fn full_width(mut self) -> Self {
		self.full_width = true;
		self
	}

	pub fn test_id(mut self, id: impl Into<String>) -> Self {
		self.test_id = Some(id.into());
		self
	}
}

impl Component for PropertyRow {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let column = use_try_consume::<LabelColumn>();
		let width = column.map_or(MIN_LABEL, |c| *c.0.read());
		let ink = if self.enabled { theme.text } else { theme.text_disabled };
		let label_cell = (!self.full_width).then(|| {
			let measured = column.map(|c| c.0);
			let text = self.label.clone();
			let cell = rect()
				.width(Size::px(width + LABEL_GAP))
				.child(
					rect()
						.position(Position::new_absolute())
						.width(Size::px(4000.))
						.opacity(0.)
						.interactive(Interactive::No)
						.child(
							rect()
								.on_sized(move |e: Event<SizedEventData>| {
									if let Some(mut m) = measured
										&& e.area.width() > *m.peek()
									{
										m.set(label_column_width([e.area.width()], MIN_LABEL));
									}
								})
								.child(label().text(text.clone()).max_lines(1)),
						),
				)
				.child(label().text(text).color(ink).max_lines(1).text_overflow(TextOverflow::Ellipsis));
			match self.hint.clone() {
				Some(hint) => Tip::new(hint).child(cell).into_element(),
				None => cell.into_element(),
			}
		});
		rect()
			.width(Size::fill())
			.min_height(Size::px(ROW_HEIGHT))
			.horizontal()
			.content(Content::flex())
			.cross_align(Alignment::center())
			.maybe_test_id(self.test_id.clone())
			.maybe_child(label_cell)
			.child(rect().width(Size::flex(1.)).child(match &self.editor {
				RowEditor::Fixed(e) => e.clone(),
				RowEditor::Switchable(build) => build(self.enabled),
			}))
	}
}

/// A section: its name over a rule, then its rows. With a collapse state the header gets a chevron and
/// takes the focus (Space or Enter toggles).
#[derive(Clone, PartialEq)]
pub struct SectionHeader {
	title: String,
	collapsed: Option<WritableSetter<bool>>,
	children: Vec<Element>,
}

impl SectionHeader {
	pub fn new(title: impl Into<String>) -> Self {
		Self { title: title.into(), collapsed: None, children: Vec::new() }
	}

	pub fn collapsible(mut self, collapsed: impl Into<WritableSetter<bool>>) -> Self {
		self.collapsed = Some(collapsed.into());
		self
	}
}

impl ChildrenExt for SectionHeader {
	fn children_mut(&mut self) -> &mut Vec<Element> {
		&mut self.children
	}
}

impl Component for SectionHeader {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let id = use_a11y();
		let focus = use_focus(id);
		let collapsed = self.collapsed.as_ref().is_some_and(|c| *c.read());
		let toggle = self.collapsed.clone();
		let header = rect()
			.width(Size::fill())
			.margin((2., 0., 6., 0.))
			.padding((0., 0., 2., 0.))
			.border(Border::new().fill(theme.border).width(BOTTOM_RULE).alignment(BorderAlignment::Inner))
			.border(focus_border(focus(), theme.accent))
			.horizontal()
			.cross_align(Alignment::center())
			.maybe(toggle.is_some(), |r| {
				let t = toggle.clone();
				r.a11y_id(id).a11y_focusable(true).on_press(move |_| {
					if let Some(mut c) = t.clone() {
						let v = !*c.peek();
						c.set(v);
					}
				})
			})
			.maybe_child(toggle.is_some().then(|| chevron(&theme, !collapsed)))
			.child(label().text(self.title.clone()).font_weight(FontWeight::SEMI_BOLD));
		rect().width(Size::fill()).child(header).maybe_child((!collapsed).then(|| {
			rect()
				.width(Size::fill())
				.padding((0., 0., SECTION_GAP, 0.))
				.spacing(ROW_SPACING)
				.children(self.children.clone())
		}))
	}
}

/// The section and description headers' chevron: 6 x 11, pointing down while open.
fn chevron(theme: &crate::theme::Theme, open: bool) -> Rect {
	rect()
		.width(Size::px(6.))
		.height(Size::px(11.))
		.margin((0., 7., 0., 1.))
		.center()
		.child(IconView::new(if open { icons::CHEVRON_DOWN } else { icons::CHEVRON_RIGHT }).color(theme.text))
}

/// The parts' description header: a chevron and "Description"; the text shows under it while open.
#[derive(Clone, PartialEq)]
pub struct DescriptionHeader {
	title: String,
	text: String,
	expanded: WritableSetter<bool>,
	tooltip: Option<String>,
}

impl DescriptionHeader {
	pub fn new(text: impl Into<String>, expanded: impl Into<WritableSetter<bool>>) -> Self {
		Self { title: "Description".into(), text: text.into(), expanded: expanded.into(), tooltip: None }
	}

	pub fn title(mut self, title: impl Into<String>) -> Self {
		self.title = title.into();
		self
	}

	pub fn tooltip(mut self, text: impl Into<String>) -> Self {
		self.tooltip = Some(text.into());
		self
	}
}

impl Component for DescriptionHeader {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let id = use_a11y();
		let focus = use_focus(id);
		let open = *self.expanded.read();
		let mut toggle = self.expanded.clone();
		let mut hovered = use_state(|| false);
		let rule = if focus().is_focused() { theme.accent } else { theme.border };
		let header = rect()
			.a11y_id(id)
			.a11y_focusable(true)
			.a11y_role(AccessibilityRole::Button)
			.width(Size::fill())
			.margin((2., 0., 6., 0.))
			.padding((0., 0., 2., 0.))
			.horizontal()
			.cross_align(Alignment::center())
			.background(if hovered() { theme.button_hover } else { Color::TRANSPARENT })
			.border(Border::new().fill(rule).width(BOTTOM_RULE).alignment(BorderAlignment::Inner))
			.on_pointer_enter(move |_| hovered.set(true))
			.on_pointer_leave(move |_| hovered.set(false))
			.on_press(move |_| {
				let v = !*toggle.peek();
				toggle.set(v);
			})
			.child(chevron(&theme, open))
			.child(label().text(self.title.clone()).font_weight(FontWeight::SEMI_BOLD));
		let header: Element = match self.tooltip.clone() {
			Some(t) => Tip::new(t).child(header).into_element(),
			None => header.into_element(),
		};
		rect().width(Size::fill()).child(header).maybe_child(open.then(|| {
			rect()
				.width(Size::fill())
				.padding((0., 0., SECTION_GAP, 0.))
				.child(paragraph().color(theme.text).span(Span::new(self.text.clone())))
		}))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn label_column_is_the_widest_label() {
		assert_eq!(label_column_width([40., 120.4, 80.], 90.), 121.);
		assert_eq!(label_column_width([40.], 90.), 90.);
		assert_eq!(label_column_width([], 90.), 90.);
	}
}
