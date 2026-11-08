//! The main window: the menu bar on top, the dock host with the Project, Properties and Messages panes around
//! the Preview, the status bar at the bottom, the context menu and modal hosts over it.

use freya::prelude::*;
use wavedude_widgets::dock::DockLabels;
use wavedude_widgets::{ContextMenuHost, DockHost, DockLayout, DockPane, MenuBar, PaneGroup, Side, use_wave_theme};

use wavedude_app_state::Event as AppEvent;

use super::app::{Cx, MenuCallback, Ui};
use super::dialogs::ModalHost;
use super::shared::{Shared, Topics};
use super::{manual, menus, messages, properties, shortcuts, status, tree};

/// The tool panes' ids, in the order View > Panes lists them after the Preview.
pub const PANES: [&str; 3] = ["project", "properties", "messages"];
/// The Preview's entry in View > Panes; it is the centre area, not a dock pane.
pub const PREVIEW: &str = "preview";

/// The side a pane goes to when shown without a saved place.
pub fn default_side(id: &str) -> Side {
	match id {
		"messages" => Side::Bottom,
		_ => Side::Left,
	}
}

/// The first start's arrangement: Project over Properties (1 : 2) in a left column a quarter of the default
/// window wide; Messages hidden at the bottom.
pub fn default_layout() -> DockLayout {
	let mut l = DockLayout::default();
	l.left.size = 320.;
	l.left.groups = vec![PaneGroup::new(vec!["project".into()], 1.), PaneGroup::new(vec!["properties".into()], 2.)];
	l.hidden = vec![("messages".into(), Side::Bottom)];
	l
}

/// The saved arrangement, unknown ids dropped; the default one when there is none or it does not parse.
pub fn initial_layout(saved: Option<&str>) -> DockLayout {
	match saved.map(serde_json::from_str::<DockLayout>) {
		Some(Ok(mut l)) => {
			l.retain_known(&PANES);
			l
		}
		Some(Err(e)) => {
			tracing::warn!("the saved pane arrangement could not be read: {e}");
			default_layout()
		}
		None => default_layout(),
	}
}

/// Shows or hides a pane (View > Panes); the Preview hides the centre area's content.
pub fn toggle_pane(ui: &Ui, id: &str) {
	if id == PREVIEW {
		let mut shown = ui.preview_shown;
		let v = !*shown.peek();
		shown.set(v);
	} else {
		let mut layout = ui.layout;
		layout.write().toggle(id, default_side(id));
	}
}

/// Opens the Messages pane, or brings it forward when shown, and focuses its list (status bar double-click).
pub fn show_messages(ui: &Ui) {
	let mut layout = ui.layout;
	layout.write().show("messages", default_side("messages"));
	let list = ui.messages_list;
	spawn(async move {
		timer(std::time::Duration::from_millis(32)).await;
		list.request_focus();
	});
}

#[derive(Clone, PartialEq)]
struct Words {
	panes: [String; 3],
	labels: DockLabels,
}

fn words(shared: &Shared) -> Words {
	shared.read(Topics::SETTINGS, |a| {
		let s = &a.strings;
		Words {
			panes: ["PaneProject", "PaneProperties", "PaneMessages"].map(|k| s.get(k).to_owned()),
			labels: DockLabels {
				position: s.get("DockWindowPosition").into(),
				move_to: ["DockMoveLeft", "DockMoveRight", "DockMoveTop", "DockMoveBottom"]
					.map(|k| s.get(k).to_owned()),
				close: s.get("DockCloseMenu").into(),
			},
		}
	})
}

/// The preview pane of the other view module.
fn preview_centre() -> Element {
	super::preview::preview_pane().into_element()
}

#[derive(PartialEq)]
pub struct MainWindow;

impl Component for MainWindow {
	fn render(&self) -> impl IntoElement {
		let cx = Cx::get();
		let theme = use_wave_theme();
		let shared = cx.shared.clone();
		let words = use_memo(move || words(&shared));
		let model = menus::use_menu_bar();
		let words = words.read().clone();
		let ui = cx.ui;
		let panes = vec![
			DockPane::new(PANES[0], words.panes[0].clone(), tree::ProjectPane),
			DockPane::new(PANES[1], words.panes[1].clone(), properties::PropertiesPane),
			DockPane::new(PANES[2], words.panes[2].clone(), messages::MessagesPane),
		];
		let centre = if *ui.preview_shown.read() {
			preview_centre()
		} else {
			rect().expanded().background(theme.window_background).into_element()
		};
		let on_menu = {
			let cx = cx.clone();
			move |action: menus::MenuAction| menus::dispatch(&cx, action)
		};
		let on_key = {
			let cx = cx.clone();
			move |e: Event<KeyboardEventData>| shortcuts::on_global_key(&cx, &e)
		};
		let on_escape = {
			let cx = cx.clone();
			move |e: Event<KeyboardEventData>| shortcuts::on_escape(&cx, &e)
		};
		let on_mouse_down = {
			let shared = cx.shared.clone();
			move |_: Event<PointerEventData>| drop(shared.send(AppEvent::MouseDown))
		};
		let on_mouse_up = {
			let shared = cx.shared.clone();
			move |_: Event<PointerEventData>| drop(shared.send(AppEvent::MouseUp))
		};
		let on_drop = {
			let cx = cx.clone();
			move |e: Event<FileEventData>| {
				if let Some(path) = e.file_paths.first() {
					cx.send(AppEvent::DropFile(path.clone()));
				}
			}
		};
		rect()
			.width(Size::fill())
			.height(Size::fill())
			.background(theme.window_background)
			.color(theme.text)
			.font_size(theme.font_size)
			.content(Content::flex())
			.on_global_key_down(on_key)
			.on_key_down(on_escape)
			.on_global_pointer_down(on_mouse_down)
			.on_global_pointer_up(on_mouse_up)
			.on_file_drop(on_drop)
			.child(MenuBar::new(model.read().clone(), on_menu))
			.child(
				rect()
					.width(Size::fill())
					.height(Size::flex(1.))
					.child(DockHost::new(ui.layout, panes, centre).labels(words.labels)),
			)
			.child(status::StatusArea)
			.child(ContextMenuHost::new(ui.menu, |callback: MenuCallback| callback.run()))
			.child(ModalHost)
			.child(manual::ManualLauncher)
	}
}
