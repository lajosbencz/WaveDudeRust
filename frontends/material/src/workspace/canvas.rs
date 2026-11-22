//! The canvas (DESIGN.md, Canvas): the toolbar with the Add menu, then the field and intensity pictures side by side
//! or stacked with a split between them, each under a header with its brightness and Auto, with its handles and
//! graph boxes ([`super::editing`]), and the selection bar. The pictures come from
//! [`waveduderust_egui_preview::pictures`]; the wheel, a middle drag and a pinch zoom and pan both.

use eframe::egui::{
	Align, Color32, Key, KeyboardShortcut, Layout, Modifiers, Rect, Role, Sense, Stroke, StrokeKind, UiBuilder,
	WidgetInfo, vec2,
};
use egui_mineral::layout::Axis;
use egui_mineral::semantic::KeySalt;
use egui_mineral::shortcut::{self, ActionId, Scope};
use egui_mineral::widget::SizeExt as _;
use egui_mineral_material::extras::TreeDrag;
use egui_mineral_material::prelude::{
	Density, EmptyPlaceholder, FloatingToolbar, IconAction, IconButton, LoadingIndicator, MaterialExt as _,
	MaterialUiExt as _, Menu, NumberField, SegmentedButton, SplitPane, TypeScale, icons,
};
use waveduderust::model::{DisplayLayer, DisplayLayerEdit, Layer, Point};
use waveduderust_app::Event;
use waveduderust_app::clipboard::Copyable;
use waveduderust_app::commands::Command;
use waveduderust_app::menus::{On, add_command};
use waveduderust_app::number::NumberSpec;
use waveduderust_app::preview::view::CanvasTransform;
use waveduderust_app::preview::{PictureExtent, drop_paste};
use waveduderust_app::properties::bounds_of;
use waveduderust_app::selection::Row;
use waveduderust_app::settings::Orientation;
use waveduderust_app::strings::strip_mnemonic;
use waveduderust_egui_preview::pictures::navigate;

use super::{Workspace, editing, inspector, layer_events, number, selection_bar};
use crate::commands::{self, Cmd, Layers, ViewCommand};
use crate::shell::Intent;
use crate::{App, Mui, Salt, menus};

/// The room around the toolbar and the pictures.
const PAD: f32 = 8.;
/// The picture's margin inside its cell.
const MARGIN: f32 = 4.;
const BRIGHTNESS_W: f32 = 120.;
/// A picture's header strip.
const HEADER_H: f32 = 40.;
/// The least room either picture keeps in a split.
const MIN_CELL: f32 = 80.;
/// The busy dim over a picture.
const DIM: Color32 = Color32::from_black_alpha(96);
/// Opens the Add menu while the canvas has the focus; the item lands at the pointer.
const ADD_KEY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::A);
const ADD_ACTION: ActionId = ActionId::from_static("canvas.add");

/// The canvas in `ui`'s room; `bar`: the selection bar shows.
pub(super) fn show(
	ws: &mut Workspace,
	ui: &mut Mui<'_>,
	app: &mut App,
	bar: bool,
	out: &mut Vec<Event>,
	intents: &mut Vec<Intent>,
) {
	let room = ui.max_rect().shrink(PAD);
	let rt = ui.rt();
	ui.scope_builder(UiBuilder::new().max_rect(room).layout(Layout::top_down(Align::Min)), |ui| {
		let ui = &mut rt.ui(ui);
		ws.menu_open = false;
		toolbar(ws, ui, app, out);
		ws.canvas_focused = false;
		ws.pointer = None;
		ui.add_space(PAD);
		let Some(layers) = Layers::of(app) else {
			let r = EmptyPlaceholder::new("canvas-empty", app.strings.get("CanvasNothingShown"))
				.icon(icons::ICON_VISIBILITY_OFF)
				.action(strip_mnemonic(app.strings.get("MenuViewField")))
				.show(ui);
			if r.action_clicked {
				out.extend(layer_events(app, Layers::Field));
			}
			return;
		};
		let extents = ws.pictures.extents();
		let mut shown = Vec::with_capacity(2);
		match layers {
			Layers::Both => {
				let side = app.settings.preview_orientation == Orientation::Horizontal;
				let axis = if side { Axis::Horizontal } else { Axis::Vertical };
				let label = app.strings.get("TipPreviewSplitter");
				let cell = |ui: &mut Mui<'_>| ui.available_rect_before_wrap();
				let r = SplitPane::new(("canvas-split", side), label)
					.axis(axis)
					.min_first(MIN_CELL)
					.min_second(MIN_CELL)
					.show(ui, cell, cell);
				for (rect, layer) in [(r.first, Layer::Field), (r.second, Layer::Intensity)] {
					let layout = Layout::top_down(Align::Min);
					let r = ui.scope_builder(UiBuilder::new().max_rect(rect).layout(layout), |ui| {
						picture(ws, &mut rt.ui(ui), app, layer, out, intents)
					});
					shown.push(r.inner);
				}
			}
			Layers::Field => shown.push(picture(ws, ui, app, Layer::Field, out, intents)),
			Layers::Intensity => shown.push(picture(ws, ui, app, Layer::Intensity, out, intents)),
		}
		if bar {
			selection_bar::show(ws, ui, app, &shown, intents);
		}
		// the poll asked for the old extents
		if ws.pictures.extents() != extents {
			ui.ctx().request_repaint();
		}
	});
	ws.focus_canvas = false;
}

/// Field / Both / Intensity, Side by side, Fit, the zoom, Handles, Labels and Add.
fn toolbar(ws: &mut Workspace, ui: &mut Mui<'_>, app: &App, out: &mut Vec<Event>) {
	let s = &app.strings;
	let label = |key| strip_mnemonic(s.get(key));
	let rt = ui.rt();
	ui.horizontal_wrapped(|ui| {
		let ui = &mut rt.ui(ui);
		let side = app.settings.preview_orientation == Orientation::Horizontal;
		let d = &app.document.view;
		let turned = if side { Orientation::Vertical } else { Orientation::Horizontal };
		let action = |icon, key, salt: &'static str, view, on: bool| {
			(Tool::View(view), IconAction::new(icon, label(key)).salt(salt).selected(on))
		};
		let actions = [
			action(
				icons::ICON_VIEW_COLUMN_2,
				"MenuViewSideBySide",
				"side-by-side",
				ViewCommand::Orientation(turned),
				side,
			),
			action(icons::ICON_FIT_SCREEN, "MenuViewFit", "fit", ViewCommand::Fit, false),
			action(icons::ICON_ZOOM_OUT, "MenuViewZoomOut", "zoom-out", ViewCommand::ZoomOut, false),
			action(icons::ICON_ZOOM_IN, "MenuViewZoomIn", "zoom-in", ViewCommand::ZoomIn, false),
			action(icons::ICON_DRAG_PAN, "MenuViewHandles", "handles", ViewCommand::Handles, d.handles_in_preview),
			action(icons::ICON_LABEL, "MenuViewLabels", "labels", ViewCommand::Labels, d.labels_in_preview),
			(Tool::Add, IconAction::new(icons::ICON_ADD, s.get("TipCanvasAdd")).salt("add")),
		];
		let mut shown = Layers::of(app);
		let mut layer_changed = false;
		let r = FloatingToolbar::new("canvas-toolbar", s.get("CanvasToolbar"))
			.custom("layers", |ui| {
				let segments = Layers::ALL.map(|l| (l, label(l.label_key())));
				let r = SegmentedButton::optional("canvas-layers", s.get("TipPreviewLayers"), &mut shown)
					.items(segments)
					.equal_width(false)
					.show(ui);
				layer_changed = r.changed;
				r.response
			})
			.actions(actions)
			.show(ui);
		if let Some(layers) = shown.filter(|_| layer_changed) {
			out.extend(layer_events(app, layers));
		}
		match r.clicked {
			Some(Tool::View(view)) => ws.view(app, view, out),
			Some(Tool::Add) => {
				ws.add_open = !ws.add_open;
				ws.add_at = None;
			}
			None => {}
		}
		if let Some(&anchor) = r.actions.get(&Tool::Add) {
			add_menu(ws, ui, app, anchor, out);
		}
	});
}

/// The toolbar's actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Tool {
	View(ViewCommand),
	Add,
}

impl KeySalt for Tool {
	fn write_key(&self, out: &mut String) {
		match self {
			Self::View(view) => Cmd::View(*view).write_key(out),
			Self::Add => out.push_str("add"),
		}
	}
}

/// The Add menu at the toolbar's Add, or at the pointer when `A` opened it while the canvas had the focus: an item
/// picked then lands at the pointer, else at the canvas centre.
fn add_menu(ws: &mut Workspace, ui: &mut Mui<'_>, app: &App, anchor: Rect, out: &mut Vec<Event>) {
	if ws.canvas_focused {
		shortcut::bind(ui.ctx(), Scope::Global, ADD_KEY, ADD_ACTION);
	}
	if shortcut::take_action(ui.ctx(), &ADD_ACTION) {
		ws.add_open = true;
		let at = ws.pointer.map(|(x, y)| Point::new(x.floor() as i32, y.floor() as i32));
		ws.add_at = at.zip(ui.input(|i| i.pointer.hover_pos()));
	}
	let s = &app.strings;
	let menu = Menu::new("canvas-add-menu", s.get("TipCanvasAdd"));
	let menu = menus::action_menu(app, menu, &menus::add_rows(s), On::Current);
	let anchor = ws.add_at.map_or(anchor, |(_, pos)| Rect::from_pos(pos));
	let picked = menus::picked(menu.show_at(ui.rt(), anchor, &mut ws.add_open).clicked);
	ws.menu_open |= ws.add_open;
	if let Some(command) = picked.and_then(|a| add_command(a, ws.add_at.map(|(at, _)| at))) {
		out.push(Event::Command(command));
		ws.focus_form();
	}
}

/// One picture under its header, in `ui`'s room, with its handles and boxes; returns where it shows.
fn picture(
	ws: &mut Workspace,
	ui: &mut Mui<'_>,
	app: &mut App,
	layer: Layer,
	out: &mut Vec<Event>,
	intents: &mut Vec<Intent>,
) -> (Layer, CanvasTransform, Rect) {
	header(ws, ui, app, layer, out);
	let area = ui.available_rect_before_wrap();
	ui.allocate_rect(area, Sense::hover());
	let canvas = app.document.project.canvas_size();
	let t = CanvasTransform::new(area.shrink(MARGIN), canvas, ws.view.normalised(canvas));
	if area.width() > 0. {
		let extent = PictureExtent { device: t.device_size(ui.ctx().pixels_per_point()), window: t.window() };
		ws.pictures.set_extent(layer, extent);
	}
	let (painter, visible) = ws.pictures.paint(ui, layer, &t, app);
	let id = ui.scope_id().with(("picture", layer as usize));
	let response = ui.interact(t.visible(), id, Sense::click_and_drag());
	let title = title(app, layer);
	response.widget_info(|| WidgetInfo::labeled(Role::Image, true, &title));
	commands::bind_pane_keys(ui.ctx(), app, id);
	if response.clicked() || response.drag_started() || (ws.focus_canvas && layer_first(app, layer)) {
		response.request_focus();
	}
	if let Some(dropped) = response.dnd_release_payload::<TreeDrag<Salt<Row>>>()
		&& let Some(p) = response.hover_pos()
	{
		let objects = dropped_objects(app, dropped.key().0);
		if !objects.is_empty() {
			out.push(drop_paste(app, &objects, &t, p));
		}
	}
	ws.view = navigate(ws.view, ui, &response, &t);
	ws.hover(&t, &response);
	if response.has_focus() {
		ws.canvas_focused = true;
	}
	let tokens = ui.tokens();
	let clip = t.visible().intersect(ui.clip_rect());
	editing::edit(ws, ui, app, layer, &t, clip, &response, tokens.color.primary, out, intents);
	if ws.pictures.busy() {
		painter.rect_filled(visible, 0., DIM);
		let spinner = Rect::from_center_size(visible.center(), vec2(48., 48.));
		let rt = ui.rt();
		ui.scope_builder(UiBuilder::new().max_rect(spinner), |ui| {
			let busy = LoadingIndicator::new(("canvas-busy", layer as usize)).label(app.strings.get("CanvasBusy"));
			rt.ui(ui).mineral(busy);
		});
	}
	if response.has_focus() {
		painter.rect_stroke(visible, 0., Stroke::new(2., tokens.color.primary), StrokeKind::Inside);
	}
	(layer, t, visible)
}

/// What dragging `row` copies: the selected items when it is one of several, else the row alone.
fn dropped_objects(app: &App, row: Row) -> Vec<Copyable> {
	match row {
		Row::Item(item) => app.selection.targets(item, &app.document.project).into_iter().map(Copyable::Item).collect(),
		_ => Copyable::of_row(row).into_iter().collect(),
	}
}

/// The first picture shown takes the canvas's focus.
fn layer_first(app: &App, layer: Layer) -> bool {
	layer == Layer::Field || !app.document.view.field.preview
}

fn title(app: &App, layer: Layer) -> String {
	app.strings
		.get(match layer {
			Layer::Field => "SectionField",
			Layer::Intensity => "SectionIntensity",
			_ => return String::new(),
		})
		.to_owned()
}

/// The picture's title, its brightness and Auto.
fn header(ws: &mut Workspace, ui: &mut Mui<'_>, app: &mut App, layer: Layer, out: &mut Vec<Event>) {
	let s = app.strings;
	let rt = ui.rt();
	let strip = vec2(ui.available_width(), HEADER_H);
	ui.allocate_ui_with_layout(strip, Layout::left_to_right(Align::Center), |ui| {
		let font = TypeScale::LabelLarge.font(ui.ctx());
		let ink = rt.tokens().color.on_surface;
		ui.add(eframe::egui::Label::new(eframe::egui::RichText::new(title(app, layer)).font(font).color(ink)));
		ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
			let ui = &mut rt.ui(ui);
			let auto = IconButton::new(
				("canvas-auto", layer as usize),
				icons::ICON_BRIGHTNESS_AUTO,
				s.get("TipAutoBrightness"),
			);
			if ui.mineral(auto).clicked() {
				ws.pictures.auto_brightness(app, layer, out);
			}
			let value = app.document.view.layer(layer).brightness;
			let (min, max) = bounds_of(DisplayLayer::BRIGHTNESS);
			let label = s.label("DisplayLayer", "Brightness");
			let place = |ui: &mut Mui<'_>, field: NumberField<'_>| {
				let field = field.label_visible(false).icon(icons::ICON_BRIGHTNESS_5).tooltip(label);
				ui.with_density(Density::MIN, |ui| ui.mineral(field.preferred_width(BRIGHTNESS_W)))
			};
			let spec = NumberSpec::logarithmic(min, max);
			let salt = ("canvas-brightness", layer as usize);
			let (r, change) = number::show(ui, salt, label, &spec, value, false, place);
			if std::mem::take(&mut ws.focus_brightness[layer as usize]) {
				inspector::focus_selected(ui.ctx(), r.response().id);
			}
			if let Some(v) = change.value {
				let edit = waveduderust_app::preview::display_edit(layer, DisplayLayerEdit::Brightness(v));
				out.extend(edit.map(|e| Event::Command(Command::Edit(e))));
			}
		});
	});
}
