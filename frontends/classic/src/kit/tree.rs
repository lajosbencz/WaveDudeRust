//! The tree view in Next's look (docs/ui/20) on egui-mineral's tree model; one focus stop, keys per docs/ui/10.

use eframe::egui::{
	self, Align2, Color32, DragAndDrop, Event, Id, Key, Modifiers, Painter, Pos2, Rect, Response, Role, ScrollArea,
	Sense, Shape, Stroke, StrokeKind, TextStyle, TextWrapMode, Ui, UiBuilder, Vec2, WidgetInfo, WidgetText, pos2, vec2,
};
use egui_mineral::expand::Expansion;
use egui_mineral::i18n::Direction;
use egui_mineral::kit::interact::{Arrows, lock_arrows};
use egui_mineral::kit::state::TempState;
use egui_mineral::runtime::Core;
use egui_mineral::tree::{self, DropPosition, TreeData, TreeMove, TreeNav, TreeRow};
use waveduderust_egui_preview::drawn as icons;

use super::tokens::{BORDER, Palette, TREE};

/// The drag payload of a row.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Dragged<K>(pub K);

/// How a row takes part in the selection.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Selected {
	No,
	/// Selected, not current.
	Other,
	Current,
}

/// Paints a row's drawing into its rect.
pub(crate) type Paint<'a> = Box<dyn Fn(&Painter, Rect) + 'a>;

/// What a row draws before its label: an icon, or a painting of `size`.
pub(crate) enum Drawing<'a> {
	Icon(icons::Icon),
	Paint(Vec2, Paint<'a>),
}

/// A trailing button's glyph.
#[derive(Clone, Copy)]
pub(crate) enum Glyph {
	Up,
	Down,
	Minus,
	Plus,
}

/// A trailing button.
pub(crate) struct Button<'a> {
	pub glyph: Glyph,
	pub tip: &'a str,
	pub enabled: bool,
}

/// How the UI code wants a row shown.
pub(crate) struct RowLook<'a> {
	pub drawing: Drawing<'a>,
	/// A check box and its tip.
	pub check: Option<(bool, &'a str)>,
	/// Greys the drawing and the label.
	pub dimmed: bool,
	pub selected: Selected,
	pub buttons: Vec<Button<'a>>,
	pub draggable: bool,
}

/// An inline rename: the row and its text so far.
pub(crate) struct Rename<K> {
	key: K,
	text: String,
	/// The box has taken the focus.
	started: bool,
}

impl<K> Rename<K> {
	pub(crate) fn new(key: K, text: String) -> Self {
		Self { key, text, started: false }
	}
}

/// What the tree was asked for in one pass.
pub(crate) struct TreeOutput<K> {
	/// The tree's own response, for its focus and the empty room's context menu.
	pub response: Response,
	/// A primary press on a row.
	pub pressed: Option<K>,
	/// A secondary click on a row, and where.
	pub context: Option<(K, Pos2)>,
	/// A row's check box was clicked.
	pub checked: Option<K>,
	/// A trailing button was clicked: its index and rect.
	pub button: Option<(K, usize, Rect)>,
	/// The keys moved to a row; whether Shift was held.
	pub walked: Option<(K, bool)>,
	/// A row was dropped, as the UI's rule placed it.
	pub moved: Option<TreeMove<K>>,
	/// The rename ended: the new text, none when reverted.
	pub renamed: Option<(K, Option<String>)>,
	/// The current row on screen.
	pub current_rect: Option<Rect>,
	/// A row was clicked.
	clicked: bool,
}

/// Whether the UI's rule takes a drop.
type Accept<'a, K> = Box<dyn Fn(&TreeMove<K>) -> bool + 'a>;

/// A tree over `roots`.
pub(crate) struct Tree<'a, N: TreeData> {
	id: Id,
	label: &'a str,
	roots: &'a [N],
	expansion: &'a mut Expansion<N::Key>,
	current: Option<N::Key>,
	focus: bool,
	rename: Option<&'a mut Option<Rename<N::Key>>>,
	accept: Option<Accept<'a, N::Key>>,
}

/// The current row the tree revealed last.
#[derive(Clone)]
struct Revealed<K>(Option<K>);

impl<K: Clone + Send + Sync + 'static> TempState for Revealed<K> {}

impl<'a, N: TreeData> Tree<'a, N> {
	/// `label` names the tree for assistive technology; `expansion` holds the expanded rows.
	pub(crate) fn new(id: Id, label: &'a str, roots: &'a [N], expansion: &'a mut Expansion<N::Key>) -> Self {
		Self { id, label, roots, expansion, current: None, focus: false, rename: None, accept: None }
	}

	/// The row the keys walk from; a new one is revealed: its ancestors expand and it scrolls into view.
	pub(crate) fn current(mut self, current: Option<N::Key>) -> Self {
		self.current = current;
		self
	}

	/// The tree takes the keyboard focus this pass.
	pub(crate) fn focus(mut self, focus: bool) -> Self {
		self.focus = focus;
		self
	}

	/// The inline rename, while there is one.
	pub(crate) fn rename(mut self, rename: &'a mut Option<Rename<N::Key>>) -> Self {
		self.rename = Some(rename);
		self
	}

	/// Rows drop on rows where `accept` takes the move.
	pub(crate) fn drops(mut self, accept: impl Fn(&TreeMove<N::Key>) -> bool + 'a) -> Self {
		self.accept = Some(Box::new(accept));
		self
	}

	/// The tree in `ui`'s room, each row shown as `look` says.
	pub(crate) fn show<'l>(mut self, ui: &mut Ui, look: impl Fn(&N) -> RowLook<'l>) -> TreeOutput<N::Key> {
		let ctx = ui.ctx().clone();
		let palette = Palette::of(ctx.theme());
		let room = ui.available_rect_before_wrap();
		let response = ui.interact(room, self.id, Sense::click());
		response.widget_info(|| WidgetInfo::labeled(Role::Tree, true, self.label));
		if self.focus || response.clicked() || response.secondary_clicked() {
			response.request_focus();
		}
		let focused = response.has_focus();
		if focused {
			lock_arrows(&ctx, self.id, Arrows::Both);
		}
		let scroll_to = self.reveal(&ctx);
		let mut out = TreeOutput {
			response,
			pressed: None,
			context: None,
			checked: None,
			button: None,
			walked: None,
			moved: None,
			renamed: None,
			current_rect: None,
			clicked: false,
		};
		let rows = tree::flatten(self.roots, self.expansion);
		let mut toggled = None;
		let mut shown = Vec::new();
		let mut tree_ui = ui.new_child(UiBuilder::new().max_rect(room));
		// show_rows steps by the row height and the spacing at its call
		tree_ui.spacing_mut().item_spacing.y = 0.;
		let scroll = ScrollArea::vertical().auto_shrink(false);
		scroll.show_rows(&mut tree_ui, TREE.row, rows.len(), |ui, range| {
			let top = ui.max_rect().top() - range.start as f32 * TREE.row;
			if let Some(i) = scroll_to.as_ref().and_then(|k| rows.iter().position(|r| r.node.key() == k)) {
				let at = Rect::from_min_size(pos2(room.left(), top + i as f32 * TREE.row), vec2(1., TREE.row));
				ui.scroll_to_rect_animation(at, None, egui::style::ScrollAnimation::none());
			}
			for i in range {
				let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), TREE.row), Sense::hover());
				let row = &rows[i];
				let look = look(row.node);
				if look.selected == Selected::Current {
					out.current_rect = Some(rect);
				}
				let paint = RowPaint { row, look: &look, rect, focused, palette };
				if self.row(ui, &paint, &mut out) {
					toggled = Some(row.node.key().clone());
				}
				shown.push((i, rect));
			}
			out.moved = self.drop(ui, &rows, &shown, palette);
		});
		if focused && self.rename.as_ref().is_none_or(|r| r.is_none()) {
			out.walked = self.walk(&ctx, &rows);
		}
		let touched = out.clicked || out.pressed.is_some() || out.checked.is_some() || out.button.is_some();
		if touched || out.context.is_some() || toggled.is_some() {
			out.response.request_focus();
		}
		if let Some(key) = toggled {
			self.expansion.toggle(key);
		}
		out
	}

	/// Expands the ancestors of a new current row; the row to scroll to.
	fn reveal(&mut self, ctx: &egui::Context) -> Option<N::Key> {
		let id = self.id.with("revealed");
		let Revealed(last) = Revealed::<N::Key>::load_or(ctx, id, || Revealed(None));
		if last == self.current {
			return None;
		}
		Revealed(self.current.clone()).store(ctx, id);
		let current = self.current.clone()?;
		self.expansion.reveal(self.roots, &current);
		Some(current)
	}

	/// One row; whether its expander was clicked.
	fn row(&mut self, ui: &mut Ui, p: &RowPaint<'_, '_, N>, out: &mut TreeOutput<N::Key>) -> bool {
		let (row, look, rect, palette) = (p.row, p.look, p.rect, p.palette);
		let key = row.node.key();
		let id = self.id.with(("row", key));
		// the tree is the focus stop, not its rows
		let sense = if look.draggable { Sense::CLICK | Sense::DRAG } else { Sense::CLICK };
		let response = ui.interact(rect, id, sense);
		let label = row.node.label();
		let selected = look.selected != Selected::No;
		response.widget_info(|| WidgetInfo::selected(Role::TreeItem, true, selected, label));
		let header = rect.left() + TREE.expander + TREE.indent * row.depth as f32;
		let band = Rect::from_x_y_ranges(header..=rect.right() - TREE.band_end, rect.y_range());
		let (fill, text) = match look.selected {
			Selected::Current if p.focused => (palette.selection, palette.selection_text),
			Selected::Current | Selected::Other => (palette.inactive_selection, palette.inactive_selection_text),
			Selected::No if response.contains_pointer() => (palette.row_hover, palette.text),
			Selected::No => (Color32::TRANSPARENT, palette.text),
		};
		let painter = ui.painter();
		painter.rect_filled(band, 0., fill);
		let ink = if look.dimmed { palette.text_disabled } else { text };
		let cy = rect.center().y;
		let mut toggled = false;
		if row.has_children {
			let column = Rect::from_x_y_ranges(header - TREE.expander..=header, rect.y_range());
			paint_chevron(painter, pos2(header - TREE.chevron_center, cy), row.expanded, text);
			toggled = ui.interact(column, id.with("expander"), Sense::CLICK).clicked();
		}
		let mut x = header + TREE.lead;
		if let Some((on, tip)) = look.check {
			let r = Rect::from_min_size(pos2(x, (cy - TREE.check / 2.).floor()), Vec2::splat(TREE.check));
			paint_check(ui.painter(), r, on, palette);
			let check = ui.interact(r, id.with("check"), Sense::CLICK).on_hover_text(tip);
			check.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, on, label));
			if check.clicked() {
				out.checked = Some(key.clone());
			}
			x += TREE.check + TREE.check_gap;
		}
		let size = match &look.drawing {
			Drawing::Icon(_) => Vec2::splat(TREE.icon),
			Drawing::Paint(size, _) => *size,
		};
		let at = Rect::from_min_size(pos2(x, cy - size.y / 2.), size);
		match &look.drawing {
			Drawing::Icon(icon) => icons::paint(ui.painter(), at, *icon, ink),
			Drawing::Paint(_, paint) => paint(ui.painter(), at),
		}
		x += size.x + TREE.icon_gap;
		let mut end = rect.right();
		let last = look.buttons.len().saturating_sub(1);
		for (i, b) in look.buttons.iter().enumerate() {
			let center = pos2(rect.right() - TREE.button_end - TREE.button_pitch * (last - i) as f32, cy);
			let r = Rect::from_center_size(center, Vec2::splat(TREE.button));
			end = end.min(r.left());
			paint_glyph(ui.painter(), center, b.glyph, if b.enabled { text } else { palette.text_disabled });
			let sense = if b.enabled { Sense::CLICK } else { Sense::hover() };
			let button = ui.interact(r, id.with(("button", i)), sense).on_hover_text(b.tip);
			button.widget_info(|| WidgetInfo::labeled(Role::Button, b.enabled, b.tip));
			if button.clicked() {
				out.button = Some((key.clone(), i, r));
			}
		}
		let name = Rect::from_x_y_ranges(x..=end, rect.y_range());
		let renaming = self.rename.as_deref_mut().filter(|r| r.as_ref().is_some_and(|r| r.key == *key));
		match renaming {
			Some(slot) => {
				if let Some(text) = slot.as_mut().and_then(|r| rename_box(ui, name, r, self.id)) {
					out.renamed = Some((key.clone(), text));
					*slot = None;
				}
			}
			None => {
				let galley = WidgetText::from(label).into_galley(
					ui,
					Some(TextWrapMode::Truncate),
					name.width().max(0.),
					TextStyle::Body,
				);
				let at =
					Align2::LEFT_CENTER.align_size_within_rect(galley.size(), name).min - vec2(0., TREE.label_rise);
				ui.painter().with_clip_rect(name.intersect(ui.clip_rect())).galley(at, galley, ink);
			}
		}
		// a press released in the same pass is no longer down on the row, but clicks it
		let down = response.is_pointer_button_down_on() || response.clicked();
		if down && ui.input(|i| i.pointer.primary_pressed()) {
			out.pressed = Some(key.clone());
		}
		out.clicked |= response.clicked();
		if response.secondary_clicked() {
			out.context = response.interact_pointer_pos().map(|at| (key.clone(), at));
		}
		if response.drag_started() {
			DragAndDrop::set_payload(ui.ctx(), Dragged(key.clone()));
		}
		toggled
	}

	/// The drop mark of a row dragged over the `shown` rows, and the move once released.
	fn drop(
		&self,
		ui: &Ui,
		rows: &[TreeRow<'_, N>],
		shown: &[(usize, Rect)],
		palette: &Palette,
	) -> Option<TreeMove<N::Key>> {
		let accept = self.accept.as_ref()?;
		let dragged = DragAndDrop::payload::<Dragged<N::Key>>(ui.ctx())?;
		let p = ui.ctx().pointer_hover_pos().filter(|p| ui.clip_rect().contains(*p))?;
		let &(i, rect) = shown.iter().find(|(_, r)| r.y_range().contains(p.y))?;
		let frac = (p.y - rect.top()) / rect.height();
		let target = tree::drop_at(self.roots, rows, &dragged.0, i, frac)?;
		let m = target.change;
		if !accept(&m) {
			return None;
		}
		if let Some(&(_, rect)) = shown.iter().find(|(r, _)| *r == target.row) {
			paint_drop(ui.painter(), rect, rows[target.row].depth, m.position, palette.drop_indicator);
		}
		if !ui.input(|i| i.pointer.any_released()) {
			return None;
		}
		DragAndDrop::clear_payload(ui.ctx());
		Some(m)
	}

	/// The walking keys taken from this pass: they expand or collapse rows in place; the row they moved to.
	fn walk(&mut self, ctx: &egui::Context, rows: &[TreeRow<'_, N>]) -> Option<(N::Key, bool)> {
		let mut keys = Vec::new();
		ctx.input_mut(|i| {
			i.events.retain(|e| {
				let Event::Key { key, pressed: true, modifiers, .. } = e else { return true };
				let shift = modifiers.matches_exact(Modifiers::SHIFT);
				let taken = match key {
					Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End => modifiers.is_none() || shift,
					Key::ArrowLeft | Key::ArrowRight => modifiers.is_none(),
					_ => false,
				};
				if taken {
					keys.push((*key, *modifiers));
				}
				!taken
			});
		});
		let dir = Core::of(ctx).map_or(Direction::Ltr, |core| core.i18n().direction());
		let mut cursor = self.current.as_ref().and_then(|c| rows.iter().position(|r| r.node.key() == c));
		let mut walked = None;
		for (key, modifiers) in keys {
			let nav = match cursor {
				Some(at) => tree::on_key(rows, at, key, modifiers, dir),
				None if key == Key::End => rows.len().checked_sub(1).map(|row| TreeNav::Focus { row, modifiers }),
				None => (!rows.is_empty()).then_some(TreeNav::Focus { row: 0, modifiers }),
			};
			match nav {
				Some(TreeNav::Focus { row, modifiers }) => {
					cursor = Some(row);
					walked = Some((rows[row].node.key().clone(), modifiers.shift_only()));
				}
				Some(TreeNav::Expand(at)) => self.expansion.expand(rows[at].node.key().clone()),
				Some(TreeNav::Collapse(at)) => self.expansion.collapse(rows[at].node.key()),
				_ => {}
			}
		}
		walked
	}
}

/// A row to paint.
struct RowPaint<'r, 'l, N: TreeData> {
	row: &'r TreeRow<'r, N>,
	look: &'r RowLook<'l>,
	rect: Rect,
	focused: bool,
	palette: &'static Palette,
}

/// The expander's chevron around `center`: pointing down when expanded, right when not.
fn paint_chevron(painter: &Painter, center: Pos2, expanded: bool, ink: Color32) {
	let points = TREE.chevron.map(|p| center + if expanded { p } else { vec2(p.y, p.x) });
	painter.add(Shape::line(points.to_vec(), Stroke::new(TREE.chevron_width, ink)));
}

/// A check box in `rect`, marked while `on`.
fn paint_check(painter: &Painter, rect: Rect, on: bool, palette: &Palette) {
	let stroke = Stroke::new(BORDER, palette.check_border);
	painter.rect(rect, 0., palette.input_background, stroke, StrokeKind::Inside);
	if on {
		let points = TREE.check_mark.map(|p| rect.center() + p);
		painter.add(Shape::line(points.to_vec(), Stroke::new(TREE.check_width, palette.text)));
	}
}

/// A trailing button's glyph around `center`, on the pixel grid.
fn paint_glyph(painter: &Painter, center: Pos2, glyph: Glyph, ink: Color32) {
	let center = center.round();
	match glyph {
		Glyph::Up | Glyph::Down => {
			let flip = if matches!(glyph, Glyph::Up) { 1. } else { -1. };
			let points = TREE.glyph_chevron.map(|p| center + vec2(p.x, p.y * flip));
			painter.add(Shape::line(points.to_vec(), Stroke::new(TREE.glyph_width, ink)));
		}
		Glyph::Minus | Glyph::Plus => {
			let ink = ink.gamma_multiply(TREE.glyph_bar_opacity);
			let bar = vec2(2. * TREE.glyph_arm, TREE.glyph_bar);
			painter.rect_filled(Rect::from_center_size(center, bar), 0., ink);
			if matches!(glyph, Glyph::Plus) {
				painter.rect_filled(Rect::from_center_size(center, vec2(bar.y, bar.x)), 0., ink);
			}
		}
	}
}

/// The drop mark on `row` at `depth`: a line along its top or bottom edge, or an outline around it.
fn paint_drop(painter: &Painter, row: Rect, depth: usize, position: DropPosition, color: Color32) {
	let left = row.left() + TREE.expander + TREE.indent * depth as f32 + TREE.lead;
	let mark = Rect::from_x_y_ranges(left..=row.right() - TREE.drop_end, row.y_range());
	match position {
		DropPosition::Before => painter.rect_filled(mark.with_max_y(mark.top() + TREE.drop), 0., color),
		DropPosition::After => painter.rect_filled(mark.with_min_y(mark.bottom() - TREE.drop), 0., color),
		DropPosition::Inside => painter.rect_stroke(mark, 0., Stroke::new(TREE.drop, color), StrokeKind::Inside),
	};
}

/// The rename box of `r` in `room`: it takes the focus with its text selected. Enter applies and Esc reverts, both
/// giving the focus back to the tree `tree`; the focus leaving applies. The outcome once it ended.
fn rename_box<K>(ui: &mut Ui, room: Rect, r: &mut Rename<K>, tree: Id) -> Option<Option<String>> {
	let edit =
		egui::TextEdit::singleline(&mut r.text).margin(egui::Margin::symmetric(2, 0)).desired_width(room.width());
	let response = ui.put(room, edit);
	if !r.started {
		response.request_focus();
		if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
			let end = egui::text::CCursor::new(r.text.chars().count());
			let all = egui::text::CCursorRange::two(egui::text::CCursor::new(0), end);
			state.cursor.set_char_range(Some(all));
			state.store(ui.ctx(), response.id);
		}
		r.started = true;
		return None;
	}
	if !response.lost_focus() {
		return None;
	}
	// taken, so the tree the focus goes back to does not take them as its own
	let (enter, esc) =
		ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape)));
	if enter || esc {
		ui.memory_mut(|m| m.request_focus(tree));
	}
	Some((!esc).then(|| r.text.clone()))
}
