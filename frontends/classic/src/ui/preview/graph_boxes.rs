//! This window's side of the pictures' probe graph boxes: painted and edited in its accent, with the box's context
//! menu.

use eframe::egui::{Id, Rect, Response, Ui};
use egui_material_icons::icons::ICON_IOS_SHARE;
use waveduderust::model::{ItemRef, Layer, ProbeId};
use waveduderust_app::App;
use waveduderust_app::Event;
use waveduderust_app::menus::{self, MenuAction, On};
use waveduderust_app::preview::view::CanvasTransform;
use waveduderust_app::selection::Row;
use waveduderust_egui_platform::input::pinching;
use waveduderust_egui_preview::graph_boxes::{box_responses, cancel_box_drag, edit_box, on_picture};

use super::Preview;
use crate::kit::Palette;
use crate::kit::menu::{self, Entry};

fn box_id(layer: Layer, probe: ProbeId) -> Id {
	Id::unique(("ProbeBox", layer as usize, probe))
}

impl Preview {
	/// Draws the graph boxes of `layer` clipped to `clip`, the selected ones last.
	pub(super) fn paint_boxes(&mut self, ui: &Ui, app: &App, layer: Layer, t: &CanvasTransform, clip: Rect) {
		let accent = Palette::of(ui.ctx().theme()).accent;
		self.boxes.paint(ui, app, layer, t, clip, |i| box_id(layer, i), self.pointer_focus, accent);
	}

	/// The boxes of `layer` within `clip` ([`edit_box`]); a right-click opens a box's menu, whose entry is returned.
	pub(super) fn edit_boxes(
		&mut self,
		ui: &Ui,
		app: &mut App,
		layer: Layer,
		t: &CanvasTransform,
		clip: Rect,
		out: &mut Vec<Event>,
	) -> Option<(MenuAction, On)> {
		let mut picked = None;
		if pinching(ui) {
			cancel_box_drag(&mut self.box_drag, app, out);
		}
		let accent = Palette::of(ui.ctx().theme()).accent;
		let views: Vec<(ProbeId, Rect, (Response, Response))> = (self.boxes.views(app, layer).iter())
			.map(|v| {
				let rect = on_picture(v, t);
				let id = box_id(layer, v.id);
				(v.id, rect, box_responses(ui, app, v, rect, clip, id, self.pointer_focus, accent))
			})
			.collect();
		for (index, rect, (response, grip)) in views {
			super::hold(&response);
			super::hold(&grip);
			edit_box(ui, app, (index, rect), t, (&response, &grip), &mut self.box_drag, &mut self.pointer_focus, out);
			let item = ItemRef::Probe(index);
			if response.secondary_clicked() && !app.selection.contains(item) {
				app.selection.select(Row::Item(item));
			}
			let on = On::Row(Some(Row::Item(item)));
			let csv = menus::enabled(app, MenuAction::ExportProbeCsv, on);
			menu::context(&response, |m| {
				let row = Entry::new(app.strings.get("MenuExportProbeCsv")).icon(ICON_IOS_SHARE).enabled(csv);
				if m.item(row).picked {
					picked = Some((MenuAction::ExportProbeCsv, on));
				}
			});
		}
		picked
	}
}
