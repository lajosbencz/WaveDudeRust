//! WaveDude Rust's app: the Slint window over the core (decision 0032).

mod document;
mod examples;
mod export;
mod files;
mod form;
mod gpu;
mod live;
mod log;
mod messages;
mod playback;
mod settings;
mod tree;
#[cfg(target_arch = "wasm32")]
mod web;

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
#[cfg(not(target_arch = "wasm32"))]
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
	/// The menu action the unsaved edits prompt holds.
	pending: Option<String>,
	settings: waveduderust_platform::settings::Settings,
	store: Option<waveduderust_platform::store::Store>,
	settings_section: usize,
	facts: waveduderust::quality::MachineFacts,
	exporting: Option<export::Exporting>,
	inbox: files::Inbox,
	example_topic: usize,
	example_search: String,
	/// The examples the dialog lists, by index into [`examples::all`].
	example_rows: Vec<usize>,
	/// The last run export, for Export last.
	last_export: Option<export::Kind>,
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

	/// A project replaced the one shown: the selection and the timeline start over, frame 0 is made.
	fn opened(&mut self) {
		self.selected = Node::Project;
		self.playback.reset();
		self.live.load(&self.doc.project, &self.doc.view);
		self.playback.frames = self.live.frames;
		self.ran = self.doc.generation;
	}

	fn node(&self, row: i32) -> Option<Node> {
		usize::try_from(row).ok().and_then(|r| self.nodes.get(r).copied())
	}
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
			if let Some(e) = &state.exporting {
				e.cancel();
			}
			state.live.stop();
			state.playback.live = false;
		}
		"welcome" => {
			show_welcome(state, app);
			app.set_welcome_shown(true);
		}
		"random-example" => {
			let nanos = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
			let e = &examples::all()[nanos as usize % examples::all().len()];
			menu(state, app, &format!("example:{}", e.file));
		}
		"examples" => {
			show_examples(state, app);
			app.set_examples_shown(true);
		}
		id if id.starts_with("example:") && !state.doc.dirty() => {
			let file = &id["example:".len()..];
			if let Some(e) = examples::all().iter().find(|e| e.file == file) {
				match waveduderust::io::wdproj::read_str(e.text) {
					Ok((project, view)) => {
						state.doc.adopt(project, view, &format!("{}.wdproj", e.title));
						state.opened();
						tracing::info!("Opened the example {}", e.title);
					}
					Err(err) => tracing::error!("the example {file} does not parse: {err}"),
				}
			}
		}
		id if (matches!(id, "new" | "open" | "close" | "exit") || id.starts_with("example:")) && state.doc.dirty() => {
			state.pending = Some(id.to_owned());
			let name = state.doc.name();
			app.set_modal(modal(ui::ModalKind::Question, "WaveDude Rust", &format!("Save the changes to {name}?"), &["Save", "Don't save", "Cancel"], 0, 2));
		}
		"new" | "close" => {
			state.playback.reset();
			let generation = state.doc.generation + 1;
			state.doc = Document::new(state.settings.undo_steps as usize);
			state.doc.generation = generation;
			state.opened();
		}
		"open" => {
			#[cfg(not(target_arch = "wasm32"))]
			if let Some(path) = files::dialog().pick_file() {
				open(state, app, path);
			}
			#[cfg(target_arch = "wasm32")]
			files::pick(&state.inbox);
		}
		"save" | "save-as" => {
			save(state, app, id == "save-as");
		}
		id if export::Kind::of(id).is_some() || id == "export-last" => {
			let kind = export::Kind::of(id).or(state.last_export);
			if state.exporting.is_none()
				&& let Some(kind) = kind
			{
				state.exporting = export::start(kind, &state.doc.project, &state.doc.view, state.playback.position, &state.settings);
				if matches!(kind, export::Kind::Run(_)) && state.exporting.is_some() {
					state.last_export = Some(kind);
				}
			}
		}
		"settings" => {
			show_settings(state, app);
			app.set_settings_shown(true);
		}
		"about" => app.set_modal(modal(
			ui::ModalKind::Info,
			"About WaveDude Rust",
			&format!("WaveDude Rust {}\nA 2D wave simulator\n\nBased on the work of WaveDude Next; Jeroen Vleggaar, Huygens Optics, author of Wave Dude 2D, Copyright (C) 2026; 0x23 (Diffraction Limited), author of WaveSimulator2D.\n\nThis program is free software under the GNU General Public License, version 3 or later.", env!("CARGO_PKG_VERSION")),
			&["Close"],
			0,
			0,
		)),
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

/// Puts `path` first in the recent files and saves the settings.
#[cfg(not(target_arch = "wasm32"))]
fn remember(state: &mut State, path: &std::path::Path) {
	state.settings.push_recent(path);
	if let Some(Err(e)) = state.store.as_ref().map(|s| state.settings.save(s)) {
		tracing::error!("{e}");
	}
}

/// Pushes the Examples dialog's topics and the rows of the topic, filtered by the search.
fn show_examples(state: &mut State, app: &AppWindow) {
	let topics: Vec<slint::SharedString> = examples::TOPICS.iter().map(|t| (*t).into()).collect();
	app.set_example_topics(ModelRc::from(topics.as_slice()));
	let query = state.example_search.to_lowercase();
	let matches = |e: &examples::Example| {
		query.is_empty() || e.title.to_lowercase().contains(&query) || e.description.to_lowercase().contains(&query)
	};
	if !query.is_empty() && !examples::all().iter().any(|e| e.topic == state.example_topic && matches(e)) {
		state.example_topic = examples::all().iter().find(|e| matches(e)).map_or(state.example_topic, |e| e.topic);
	}
	state.example_rows = (0..examples::all().len())
		.filter(|&i| examples::all()[i].topic == state.example_topic && matches(&examples::all()[i]))
		.collect();
	let rows: Vec<ui::ExampleRow> = state
		.example_rows
		.iter()
		.map(|&i| ui::ExampleRow {
			title: examples::all()[i].title.as_str().into(),
			description: examples::all()[i].description.as_str().into(),
		})
		.collect();
	app.set_example_topic(i32::try_from(state.example_topic).unwrap_or_default());
	app.set_example_rows(ModelRc::new(VecModel::from(rows)));
}

/// Pushes the Welcome page's RAM, levers and check box.
fn show_welcome(state: &State, app: &AppWindow) {
	app.set_welcome_ram(format!("{:.1} GiB", state.facts.physical_bytes as f64 / f64::from(1u32 << 30)).into());
	app.set_welcome_levers(settings::levers(&state.settings).into());
	app.set_welcome_show_startup(state.settings.show_startup);
}

/// Applies the settings to the window, the history and the run.
fn apply_settings(state: &mut State, app: &AppWindow) {
	let s = &state.settings;
	app.global::<ui::Theme<'_>>().set_mode(settings::theme_mode(s));
	app.set_preview_stacked(s.preview_orientation == waveduderust_platform::settings::Orientation::Vertical);
	state.doc.set_limit(s.undo_steps as usize);
	state.live.quality = s.preview_quality();
	state.live.auto_rerun = s.auto_rerun;
}

/// Pushes the Settings window's sections and rows.
fn show_settings(state: &State, app: &AppWindow) {
	let sections: Vec<slint::SharedString> = settings::SECTIONS.iter().map(|s| (*s).into()).collect();
	app.set_settings_sections(ModelRc::from(sections.as_slice()));
	app.set_settings_section(i32::try_from(state.settings_section).unwrap_or_default());
	app.set_settings_rows(ModelRc::new(VecModel::from(settings::rows(&state.settings, state.settings_section))));
}

/// A message window's content (docs/ui/70, Message window).
fn modal(kind: ui::ModalKind, title: &str, text: &str, buttons: &[&str], default: i32, cancel: i32) -> ui::Modal {
	let buttons: Vec<slint::SharedString> = buttons.iter().map(|b| (*b).into()).collect();
	ui::Modal {
		shown: true,
		kind,
		title: title.into(),
		text: text.into(),
		buttons: ModelRc::from(buttons.as_slice()),
		default,
		cancel,
	}
}

/// Saves the project, to a picked file for Save As or without a file; `false` when cancelled or failed.
#[cfg(target_arch = "wasm32")]
fn save(state: &mut State, app: &AppWindow, _: bool) -> bool {
	let name = state.doc.name();
	match files::download(&name, &state.doc.text()) {
		Ok(()) => {
			state.doc.mark_saved();
			tracing::info!("saved {name} as a download");
			true
		}
		Err(e) => {
			tracing::error!("cannot save {name}: {e}");
			app.set_modal(modal(
				ui::ModalKind::Error,
				"WaveDude Rust",
				&format!("Could not save '{name}'."),
				&["OK"],
				0,
				0,
			));
			false
		}
	}
}

#[cfg(not(target_arch = "wasm32"))]
fn save(state: &mut State, app: &AppWindow, pick: bool) -> bool {
	let Some(path) = state.doc.path.clone().filter(|_| !pick).or_else(|| {
		files::dialog().set_file_name(state.doc.suggested.as_deref().unwrap_or("Untitled.wdproj")).save_file()
	}) else {
		return false;
	};
	match state.doc.save_as(&path) {
		Ok(()) => {
			tracing::info!("saved {}", path.display());
			remember(state, &path);
			true
		}
		Err(e) => {
			tracing::error!("cannot save {}: {e}", path.display());
			app.set_modal(modal(
				ui::ModalKind::Error,
				"WaveDude Rust",
				&format!("Could not save '{}'.", path.display()),
				&["OK"],
				0,
				0,
			));
			false
		}
	}
}

/// The answer to the message window: for the unsaved edits prompt, Save, Don't save or Cancel.
fn answer(state: &mut State, app: &AppWindow, button: i32) {
	app.set_modal(ui::Modal::default());
	if let Some(then) = state.pending.take() {
		let go = match button {
			0 => save(state, app, false),
			1 => true,
			_ => false,
		};
		if go {
			state.doc.mark_saved();
			menu(state, app, &then);
		}
	}
}

/// A transport bar or timeline action (docs/ui/50).
fn transport(state: &mut State, app: &AppWindow, id: &str) {
	let p = &mut state.playback;
	match id {
		"play" if p.playing => p.pause(),
		"play" => {
			p.play();
			state.live.fill();
		}
		"prev" => p.seek(i64::from(p.position) - 1),
		"next" => p.seek(i64::from(p.position) + 1),
		"in" => p.set_in(),
		"out" => p.set_out(),
		"clear-range" => (p.start, p.end) = (None, None),
		"loop" => p.looping ^= true,
		"live" => p.live ^= true,
		"pause-run" => state.live.toggle_pause(),
		"layout" => {
			use waveduderust_platform::settings::Orientation;
			state.settings.preview_orientation = if true { Orientation::Vertical } else { Orientation::Horizontal };
			if let Some(Err(e)) = state.store.as_ref().map(|s| state.settings.save(s)) {
				tracing::error!("{e}");
			}
		}
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

#[cfg(not(target_arch = "wasm32"))]
fn open(state: &mut State, app: &AppWindow, path: PathBuf) {
	match state.doc.open(&path) {
		Ok(()) => {
			state.opened();
			remember(state, &path);
			tracing::info!("Opened {}", path.display());
		}
		Err(e) => {
			tracing::error!("cannot open {}: {e}", path.display());
			app.set_modal(modal(
				ui::ModalKind::Error,
				"WaveDude Rust",
				&format!("Could not open '{}'.", path.display()),
				&["OK"],
				0,
				0,
			));
		}
	}
	state.show(app);
}

async fn start() -> Result<(), Box<dyn std::error::Error>> {
	let ring = log::init();
	let context = gpu::select().await?;
	let app = AppWindow::new()?;
	let live = Live::new(&context)?;
	let store = waveduderust_platform::store::Store::open();
	let facts = waveduderust_platform::machine::machine_facts();
	let settings = store.as_ref().map_or_else(Default::default, |s| {
		waveduderust_platform::settings::Settings::load(s, false, || facts).settings
	});
	let state = Rc::new(RefCell::new(State {
		doc: Document::new(UNDO_STEPS),
		nodes: Vec::new(),
		selected: Node::Project,
		live,
		playback: Playback::default(),
		messages: Messages::new(ring),
		pending: None,
		settings,
		store,
		settings_section: 0,
		facts,
		exporting: None,
		inbox: files::Inbox::default(),
		example_topic: 0,
		example_search: String::new(),
		example_rows: Vec::new(),
		last_export: None,
		ran: 0,
	}));
	apply_settings(&mut state.borrow_mut(), &app);
	state.borrow_mut().opened();
	if state.borrow().settings.show_startup && std::env::args_os().nth(1).is_none() {
		show_welcome(&state.borrow(), &app);
		app.set_welcome_shown(true);
	}
	#[cfg(not(target_arch = "wasm32"))]
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
	app.on_example_topic_picked(move |i| {
		if let Some(app) = weak.upgrade() {
			let s = &mut *st.borrow_mut();
			s.example_topic = usize::try_from(i).unwrap_or_default();
			show_examples(s, &app);
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_example_search(move |t| {
		if let Some(app) = weak.upgrade() {
			let s = &mut *st.borrow_mut();
			s.example_search = t.into();
			show_examples(s, &app);
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_example_open(move |row| {
		if let Some(app) = weak.upgrade() {
			let s = &mut *st.borrow_mut();
			if let Some(&i) = usize::try_from(row).ok().and_then(|r| s.example_rows.get(r)) {
				app.set_examples_shown(false);
				menu(s, &app, &format!("example:{}", examples::all()[i].file));
			}
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_welcome_preset(move |i| {
		if let Some(app) = weak.upgrade() {
			let s = &mut *st.borrow_mut();
			let facts = s.facts;
			settings::preset(&mut s.settings, s.store.as_ref(), usize::try_from(i).unwrap_or_default(), facts);
			apply_settings(s, &app);
			show_welcome(s, &app);
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_welcome_show_startup_toggled(move |on| {
		if let Some(app) = weak.upgrade() {
			let s = &mut *st.borrow_mut();
			s.settings.show_startup = on;
			if let Some(Err(e)) = s.store.as_ref().map(|store| s.settings.save(store)) {
				tracing::error!("{e}");
			}
			show_welcome(s, &app);
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_welcome_action(move |action| {
		if let Some(app) = weak.upgrade() {
			app.set_welcome_shown(false);
			let id = if action == "random" { "random-example" } else { action.as_str() };
			menu(&mut st.borrow_mut(), &app, id);
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_settings_pick(move |i| {
		if let Some(app) = weak.upgrade() {
			let s = &mut *st.borrow_mut();
			s.settings_section = usize::try_from(i).unwrap_or_default();
			show_settings(s, &app);
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_settings_set(move |row, value| {
		if let Some(app) = weak.upgrade() {
			let s = &mut *st.borrow_mut();
			let row = usize::try_from(row).unwrap_or(usize::MAX);
			settings::set_row(&mut s.settings, s.store.as_ref(), s.settings_section, row, &value);
			apply_settings(s, &app);
			show_settings(s, &app);
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_modal_answer(move |button| {
		if let Some(app) = weak.upgrade() {
			let s = &mut *st.borrow_mut();
			answer(s, &app, button);
			s.show(&app);
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.window().on_close_requested(move || {
		let Some(app) = weak.upgrade() else { return slint::CloseRequestResponse::HideWindow };
		let s = &mut *st.borrow_mut();
		if s.doc.dirty() {
			menu(s, &app, "exit");
			slint::CloseRequestResponse::KeepWindowShown
		} else {
			slint::CloseRequestResponse::HideWindow
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
			s.playback.tick(web_time::Instant::now(), s.doc.view.video.fps, shown.made);
			app.set_transport(transport_state(&s.playback, &shown));
			if let Some(e) = &mut s.exporting {
				match e.poll() {
					Ok(progress) => app.set_run_status(progress.into()),
					Err(outcome) => {
						s.exporting = None;
						match outcome {
							Ok(written) => tracing::info!("exported {written}"),
							Err(e) => {
								tracing::error!("{e}");
								app.set_modal(modal(
									ui::ModalKind::Error,
									"WaveDude Rust",
									"The export failed.",
									&["OK"],
									0,
									0,
								));
							}
						}
					}
				}
			}
			let picked: Vec<_> = s.inbox.borrow_mut().drain(..).collect();
			for (name, text) in picked {
				match s.doc.open_text(&name, &text) {
					Ok(()) => {
						s.opened();
						tracing::info!("Opened {name}");
					}
					Err(e) => {
						tracing::error!("cannot open {name}: {e}");
						app.set_modal(modal(
							ui::ModalKind::Error,
							"WaveDude Rust",
							&format!("Could not open '{name}'."),
							&["OK"],
							0,
							0,
						));
					}
				}
			}
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
	#[cfg(target_arch = "wasm32")]
	{
		// The browser's event loop outlives this function.
		std::mem::forget(timer);
		std::mem::forget(app.clone_strong());
	}
	app.run()?;
	Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
	pollster::block_on(start())
}

#[cfg(target_arch = "wasm32")]
fn main() {
	std::panic::set_hook(Box::new(|info| web_sys::console::error_1(&info.to_string().into())));
	wasm_bindgen_futures::spawn_local(async {
		if let Err(e) = start().await {
			web_sys::console::error_1(&format!("WaveDude Rust could not start: {e}").into());
		}
	});
}
