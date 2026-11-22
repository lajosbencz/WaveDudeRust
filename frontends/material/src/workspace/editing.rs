//! Editing on a picture (DESIGN.md, Canvas): the handles, each a focusable widget over its overlay handle, the
//! presses, drags, marquee, Esc and keys that select and edit through [`waveduderust_app::preview::overlay`], the
//! graph boxes through [`waveduderust_egui_preview::graph_boxes`], the context menus of the picture, a handle and a box,
//! the handles' tooltips and cursors, then the overlay and the boxes drawn.

use eframe::egui::accesskit::Role;
use eframe::egui::{Color32, CursorIcon, Id, IdSalt, Key, Rect, Response, Sense, Ui, UiBuilder, Vec2};
use egui_mineral::i18n::I18n;
use egui_mineral::layout::{Constraints, Measure, MinTarget};
use egui_mineral::semantic::{KeySalt, SemanticNode, StableId};
use egui_mineral::widget::{MineralWidget, WidgetFrame, WidgetResponse};
use egui_mineral_material::prelude::{Material, Menu, MenuItem, Rt, icons};
use waveduderust::model::{ItemRef, Layer, ProbeId};
use waveduderust::unstable::view::overlay::{Handle, HandleKind};
use waveduderust_app::Event;
use waveduderust_app::menus::{ExportKindChoice, MenuAction, On};
use waveduderust_app::preview::handles::{HIT_RADIUS, hit};
use waveduderust_app::preview::overlay::handle_tip;
use waveduderust_app::preview::view::CanvasTransform;
use waveduderust_app::selection::Row;
use waveduderust_app::strings::strip_mnemonic;
use waveduderust_egui_platform::input::{lock_arrows, pinched_press, pinching};
use waveduderust_egui_preview::graph_boxes::{box_responses, cancel_box_drag, edit_box, on_picture};
use waveduderust_egui_preview::overlay::{drive_pointer, focus_selects, handle_keys};

use super::Workspace;
use crate::commands::Cmd;
use crate::menus::{self, runs_on};
use crate::shell::Intent;
use crate::{App, Mui, Salt, commands};

/// A handle's keyboard and accessibility widget; the pointer goes to the picture under it, which hit-tests the
/// overlay, so the handle senses the focus alone.
struct HandleWidget {
	key: String,
	label: String,
	size: Vec2,
	selected: bool,
}

impl MineralWidget for HandleWidget {
	type Kit = Material;
	type State = ();
	type Output = ();

	fn id_salt(&self) -> IdSalt {
		IdSalt::new(&self.key)
	}

	fn sense(&self) -> Sense {
		Sense::focusable_noninteractive()
	}

	fn min_target(&self, _: &Mui<'_>) -> MinTarget {
		MinTarget::None
	}

	fn layout(&self, _ui: &Mui<'_>, _constraints: Constraints, _state: &()) -> Measure {
		self.size.into()
	}

	fn update(&mut self, _ui: &mut Mui<'_>, frame: &WidgetFrame<'_>, _state: &mut ()) -> WidgetResponse<()> {
		WidgetResponse::new(frame.response.clone(), ())
	}

	fn semantics(&self, _i18n: &I18n, response: &WidgetResponse<()>, _state: &()) -> Option<SemanticNode> {
		let mut node = SemanticNode::new(StableId::new(self.key.clone()), Role::Button, &self.label);
		node.state.selected = Some(self.selected);
		node.state.focused = response.has_focus();
		Some(node)
	}
}

/// A handle's agent key: `canvas-<layer>-<item>-<handle>`.
fn handle_key(layer: Layer, h: &Handle) -> String {
	let layer = match layer {
		Layer::Field => "field",
		Layer::Intensity => "intensity",
		_ => "layer",
	};
	let kind = Salt(h.item.kind()).word();
	let index = Salt(h.item).number();
	let part = match h.kind {
		HandleKind::Source => "source".into(),
		HandleKind::SourceLever => "sourcelever".into(),
		HandleKind::PartCentre => "partcentre".into(),
		HandleKind::PartLever => "partlever".into(),
		HandleKind::Grip => "grip".into(),
		HandleKind::Point(i) => format!("point({i})"),
		HandleKind::Node(i) => format!("node({i})"),
		HandleKind::InHandle(i) => format!("inhandle({i})"),
		HandleKind::OutHandle(i) => format!("outhandle({i})"),
		HandleKind::ProbeA => "probea".into(),
		HandleKind::ProbeB => "probeb".into(),
	};
	format!("canvas-{layer}-{kind}{index}-{part}")
}

/// The picture's handles and boxes under its `response`, then the overlay and the boxes drawn in `accent`.
#[expect(clippy::too_many_arguments, reason = "one picture's room, state and outputs")]
pub(super) fn edit(
	ws: &mut Workspace,
	ui: &mut Mui<'_>,
	app: &mut App,
	layer: Layer,
	t: &CanvasTransform,
	clip: Rect,
	response: &Response,
	accent: Color32,
	out: &mut Vec<Event>,
	intents: &mut Vec<Intent>,
) {
	let rt = ui.rt();
	boxes(ws, rt, ui, app, layer, t, clip, accent, out, intents);
	let focused = handle_widgets(ws, ui, app, layer, t, clip, out, intents);
	pointer(ws, ui, app, layer, t, response, out);
	picture_menu(ws, rt, ui, app, layer, t, response, out, intents);
	let marquee = ws.grab.as_ref().filter(|(l, _)| *l == layer).and_then(|(_, g)| g.marquee());
	ws.overlays.paint(ui, app, layer, t, clip, focused, marquee, accent);
	let box_id = move |i| box_id(layer, i);
	ws.boxes.paint(ui, app, layer, t, clip, box_id, ws.pointer_focus, accent);
}

fn box_id(layer: Layer, probe: ProbeId) -> Id {
	Id::unique(("canvas-box", layer as usize, probe))
}

/// One focusable widget per handle inside `clip`, in overlay order (the Tab order): focusing one selects its item,
/// arrows nudge it (Shift ten), Del removes its item, Shift+F10 opens its item's menu. Returns the focused one.
#[expect(clippy::too_many_arguments, reason = "one picture's room, state and outputs")]
fn handle_widgets(
	ws: &mut Workspace,
	ui: &mut Mui<'_>,
	app: &mut App,
	layer: Layer,
	t: &CanvasTransform,
	clip: Rect,
	out: &mut Vec<Event>,
	intents: &mut Vec<Intent>,
) -> Option<usize> {
	let rt = ui.rt();
	let handles = ws.overlays.model(app, layer, t).handles.clone();
	let mut focused = None;
	for (i, h) in handles.iter().enumerate() {
		let (x, y) = h.at;
		let rect = Rect::from_center_size(t.to_screen(x, y), Vec2::splat(2. * HIT_RADIUS)).intersect(clip);
		if !rect.is_positive() {
			continue;
		}
		let key = handle_key(layer, h);
		let widget = HandleWidget { label: handle_tip(app, h), size: rect.size(), selected: h.selected, key };
		// an explicit scope keeps the widget's id when handles before it come or go
		let builder = UiBuilder::new().scope_id(ui.scope_id().with(&widget.key)).max_rect(rect);
		let r = ui.scope_builder(builder, |ui| rt.ui(ui).mineral(widget).into_response()).inner;
		if r.gained_focus() {
			focus_selects(app, h.item);
		}
		if Menu::context_requested(rt, &r) {
			item_menu(rt, app, h.item, &r, intents);
			ws.menu_open |= Menu::is_context_open(ui.ctx(), &r);
		}
		if !r.has_focus() {
			continue;
		}
		focused = Some(i);
		ws.canvas_focused = true;
		commands::bind_pane_keys(ui.ctx(), app, r.id);
		lock_arrows(ui, r.id);
		handle_keys(ui, app, h, out);
	}
	focused
}

/// The pointer on the picture through [`drive_pointer`]; Esc also cancels a box drag. The cursor grabs over a
/// handle and while a grab edits.
fn pointer(
	ws: &mut Workspace,
	ui: &Ui,
	app: &mut App,
	layer: Layer,
	t: &CanvasTransform,
	response: &Response,
	out: &mut Vec<Event>,
) {
	let other = ws.box_drag.is_some();
	let outcome = drive_pointer(ui, response, app, &mut ws.overlays, &mut ws.grab, other, layer, t, out);
	if outcome.escaped {
		cancel_box_drag(&mut ws.box_drag, app, out);
	}
	match &ws.grab {
		Some((l, g)) if *l == layer && g.edits() => ui.ctx().set_cursor_icon(CursorIcon::Grabbing),
		Some(_) => {}
		None if outcome.hovered.is_some() => ui.ctx().set_cursor_icon(CursorIcon::Grab),
		None => {}
	}
}

/// The graph boxes of `layer`: [`edit_box`] each, Esc or a second finger cancels a box drag, a right-click or
/// Shift+F10 opens a box's menu.
#[expect(clippy::too_many_arguments, reason = "one picture's room, state and outputs")]
fn boxes(
	ws: &mut Workspace,
	rt: &Rt,
	ui: &Ui,
	app: &mut App,
	layer: Layer,
	t: &CanvasTransform,
	clip: Rect,
	accent: Color32,
	out: &mut Vec<Event>,
	intents: &mut Vec<Intent>,
) {
	if pinching(ui) {
		cancel_box_drag(&mut ws.box_drag, app, out);
	}
	let pointer_focus = ws.pointer_focus;
	let views: Vec<(ProbeId, Rect, (Response, Response))> = (ws.boxes.views(app, layer).iter())
		.map(|v| {
			let rect = on_picture(v, t);
			(v.id, rect, box_responses(ui, app, v, rect, clip, box_id(layer, v.id), pointer_focus, accent))
		})
		.collect();
	for (index, rect, (response, grip)) in views {
		edit_box(ui, app, (index, rect), t, (&response, &grip), &mut ws.box_drag, &mut ws.pointer_focus, out);
		if response.has_focus() || grip.has_focus() {
			ws.canvas_focused = true;
		}
		let item = ItemRef::Probe(index);
		if Menu::context_requested(rt, &response) {
			if response.secondary_clicked() && !app.selection.contains(item) {
				app.selection.select(Row::Item(item));
			}
			let on = On::Row(Some(Row::Item(item)));
			let csv = MenuItem::new(strip_mnemonic(app.strings.get("MenuExportProbeCsv")))
				.leading(icons::ICON_IOS_SHARE)
				.disabled(!runs_on(app, MenuAction::ExportProbeCsv, on));
			let menu = Menu::new(("canvas-box-menu", layer as usize, Salt(item)), app.strings.get("NavMore"))
				.item("export-probe-csv", csv);
			if menu.show_context(rt, &response).clicked.is_some() {
				intents.push(Intent::On(MenuAction::ExportProbeCsv, on));
			}
			ws.menu_open |= Menu::is_context_open(ui.ctx(), &response);
		}
	}
}

/// The context menu of the picture: over a handle its item's (the outline row's), else the picture's own.
#[expect(clippy::too_many_arguments, reason = "one picture's room, state and outputs")]
fn picture_menu(
	ws: &mut Workspace,
	rt: &Rt,
	ui: &Ui,
	app: &mut App,
	layer: Layer,
	t: &CanvasTransform,
	response: &Response,
	out: &mut Vec<Event>,
	intents: &mut Vec<Intent>,
) {
	if pinched_press(ui.ctx()) || !Menu::context_requested(rt, response) {
		return;
	}
	if !Menu::is_context_open(ui.ctx(), response) {
		let keyboard = response.has_focus() && ui.input(|i| i.key_pressed(Key::F10));
		let at = response.hover_pos().filter(|_| !keyboard);
		let model = ws.overlays.model(app, layer, t);
		let item = at.and_then(|p| hit(model, t.to_canvas(p), t.scale)).map(|i| model.handles[i].item);
		if let Some(item) = item.filter(|i| !app.selection.contains(*i)) {
			app.selection.select(Row::Item(item));
		}
		ws.menu_on[layer as usize] = item;
	}
	match ws.menu_on[layer as usize] {
		Some(item) => item_menu(rt, app, item, response, intents),
		None => match own_menu(app, layer).show_context(rt, response).clicked {
			Some(PictureEntry::Brightness) => ws.focus_brightness[layer as usize] = true,
			Some(PictureEntry::Auto) => ws.pictures.auto_brightness(app, layer, out),
			Some(PictureEntry::Export(kind)) => intents.push(Intent::Act(MenuAction::Export(kind))),
			None => {}
		},
	}
	ws.menu_open |= Menu::is_context_open(ui.ctx(), response);
}

/// The picture menu's export entries of `layer`: string key and kind.
fn exports(layer: Layer) -> Vec<(&'static str, ExportKindChoice)> {
	let frame = match layer {
		Layer::Field => Some(ExportKindChoice::FieldFrame),
		Layer::Intensity => Some(ExportKindChoice::IntensityFrame),
		_ => None,
	};
	frame
		.map(|frame| ("MenuExportThisFrame", frame))
		.into_iter()
		.chain([
			("MenuFileExportField", ExportKindChoice::Field),
			("MenuFileExportIntensity", ExportKindChoice::Intensity),
			("MenuFileExportBoth", ExportKindChoice::Both),
		])
		.collect()
}

/// The picture menu's entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum PictureEntry {
	/// Focuses the header's brightness field.
	Brightness,
	Auto,
	Export(ExportKindChoice),
}

impl KeySalt for PictureEntry {
	fn write_key(&self, out: &mut String) {
		match self {
			Self::Brightness => out.push_str("brightness"),
			Self::Auto => out.push_str("auto-brightness"),
			Self::Export(kind) => Cmd::Act(MenuAction::Export(*kind)).write_key(out),
		}
	}
}

/// The picture's own menu: Brightness, Auto brightness, then the exports.
fn own_menu(app: &App, layer: Layer) -> Menu<PictureEntry> {
	let s = &app.strings;
	let auto = MenuItem::new(strip_mnemonic(s.get("MenuAutoBrightness"))).leading(icons::ICON_BRIGHTNESS_AUTO);
	let brightness = MenuItem::new(s.label("DisplayLayer", "Brightness")).leading(icons::ICON_BRIGHTNESS_5);
	let mut menu = Menu::new(("canvas-menu", layer as usize), s.get("NavMore"))
		.item(PictureEntry::Brightness, brightness)
		.item(PictureEntry::Auto, auto)
		.divider();
	for (i, (key, kind)) in exports(layer).into_iter().enumerate() {
		let action = MenuAction::Export(kind);
		let icon = if i == 0 { icons::ICON_IMAGE } else { icons::ICON_IOS_SHARE };
		let item = MenuItem::new(strip_mnemonic(s.get(key))).leading(icon).disabled(!runs_on(app, action, On::Current));
		menu = menu.item(PictureEntry::Export(kind), item);
	}
	menu
}

/// The outline row's menu of `item` on `target`.
fn item_menu(rt: &Rt, app: &App, item: ItemRef, target: &Response, intents: &mut Vec<Intent>) {
	let row = Row::Item(item);
	let on = On::Row(Some(row));
	let menu = Menu::new(("canvas-item-menu", Salt(item)), app.strings.get("NavMore"));
	let menu = menus::action_menu(app, menu, &menus::row_menu(app, row), on);
	if let Some(action) = menus::picked(menu.show_context(rt, target).clicked) {
		intents.push(Intent::On(action, on));
	}
}

impl Workspace {
	/// A press on the pictures or a box drag is in progress.
	pub(crate) fn dragging(&self) -> bool {
		self.grab.is_some() || self.box_drag.is_some()
	}

	/// Esc cancels a drag or closes a canvas menu rather than stopping the run.
	pub(crate) fn takes_escape(&self) -> bool {
		self.dragging() || self.menu_open
	}

	/// The canvas point under the pointer: where `A` adds.
	pub(super) fn hover(&mut self, t: &CanvasTransform, response: &Response) {
		if let Some(p) = response.hover_pos() {
			self.pointer = Some(t.to_canvas(p));
		}
	}
}

/// The room a selection bar of `size` takes over the selection at `sel` inside `visible`: above it, below it at
/// the top edge, its centre over the selection's, held inside.
pub(super) fn bar_rect(sel: Rect, size: Vec2, visible: Rect, gap: f32) -> Rect {
	let above = sel.top() - gap - size.y;
	let y = if above >= visible.top() { above } else { (sel.bottom() + gap).min(visible.bottom() - size.y) };
	let x = (sel.center().x - size.x / 2.).clamp(visible.left(), (visible.right() - size.x).max(visible.left()));
	Rect::from_min_size((x, y.max(visible.top())).into(), size)
}

#[cfg(test)]
mod tests {
	use eframe::egui::vec2;

	use super::*;

	#[test]
	fn the_bar_sits_above_the_selection_or_below_it_at_the_top() {
		let visible = Rect::from_min_size((0., 0.).into(), vec2(400., 300.));
		let size = vec2(200., 40.);
		let middle = Rect::from_min_size((150., 150.).into(), vec2(20., 20.));
		let r = bar_rect(middle, size, visible, 8.);
		assert_eq!((r.left(), r.bottom()), (60., 142.));
		let top = Rect::from_min_size((0., 10.).into(), vec2(20., 20.));
		let r = bar_rect(top, size, visible, 8.);
		assert_eq!((r.left(), r.top()), (0., 38.), "below, held inside");
	}

	#[test]
	fn handle_keys_name_layer_item_and_handle() {
		use waveduderust::unstable::view::overlay::HandleKind;
		let part = waveduderust::model::Project::default()
			.add_part(waveduderust::model::PartKind::Rectangle(Default::default()));
		let h =
			Handle { item: ItemRef::Part(part), kind: HandleKind::Point(3), at: (0., 0.), from: None, selected: false };
		assert_eq!(handle_key(Layer::Field, &h), format!("canvas-field-part{}-point(3)", part.get()));
	}
}
