//! The Project pane (docs/ui/20): the toolbar and the tree of the project, its selection mirrored both ways
//! with the state's, its actions sent as commands.

use std::borrow::Cow;

use freya::prelude::*;
use wavedude_core::model::{Lens, Part, PartKind, Project};
use wavedude_widgets::icon::icons;
use wavedude_widgets::tree::{DropPosition, RowKind, reorder_index};
use wavedude_widgets::{Icon, IconView, TreeAction, TreeRow, TreeSelection, TreeView, use_wave_theme};

use wavedude_app_state::Event as AppEvent;
use wavedude_app_state::commands::{Command, NewPart};
use wavedude_app_state::selection::{ItemKind, ItemRef, Row};
use wavedude_app_state::strings::Strings;

use super::app::Cx;
use super::menus::{self, glyphs};
use super::shared::{App, Topics};

/// The drawing of a part kind (the menus and the rows of parts other than lenses).
pub fn kind_icon(kind: NewPart) -> Icon {
	match kind {
		NewPart::Lens => {
			Icon::path("M3.5 1.5 Q0.5 7.5 3.5 13.5 Q6.5 7.5 3.5 1.5 Z M9.5 1.5 Q12.5 7.5 9.5 13.5 M13.5 1.5 L13.5 13.5")
		}
		NewPart::Fiber => Icon::path("M0.5 4.5 C6.5 4.5 7.5 10.5 14.5 10.5 M0.5 7.5 C6.5 7.5 7.5 13.5 14.5 13.5"),
		NewPart::BraggReflector => {
			Icon::path("M2.5 1.5 L2.5 13.5 M5.5 1.5 L5.5 13.5 M8.5 1.5 L8.5 13.5 M11.5 1.5 L11.5 13.5")
		}
		NewPart::GrinLens => Icon::path(
			"M1.5 1.5 L13.5 1.5 L13.5 13.5 L1.5 13.5 Z M1.5 4.5 L13.5 4.5 M1.5 7.5 L13.5 7.5 M1.5 10.5 L13.5 10.5",
		),
		NewPart::ZonePlate => Icon::path("M7.5 0.5 L7.5 2.5 M7.5 4.5 L7.5 5.5 M7.5 9.5 L7.5 10.5 M7.5 12.5 L7.5 14.5"),
		NewPart::Polygon => Icon::path("M4.5 1.5 L10.5 1.5 L13.5 7.5 L10.5 13.5 L4.5 13.5 L1.5 7.5 Z"),
		NewPart::Rectangle => Icon::path("M1.5 3.5 L13.5 3.5 L13.5 11.5 L1.5 11.5 Z"),
		NewPart::Ellipse => Icon::path("M7.5 3.5 A6 4 0 1 1 7.49 3.5 Z"),
		NewPart::Triangle => Icon::path("M7.5 1.5 L13.5 13.5 L1.5 13.5 Z"),
		NewPart::Outline => Icon::path("M2.5 3.5 L8.5 1.5 L13.5 6.5 L10.5 13.5 L4.5 11.5 L1.5 7.5 Z"),
		NewPart::XorGate => {
			Icon::path("M1.5 2.5 Q4.5 7.5 1.5 12.5 M3.5 2.5 Q6.5 7.5 3.5 12.5 Q11.5 12.5 14.5 7.5 Q11.5 2.5 3.5 2.5")
		}
		NewPart::AndGate => Icon::path("M2.5 2.5 L8.5 2.5 A5 5 0 0 1 8.5 12.5 L2.5 12.5 Z"),
		NewPart::OrGate => Icon::path("M2.5 2.5 Q5.5 7.5 2.5 12.5 Q10.5 12.5 14.5 7.5 Q10.5 2.5 2.5 2.5"),
	}
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Surface {
	Convex,
	Flat,
	Concave,
}

/// A surface by its sag at the rim: flat under half a pixel; `bulge` is true when the radius's sign bulges
/// outward on that face.
fn surface(radius: f64, diameter: f64, bulge: bool) -> Surface {
	let r = radius.abs();
	let h = diameter / 2.;
	let sag = if radius == 0. {
		0.
	} else if r <= h {
		r
	} else {
		r - (r * r - h * h).sqrt()
	};
	match (sag < 0.5, bulge) {
		(true, _) => Surface::Flat,
		(false, true) => Surface::Convex,
		(false, false) => Surface::Concave,
	}
}

/// A lens row's icon: its own profile from R1, R2 and the diameter.
pub fn lens_icon(lens: &Lens) -> Icon {
	let d = f64::from(lens.diameter);
	let front = surface(lens.r1, d, lens.r1 > 0.);
	let back = surface(lens.r2, d, lens.r2 < 0.);
	let (l, lc) = match front {
		Surface::Convex => (6.5, 1.5),
		Surface::Flat => (4.5, 4.5),
		Surface::Concave => (3.5, 6.5),
	};
	let (r, rc) = match back {
		Surface::Convex => (8.5, 13.5),
		Surface::Flat => (10.5, 10.5),
		Surface::Concave => (11.5, 8.5),
	};
	let path = format!("M{l} 1.5 L{r} 1.5 Q{rc} 7.5 {r} 13.5 L{l} 13.5 Q{lc} 7.5 {l} 1.5 Z");
	Icon::Path { stroke: Cow::Owned(path), fill: None }
}

fn part_icon(part: &Part) -> Icon {
	let kind = match &part.kind {
		PartKind::Lens(l) => return lens_icon(l),
		PartKind::Polygon(_) => NewPart::Polygon,
		PartKind::Rectangle(_) => NewPart::Rectangle,
		PartKind::Ellipse(_) => NewPart::Ellipse,
		PartKind::Triangle(_) => NewPart::Triangle,
		PartKind::Outline(_) => NewPart::Outline,
		PartKind::Fiber(_) => NewPart::Fiber,
		PartKind::BraggReflector(_) => NewPart::BraggReflector,
		PartKind::GrinLens(_) => NewPart::GrinLens,
		PartKind::ZonePlate(_) => NewPart::ZonePlate,
		PartKind::LogicGate(g) => match g.kind {
			wavedude_core::model::GateKind::Xor => NewPart::XorGate,
			wavedude_core::model::GateKind::And => NewPart::AndGate,
			wavedude_core::model::GateKind::Or => NewPart::OrGate,
		},
	};
	kind_icon(kind)
}

const PROJECT_ICON: Icon =
	Icon::path("M1.5 1.5 L13.5 1.5 L13.5 13.5 L1.5 13.5 Z M1.5 9.5 Q4.5 5.5 7.5 9.5 Q10.5 13.5 13.5 9.5");
const SCENE_ICON: Icon = glyphs::FRAME;
const SIMULATION_ICON: Icon = Icon::path("M7.5 1.5 A6 6 0 1 1 7.49 1.5 Z M7.5 4.5 L7.5 7.5 L10.5 7.5");
const DISPLAY_ICON: Icon =
	Icon::path("M0.5 1.5 L14.5 1.5 L14.5 10.5 L0.5 10.5 Z M4.5 13.5 L10.5 13.5 M7.5 10.5 L7.5 13.5");
const PART_ICON: Icon = Icon::path("M7.5 1.5 Q3.5 7.5 7.5 13.5 Q11.5 7.5 7.5 1.5 Z");
const SCHEME_ICON: Icon = glyphs::THEME;

fn folder_of(kind: ItemKind) -> (&'static str, Icon, u32) {
	match kind {
		ItemKind::Source => ("TreeSources", icons::SOURCE, 0),
		ItemKind::Part => ("TreeParts", PART_ICON, 1),
		ItemKind::Probe => ("TreeProbes", icons::PROBE, 2),
		ItemKind::Scheme => ("TreeColorSchemes", SCHEME_ICON, 3),
	}
}

/// The item's Enabled (or Active) flag; `None` for schemes.
fn enabled_of(project: &Project, item: ItemRef) -> Option<bool> {
	match item.kind {
		ItemKind::Source => project.sources.get(item.index).map(|s| s.enabled),
		ItemKind::Part => project.parts.get(item.index).map(|p| p.base.enabled),
		ItemKind::Probe => project.probes.get(item.index).map(|p| p.active),
		ItemKind::Scheme => None,
	}
}

/// The tree's rows in tree order.
pub fn rows(project: &Project, s: &Strings) -> Vec<TreeRow<Row>> {
	let mut rows = vec![
		TreeRow::new(Row::Project, 0, RowKind::Root, s.get("TreeProject")).icon(PROJECT_ICON).test_id("Tree:Project"),
		TreeRow::new(Row::Scene, 1, RowKind::Settings, s.get("TreeScene")).icon(SCENE_ICON).test_id("Tree:Scene"),
		TreeRow::new(Row::Simulation, 1, RowKind::Settings, s.get("TreeSimulation"))
			.icon(SIMULATION_ICON)
			.test_id("Tree:Simulation"),
		TreeRow::new(Row::Display, 1, RowKind::Settings, s.get("TreeDisplay"))
			.icon(DISPLAY_ICON)
			.test_id("Tree:Display"),
	];
	for kind in ItemKind::ALL {
		let (key, icon, group) = folder_of(kind);
		rows.push(
			TreeRow::new(Row::Folder(kind), 1, RowKind::Folder, s.get(key))
				.icon(icon.clone())
				.group(group)
				.can_add()
				.add_tip(s.get("TipAdd"))
				.test_id(format!("Tree:{key}")),
		);
		for (index, name) in kind.names(project).into_iter().enumerate() {
			let item = ItemRef::new(kind, index);
			let icon = match kind {
				ItemKind::Part => part_icon(&project.parts[index]),
				_ => icon.clone(),
			};
			let mut row = TreeRow::new(Row::Item(item), 2, RowKind::Item, name)
				.icon(icon)
				.group(group)
				.test_id(format!("Tree:{name}"));
			if let Some(on) = enabled_of(project, item) {
				let tip = match kind {
					ItemKind::Source => "TipSourceEnabled",
					ItemKind::Part => "TipPartEnabled",
					ItemKind::Probe | ItemKind::Scheme => "TipProbeActive",
				};
				row = row.enabled(on).enabled_tip(s.get(tip));
			}
			rows.push(row);
		}
	}
	rows
}

/// The state's selection as the tree view's.
fn tree_selection(app: &App) -> TreeSelection<Row> {
	let sel = &app.selection;
	TreeSelection {
		current: sel.current(),
		selected: sel.items().iter().map(|&i| Row::Item(i)).collect(),
		anchor: sel.anchor().map(Row::Item),
	}
}

fn item_of(row: &Row) -> Option<ItemRef> {
	match row {
		Row::Item(i) => Some(*i),
		_ => None,
	}
}

/// Takes the tree view's selection into the state.
fn assign_selection(cx: &Cx, sel: &TreeSelection<Row>) {
	let items: Vec<ItemRef> = sel.selected.iter().filter_map(item_of).collect();
	let anchor = sel.anchor.as_ref().and_then(item_of);
	cx.shared.update(|a| a.selection.assign(items, sel.current, anchor));
}

fn add(cx: &Cx, kind: ItemKind, at: (f64, f64)) {
	match kind {
		ItemKind::Source => add_then_edit(cx, Command::AddSource { at: None }),
		ItemKind::Probe => add_then_edit(cx, Command::AddProbe { at: None }),
		ItemKind::Scheme => add_then_edit(cx, Command::AddScheme),
		ItemKind::Part => open_part_menu(cx, at),
	}
}

/// Adds an item and puts the caret in its form's first box (docs/ui/20, Add).
pub fn add_then_edit(cx: &Cx, command: Command) {
	cx.send(AppEvent::Command(command));
	request_form_focus(cx);
}

/// Asks the Properties pane to take the keyboard focus.
pub fn request_form_focus(cx: &Cx) {
	let mut f = cx.ui.focus_form;
	*f.write() += 1;
}

/// The Add part menu at `at`.
pub fn open_part_menu(cx: &Cx, at: (f64, f64)) {
	let model = cx.shared.peek(|a| wavedude_widgets::MenuModel::new(menus::part_entries(&a.strings, "AddPart:")));
	let cx2 = cx.clone();
	cx.ui.open_menu(at, model, move |action| menus::dispatch(&cx2, action));
}

fn first_item(keys: &[Row]) -> Option<ItemRef> {
	keys.iter().find_map(item_of)
}

fn on_action(cx: &Cx, action: TreeAction<Row>, pointer: (f64, f64)) {
	let command = |c: Command| cx.send(AppEvent::Command(c));
	match action {
		TreeAction::ToggleEnabled(keys) => {
			if let Some(target) = first_item(&keys) {
				command(Command::ToggleEnabled { target });
			}
		}
		TreeAction::Activate(_) => request_form_focus(cx),
		TreeAction::Rename(row, name) => {
			if let Some(target) = item_of(&row) {
				command(Command::Rename { target, name });
			}
		}
		TreeAction::Delete(keys) => {
			if let Some(target) = first_item(&keys) {
				command(Command::Remove { target });
			}
		}
		TreeAction::Duplicate(keys) => {
			if let Some(target) = first_item(&keys) {
				command(Command::Duplicate { target });
			}
		}
		TreeAction::MoveUp(keys) => {
			if let Some(target) = first_item(&keys) {
				menus::move_item(cx, target, true);
			}
		}
		TreeAction::MoveDown(keys) => {
			if let Some(target) = first_item(&keys) {
				menus::move_item(cx, target, false);
			}
		}
		TreeAction::Add(Row::Folder(kind)) => add(cx, kind, pointer),
		TreeAction::Add(_) => {}
		TreeAction::Drop { dragged, position } => {
			let Some(target) = item_of(&dragged) else { return };
			let list: Vec<Row> = cx.shared.peek(|a| {
				(0..target.kind.len(&a.document.project)).map(|i| Row::Item(ItemRef::new(target.kind, i))).collect()
			});
			let position = match position {
				DropPosition::Append(_) => DropPosition::After(*list.last().expect("the dragged item's list holds it")),
				p => p,
			};
			if let Some(to) = reorder_index(&list, &dragged, &position)
				&& to != target.index
			{
				command(Command::Reorder { target, to });
			}
		}
		TreeAction::DropOutside { dragged, at } => {
			if let Some(item) = item_of(&dragged) {
				drop_on_picture(cx, item, at);
			}
		}
		TreeAction::ContextMenu { key, at } => {
			let model = cx.shared.peek(|a| menus::tree_row_menu(a, key));
			let cx2 = cx.clone();
			cx.ui.open_menu(at, model, move |action| menus::act(&cx2, action, menus::On::Row(key)));
		}
	}
}

/// A row dropped on a picture: a copy of the item lands where it was dropped (docs/ui/20, drag and drop).
fn drop_on_picture(cx: &Cx, item: ItemRef, at: (f64, f64)) {
	let target = cx.ui.pictures.peek().iter().flatten().find_map(|((ox, oy), t)| {
		let local = (at.0 as f32 - ox, at.1 as f32 - oy);
		t.contains(local.0, local.1).then(|| t.to_canvas(local.0, local.1))
	});
	let Some((x, y)) = target else { return };
	let (text, current) = cx.shared.peek(|a| {
		let objects = [wavedude_app_state::clipboard::Copyable::Item(item)];
		(wavedude_app_state::clipboard::write_fragment(&a.document.project, &objects), a.selection.current())
	});
	let place_at = Some(wavedude_core::model::Point::new(x.floor() as i32, y.floor() as i32));
	let target = wavedude_app_state::clipboard::PasteTarget { current, before: false };
	cx.send(AppEvent::Paste { text, target, place_at });
}

fn tool_button(
	icon: Icon,
	text: Option<String>,
	tip: String,
	enabled: bool,
	test: &'static str,
	on_press: impl FnMut(Event<PressEventData>) + 'static,
) -> Element {
	let theme = use_wave_theme();
	let ink = if enabled { theme.text } else { theme.text_disabled };
	let mut content =
		rect().horizontal().cross_align(Alignment::center()).spacing(4.).child(IconView::new(icon).color(ink));
	if let Some(text) = text {
		content = content.child(label().text(text).color(ink));
	}
	TooltipContainer::new(Tooltip::new_text(tip))
		.child(super::properties::tagged(
			Button::new().flat().compact().enabled(enabled).on_press(on_press).child(content),
			test,
		))
		.into_element()
}

#[derive(Clone, PartialEq)]
struct Snapshot {
	rows: Vec<TreeRow<Row>>,
	selection: TreeSelection<Row>,
	words: [String; 9],
	can_up: bool,
	can_down: bool,
	can_remove: bool,
}

fn snapshot(app: &App) -> Snapshot {
	let s = &app.strings;
	let project = &app.document.project;
	let current = app.selection.current_item();
	Snapshot {
		rows: rows(project, s),
		selection: tree_selection(app),
		words: [
			"ToolAddSource",
			"ToolAddProbe",
			"ToolAddPart",
			"TipAddSource",
			"TipAddProbe",
			"TipAddPart",
			"TipMoveUp",
			"TipMoveDown",
			"TipRemove",
		]
		.map(|k| s.get(k).to_owned()),
		can_up: current.is_some_and(|i| i.index > 0),
		can_down: current.is_some_and(|i| i.index + 1 < i.kind.len(project)),
		can_remove: current.is_some(),
	}
}

/// The Project pane.
#[derive(PartialEq)]
pub struct ProjectPane;

impl Component for ProjectPane {
	fn render(&self) -> impl IntoElement {
		let cx = Cx::get();
		let theme = use_wave_theme();
		let snap = {
			let shared = cx.shared.clone();
			use_memo(move || {
				#[cfg(test)]
				super::shared::count_run("tree");
				shared.read(Topics::DOCUMENT | Topics::SELECTION | Topics::SETTINGS, snapshot)
			})
		};
		let mut tree = cx.ui.tree;
		{
			use_side_effect(move || {
				let wanted = snap.read().selection.clone();
				if tree.peek().selection != wanted {
					tree.write().selection = wanted;
				}
			});
		}
		{
			let cx = cx.clone();
			use_side_effect(move || {
				let sel = tree.read().selection.clone();
				if snap.peek().selection != sel {
					assign_selection(&cx, &sel);
				}
			});
		}
		let mut pointer = use_state(|| (0., 0.));
		let mut part_button = use_state(|| (0., 0.));
		let snap = snap.read().clone();
		let w = &snap.words;
		let press = |f: fn(&Cx)| {
			let cx = cx.clone();
			move |_: Event<PressEventData>| f(&cx)
		};
		let current = cx.shared.peek(|a| a.selection.current_item());
		let toolbar = rect()
			.horizontal()
			.width(Size::fill())
			.padding(2.)
			.spacing(2.)
			.cross_align(Alignment::center())
			.child(tool_button(
				icons::SOURCE,
				Some(w[0].clone()),
				w[3].clone(),
				true,
				"ToolAddSource",
				press(|cx| add_then_edit(cx, Command::AddSource { at: None })),
			))
			.child(tool_button(
				icons::PROBE,
				Some(w[1].clone()),
				w[4].clone(),
				true,
				"ToolAddProbe",
				press(|cx| add_then_edit(cx, Command::AddProbe { at: None })),
			))
			.child(
				rect()
					.on_sized(move |e: Event<SizedEventData>| {
						part_button.set((f64::from(e.area.min_x()), f64::from(e.area.max_y())))
					})
					.child(tool_button(PART_ICON, Some(w[2].clone()), w[5].clone(), true, "ToolAddPart", {
						let cx = cx.clone();
						move |_| open_part_menu(&cx, *part_button.peek())
					})),
			)
			.child(rect().width(Size::px(1.)).height(Size::px(16.)).background(theme.hairline))
			.child(tool_button(icons::CHEVRON_UP, None, w[6].clone(), snap.can_up, "ToolMoveUp", {
				let cx = cx.clone();
				move |_| {
					if let Some(t) = current {
						menus::move_item(&cx, t, true);
					}
				}
			}))
			.child(tool_button(icons::CHEVRON_DOWN, None, w[7].clone(), snap.can_down, "ToolMoveDown", {
				let cx = cx.clone();
				move |_| {
					if let Some(t) = current {
						menus::move_item(&cx, t, false);
					}
				}
			}))
			.child(tool_button(icons::MINUS, None, w[8].clone(), snap.can_remove, "ToolRemove", {
				let cx = cx.clone();
				move |_| {
					if let Some(target) = current {
						cx.send(AppEvent::Command(Command::Remove { target }));
					}
				}
			}));
		let on_tree = {
			let cx = cx.clone();
			move |a: TreeAction<Row>| on_action(&cx, a, *pointer.peek())
		};
		rect()
			.expanded()
			.background(theme.tool_window_background)
			.content(Content::flex())
			.on_pointer_move(move |e: Event<PointerEventData>| {
				let p = e.global_location();
				pointer.set((p.x, p.y));
			})
			.child(toolbar)
			.child(
				rect()
					.width(Size::fill())
					.height(Size::flex(1.))
					.child(TreeView::new(snap.rows, tree, on_tree).test_id("ProjectTree")),
			)
	}
}
