//! WaveDude Rust's app: the Slint window over the core (decision 0032).

mod document;
mod tree;

#[allow(
	unused_qualifications,
	unreachable_pub,
	missing_docs,
	clippy::all,
	clippy::pedantic,
	clippy::nursery,
	clippy::restriction
)]
mod ui {
	slint::include_modules!();
}

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, VecModel};
use ui::AppWindow;

use crate::document::Document;
use crate::tree::Node;

/// Undo steps until the settings arrive (decision 0030).
const UNDO_STEPS: usize = 1000;

/// What the window shows.
struct State {
	doc: Document,
	nodes: Vec<Node>,
	selected: Node,
}

impl State {
	/// Pushes the state into the window's properties.
	fn show(&mut self, app: &AppWindow) {
		let (nodes, rows) = tree::rows(&self.doc.project, self.selected);
		self.nodes = nodes;
		app.set_tree_rows(ModelRc::new(VecModel::from(rows)));
		app.set_window_title(self.doc.title().into());
		app.set_can_undo(self.doc.can_undo());
		app.set_can_redo(self.doc.can_redo());
	}

	fn node(&self, row: i32) -> Option<Node> {
		usize::try_from(row).ok().and_then(|r| self.nodes.get(r).copied())
	}
}

fn project_dialog() -> rfd::FileDialog {
	rfd::FileDialog::new().add_filter("WaveDude project", &["wdproj", "xml"]).add_filter("Wave Dude 2D", &["json"])
}

/// Runs a menu action by its id (docs/ui/10).
fn menu(state: &mut State, app: &AppWindow, id: &str) {
	match id {
		"new" => {
			state.doc = Document::new(UNDO_STEPS);
			state.selected = Node::Project;
		}
		"open" => {
			if let Some(path) = project_dialog().pick_file() {
				open(state, app, path);
			}
		}
		"save" | "save-as" => {
			let path = state
				.doc
				.path
				.clone()
				.filter(|_| id == "save")
				.or_else(|| project_dialog().set_file_name("Untitled.wdproj").save_file());
			if let Some(path) = path {
				match state.doc.save_as(&path) {
					Ok(()) => app.set_status_text(format!("Saved {}", path.display()).into()),
					Err(e) => app.set_status_text(format!("Cannot save: {e}").into()),
				}
			}
		}
		"undo" => state.doc.undo(),
		"redo" => state.doc.redo(),
		"delete" => {
			let node = state.selected;
			state.doc.edit(|p| tree::remove(p, node));
			state.selected = Node::Project;
		}
		"exit" => {
			let _ = app.hide();
		}
		other => tracing::info!("menu: {other}"),
	}
	state.show(app);
}

fn open(state: &mut State, app: &AppWindow, path: PathBuf) {
	match state.doc.open(&path) {
		Ok(()) => {
			state.selected = Node::Project;
			app.set_status_text(format!("Opened {}", path.display()).into());
		}
		Err(e) => app.set_status_text(format!("Cannot open: {e}").into()),
	}
	state.show(app);
}

fn main() -> Result<(), slint::PlatformError> {
	let app = AppWindow::new()?;
	let state =
		Rc::new(RefCell::new(State { doc: Document::new(UNDO_STEPS), nodes: Vec::new(), selected: Node::Project }));
	if let Some(path) = std::env::args_os().nth(1) {
		open(&mut state.borrow_mut(), &app, path.into());
	}
	state.borrow_mut().show(&app);

	let on = |f: fn(&mut State, &AppWindow, i32, i32)| {
		let (state, weak) = (state.clone(), app.as_weak());
		move |row: i32, arg: i32| {
			if let Some(app) = weak.upgrade() {
				let mut s = state.borrow_mut();
				f(&mut s, &app, row, arg);
				s.show(&app);
			}
		}
	};
	let select = on(|s, _, row, _| s.selected = s.node(row).unwrap_or_default());
	app.on_tree_select(move |row| select(row, 0));
	let toggle = on(|s, _, row, _| {
		if let Some(node) = s.node(row) {
			s.doc.edit(|p| tree::toggle(p, node));
		}
	});
	app.on_tree_toggle(move |row| toggle(row, 0));
	app.on_tree_shift(on(|s, _, row, by| {
		if let Some(node) = s.node(row) {
			let mut to = node;
			s.doc.edit(|p| to = tree::shift(p, node, by as isize));
			s.selected = to;
		}
	}));
	let remove = on(|s, _, row, _| {
		if let Some(node) = s.node(row) {
			s.doc.edit(|p| tree::remove(p, node));
			s.selected = Node::Project;
		}
	});
	app.on_tree_remove(move |row| remove(row, 0));
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_menu(move |id| {
		if let Some(app) = weak.upgrade() {
			menu(&mut st.borrow_mut(), &app, &id);
		}
	});
	app.run()
}
