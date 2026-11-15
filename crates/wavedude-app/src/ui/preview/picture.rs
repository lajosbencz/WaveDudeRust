//! One picture's cell (docs/ui/40-preview-pane.md): the header strip, the picture fitted into the rest or zoomed
//! in, on whole device pixels, the edge line, the overlay, the graph boxes and the busy dim. The picture changes
//! only when a frame for its position arrives; until then the last one stays, and before the first an image scene
//! shows its own picture.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use eframe::egui::{
	Color32, Id, InputState, Painter, PointerButton, Pos2, Rect, Response, Role, Sense, Shape, Stroke, Ui, WidgetInfo,
	pos2, vec2,
};
use egui_material_icons::MaterialIcon;
use egui_material_icons::icons::{ICON_BRIGHTNESS_6, ICON_BRIGHTNESS_AUTO, ICON_IMAGE, ICON_IOS_SHARE};
use wavedude_app_state::Event;
use wavedude_app_state::clipboard::{Copyable, PasteTarget, write_fragment};
use wavedude_app_state::commands::Command;
use wavedude_app_state::platform::HAS_EXPORTS;
use wavedude_app_state::strings::Strings;
use wavedude_core::model::{DisplayLayerEdit, ItemRef, Layer, Point};
use wavedude_view::DeviceGrid;
use wavedude_view::overlay::{edge_color, edge_line};

use super::header_strip::{self, CELL_MARGIN, Strip, StripAction, strip_and_area, strip_beside};
use super::{Preview, pressed_on};
use crate::App;
use crate::menus::{ExportKindChoice, MenuAction, On};
use crate::preview::view::{CanvasTransform, View};
use crate::preview::{PictureExtent, display_edit};
use crate::ui::menu_bar::{self, Slot};
use crate::ui::number_box::wheel_notches;
use crate::ui::shortcuts;
use crate::ui::tree::DraggedItem;

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

/// `r` with its edges on the nearest whole device pixels of `grid`.
fn snapped(grid: &DeviceGrid, r: Rect) -> Rect {
	let [left, top, right, bottom] = grid.snapped((r.min.x, r.min.y), (r.max.x, r.max.y));
	grid.rect((left, top), (right, bottom))
}

fn spinner(painter: &Painter, centre: Pos2, seconds: f64) {
	let r = SPINNER / 2. - SPINNER_WIDTH / 2. - 0.5;
	let start = seconds.fract() as f32 * TAU - FRAC_PI_2;
	let points = (0..=36).map(|i| start + i as f32 / 36. * 1.5 * PI).map(|a| centre + r * vec2(a.cos(), a.sin()));
	painter.add(Shape::line(points.collect(), Stroke::new(SPINNER_WIDTH, Color32::WHITE)));
}

/// An item of the tree dropped on a picture at `p`: a copy of it lands on the canvas pixel there (docs/ui/20, Drag
/// and drop).
fn drop_copy(app: &App, item: ItemRef, t: &CanvasTransform, p: Pos2) -> Event {
	let text = write_fragment(&app.document.project, &[Copyable::Item(item)]);
	let (x, y) = t.to_canvas(p);
	let target = PasteTarget { current: app.selection.current(), before: false };
	Event::Paste { text, target, place_at: Some(Point::new(x.floor() as i32, y.floor() as i32)) }
}

/// What an entry of a picture's context menu does.
enum PictureEntry {
	/// Edit the layer's brightness.
	Brightness,
	Auto,
	Window(MenuAction),
}

/// The context menu of the picture of `layer` and of its header strip.
fn picture_menu(ui: &mut Ui, s: &Strings, layer: Layer) -> Option<PictureEntry> {
	let frame = match layer {
		Layer::Field => ExportKindChoice::FieldFrame,
		Layer::Intensity => ExportKindChoice::IntensityFrame,
	};
	let mut picked = None;
	let mut item = |ui: &mut Ui, icon: MaterialIcon, key: &str, entry: PictureEntry| {
		let shortcut = match entry {
			PictureEntry::Window(action) => shortcuts::text(ui.ctx(), action),
			_ => None,
		};
		if menu_bar::entry(ui, Slot::Icon(icon), s.get(key), None, shortcut).clicked() {
			picked = Some(entry);
		}
	};
	item(ui, ICON_BRIGHTNESS_6, "MenuBrightness", PictureEntry::Brightness);
	item(ui, ICON_BRIGHTNESS_AUTO, "MenuAutoBrightness", PictureEntry::Auto);
	if HAS_EXPORTS {
		ui.separator();
		let export = |kind| PictureEntry::Window(MenuAction::Export(kind));
		item(ui, ICON_IMAGE, "MenuExportThisFrame", export(frame));
		item(ui, ICON_IOS_SHARE, "MenuFileExportField", export(ExportKindChoice::Field));
		item(ui, ICON_IOS_SHARE, "MenuFileExportIntensity", export(ExportKindChoice::Intensity));
		item(ui, ICON_IOS_SHARE, "MenuFileExportBoth", export(ExportKindChoice::Both));
	}
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
			self.extents[layer as usize] = extent;
		}
		let clip = t.visible().intersect(ui.clip_rect());
		self.paint(ui, layer, &t, app, clip);
		let s = app.strings;
		let title = s.get(match layer {
			Layer::Field => "SectionField",
			Layer::Intensity => "SectionIntensity",
		});
		let strip_area = ui.interact(strip_rect, ui.scope_id().with(("PreviewStrip", layer as usize)), Sense::CLICK);
		let picture = self.input(ui, layer, &t, title);
		if let Some(dropped) = picture.dnd_release_payload::<DraggedItem>()
			&& let Some(p) = picture.hover_pos()
		{
			out.push(drop_copy(app, dropped.0, &t, p));
		}
		self.edit_handles(ui, &picture, app, layer, &t, out);
		if let Some(action) = self.edit_boxes(ui, app, layer, &t, clip, out) {
			*picked = Some(action);
		}
		let mut entry = None;
		for r in [&picture, &strip_area] {
			r.context_menu(|ui| {
				if let Some(e) = picture_menu(ui, &s, layer) {
					entry = Some(e);
				}
			});
		}
		let strip = Strip {
			title,
			caption: s.label("DisplayLayer", "Brightness"),
			hint: s.hint("DisplayLayer", "Brightness"),
			auto: s.get("ButtonAutoBrightness"),
			auto_tip: s.get("TipAutoBrightness"),
			brightness: app.document.project.display.layer(layer).brightness,
			id: brightness_id(layer),
			event: &|v| Event::Command(Command::Edit(display_edit(layer, DisplayLayerEdit::Brightness(v)))),
		};
		match header_strip::show(ui, strip_rect, beside, &strip, &mut self.editing[layer as usize]) {
			Some(StripAction::Brightness(v)) => out.push((strip.event)(v)),
			Some(StripAction::Auto) => self.auto_brightness(app, layer, out),
			None => {}
		}
		match entry {
			Some(PictureEntry::Brightness) => {
				self.editing[layer as usize] = true;
				ui.memory_mut(|m| m.request_focus(brightness_id(layer)));
			}
			Some(PictureEntry::Auto) => self.auto_brightness(app, layer, out),
			Some(PictureEntry::Window(action)) => *picked = Some((action, On::Current)),
			None => {}
		}
	}

	/// The picture, its edge line, the overlay, the graph boxes and the busy dim, clipped to the visible picture
	/// and the edge line's room.
	fn paint(&mut self, ui: &Ui, layer: Layer, t: &CanvasTransform, app: &App, clip: Rect) {
		let grid = DeviceGrid::new(ui.ctx().pixels_per_point(), 0., 0.);
		let visible = snapped(&grid, t.visible());
		let painter = ui.painter_at(visible.expand(grid.hairline() / grid.scale));
		let picture = t.picture();
		let full = snapped(&grid, picture);
		painter.rect_filled(full, 0., Color32::BLACK);
		let uv = Rect::from_min_max(Pos2::ZERO, pos2(1., 1.));
		// A windowed picture covers its canvas part alone, the rest of the picture stays black.
		let dst = |window: Option<Rect>| {
			window.map_or(full, |w| {
				snapped(&grid, Rect::from_min_max(t.to_screen(w.min.x, w.min.y), t.to_screen(w.max.x, w.max.y)))
			})
		};
		match (self.shown[layer as usize], &self.textures[layer as usize], &self.gpu) {
			(_, _, Some(gpu)) if self.on_device && self.device.has(layer) => {
				self.device.paint(gpu, &painter, layer, dst(self.device.window(layer)));
			}
			(Some(window), Some(texture), _) if !self.on_device => {
				painter.image(texture.id(), dst(window), uv, Color32::WHITE);
			}
			_ => {
				if let Some(scene) = self.scene_image.as_ref().filter(|_| self.scene_shown) {
					painter.image(scene.id(), full, uv, Color32::WHITE);
				}
			}
		}
		if let Some(color) = edge_color(&app.document.project.scene) {
			painter.add(edge_line(grid, (picture.min.x, picture.min.y, picture.width(), picture.height()), color));
		}
		self.paint_overlay(ui, app, layer, t, clip);
		self.paint_boxes(ui, app, layer, t, clip);
		if self.busy {
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
		// not `zoom_delta`, which counts the wheel's Ctrl-zoom
		match ui.input(InputState::multi_touch) {
			Some(m) => {
				// a gesture that began over the picture itself, not over a popup or beside it
				let began_here =
					t.visible().contains(m.start_pos) && ui.ctx().layer_id_at(m.start_pos) == Some(ui.layer_id());
				if began_here {
					let before = m.center_pos - m.translation_delta;
					self.view =
						self.view.pinched(m.zoom_delta, t.to_canvas(before), m.translation_delta, t.scale, t.canvas);
				}
			}
			None => {
				let notches = wheel_notches(ui, t.visible());
				if let Some(p) = response.hover_pos().filter(|_| notches != 0) {
					let factor = View::STEP.powi(notches);
					self.view = self.view.normalised(t.canvas).zoomed(factor, t.to_canvas(p), t.canvas);
				}
			}
		}
		if response.dragged_by(PointerButton::Middle) {
			self.view = self.view.panned(response.drag_delta(), t.scale);
		}
		response
	}
}

#[cfg(test)]
mod tests {
	use eframe::egui::{self, Area, Order, TouchPhase};

	use super::super::frames::{Frames, PICTURE, touch};
	use super::*;

	/// A frame of the field picture's pointer input, a popup over all of it when `covered`.
	fn run(frames: &mut Frames, events: Vec<egui::Event>, covered: bool) {
		frames.run(events, |preview, ui, _, t, _| {
			if covered {
				Area::new(Id::unique("popup")).order(Order::Foreground).fixed_pos(PICTURE.min).show(ui.ctx(), |ui| {
					ui.allocate_exact_size(PICTURE.size(), Sense::click());
				});
			}
			preview.input(ui, Layer::Field, t, "Field");
		});
	}

	fn fingers(centre: Pos2, half_gap: f32, phase: TouchPhase) -> Vec<egui::Event> {
		[(0, -half_gap), (1, half_gap)].map(|(id, dx)| touch(id, phase, centre + vec2(dx, 0.))).into()
	}

	/// The zoom after two fingers begin about `from` and spread about `to`; with `extra` events as they begin.
	fn zoom_after(from: Pos2, to: Pos2, covered: bool, extra: Vec<egui::Event>) -> f32 {
		let mut frames = Frames::new();
		run(&mut frames, vec![], covered);
		run(&mut frames, vec![egui::Event::PointerMoved(from)], covered);
		run(&mut frames, [fingers(from, 10., TouchPhase::Start), extra].concat(), covered);
		run(&mut frames, fingers(to, 30., TouchPhase::Move), covered);
		frames.preview.view.zoom
	}

	fn picture() -> Rect {
		let frames = Frames::new();
		CanvasTransform::new(PICTURE, frames.app.document.project.canvas_size(), View::default()).visible()
	}

	#[test]
	fn a_pinch_zooms_the_picture_it_began_on() {
		let on = picture().center();
		assert!(zoom_after(on, on, false, vec![]) > 1.);
	}

	#[test]
	fn a_pinch_is_the_pictures_by_where_it_began() {
		let (on, off) = (picture().center(), pos2(picture().center().x, PICTURE.min.y + 1.));
		assert!(!picture().contains(off));
		assert_eq!(zoom_after(off, on, false, vec![]), 1., "begun beside the picture, drifted onto it");
		assert!(zoom_after(on, off, false, vec![]) > 1., "begun on the picture, drifted off it");
		assert_eq!(zoom_after(on, on, true, vec![]), 1., "begun under a popup");
	}

	#[test]
	fn the_point_between_the_fingers_stays_between_them() {
		let (from, to) = (picture().center() - vec2(40., 0.), picture().center() + vec2(20., 10.));
		let mut frames = Frames::new();
		let canvas = frames.app.document.project.canvas_size();
		let fit = CanvasTransform::new(PICTURE, canvas, View::default());
		run(&mut frames, vec![], false);
		run(&mut frames, vec![egui::Event::PointerMoved(from)], false);
		run(&mut frames, fingers(from, 10., TouchPhase::Start), false);
		run(&mut frames, fingers(to, 30., TouchPhase::Move), false);
		let zoomed = CanvasTransform::new(PICTURE, canvas, frames.preview.view);
		assert!(frames.preview.view.zoom > 1.);
		let point = fit.to_canvas(from);
		let moved = zoomed.to_screen(point.0, point.1);
		assert!((moved - to).length() < 1e-2, "{moved:?} for {to:?}");
	}

	#[test]
	fn the_wheel_waits_while_two_fingers_are_down() {
		let on = picture().center();
		let wheel = egui::Event::MouseWheel {
			unit: egui::MouseWheelUnit::Line,
			delta: vec2(0., 1.),
			phase: TouchPhase::Move,
			source: egui::MouseWheelSource::Wheel,
			modifiers: Default::default(),
		};
		let mut frames = Frames::new();
		run(&mut frames, vec![], false);
		run(&mut frames, vec![egui::Event::PointerMoved(on), wheel.clone()], false);
		assert!(frames.preview.view.zoom > 1., "the wheel zooms alone");
		let mut frames = Frames::new();
		run(&mut frames, vec![], false);
		run(&mut frames, vec![egui::Event::PointerMoved(on)], false);
		run(&mut frames, [fingers(on, 10., TouchPhase::Start), vec![wheel]].concat(), false);
		assert_eq!(frames.preview.view.zoom, 1., "the fingers' frame takes no wheel");
	}
}
