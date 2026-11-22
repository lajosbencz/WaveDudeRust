//! The workspace (DESIGN.md, Workspace): the canvas with its run bar, and the outline and inspector columns, sheets
//! below Expanded. Column state lasts for the session (SET-11).

mod canvas;
mod editing;
mod inspector;
mod number;
mod outline;
mod run_bar;
mod selection_bar;

use std::cell::RefCell;

use eframe::egui::{CentralPanel, Frame, Id, Margin, Panel, Pos2, Rangef, Stroke, Ui, Vec2};
use eframe::egui_wgpu::RenderState;
use egui_mineral::layout::Axis;
use egui_mineral_material::adaptive::WindowClass;
use egui_mineral_material::prelude::{BottomSheet, MaterialExt as _, SideSheet, SplitPane};
use waveduderust::model::{DisplayLayerEdit, ItemRef, Layer, Point, ViewEdit};
use waveduderust_app::Event;
use waveduderust_app::commands::{Command, Edit};
use waveduderust_app::preview::graph_boxes::BoxDrag;
use waveduderust_app::preview::overlay::Grab;
use waveduderust_app::preview::pane_event;
use waveduderust_app::preview::view::View;
use waveduderust_app::strings::strip_mnemonic;
use waveduderust_egui_preview::graph_boxes::GraphBoxes;
use waveduderust_egui_preview::overlay::Overlays;
use waveduderust_egui_preview::pictures::Pictures;
use waveduderust_egui_preview::timeline::Timeline;

use self::inspector::Inspector;
use self::outline::Outline;
pub(crate) use self::outline::keyed as outline_keys;
use crate::commands::{Layers, ViewCommand};
use crate::shell::Intent;
use crate::{App, Mui};

/// Where the outline sits beside the inspector: its own column from this width on, else above the inspector.
const OUTLINE_COLUMN_FROM: f32 = 1200.;
/// Fits "Photomultiplier 1" at depth 1: the tree spends 120 px on indent, chevron slot, box and icon.
/// A column's width: default, then the resizable range.
#[derive(Clone, Copy)]
struct Widths(f32, Rangef);
const OUTLINE_WIDTHS: Widths = Widths(300., Rangef { min: 240., max: 480. });
const INSPECTOR_WIDTHS: Widths = Widths(340., Rangef { min: 280., max: 480. });
/// The outline's share of the single left column.
const OUTLINE_SHARE: f32 = 0.4;

/// Which columns show, session state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Columns {
	pub(crate) outline: bool,
	pub(crate) inspector: bool,
	/// Canvas only is on: the columns shown before it, which it restores.
	pub(crate) before: Option<(bool, bool)>,
}

impl Default for Columns {
	fn default() -> Self {
		Self { outline: true, inspector: true, before: None }
	}
}

impl Columns {
	pub(crate) fn canvas_only(self) -> bool {
		self.before.is_some()
	}

	/// The columns after `view`.
	pub(crate) fn toggled(self, view: ViewCommand) -> Self {
		match view {
			ViewCommand::Outline => Self { outline: !self.outline, before: None, ..self },
			ViewCommand::Inspector => Self { inspector: !self.inspector, before: None, ..self },
			ViewCommand::CanvasOnly => match self.before {
				Some((outline, inspector)) => Self { outline, inspector, before: None },
				None => Self { outline: false, inspector: false, before: Some((self.outline, self.inspector)) },
			},
			_ => self,
		}
	}
}

/// The workspace's state between passes.
pub(crate) struct Workspace {
	pub(crate) columns: Columns,
	pictures: Pictures,
	/// The zoom over the fit, shared by both pictures.
	view: View,
	timeline: Timeline,
	/// The timeline focused by a press, which shows no focus ring.
	pointer_focus: Option<Id>,
	/// The custom speed's draft.
	speed: f64,
	speed_open: bool,
	/// The speed chip's presets are open.
	speeds_open: bool,
	/// The run bar's overflow menu is open.
	run_more_open: bool,
	/// The canvas takes the focus on the next pass.
	focus_canvas: bool,
	/// The columns are sheets below Expanded, which open on demand: outline, inspector.
	sheets: Option<(bool, bool)>,
	outline: Outline,
	inspector: Inspector,
	/// What the outline asked of the inspector this pass, applied once the state took this pass's events.
	asked: outline::Asked,
	overlays: Overlays,
	boxes: GraphBoxes,
	/// A press in progress on a picture, with its layer.
	grab: Option<(Layer, Grab)>,
	box_drag: Option<BoxDrag>,
	/// The item each picture's open context menu is for; `None` inside: the picture's own menu.
	menu_on: [Option<ItemRef>; 2],
	/// The picture's menu asked for its brightness field to take the focus.
	focus_brightness: [bool; 2],
	/// The canvas point under the pointer, on either picture.
	pointer: Option<(f32, f32)>,
	/// A picture, a handle or a box has the keyboard focus.
	canvas_focused: bool,
	/// A canvas menu is open.
	menu_open: bool,
	/// The canvas toolbar's Add menu is open.
	add_open: bool,
	/// Where the Add menu's item lands and where the menu shows: the pointer when `A` opened it.
	add_at: Option<(Point, Pos2)>,
	/// The selection bar's size last pass.
	bar_size: Vec2,
	/// The selection bar's more menu is open.
	more_open: bool,
}

impl Workspace {
	pub(crate) fn new(gpu: Option<RenderState>) -> Self {
		Self {
			columns: Columns::default(),
			pictures: Pictures::new(gpu),
			view: View::default(),
			timeline: Timeline::default(),
			pointer_focus: None,
			speed: 1.,
			speed_open: false,
			speeds_open: false,
			run_more_open: false,
			focus_canvas: false,
			sheets: None,
			outline: Outline::default(),
			inspector: Inspector::default(),
			asked: outline::Asked::default(),
			overlays: Overlays::default(),
			boxes: GraphBoxes::default(),
			grab: None,
			box_drag: None,
			menu_on: [None; 2],
			focus_brightness: [false; 2],
			pointer: None,
			canvas_focused: false,
			menu_open: false,
			add_open: false,
			add_at: None,
			bar_size: Vec2::ZERO,
			more_open: false,
		}
	}

	/// The inspector shows with its form's first editor focused.
	pub(crate) fn focus_form(&mut self) {
		self.inspector.focus_first();
		self.show_inspector();
	}

	/// The inspector shows with the current item's name focused.
	pub(crate) fn focus_name(&mut self) {
		self.inspector.focus_name();
		self.show_inspector();
	}

	fn show_inspector(&mut self) {
		match &mut self.sheets {
			Some((_, inspector)) => *inspector = true,
			None if !self.columns.inspector => self.columns = self.columns.toggled(ViewCommand::Inspector),
			None => {}
		}
	}

	/// Polls the pictures, whether the workspace shows or a page covers it.
	pub(crate) fn poll(&mut self, ui: &Ui, app: &mut App, out: &mut Vec<Event>) {
		self.pictures.poll(app, out);
		if self.pictures.due(app) {
			ui.ctx().request_repaint_after(waveduderust_app::preview::POLL);
		}
	}

	/// The canvas takes the focus, so the playback keys work.
	pub(crate) fn focus_canvas(&mut self) {
		self.focus_canvas = true;
	}

	/// Applies a view command of the workspace's.
	pub(crate) fn view(&mut self, app: &App, view: ViewCommand, out: &mut Vec<Event>) {
		let canvas = app.document.project.canvas_size();
		let centre = self.view.normalised(canvas).centre;
		match view {
			ViewCommand::Outline | ViewCommand::Inspector | ViewCommand::CanvasOnly => match &mut self.sheets {
				Some((outline, _)) if view == ViewCommand::Outline => *outline = !*outline,
				Some((_, inspector)) if view == ViewCommand::Inspector => *inspector = !*inspector,
				Some(_) => {}
				None => self.columns = self.columns.toggled(view),
			},
			ViewCommand::Layers(layers) => out.extend(layer_events(app, layers)),
			ViewCommand::Orientation(o) => out.push(Event::SetPreviewOrientation(o)),
			ViewCommand::Fit => self.view = View::fit(canvas),
			ViewCommand::ZoomIn => self.view = self.view.normalised(canvas).zoomed(View::STEP, centre, canvas),
			ViewCommand::ZoomOut => self.view = self.view.normalised(canvas).zoomed(1. / View::STEP, centre, canvas),
			ViewCommand::Pane(key) => out.push(pane_event(app, key)),
			ViewCommand::Handles => {
				let on = !app.document.view.handles_in_preview;
				out.push(display_event(ViewEdit::HandlesInPreview(on)));
			}
			ViewCommand::Labels => {
				let on = !app.document.view.labels_in_preview;
				out.push(display_event(ViewEdit::LabelsInPreview(on)));
			}
			ViewCommand::Messages
			| ViewCommand::UiZoom(_)
			| ViewCommand::Palette
			| ViewCommand::Shortcuts
			| ViewCommand::Back => {}
		}
	}

	/// The columns, the run bar and the canvas in `ui`'s room.
	pub(crate) fn show(
		&mut self,
		ui: &mut Mui<'_>,
		class: WindowClass,
		app: &mut App,
		out: &mut Vec<Event>,
		intents: &mut Vec<Intent>,
	) {
		if class >= WindowClass::Expanded {
			self.sheets = None;
			self.columns_ui(ui, app, out, intents);
		} else {
			self.sheets_ui(ui, app, out, intents);
		}
		let rt = ui.rt();
		let surface = ui.tokens().color.surface_container_low;
		Panel::bottom("run-bar").frame(Frame::NONE.fill(surface)).show_separator_line(false).show(ui, |ui| {
			run_bar::show(self, &mut rt.ui(ui), app, out);
		});
		let bar = app.settings.touch_first || class < WindowClass::Expanded;
		CentralPanel::default()
			.frame(Frame::NONE)
			.show(ui, |ui| canvas::show(self, &mut rt.ui(ui), app, bar, out, intents));
		let asked = std::mem::take(&mut self.asked);
		if asked.form {
			self.focus_form();
		}
		if asked.name {
			self.focus_name();
		}
	}

	fn columns_ui(&mut self, ui: &mut Mui<'_>, app: &mut App, out: &mut Vec<Event>, intents: &mut Vec<Intent>) {
		let rt = ui.rt();
		let beside = ui.available_width() >= OUTLINE_COLUMN_FROM;
		let (outline, inspector) = (self.columns.outline, self.columns.inspector);
		if beside {
			if outline {
				// the outline rows have their own inset
				column(ui, "outline-column", true, OUTLINE_WIDTHS, Margin::symmetric(4, 8), |ui| {
					self.outline_ui(ui, app, out, intents);
				});
			}
			if inspector {
				column(ui, "inspector-column", false, INSPECTOR_WIDTHS, Margin::symmetric(12, 8), |ui| {
					self.inspector.show(ui, app, out, intents);
				});
			}
		} else if outline || inspector {
			column(ui, "outline-column", true, INSPECTOR_WIDTHS, Margin::ZERO, |ui| match (outline, inspector) {
				(true, true) => {
					let label = app.strings.get("ColumnsSplitter");
					// both panes run in turn, never together
					let all = RefCell::new((&mut *self, &mut *app, &mut *out, &mut *intents));
					SplitPane::new("outline-split", label)
						.axis(Axis::Vertical)
						.default_fraction(OUTLINE_SHARE)
						.min_first(120.)
						.min_second(160.)
						.show(
							ui,
							|ui| {
								Frame::NONE.inner_margin(Margin::symmetric(4, 8)).show(ui, |ui| {
									let (this, app, out, intents) = &mut *all.borrow_mut();
									this.outline_ui(&mut rt.ui(ui), app, out, intents);
								});
							},
							|ui| {
								Frame::NONE.inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
									let (this, app, out, intents) = &mut *all.borrow_mut();
									this.inspector.show(&mut rt.ui(ui), app, out, intents);
								});
							},
						);
				}
				(true, false) => self.outline_ui(ui, app, out, intents),
				_ => self.inspector.show(ui, app, out, intents),
			});
		}
	}

	/// The outline.
	fn outline_ui(&mut self, ui: &mut Mui<'_>, app: &mut App, out: &mut Vec<Event>, intents: &mut Vec<Intent>) {
		self.asked = self.outline.show(ui, app, out, intents);
	}

	/// Below Expanded: the outline as a modal side sheet, the inspector as a standard bottom sheet.
	fn sheets_ui(&mut self, ui: &mut Mui<'_>, app: &mut App, out: &mut Vec<Event>, intents: &mut Vec<Intent>) {
		let rt = ui.rt();
		let (mut outline_open, before) = *self.sheets.get_or_insert((false, false));
		let headline = strip_mnemonic(app.strings.get("MenuViewOutline"));
		SideSheet::new("outline-sheet")
			.headline(headline)
			.show_modal(rt, &mut outline_open, |ui| self.outline_ui(ui, app, out, intents));
		let mut inspector_open = before;
		let inspector = &mut self.inspector;
		BottomSheet::new("inspector-sheet")
			.detents([0.4, 0.9])
			.show(rt, &mut inspector_open, |ui| inspector.show(ui, app, out, intents));
		self.sheets = Some((outline_open, inspector_open));
	}
}

/// A flat full-height pane on one side of the canvas: the tree rows' surface tone, a 1 px divider toward the canvas.
fn column(
	ui: &mut Mui<'_>,
	id: &'static str,
	start: bool,
	widths: Widths,
	padding: Margin,
	add: impl FnOnce(&mut Mui<'_>),
) {
	let rt = ui.rt();
	let color = &ui.tokens().color;
	let (fill, line) = (color.surface, color.outline_variant);
	let panel = if start { Panel::left(id) } else { Panel::right(id) };
	let rect = panel
		.resizable(true)
		.default_size(widths.0)
		.size_range(widths.1)
		.show_separator_line(false)
		.frame(Frame::NONE.fill(fill).inner_margin(padding))
		.show(ui, |ui| add(&mut rt.ui(ui)))
		.response
		.rect;
	let x = if start { rect.right() - 0.5 } else { rect.left() + 0.5 };
	ui.painter().vline(x, rect.y_range(), Stroke::new(1., line));
}

/// The events that show `layers`.
fn layer_events(app: &App, layers: Layers) -> Vec<Event> {
	let d = &app.document.view;
	let (field, intensity) = layers.shows();
	[(Layer::Field, d.field.preview, field), (Layer::Intensity, d.intensity.preview, intensity)]
		.into_iter()
		.filter(|(_, now, want)| now != want)
		.filter_map(|(layer, _, want)| display_edit(layer, want).map(|e| Event::Command(Command::Edit(e))))
		.collect()
}

fn display_event(edit: ViewEdit) -> Event {
	Event::Command(Command::Edit(Edit::View(edit)))
}

fn display_edit(layer: Layer, on: bool) -> Option<Edit> {
	waveduderust_app::preview::display_edit(layer, DisplayLayerEdit::Preview(on))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn canvas_only_hides_both_columns_and_restores_them() {
		let start = Columns { outline: true, inspector: false, before: None };
		let only = start.toggled(ViewCommand::CanvasOnly);
		assert!(!only.outline && !only.inspector && only.canvas_only());
		assert_eq!(only.toggled(ViewCommand::CanvasOnly), start);
	}

	#[test]
	fn a_column_toggle_leaves_canvas_only() {
		let only = Columns::default().toggled(ViewCommand::CanvasOnly);
		let next = only.toggled(ViewCommand::Inspector);
		assert!(next.inspector && !next.outline && !next.canvas_only());
	}
}
