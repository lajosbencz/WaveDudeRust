//! Property form: two columns (labels as wide as the widest, editors filling the rest), full-width rows,
//! section headers that collapse, the parts' description header, hints as tooltips.

use freya::prelude::*;

use crate::icon::{IconView, icons};
use crate::theme::use_wave_theme;
use crate::util::{TestIdExt, focus_border};

/// Width of the label column: the widest label, at least `minimum`.
pub fn label_column_width(widths: impl IntoIterator<Item = f32>, minimum: f32) -> f32 {
	widths.into_iter().fold(minimum, f32::max).ceil()
}

/// The widest label seen by the rows of one grid, shared through context.
#[derive(Clone, Copy, PartialEq)]
struct LabelColumn(State<f32>);

const MIN_LABEL: f32 = 90.;
const LABEL_GAP: f32 = 10.;

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
				.padding((4., 6., 8., 6.))
				.background(theme.tool_window_background)
				.color(theme.text)
				.font_size(theme.font_size)
				.children(self.children.clone()),
		)
	}

	fn render_key(&self) -> DiffKey {
		self.key.clone().or(self.default_key())
	}
}

/// A label and its editor; `full_width` spans both columns (check boxes carrying their own label).
#[derive(Clone, PartialEq)]
pub struct PropertyRow {
	label: String,
	editor: Element,
	hint: Option<String>,
	enabled: bool,
	full_width: bool,
	test_id: Option<String>,
}

impl PropertyRow {
	pub fn new(label: impl Into<String>, editor: impl IntoElement) -> Self {
		Self {
			label: label.into(),
			editor: editor.into_element(),
			hint: None,
			enabled: true,
			full_width: false,
			test_id: None,
		}
	}

	pub fn hint(mut self, hint: impl Into<String>) -> Self {
		self.hint = Some(hint.into());
		self
	}

	/// A disabled row greys its label; the editor is disabled by the caller.
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
				Some(hint) => TooltipContainer::new(Tooltip::new_text(hint)).child(cell).into_element(),
				None => cell.into_element(),
			}
		});
		rect()
			.width(Size::fill())
			.min_height(Size::px(theme.row_height + 2.))
			.horizontal()
			.content(Content::flex())
			.cross_align(Alignment::center())
			.maybe_test_id(self.test_id.clone())
			.maybe_child(label_cell)
			.child(rect().width(Size::flex(1.)).child(self.editor.clone()))
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
			.padding((8., 0., 2., 0.))
			.border(focus_border(focus(), theme.accent))
			.maybe(toggle.is_some(), |r| {
				let t = toggle.clone();
				r.a11y_id(id).a11y_focusable(true).on_press(move |_| {
					if let Some(mut c) = t.clone() {
						let v = !*c.peek();
						c.set(v);
					}
				})
			})
			.child(
				rect()
					.horizontal()
					.cross_align(Alignment::center())
					.maybe_child(toggle.is_some().then(|| {
						IconView::new(if collapsed { icons::CHEVRON_RIGHT } else { icons::CHEVRON_DOWN })
							.color(theme.text_dim)
					}))
					.child(label().text(self.title.clone()).color(theme.text_dim).font_weight(FontWeight::BOLD)),
			)
			.child(rect().margin((2., 0., 0., 0.)).width(Size::fill()).height(Size::px(1.)).background(theme.hairline));
		rect().width(Size::fill()).child(header).maybe_child(
			(!collapsed).then(|| rect().width(Size::fill()).padding((2., 0., 6., 0.)).children(self.children.clone())),
		)
	}
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
		let header = rect()
			.a11y_id(id)
			.a11y_focusable(true)
			.a11y_role(AccessibilityRole::Button)
			.height(Size::px(theme.row_height))
			.horizontal()
			.cross_align(Alignment::center())
			.border(focus_border(focus(), theme.accent))
			.on_press(move |_| {
				let v = !*toggle.peek();
				toggle.set(v);
			})
			.child(IconView::new(if open { icons::CHEVRON_DOWN } else { icons::CHEVRON_RIGHT }).color(theme.text))
			.child(label().text(self.title.clone()));
		let header: Element = match self.tooltip.clone() {
			Some(t) => TooltipContainer::new(Tooltip::new_text(t)).child(header).into_element(),
			None => header.into_element(),
		};
		rect().width(Size::fill()).child(header).maybe_child(open.then(|| {
			rect()
				.width(Size::fill())
				.padding((0., 4., 6., 20.))
				.child(paragraph().color(theme.text_dim).span(Span::new(self.text.clone())))
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
