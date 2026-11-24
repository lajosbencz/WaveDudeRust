//! WaveDude Rust's app: the Slint window over the core (decision 0032).

mod document;
mod form;
mod gpu;
mod live;
mod log;
mod messages;
mod playback;
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
use crate::live::Live;
use crate::messages::Messages;
use crate::playback::Playback;
use crate::tree::Node;

/// Undo steps until the settings arrive (decision 0030).
const UNDO_STEPS: usize = 1000;

/// What the window shows.
struct State {
	doc: Document,
	nodes: Vec<Node>,
	selected: Node,
	live: Live,
	playback: Playback,
	messages: Messages,
	/// The document generation the run started from.
	ran: u64,
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
		if self.ran != self.doc.generation {
			self.ran = self.doc.generation;
			self.live.edited(&self.doc.project, &self.doc.view);
		}
		app.set_run_active(self.live.active());
		let (heading, rows) = form::of_selected(&self.doc.project, &self.doc.view, self.selected);
		app.set_props_heading(heading.into());
		app.set_props_rows(ModelRc::new(VecModel::from(rows)));
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
		"run" => {
			state.live.run(&state.doc.project, &state.doc.view);
			state.playback.frames = state.live.frames;
			state.playback.follow();
		}
		"pause" => state.live.toggle_pause(),
		"stop" => {
			state.live.stop();
			state.playback.live = false;
		}
		"new" => {
			state.playback.reset();
			let generation = state.doc.generation + 1;
			state.doc = Document::new(UNDO_STEPS);
			state.doc.generation = generation;
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
					Ok(()) => tracing::info!("saved {}", path.display()),
					Err(e) => tracing::error!("cannot save {}: {e}", path.display()),
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

/// A transport bar or timeline action (docs/ui/50).
fn transport(state: &mut State, app: &AppWindow, id: &str) {
	let p = &mut state.playback;
	match id {
		"play" if p.playing => p.pause(),
		"play" => p.play(),
		"prev" => p.seek(i64::from(p.position) - 1),
		"next" => p.seek(i64::from(p.position) + 1),
		"in" => p.set_in(),
		"out" => p.set_out(),
		"clear-range" => (p.start, p.end) = (None, None),
		"loop" => p.looping ^= true,
		"live" => p.live ^= true,
		"pause-run" => state.live.toggle_pause(),
		other => menu(state, app, other),
	}
}

fn transport_state(p: &Playback, shown: &live::Shown) -> ui::Transport {
	let int = |v: u32| i32::try_from(v).unwrap_or(i32::MAX);
	let (start, end) = p.range();
	ui::Transport {
		frames: int(p.frames),
		position: int(p.position),
		made: shown.made.map_or(-1, int),
		start: int(start),
		end: int(end),
		ranged: p.start.is_some() || p.end.is_some(),
		playing: p.playing,
		live: p.live,
		looping: p.looping,
		running: shown.running,
		step: shown.step.map_or_else(Default::default, |s| format!("step {s}").into()),
	}
}

fn open(state: &mut State, app: &AppWindow, path: PathBuf) {
	match state.doc.open(&path) {
		Ok(()) => {
			state.selected = Node::Project;
			state.playback.reset();
			tracing::info!("opened {}", path.display());
		}
		Err(e) => tracing::error!("cannot open {}: {e}", path.display()),
	}
	state.show(app);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let ring = log::init();
	let (device, queue) = gpu::select()?;
	let app = AppWindow::new()?;
	let live = Live::new(device, queue)?;
	let state = Rc::new(RefCell::new(State {
		doc: Document::new(UNDO_STEPS),
		nodes: Vec::new(),
		selected: Node::Project,
		live,
		playback: Playback::default(),
		messages: Messages::new(ring),
		ran: 0,
	}));
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
	app.on_props_set(move |row, text| {
		if let Some(app) = weak.upgrade() {
			let mut s = st.borrow_mut();
			let node = s.selected;
			let mut result = Ok(());
			let row = usize::try_from(row).unwrap_or(usize::MAX);
			if node == Node::Display {
				let doc = &mut s.doc;
				result = form::set_selected(&mut doc.project, &mut doc.view, node, row, &text);
			} else {
				let view = &mut s.doc.view.clone();
				s.doc.edit(|p| result = form::set_selected(p, view, node, row, &text));
			}
			app.set_props_error(result.err().unwrap_or_default().into());
			s.show(&app);
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_menu(move |id| {
		if let Some(app) = weak.upgrade() {
			menu(&mut st.borrow_mut(), &app, &id);
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_transport_action(move |id| {
		if let Some(app) = weak.upgrade() {
			transport(&mut st.borrow_mut(), &app, &id);
		}
	});
	let st = state.clone();
	app.on_seek(move |frame| st.borrow_mut().playback.seek(i64::from(frame)));
	let st = state.clone();
	app.on_transport_speed(move |v| {
		if let Ok(speed) = v.parse::<f64>() {
			st.borrow_mut().playback.speed = speed.clamp(0.01, 100.0);
		}
	});
	let on_log = |f: fn(&mut Messages, slint::SharedString)| {
		let (st, weak) = (state.clone(), app.as_weak());
		move |arg: slint::SharedString| {
			if let Some(app) = weak.upgrade() {
				let m = &mut st.borrow_mut().messages;
				f(m, arg);
				m.show(&app);
			}
		}
	};
	let search = on_log(|m, t| m.search(&t));
	app.on_log_search(search);
	let clear = on_log(|m, _| m.clear());
	app.on_log_clear(move || clear(slint::SharedString::new()));
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_log_toggle(move |level| {
		if let Some(app) = weak.upgrade() {
			let m = &mut st.borrow_mut().messages;
			m.toggle(level);
			m.show(&app);
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	let timer = slint::Timer::default();
	timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(16), move || {
		if let Some(app) = weak.upgrade() {
			let s = &mut *st.borrow_mut();
			s.live.seek(s.playback.position);
			let shown = s.live.poll(&s.doc.project, &s.doc.view);
			s.playback.frames = s.live.frames.max(s.playback.frames);
			if s.playback.live && !shown.running && shown.made.is_some_and(|m| m + 1 >= s.playback.frames) {
				s.playback.position = shown.made.unwrap_or_default();
				s.playback.live = false;
			}
			s.playback.tick(std::time::Instant::now(), s.doc.view.video.fps, shown.made);
			app.set_transport(transport_state(&s.playback, &shown));
			if s.messages.take() {
				s.messages.show(&app);
			}
			if let Some(image) = shown.field {
				app.set_field(image);
			}
			if let Some(image) = shown.intensity {
				app.set_intensity(image);
			}
			app.set_run_status(shown.status.into());
		}
	});
	app.run()?;
	Ok(())
}
