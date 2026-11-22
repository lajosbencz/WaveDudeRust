//! The selection bar (DESIGN.md, Canvas): on touch-first devices and below Expanded, a floating pill above the
//! canvas selection's bounds (below them at the top edge, hidden during a drag): the item's name, which opens the
//! inspector, Enabled, Duplicate, Remove, and a more menu (Cut, Copy, the moves, Export probe data as CSV).

use eframe::egui::{Align, Layout, Rect, UiBuilder};
use egui_mineral::semantic::KeySalt;
use egui_mineral_material::prelude::{FloatingToolbar, IconAction, Menu, icons};
use waveduderust::model::Layer;
use waveduderust_app::menus::{MenuAction, On};
use waveduderust_app::outline::enabled_of;
use waveduderust_app::preview::overlay::selection_bounds;
use waveduderust_app::preview::view::CanvasTransform;
use waveduderust_app::strings::strip_mnemonic;

use super::Workspace;
use super::editing::bar_rect;
use crate::commands::Cmd;
use crate::menus::{self, row_icon, runs_on};
use crate::shell::Intent;
use crate::{App, Mui};

/// The gap between the selection and the bar.
const GAP: f32 = 8.;

/// The bar over the selection on the first of the `shown` pictures (layer, transform, visible room) that shows it:
/// a probe's handles are on its layer only.
pub(super) fn show(
	ws: &mut Workspace,
	ui: &mut Mui<'_>,
	app: &App,
	shown: &[(Layer, CanvasTransform, Rect)],
	intents: &mut Vec<Intent>,
) {
	// a hidden bar closes its menu
	let more_open = std::mem::take(&mut ws.more_open);
	let Some(item) = app.selection.current_item() else { return };
	if ws.dragging() {
		return;
	}
	let Some((sel, visible)) = shown.iter().find_map(|(layer, t, visible)| {
		let ((x0, y0), (x1, y1)) = selection_bounds(ws.overlays.model(app, *layer, t))?;
		let sel = Rect::from_two_pos(t.to_screen(x0, y0), t.to_screen(x1, y1));
		visible.intersects(sel).then_some((sel, *visible))
	}) else {
		return;
	};
	ws.more_open = more_open;
	let s = &app.strings;
	let project = &app.document.project;
	let name = item.name(project).unwrap_or_default();
	let others = app.selection.items().len().saturating_sub(1);
	let name = if others > 0 { s.format("InspectorMore", &[&name, &others.to_string()]) } else { name.to_owned() };
	let label = |key| strip_mnemonic(s.get(key));
	let icon = row_icon(project, waveduderust_app::selection::Row::Item(item));
	let enabled = enabled_of(project, item).unwrap_or(false);
	let act = |icon, key, salt: &'static str, action, on: bool| {
		let a = IconAction::new(icon, label(key)).salt(salt).selected(on);
		(Bar::Act(action), a.disabled(!runs_on(app, action, On::Current)))
	};
	let actions = [
		(Bar::Name, IconAction::new(icon, name).salt("name")),
		act(icons::ICON_TOGGLE_ON, "MenuProjectToggleEnabled", "enabled", MenuAction::ToggleEnabled, enabled),
		act(icons::ICON_FILE_COPY, "MenuProjectDuplicate", "duplicate", MenuAction::Duplicate, false),
		act(icons::ICON_DELETE, "MenuProjectRemove", "remove", MenuAction::Delete, false),
		(Bar::More, IconAction::new(icons::ICON_MORE_VERT, s.get("NavMore")).salt("more")),
	];
	let room = bar_rect(sel, ws.bar_size, visible, GAP);
	let builder = UiBuilder::new().id_salt("selection-bar").max_rect(room).layout(Layout::left_to_right(Align::Center));
	let rt = ui.rt();
	let r = ui
		.scope_builder(builder, |ui| {
			FloatingToolbar::new("selection-bar", s.get("CanvasSelectionBar")).actions(actions).show(&mut rt.ui(ui))
		})
		.inner;
	ws.bar_size = r.response.rect.size();
	match r.clicked {
		Some(Bar::Name) => ws.focus_form(),
		Some(Bar::More) => ws.more_open = !ws.more_open,
		Some(Bar::Act(action)) => intents.push(Intent::Act(action)),
		None => {}
	}
	let Some(&anchor) = r.actions.get(&Bar::More) else { return };
	let menu = menus::action_menu(
		app,
		Menu::new("selection-bar-menu", s.get("NavMore")),
		&menus::header_menu(app),
		On::Current,
	);
	if let Some(action) = menus::picked(menu.show_at(ui.rt(), anchor, &mut ws.more_open).clicked) {
		intents.push(Intent::Act(action));
	}
	ws.menu_open |= ws.more_open;
}

/// The bar's entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Bar {
	Name,
	Act(MenuAction),
	More,
}

impl KeySalt for Bar {
	fn write_key(&self, out: &mut String) {
		match self {
			Self::Name => out.push_str("name"),
			Self::Act(action) => Cmd::Act(*action).write_key(out),
			Self::More => out.push_str("more"),
		}
	}
}
