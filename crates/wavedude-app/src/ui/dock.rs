//! The dock of docs/ui: the Preview in the centre with the Project, Properties and Messages panes around it. A
//! pane's close button, its Window position menu and View > Panes hide it in its place, where showing it brings it
//! back; the arrangement is saved in the settings as it changes.
//!
//! A pane dragged by its tab, or a group by its tab bar, lands where docs/ui/00 (Docking behaviour) lets it: at an
//! outer edge, beside a group or as a tab of one; never into its own group, and beside the Preview only. The drag,
//! its overlay and the drop are the dock's own; egui_tiles drags nothing.

use eframe::egui::{
	self, Color32, CursorIcon, Key, LayerId, Modifiers, Order, Pos2, Rect, Response, Sense, Stroke, StrokeKind,
	TextStyle, Vec2,
};
use egui_material_icons::icons::ICON_ARROW_DROP_DOWN;
use egui_tiles::{
	Container, EditAction, Linear, LinearDir, SimplificationOptions, TabState, Tabs, Tile, TileId, Tiles, Tree,
	UiResponse,
};
use wavedude_app_state::strings::Strings;

use super::menu_keys;
use crate::window::Pane;

/// The height of a pane's title and tab bar.
const TAB_BAR_HEIGHT: f32 = 21.;
/// The Window position button's glyph.
const ICON_SIZE: f32 = 16.;
/// The share of the window a pane moved to an edge takes.
const EDGE_SHARE: f32 = 0.25;
/// The share of a group a pane dropped beside it takes.
const BESIDE_SHARE: f32 = 1. / 3.;
/// The depth of the strips along the dock's outer edges that drop at an edge, logical pixels.
const EDGE_STRIP: f32 = 40.;
/// The share of a group's width or height at each side that drops beside it; its middle drops into it.
const SIDE_ZONE: f32 = 0.3;
/// The opacity of the drop target's fill.
const DROP_FILL: f32 = 0x50 as f32 / 255.;
/// The drop target's border and the dragged title's offset from the pointer, logical pixels.
const DROP_BORDER: f32 = 2.;
const GHOST_OFFSET: f32 = 12.;
const GHOST_PADDING: Vec2 = Vec2::new(8., 2.);

/// Where the Window position menu moves a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
	Left,
	Right,
	Top,
	Bottom,
}

impl Side {
	const ALL: [(Side, &'static str); 4] = [
		(Side::Left, "DockMoveLeft"),
		(Side::Right, "DockMoveRight"),
		(Side::Top, "DockMoveTop"),
		(Side::Bottom, "DockMoveBottom"),
	];

	/// The part of `rect` at this side taking `share` of its width or height.
	fn part(self, rect: Rect, share: f32) -> Rect {
		let (w, h) = (rect.width() * share, rect.height() * share);
		match self {
			Side::Left => Rect::from_min_size(rect.min, Vec2::new(w, rect.height())),
			Side::Right => Rect::from_min_max(Pos2::new(rect.right() - w, rect.top()), rect.max),
			Side::Top => Rect::from_min_size(rect.min, Vec2::new(rect.width(), h)),
			Side::Bottom => Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - h), rect.max),
		}
	}

	/// How far `p` lies from this side of `rect`, logical pixels, and as a share of its width or height.
	fn distance(self, rect: Rect, p: Pos2) -> (f32, f32) {
		let (d, extent) = match self {
			Side::Left => (p.x - rect.left(), rect.width()),
			Side::Right => (rect.right() - p.x, rect.width()),
			Side::Top => (p.y - rect.top(), rect.height()),
			Side::Bottom => (rect.bottom() - p.y, rect.height()),
		};
		(d, d / extent)
	}
}

/// Where a dragged pane or group lands.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drop {
	/// A group of its own along an outer edge of the dock.
	Edge(Side),
	/// A tab of the group.
	Tab(TileId),
	/// A group of its own beside the group.
	Beside(TileId, Side),
}

/// What a pane's Window position menu asked for.
#[derive(Clone, Copy, Debug)]
enum Placing {
	Move(Side),
	Close,
}

pub(super) struct Dock {
	tree: Tree<Pane>,
	/// View > Panes > Preview: whether the centre shows the preview.
	pub(super) preview_shown: bool,
	/// The tree was edited since [`Dock::take_change`].
	changed: bool,
	/// The arrangement as last saved or loaded.
	saved: Tree<Pane>,
}

impl Dock {
	/// The arrangement saved in the settings; the first start's one when there is none or it is not readable.
	pub(super) fn load(saved: Option<&str>) -> Self {
		let tree = saved.and_then(|json| {
			serde_json::from_str::<Tree<Pane>>(json)
				.map_err(|e| e.to_string())
				.and_then(|tree| {
					let mut reached = Vec::new();
					if let Some(root) = tree.root {
						reach(&tree.tiles, root, &mut reached);
					}
					match Pane::ALL.into_iter().find(|p| !reached.contains(p)) {
						Some(missing) => Err(format!("its {missing:?} pane is not in the window")),
						None => Ok(tree),
					}
				})
				.inspect_err(|e| tracing::warn!("the saved pane arrangement could not be read: {e}"))
				.ok()
		});
		let tree = tree.unwrap_or_else(first_layout);
		Self { saved: tree.clone(), tree, preview_shown: true, changed: false }
	}

	/// The arrangement for the settings.
	pub(super) fn save(&self) -> Option<String> {
		serde_json::to_string(&self.tree).inspect_err(|e| tracing::warn!("the pane arrangement is not saved: {e}")).ok()
	}

	/// The arrangement for the settings when an edit since the last call changed it.
	pub(super) fn take_change(&mut self) -> Option<String> {
		if !std::mem::take(&mut self.changed) || self.tree == self.saved {
			return None;
		}
		self.saved.clone_from(&self.tree);
		self.save()
	}

	fn tile(&self, pane: Pane) -> Option<TileId> {
		self.tree.tiles.find_pane(&pane)
	}

	/// Whether a pane is in the window (a tab behind another counts).
	pub(super) fn is_shown(&self, pane: Pane) -> bool {
		match pane {
			Pane::Preview => self.preview_shown,
			_ => self.tile(pane).is_some_and(|t| self.tree.tiles.is_visible(t)),
		}
	}

	/// Shows a pane where it was last, in front of its tabs.
	pub(super) fn show(&mut self, pane: Pane) {
		if pane == Pane::Preview {
			self.preview_shown = true;
		} else if let Some(tile) = self.tile(pane) {
			self.tree.tiles.set_visible(tile, true);
			self.tree.make_active(|id, _| id == tile);
			self.changed = true;
		}
	}

	/// View > Panes.
	pub(super) fn toggle(&mut self, pane: Pane) {
		match pane {
			Pane::Preview => self.preview_shown = !self.preview_shown,
			_ if self.is_shown(pane) => {
				if let Some(tile) = self.tile(pane) {
					self.tree.tiles.set_visible(tile, false);
					self.changed = true;
				}
			}
			_ => self.show(pane),
		}
	}

	/// The dock in `ui`'s room, `draw` drawing each pane shown.
	pub(super) fn ui<'a>(
		&mut self,
		ui: &mut egui::Ui,
		strings: &'a Strings,
		draw: &'a mut dyn FnMut(&mut egui::Ui, Pane),
	) {
		let host = ui.available_rect_before_wrap();
		let preview = self.tile(Pane::Preview);
		let tiles = &self.tree.tiles;
		let bare = preview.and_then(|p| tiles.parent_of(p)).filter(|&parent| match tiles.get_container(parent) {
			Some(Container::Tabs(tabs)) => tabs.children.iter().all(|&c| Some(c) == preview || !tiles.is_visible(c)),
			_ => false,
		});
		let preview_shown = self.preview_shown;
		let mut behavior = Behavior {
			strings,
			bare,
			root: self.tree.root,
			preview_shown,
			draw,
			placing: None,
			drag: None,
			edited: false,
		};
		self.tree.ui(&mut behavior, ui);
		self.changed |= behavior.edited;
		if let Some((tile, response)) = behavior.drag {
			let pointer = response.interact_pointer_pos();
			let target = pointer.and_then(|p| self.target_at(tile, host, p));
			if response.dragged() {
				if let Some(p) = pointer {
					self.paint_drag(ui, target.map(|t| t.1), tile, p, strings);
				}
			} else if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
				// egui ends a drag on Esc.
			} else if let Some((to, _)) = target {
				self.drop(tile, to);
			}
		}
		if let Some((tile, placing)) = behavior.placing {
			match placing {
				Placing::Close => {
					self.tree.tiles.set_visible(tile, false);
					self.changed = true;
				}
				Placing::Move(side) => self.drop(tile, Drop::Edge(side)),
			}
		}
	}

	/// A tile's group: a pane's tabs, or the tabs themselves.
	fn group_of(&self, tile: TileId) -> Option<TileId> {
		match self.tree.tiles.get(tile) {
			Some(Tile::Container(Container::Tabs(_))) => Some(tile),
			_ => self.tree.tiles.parent_of(tile),
		}
	}

	/// Where `dragged` lands with the pointer at `p` in the dock's `host`, with the room it will take there: an
	/// outer edge's strip, else a side of the group under the pointer, else its middle; the Preview's group takes
	/// its sides only, and the dragged tile's own group nothing.
	fn target_at(&self, dragged: TileId, host: Rect, p: Pos2) -> Option<(Drop, Rect)> {
		const SIDES: [Side; 4] = [Side::Left, Side::Right, Side::Top, Side::Bottom];
		if !host.contains(p) {
			return None;
		}
		if let Some(side) = SIDES.into_iter().find(|s| s.distance(host, p).0 < EDGE_STRIP) {
			return Some((Drop::Edge(side), side.part(host, EDGE_SHARE)));
		}
		let tiles = &self.tree.tiles;
		let (group, rect) = tiles
			.iter()
			.filter(|(_, tile)| matches!(tile, Tile::Container(Container::Tabs(_))))
			.filter_map(|(&id, _)| Some((id, tiles.rect(id)?)))
			.find(|(_, rect)| rect.contains(p))?;
		if Some(group) == self.group_of(dragged) {
			return None;
		}
		let (side, share) = SIDES
			.into_iter()
			.map(|s| (s, s.distance(rect, p).1))
			.min_by(|a, b| a.1.total_cmp(&b.1))
			.expect("a rectangle has sides");
		if share < SIDE_ZONE {
			Some((Drop::Beside(group, side), side.part(rect, BESIDE_SHARE)))
		} else if self.tile(Pane::Preview).and_then(|t| tiles.parent_of(t)) != Some(group) {
			Some((Drop::Tab(group), rect))
		} else {
			None
		}
	}

	/// Moves `tile`, a pane or a group, to `to`.
	fn drop(&mut self, tile: TileId, to: Drop) {
		self.changed = true;
		let tiles = &mut self.tree.tiles;
		if let Some(parent) = tiles.parent_of(tile)
			&& let Some(Tile::Container(container)) = tiles.get_mut(parent)
		{
			container.retain(|child| child != tile);
		}
		let group = match tiles.get(tile) {
			Some(Tile::Container(Container::Tabs(_))) => tile,
			_ => tiles.insert_tab_tile(vec![tile]),
		};
		match to {
			Drop::Edge(side) => {
				if let Some(root) = self.tree.root {
					self.wrap(root, group, side, EDGE_SHARE);
				}
			}
			Drop::Beside(target, side) => self.wrap(target, group, side, BESIDE_SHARE),
			Drop::Tab(target) => {
				let moved = match tiles.remove(group) {
					Some(Tile::Container(container)) => container.children().copied().collect(),
					_ => Vec::new(),
				};
				if let Some(Tile::Container(Container::Tabs(tabs))) = tiles.get_mut(target) {
					tabs.children.extend(&moved);
					tabs.active = moved.first().copied();
				}
			}
		}
	}

	/// Puts `group` at `side` of `target`, taking `share` of it: `target` becomes their split under its own id.
	fn wrap(&mut self, target: TileId, group: TileId, side: Side, share: f32) {
		let tiles = &mut self.tree.tiles;
		let Some(tile) = tiles.remove(target) else { return };
		let kept = tiles.insert_new(tile);
		let split = match side {
			Side::Left => Linear::new_binary(LinearDir::Horizontal, [group, kept], share),
			Side::Top => Linear::new_binary(LinearDir::Vertical, [group, kept], share),
			Side::Right => Linear::new_binary(LinearDir::Horizontal, [kept, group], 1. - share),
			Side::Bottom => Linear::new_binary(LinearDir::Vertical, [kept, group], 1. - share),
		};
		tiles.insert(target, Tile::Container(split.into()));
	}

	/// The drop target's room and the dragged title by the pointer `p`, over the dock.
	fn paint_drag(&self, ui: &egui::Ui, room: Option<Rect>, dragged: TileId, p: Pos2, strings: &Strings) {
		ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
		let accent = ui.visuals().widgets.active.bg_fill;
		let painter = ui.ctx().layer_painter(LayerId::new(Order::Tooltip, ui.scope_id().with("dock_drop")));
		if let Some(room) = room {
			painter.rect(
				room,
				0.,
				accent.gamma_multiply(DROP_FILL),
				Stroke::new(DROP_BORDER, accent),
				StrokeKind::Inside,
			);
		}
		let tiles = &self.tree.tiles;
		let pane = match tiles.get(dragged) {
			Some(Tile::Container(Container::Tabs(tabs))) => tabs.active.and_then(|a| tiles.get_pane(&a)),
			_ => tiles.get_pane(&dragged),
		};
		if let Some(pane) = pane {
			let font = TextStyle::Button.resolve(ui.style());
			let galley = painter.layout_no_wrap(strings.get(pane.title_key()).to_owned(), font, Color32::WHITE);
			let ghost = Rect::from_min_size(p + Vec2::splat(GHOST_OFFSET), galley.size() + 2. * GHOST_PADDING);
			painter.rect_filled(ghost, 0., accent);
			painter.galley(ghost.min + GHOST_PADDING, galley, Color32::WHITE);
		}
	}
}

/// Collects the panes under `tile`.
fn reach(tiles: &Tiles<Pane>, tile: TileId, panes: &mut Vec<Pane>) {
	match tiles.get(tile) {
		Some(Tile::Pane(pane)) => panes.push(*pane),
		Some(Tile::Container(container)) => container.children().for_each(|&child| reach(tiles, child, panes)),
		None => {}
	}
}

/// The first start's arrangement: Project over Properties (1 : 2) in a left column a quarter of the window wide;
/// Messages hidden at the bottom.
fn first_layout() -> Tree<Pane> {
	let mut tiles = Tiles::default();
	let panes = Pane::ALL.map(|pane| tiles.insert_pane(pane));
	let [preview, project, properties, messages] = panes.map(|pane| tiles.insert_tab_tile(vec![pane]));
	let mut column = Linear::new(LinearDir::Vertical, vec![project, properties]);
	column.shares.set_share(properties, 2.);
	let column = tiles.insert_container(column);
	let main = tiles.insert_container(Linear::new_binary(LinearDir::Horizontal, [column, preview], EDGE_SHARE));
	let root = tiles.insert_container(Linear::new_binary(LinearDir::Vertical, [main, messages], 1. - EDGE_SHARE));
	tiles.set_visible(panes[3], false);
	Tree::new(egui::Id::unique("dock"), root, tiles)
}

struct Behavior<'a> {
	strings: &'a Strings,
	/// The Preview's tab group while it holds no other shown pane: it shows no tab bar.
	bare: Option<TileId>,
	root: Option<TileId>,
	/// The centre shows the Preview.
	preview_shown: bool,
	/// Draws a pane.
	draw: &'a mut dyn FnMut(&mut egui::Ui, Pane),
	placing: Option<(TileId, Placing)>,
	/// The pane or group being dragged, by its tab or tab bar.
	drag: Option<(TileId, Response)>,
	/// The tree changed a share or a group's active tab.
	edited: bool,
}

impl Behavior<'_> {
	/// The Preview, its bare group and the root stay in place.
	fn is_movable(&self, tiles: &Tiles<Pane>, tile: TileId) -> bool {
		tiles.get_pane(&tile) != Some(&Pane::Preview) && self.bare != Some(tile) && self.root != Some(tile)
	}

	/// Notes a drag of `tile` by `response`, a widget sensing drags.
	fn drag_by(&mut self, tile: TileId, response: Response) -> Response {
		let response = response.on_hover_cursor(CursorIcon::Grab);
		if response.dragged() || response.drag_stopped() {
			self.drag = Some((tile, response.clone()));
		}
		response
	}
}

impl egui_tiles::Behavior<Pane> for Behavior<'_> {
	fn pane_ui(&mut self, ui: &mut egui::Ui, _tile_id: TileId, pane: &mut Pane) -> UiResponse {
		if *pane != Pane::Preview || self.preview_shown {
			(self.draw)(ui, *pane);
		}
		UiResponse::None
	}

	fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
		self.strings.get(pane.title_key()).into()
	}

	fn is_tab_closable(&self, tiles: &Tiles<Pane>, tile_id: TileId) -> bool {
		tiles.get_pane(&tile_id).is_some_and(|p| *p != Pane::Preview)
	}

	/// Hides the pane after the frame's layout, which the tree finishes with the pane in it.
	fn on_tab_close(&mut self, _tiles: &mut Tiles<Pane>, tile_id: TileId) -> bool {
		self.placing = Some((tile_id, Placing::Close));
		false
	}

	fn top_bar_right_ui(
		&mut self,
		tiles: &Tiles<Pane>,
		ui: &mut egui::Ui,
		_tile_id: TileId,
		tabs: &Tabs,
		_scroll_offset: &mut f32,
	) {
		let Some(active) = tabs.active.filter(|a| tiles.get_pane(a).is_some_and(|p| *p != Pane::Preview)) else {
			return;
		};
		let s = self.strings;
		let button = egui::Button::new(ICON_ARROW_DROP_DOWN.rich_text().size(ICON_SIZE)).frame(false);
		let (response, shown) = egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
			for (side, key) in Side::ALL {
				if menu_keys::item(ui, s.get(key)).clicked() {
					self.placing = Some((active, Placing::Move(side)));
				}
			}
			ui.separator();
			if menu_keys::item(ui, s.get("DockCloseMenu")).clicked() {
				self.placing = Some((active, Placing::Close));
			}
		});
		if shown.is_some() {
			menu_keys::menu(ui.ctx(), egui::Popup::default_response_id(&response), response.id);
		}
		response.on_hover_text(s.get("DockWindowPosition"));
	}

	/// The active pane's title is accent-coloured with white text.
	fn tab_bg_color(
		&self,
		visuals: &egui::Visuals,
		_tiles: &Tiles<Pane>,
		_tile_id: TileId,
		state: &TabState,
	) -> Color32 {
		if state.active { visuals.widgets.active.bg_fill } else { Color32::TRANSPARENT }
	}

	fn tab_text_color(
		&self,
		visuals: &egui::Visuals,
		_tiles: &Tiles<Pane>,
		_tile_id: TileId,
		state: &TabState,
	) -> Color32 {
		if state.active { Color32::WHITE } else { visuals.widgets.noninteractive.text_color() }
	}

	/// The active tab's close cross is white on its accent title, as its text. The tab takes an id of its own, as
	/// the tree drops any drag of the tile's id.
	fn tab_ui(
		&mut self,
		tiles: &mut Tiles<Pane>,
		ui: &mut egui::Ui,
		id: egui::Id,
		tile_id: TileId,
		state: &TabState,
	) -> Response {
		let style = ui.style().clone();
		if state.active {
			let widgets = &mut ui.visuals_mut().widgets;
			for w in [&mut widgets.inactive, &mut widgets.hovered, &mut widgets.active] {
				w.fg_stroke.color = Color32::WHITE;
			}
		}
		let response = egui_tiles::Behavior::tab_ui(&mut DefaultTab(self), tiles, ui, id.with("dock"), tile_id, state);
		ui.set_style(style);
		if self.is_movable(tiles, tile_id) { self.drag_by(tile_id, response.interact(Sense::drag())) } else { response }
	}

	/// The bar's room after the tabs drags their group and shows a dotted grip.
	fn tab_bar_trailing_ui(&mut self, tiles: &Tiles<Pane>, ui: &mut egui::Ui, tile_id: TileId, _tabs: &Tabs) {
		if self.is_movable(tiles, tile_id) {
			let room = ui.available_rect_before_wrap().intersect(ui.clip_rect());
			paint_grip(ui, room);
			let response = ui.interact(room, ui.scope_id().with("dock_bar"), Sense::drag());
			self.drag_by(tile_id, response);
		}
	}

	fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
		TAB_BAR_HEIGHT
	}

	fn is_tab_bar_visible(&self, tile_id: TileId, _tabs: &Tabs) -> bool {
		self.bare != Some(tile_id)
	}

	/// The dock carries the drags.
	fn is_tile_draggable(&self, _tiles: &Tiles<Pane>, _tile_id: TileId) -> bool {
		false
	}

	fn on_edit(&mut self, _action: EditAction) {
		self.edited = true;
	}

	fn simplification_options(&self) -> SimplificationOptions {
		SimplificationOptions { all_panes_must_have_tabs: true, ..SimplificationOptions::default() }
	}
}

/// The grip band's height and its dots' period; a row's dots sit at every `GRIP_TILE`, odd rows shifted by half.
const GRIP_HEIGHT: f32 = 5.;
const GRIP_TILE: f32 = 4.;

/// The dotted band of a title bar's drag room, inset 4 px from the tabs and 6 px from the buttons.
fn paint_grip(ui: &egui::Ui, room: Rect) {
	let (left, right) = (room.left() + 4., room.right() - 6.);
	if right - left < GRIP_TILE {
		return;
	}
	let ppp = ui.pixels_per_point();
	let top = ((room.center().y - GRIP_HEIGHT / 2.) * ppp).round() / ppp;
	let left = (left * ppp).round() / ppp;
	let color = super::theme::Palette::of(ui.ctx().theme()).title_grip;
	let mut mesh = egui::Mesh::default();
	for row in 0..3u8 {
		let y = top + f32::from(row) * 2.;
		let mut x = left + if row % 2 == 0 { 0. } else { 2. };
		while x < right {
			mesh.add_colored_rect(Rect::from_min_size(Pos2::new(x, y), Vec2::splat(1.)), color);
			x += GRIP_TILE;
		}
	}
	ui.painter().add(mesh);
}

/// egui_tiles' own tab, with [`Behavior`]'s title, colours and close.
struct DefaultTab<'b, 'a>(&'b mut Behavior<'a>);

impl egui_tiles::Behavior<Pane> for DefaultTab<'_, '_> {
	fn pane_ui(&mut self, ui: &mut egui::Ui, tile_id: TileId, pane: &mut Pane) -> UiResponse {
		self.0.pane_ui(ui, tile_id, pane)
	}

	fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
		self.0.tab_title_for_pane(pane)
	}

	fn on_tab_close(&mut self, tiles: &mut Tiles<Pane>, tile_id: TileId) -> bool {
		self.0.on_tab_close(tiles, tile_id)
	}

	fn tab_bg_color(&self, visuals: &egui::Visuals, tiles: &Tiles<Pane>, tile_id: TileId, state: &TabState) -> Color32 {
		self.0.tab_bg_color(visuals, tiles, tile_id, state)
	}

	fn tab_text_color(
		&self,
		visuals: &egui::Visuals,
		tiles: &Tiles<Pane>,
		tile_id: TileId,
		state: &TabState,
	) -> Color32 {
		self.0.tab_text_color(visuals, tiles, tile_id, state)
	}

	fn is_tile_draggable(&self, tiles: &Tiles<Pane>, tile_id: TileId) -> bool {
		self.0.is_tile_draggable(tiles, tile_id)
	}
}

#[cfg(test)]
mod tests {
	use wavedude_app_state::strings::Language;

	use super::*;

	/// Lays the dock out in a 1000 x 800 window; the dock's rectangle.
	fn lay_out(dock: &mut Dock) -> Rect {
		let ctx = egui::Context::default();
		let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 800.));
		let mut host = Rect::NOTHING;
		let output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
			host = ui.available_rect_before_wrap();
			dock.ui(ui, &Strings::new(Language::English), &mut |_, _| {});
		});
		output.drop_without_applying_deltas();
		host
	}

	fn group(dock: &Dock, pane: Pane) -> (TileId, Rect) {
		let group = dock.group_of(dock.tile(pane).unwrap()).unwrap();
		(group, dock.tree.tiles.rect(group).unwrap())
	}

	#[test]
	fn a_pane_drops_at_an_edge_beside_a_group_or_into_one_but_not_its_own_or_the_previews() {
		let mut dock = Dock::load(None);
		let host = lay_out(&mut dock);
		let project = dock.tile(Pane::Project).unwrap();
		let at = |p: Pos2| dock.target_at(project, host, p);
		let own_rect = group(&dock, Pane::Project).1;
		let (properties, properties_rect) = group(&dock, Pane::Properties);
		let (preview, preview_rect) = group(&dock, Pane::Preview);
		assert_eq!(at(own_rect.center()), None, "its own group");
		assert_eq!(at(own_rect.center() + Vec2::new(own_rect.width() * 0.4, 0.)), None, "its own group's side");
		assert_eq!(at(properties_rect.center()), Some((Drop::Tab(properties), properties_rect)));
		assert_eq!(at(preview_rect.center()), None, "the Preview takes no tab");
		let right = Pos2::new(preview_rect.right() - 50., preview_rect.center().y);
		let (to, room) = at(right).unwrap();
		assert_eq!(to, Drop::Beside(preview, Side::Right));
		assert!(
			(room.width() - preview_rect.width() * BESIDE_SHARE).abs() < 0.5 && room.right() == preview_rect.right()
		);
		let (to, room) = at(Pos2::new(host.left() + 10., host.center().y)).unwrap();
		assert_eq!(to, Drop::Edge(Side::Left));
		assert!((room.width() - host.width() * EDGE_SHARE).abs() < 0.5);
	}

	#[test]
	fn a_drop_makes_a_tab_a_split_or_an_edge_group() {
		let mut dock = Dock::load(None);
		lay_out(&mut dock);
		let (project, properties) = (dock.tile(Pane::Project).unwrap(), dock.tile(Pane::Properties).unwrap());
		let (properties_group, _) = group(&dock, Pane::Properties);
		dock.drop(project, Drop::Tab(properties_group));
		lay_out(&mut dock);
		assert_eq!(dock.group_of(project), dock.group_of(properties), "Project tabbed into Properties");
		assert!(dock.tree.tiles.rect(project).is_some(), "the dropped tab is the active one");

		let (preview_group, preview_rect) = group(&dock, Pane::Preview);
		dock.drop(project, Drop::Beside(preview_group, Side::Right));
		lay_out(&mut dock);
		let (_, project_rect) = group(&dock, Pane::Project);
		assert!(project_rect.left() >= preview_rect.right() - preview_rect.width() * BESIDE_SHARE - 1.);
		assert_eq!(dock.group_of(dock.tile(Pane::Preview).unwrap()), Some(group(&dock, Pane::Preview).0));

		let host = lay_out(&mut dock);
		let (properties_group, _) = group(&dock, Pane::Properties);
		dock.drop(properties_group, Drop::Edge(Side::Bottom));
		lay_out(&mut dock);
		let (_, rect) = group(&dock, Pane::Properties);
		assert!((rect.bottom() - host.bottom()).abs() < 1. && (rect.width() - host.width()).abs() < 1.);
		assert!(Pane::ALL.into_iter().all(|p| p == Pane::Messages || dock.is_shown(p)));
	}

	#[test]
	fn the_first_start_hides_the_messages_only() {
		let dock = Dock::load(None);
		let shown = Pane::ALL.map(|p| dock.is_shown(p));
		assert_eq!(shown, [true, true, true, false]);
	}

	#[test]
	fn a_hidden_pane_comes_back_in_its_place_and_the_arrangement_is_saved() {
		let mut dock = Dock::load(None);
		dock.toggle(Pane::Messages);
		dock.toggle(Pane::Project);
		let mut loaded = Dock::load(dock.save().as_deref());
		assert!(loaded.is_shown(Pane::Messages) && !loaded.is_shown(Pane::Project));
		let parent = |d: &Dock| d.tile(Pane::Project).and_then(|t| d.tree.tiles.parent_of(t));
		let before = parent(&loaded);
		loaded.toggle(Pane::Project);
		assert!(loaded.is_shown(Pane::Project));
		assert_eq!(parent(&loaded), before);
	}

	#[test]
	fn an_arrangement_with_a_pane_off_the_root_gives_the_first_start_one() {
		let mut dock = Dock::load(None);
		dock.toggle(Pane::Messages);
		let messages = dock.tile(Pane::Messages).unwrap();
		let tiles = &mut dock.tree.tiles;
		if let Some(parent) = tiles.parent_of(messages)
			&& let Some(Tile::Container(container)) = tiles.get_mut(parent)
		{
			container.retain(|child| child != messages);
		}
		let loaded = Dock::load(dock.save().as_deref());
		assert!(!loaded.is_shown(Pane::Messages));
		let mut reached = Vec::new();
		reach(&loaded.tree.tiles, loaded.tree.root.unwrap(), &mut reached);
		assert_eq!(reached.len(), Pane::ALL.len());
	}

	#[test]
	fn an_unreadable_arrangement_gives_the_first_start_one() {
		let dock = Dock::load(Some(r#"{"left":{"size":0,"groups":[]}}"#));
		assert!(dock.is_shown(Pane::Project) && !dock.is_shown(Pane::Messages));
	}
}
