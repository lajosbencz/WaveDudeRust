//! WaveDude Rust's app: the Slint window over the core (decision 0032).

mod document;
mod export;
mod form;
mod gpu;
mod live;
mod log;
mod messages;
mod playback;
mod settings;
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
	/// The menu action the unsaved edits prompt holds.
	pending: Option<String>,
	settings: waveduderust_platform::settings::Settings,
	store: Option<waveduderust_platform::store::Store>,
	settings_section: usize,
	exporting: Option<export::Exporting>,
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
			if let Some(e) = &state.exporting {
				e.cancel();
			}
			state.live.stop();
			state.playback.live = false;
		}
		"new" | "open" | "close" | "exit" if state.doc.dirty() => {
			state.pending = Some(id.to_owned());
			let name = state.doc.path.as_deref().and_then(std::path::Path::file_name).map_or_else(|| "Untitled".into(), |n| n.to_string_lossy().into_owned());
			app.set_modal(modal(ui::ModalKind::Question, "WaveDude Rust", &format!("Save the changes to {name}?"), &["Save", "Don't save", "Cancel"], 0, 2));
		}
		"new" | "close" => {
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
			state.doc = Document::new(UNDO_STEPS);
			let _ = app.hide();
		}
		other => tracing::info!("menu: {other}"),
	}
	state.show(app);
}

/// Puts `path` first in the recent files and saves the settings.
fn remember(state: &mut State, path: &std::path::Path) {
	state.settings.push_recent(path);
	if let Some(Err(e)) = state.store.as_ref().map(|s| state.settings.save(s)) {
		tracing::error!("{e}");
	}
}

/// Applies the settings to the window, the history and the run.
fn apply_settings(state: &mut State, app: &AppWindow) {
	let s = &state.settings;
	app.global::<ui::Theme<'_>>().set_mode(settings::theme_mode(s));
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
fn save(state: &mut State, app: &AppWindow, pick: bool) -> bool {
	let Some(path) = state
		.doc
		.path
		.clone()
		.filter(|_| !pick)
		.or_else(|| project_dialog().set_file_name("Untitled.wdproj").save_file())
	else {
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
			remember(state, &path);
			tracing::info!("opened {}", path.display());
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let ring = log::init();
	let (device, queue) = gpu::select()?;
	let app = AppWindow::new()?;
	let live = Live::new(device, queue)?;
	let store = waveduderust_platform::settings::AppDirs::platform().map(|d| d.store());
	let settings = store.as_ref().map_or_else(Default::default, |s| {
		waveduderust_platform::settings::Settings::load(s, false, || {
			waveduderust::run::machine_facts(&waveduderust::run::FrameStore::default_dir())
		})
		.settings
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
		exporting: None,
		last_export: None,
		ran: 0,
	}));
	apply_settings(&mut state.borrow_mut(), &app);
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
			settings::set(&mut s.settings, s.store.as_ref(), s.settings_section, row, &value);
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
			s.playback.tick(std::time::Instant::now(), s.doc.view.video.fps, shown.made);
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
