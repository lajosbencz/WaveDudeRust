//! One picture's cell (docs/ui/40-preview-pane.md): the header strip, the picture fitted into the rest or zoomed
//! in, on whole device pixels, the edge line, the overlay, the graph boxes and the busy dim. The picture changes
//! only when a frame for its position arrives; until then the last one stays, and before the first an image scene
//! shows its own picture.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use eframe::egui::{Color32, Id, Painter, Pos2, Rect, Response, Role, Sense, Shape, Stroke, Ui, WidgetInfo, vec2};
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons::{ICON_BRIGHTNESS_6, ICON_BRIGHTNESS_AUTO, ICON_IMAGE, ICON_IOS_SHARE};
use waveduderust::model::{DisplayLayerEdit, Layer};
use waveduderust_app::App;
use waveduderust_app::Event;
use waveduderust_app::clipboard::Copyable;
use waveduderust_app::commands::Command;
use waveduderust_app::menus::{self, ExportKindChoice, MenuAction, On};
use waveduderust_app::preview::view::CanvasTransform;
use waveduderust_app::preview::{PictureExtent, display_edit, drop_paste};
use waveduderust_app::selection::Row;
use waveduderust_egui_platform::input::{pinched_press, pressed_on};
use waveduderust_egui_preview::pictures::navigate;

use super::Preview;
use super::header_strip::{self, CELL_MARGIN, Strip, StripAction, strip_and_area, strip_beside};
use crate::kit::menu::{self, Entry, MenuUi};
use crate::ui::shortcuts;
use crate::ui::tree::{DraggedItem, RowKey};

/// The margin between the picture and its area's edges: the cell's margin on the outer sides, the gap to the header
/// strip on the strip's side.
const MARGIN: f32 = 4.;
/// The busy dim's black.
const DIM: Color32 = Color32::from_black_alpha(115);
/// The busy spinner: a three-quarter ring turning once a second.
const SPINNER: f32 = 32.;
const SPINNER_WIDTH: f32 = 3.;

/// The id of a layer's brightness box.
pub(super) fn brightness_id(layer: Layer) -> Id {
	Id::unique(("PreviewBrightness", layer as usize))
}

fn spinner(painter: &Painter, centre: Pos2, seconds: f64) {
	let r = SPINNER / 2. - SPINNER_WIDTH / 2. - 0.5;
	let start = seconds.fract() as f32 * TAU - FRAC_PI_2;
	let points = (0..=36).map(|i| start + i as f32 / 36. * 1.5 * PI).map(|a| centre + r * vec2(a.cos(), a.sin()));
	painter.add(Shape::line(points.collect(), Stroke::new(SPINNER_WIDTH, Color32::WHITE)));
}

/// What an entry of a picture's context menu does.
enum PictureEntry {
	/// Edit the layer's brightness.
	Brightness,
	Auto,
	Window(MenuAction),
}

/// The context menu of the picture of `layer` and of its header strip.
fn picture_menu(m: &mut MenuUi<'_, '_>, app: &App, layer: Layer) -> Option<PictureEntry> {
	let s = &app.strings;
	let frame = match layer {
		Layer::Field => ExportKindChoice::FieldFrame,
		Layer::Intensity => ExportKindChoice::IntensityFrame,
		_ => return None,
	};
	let mut picked = None;
	let mut item = |m: &mut MenuUi<'_, '_>, icon: MaterialIcon, key: &str, entry: PictureEntry| {
		let (shortcut, enabled) = match entry {
			PictureEntry::Window(action) => {
				(shortcuts::text(m.ctx(), action), menus::enabled(app, action, On::Current))
			}
			_ => (None, true),
		};
		if m.item(Entry::new(s.get(key)).icon(icon).shortcut(shortcut).enabled(enabled)).picked {
			picked = Some(entry);
		}
	};
	item(m, ICON_BRIGHTNESS_6, "MenuBrightness", PictureEntry::Brightness);
	item(m, ICON_BRIGHTNESS_AUTO, "MenuAutoBrightness", PictureEntry::Auto);
	m.separator();
	let export = |kind| PictureEntry::Window(MenuAction::Export(kind));
	item(m, ICON_IMAGE, "MenuExportThisFrame", export(frame));
	m.separator();
	item(m, ICON_IOS_SHARE, "MenuFileExportField", export(ExportKindChoice::Field));
	item(m, ICON_IOS_SHARE, "MenuFileExportIntensity", export(ExportKindChoice::Intensity));
	item(m, ICON_IOS_SHARE, "MenuFileExportBoth", export(ExportKindChoice::Both));
	picked
}

impl Preview {
	/// The cell of `layer` in `cell`; a context menu entry for the window goes to `picked`.
	pub(super) fn cell(
		&mut self,
		ui: &mut Ui,
		cell: Rect,
		layer: Layer,
		app: &mut App,
		out: &mut Vec<Event>,
		picked: &mut Option<(MenuAction, On)>,
	) {
		let canvas = app.document.project.canvas_size();
		let frame = cell.shrink(CELL_MARGIN);
		let beside = strip_beside((frame.width(), frame.height()), canvas);
		let (strip_rect, area) = strip_and_area(cell, beside);
		let t = CanvasTransform::new(area.shrink(MARGIN), canvas, self.view.normalised(canvas));
		if cell.width() > 0. {
			let extent = PictureExtent { device: t.device_size(ui.ctx().pixels_per_point()), window: t.window() };
			self.pictures.set_extent(layer, extent);
		}
		let clip = t.visible().intersect(ui.clip_rect());
		self.paint(ui, layer, &t, app, clip);
		let s = app.strings;
		let title = s.get(match layer {
			Layer::Field => "SectionField",
			Layer::Intensity => "SectionIntensity",
			_ => return,
		});
		let strip_area = ui.interact(strip_rect, ui.scope_id().with(("PreviewStrip", layer as usize)), Sense::CLICK);
		let picture = self.input(ui, layer, &t, title);
		if let Some(dropped) = picture.dnd_release_payload::<DraggedItem>()
			&& let RowKey(Row::Item(item)) = dropped.0
			&& let Some(p) = picture.hover_pos()
		{
			out.push(drop_paste(app, &[Copyable::Item(item)], &t, p));
		}
		self.edit_handles(ui, &picture, app, layer, &t, out);
		if let Some(action) = self.edit_boxes(ui, app, layer, &t, clip, out) {
			*picked = Some(action);
		}
		let mut entry = None;
		for r in [&picture, &strip_area].into_iter().filter(|_| !pinched_press(ui.ctx())) {
			if let Some(Some(e)) = menu::context(r, |m| picture_menu(m, app, layer)) {
				entry = Some(e);
			}
		}
		let strip = Strip {
			title,
			caption: s.label("DisplayLayer", "Brightness"),
			hint: s.hint("DisplayLayer", "Brightness"),
			auto: s.get("ButtonAutoBrightness"),
			auto_tip: s.get("TipAutoBrightness"),
			brightness: app.document.view.layer(layer).brightness,
			id: brightness_id(layer),
			event: &|v| display_edit(layer, DisplayLayerEdit::Brightness(v)).map(|e| Event::Command(Command::Edit(e))),
		};
		match header_strip::show(ui, strip_rect, beside, &strip, &mut self.box_focus[layer as usize]) {
			Some(StripAction::Brightness(v)) => out.extend((strip.event)(v)),
			Some(StripAction::Auto) => self.pictures.auto_brightness(app, layer, out),
			None => {}
		}
		match entry {
			Some(PictureEntry::Brightness) => self.box_focus[layer as usize] = header_strip::BoxFocus::Asked,
			Some(PictureEntry::Auto) => self.pictures.auto_brightness(app, layer, out),
			Some(PictureEntry::Window(action)) => *picked = Some((action, On::Current)),
			None => {}
		}
	}

	/// The picture, its edge line, the overlay, the graph boxes and the busy dim, clipped to the visible picture
	/// and the edge line's room.
	fn paint(&mut self, ui: &Ui, layer: Layer, t: &CanvasTransform, app: &App, clip: Rect) {
		let (painter, visible) = self.pictures.paint(ui, layer, t, app);
		self.paint_overlay(ui, app, layer, t, clip);
		self.paint_boxes(ui, app, layer, t, clip);
		if self.pictures.busy() {
			painter.rect_filled(visible, 0., DIM);
			spinner(&painter, visible.center(), ui.input(|i| i.time));
			ui.ctx().request_repaint();
		}
	}

	/// The picture's pointer: a press focuses it, the wheel zooms about the point under the pointer, a middle-button
	/// drag pans a zoomed view, and a pinch zooms about and a two-finger drag pans with the fingers' centre.
	fn input(&mut self, ui: &Ui, layer: Layer, t: &CanvasTransform, title: &str) -> Response {
		let response =
			ui.interact(t.visible(), ui.scope_id().with(("PreviewPicture", layer as usize)), Sense::click_and_drag());
		response.widget_info(|| WidgetInfo::labeled(Role::Image, true, title));
		super::hold(&response);
		if pressed_on(&response) && !response.has_focus() {
			response.request_focus();
		}
		self.view = navigate(self.view, ui, &response, t);
		response
	}
}
