//! WaveDude Rust's app: the Slint window over the core (decision 0032).

mod canvas;
mod clipboard;
#[cfg(not(target_arch = "wasm32"))]
mod crash;
mod docking;
mod document;
mod examples;
mod export;
mod files;
mod form;
mod gpu;
mod graphs;
mod icons;
mod live;
mod log;
mod memory;
mod messages;
mod models;
mod number;
mod playback;
#[cfg(not(target_arch = "wasm32"))]
mod recovery;
mod scene_watch;
mod selection;
mod settings;
mod state;
mod tree;
mod wake;
#[cfg(target_arch = "wasm32")]
mod web;
mod wire;

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
use crate::selection::Selection;
use crate::state::{Shown, State};
use crate::tree::Node;
use crate::ui::{Command, TransportAction};
use crate::wire::Handle;

/// Undo steps until the settings arrive (decision 0030).
const UNDO_STEPS: usize = 1000;

/// Moves the current row `by` places in its list, the selection following it.
fn shift(state: &mut State, by: isize) {
	let (node, mut to) = (state.selection.current(), state.selection.current());
	state.doc.edit(|p| to = tree::shift(p, node, by));
	state.selection.set(to);
}

/// What the user asked of the app: a command, or opening a file or an example, which unsaved edits may hold up.
#[derive(Clone, Debug)]
enum Action {
	Command(Command),
	#[cfg_attr(target_arch = "wasm32", allow(dead_code, reason = "no recent files in the browser"))]
	OpenFile(PathBuf),
	/// An example by its index in `examples::all()`.
	Example(usize),
}

impl Action {
	/// Whether it replaces the project, so asks about unsaved edits first.
	fn replaces(&self) -> bool {
		matches!(
			self,
			Action::Command(Command::New | Command::Open | Command::Close | Command::Exit)
				| Action::OpenFile(_)
				| Action::Example(_)
		)
	}
}

/// Appends `item` at the centre of the canvas and selects it.
fn add(state: &mut State, item: ui::NewItem) {
	let (centre, mut to) = (state.doc.project.canvas_centre(), Node::Project);
	state.doc.edit(|p| to = tree::add(p, item, centre));
	state.selection.set(to);
}

/// Runs a menu entry's, shortcut's or button's command (docs/ui/10).
fn command(state: &mut State, app: &AppWindow, command: Command) {
	act(state, app, Action::Command(command));
}

fn act(state: &mut State, app: &AppWindow, action: Action) {
	if matches!(action, Action::Command(Command::Exit)) && state.exporting.is_some() {
		state.pending = Some(action);
		tell(app, ui::Message::ExportRunning, "");
		return;
	}
	// the export reads the run of this project (decision 0012)
	if action.replaces() && state.exporting.is_some() {
		tracing::warn!("the export holds the project: stop it first");
		return;
	}
	if action.replaces() && state.doc.dirty() {
		state.pending = Some(action);
		let name = state.doc.name().unwrap_or_else(|| app.global::<ui::Words<'_>>().get_untitled().into());
		tell(app, ui::Message::SaveChanges, &name);
		state.show(app);
		return;
	}
	let command = match action {
		Action::Command(command) => command,
		#[cfg(not(target_arch = "wasm32"))]
		Action::OpenFile(path) => {
			open(state, app, path);
			return state.show(app);
		}
		#[cfg(target_arch = "wasm32")]
		Action::OpenFile(_) => return,
		Action::Example(i) => {
			let e = &examples::all()[i];
			match waveduderust::io::wdproj::read_str(e.text) {
				Ok((project, view)) => {
					state.doc.adopt(project, view, &format!("{}.wdproj", e.title));
					state.opened();
					tracing::info!("Opened the example {}", e.title);
				}
				Err(err) => tracing::error!("the example {} does not parse: {err}", e.file),
			}
			return state.show(app);
		}
	};
	match command {
		Command::Run => {
			// F5 reaches here past a disabled menu entry
			if !state.can_run(state.exporting.is_some()) {
				return;
			}
			if let Some(e) = state.start_error() {
				return tracing::error!("{e}");
			}
			if state.live.run(&state.doc.project, &state.doc.view) {
				state.playback.frames = state.live.frames;
				state.playback.follow();
			}
		}
		Command::Pause => state.live.toggle_pause(),
		Command::Discard if state.commands().can_discard => tell(app, ui::Message::DiscardFrames, ""),
		Command::Discard => {}
		// the menu, Shift+F5 and Esc: a running export first, else the run
		Command::Stop => {
			if let Some(e) = &state.exporting {
				e.cancel();
				if e.waits().is_some() {
					tracing::info!("Export cancelled before its first frame");
					release_export(state, app);
				}
			} else {
				state.live.stop();
				state.playback.live = false;
			}
		}
		Command::Welcome => {
			show_settings(state, app);
			app.set_welcome_shown(true);
		}
		Command::RandomExample => {
			let nanos =
				web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
			return act(state, app, Action::Example(nanos as usize % examples::all().len()));
		}
		Command::Examples => {
			show_examples(state, app);
			app.set_examples_shown(true);
		}
		Command::New | Command::Close => {
			state.playback.reset();
			let generation = state.doc.generation + 1;
			state.doc = Document::new(state.settings.undo_steps as usize);
			state.doc.generation = generation;
			state.opened();
		}
		Command::Open => {
			#[cfg(not(target_arch = "wasm32"))]
			if let Some(path) = files::dialog(&app.global::<ui::Words<'_>>()).pick_file() {
				open(state, app, path);
			}
			#[cfg(target_arch = "wasm32")]
			files::pick(&state.inbox, &app.global::<ui::Words<'_>>());
		}
		Command::Github => {
			if let Err(e) = webbrowser::open(env!("CARGO_PKG_REPOSITORY")) {
				tracing::error!("cannot open the project page: {e}");
			}
		}
		Command::Save | Command::SaveAs => {
			save(state, app, if command == Command::SaveAs { SaveTo::Picked } else { SaveTo::File });
		}
		Command::ExportField
		| Command::ExportIntensity
		| Command::ExportBoth
		| Command::ExportFieldFrame
		| Command::ExportIntensityFrame
		| Command::ExportLast => {
			if state.exporting.is_some() {
				return;
			}
			match export::Kind::of(command) {
				Some(kind) => {
					let words = app.global::<ui::Words<'_>>();
					let name = state.doc.name().map_or_else(
						|| words.get_export_file().to_string(),
						|n| {
							std::path::Path::new(&n).file_stem().map_or(n.clone(), |s| s.to_string_lossy().into_owned())
						},
					);
					let opened =
						export::Dialog::open(kind, &state.doc, &name, state.playback.position, &state.settings, &words);
					if let Some((dialog, data)) = opened {
						state.export_dialog = Some(dialog);
						app.set_export_data(data);
						app.set_export_shown(true);
					}
				}
				// Export last repeats the outputs and asks nothing
				None => {
					if let Some(plan) = state.last_export.clone() {
						begin_export(state, app, &plan);
					}
				}
			}
		}
		Command::Settings => {
			show_settings(state, app);
			app.set_settings_shown(true);
		}
		Command::About => {
			app.set_about(about(state));
			app.set_about_shown(true);
		}
		Command::Undo => {
			state.doc.undo();
			state.selection.collapse();
		}
		Command::Redo => {
			state.doc.redo();
			state.selection.collapse();
		}
		Command::Copy | Command::Cut => {
			let group = state.selection.group();
			let copied = clipboard::copy(&state.doc.project, &state.doc.view, &group)
				.map(|f| clipboard::write(waveduderust::io::wdproj::write_fragment(&f)));
			match copied {
				Some(Ok(())) if command == Command::Cut && state.commands().can_delete => remove(state, &group),
				Some(Err(e)) => tracing::error!("clipboard: {e}"),
				_ => {}
			}
		}
		Command::Paste => {
			clipboard::read(&state.pasted);
			paste(state);
		}
		Command::Delete if state.commands().can_delete => remove(state, &state.selection.group()),
		Command::MoveUp if state.commands().can_move_up => shift(state, -1),
		Command::MoveDown if state.commands().can_move_down => shift(state, 1),
		Command::ToggleMine => state.toggle_mine(),
		Command::ExportProbeCsv => {
			if let Node::Probe(i) = state.selection.current() {
				export_probe_csv(state, app, i);
			}
		}
		Command::Duplicate if state.commands().can_delete => {
			let (group, mut copies) = (state.selection.group(), Vec::new());
			state.doc.edit(|p| copies = tree::duplicate_all(p, &group));
			if !copies.is_empty() {
				state.selection.set_group(copies);
			}
		}
		Command::ToggleEnabled if state.commands().can_toggle => {
			let group = state.selection.group();
			state.doc.edit(|p| group.iter().for_each(|&n| tree::toggle(p, n)));
		}
		Command::ToggleSelected => {
			let node = state.selection.current();
			state.selection.toggle(node);
		}
		Command::SelectAll => state.selection.select_all(&state.doc.project),
		Command::Exit => {
			#[cfg(not(target_arch = "wasm32"))]
			keep_window(state, app);
			let _ = app.hide();
		}
		// unavailable here
		Command::Delete | Command::MoveUp | Command::MoveDown | Command::Duplicate | Command::ToggleEnabled => {}
	}
	state.show(app);
}

/// Removes the items of `group` (list items only) and keeps the user at the slot of the last one.
fn remove(state: &mut State, group: &[Node]) {
	let items: Vec<Node> = group.iter().copied().filter(|&n| tree::is_item(n)).collect();
	let mut to = None;
	state.doc.edit(|p| to = tree::remove_all(p, &items));
	if let Some(to) = to {
		state.selection.set(to);
	}
}

/// Pastes the clipboard text read for it; text that holds no objects is logged.
fn paste(state: &mut State) {
	let texts = state.pasted.take();
	for text in texts {
		match waveduderust::io::wdproj::read_fragment(&text) {
			Ok(fragment) if !fragment.is_empty() => {
				let current = state.selection.current();
				let mut last = None;
				state.doc.edit_both(None, |p, v| last = clipboard::paste(p, v, fragment, current));
				state.selection.set(last.unwrap_or(current));
			}
			_ => tracing::info!("the clipboard holds no project objects"),
		}
	}
}

/// Puts the window where it was left: size and corner in logical pixels, maximized.
#[cfg(not(target_arch = "wasm32"))]
fn place_window(settings: &waveduderust_platform::settings::Settings, app: &AppWindow) {
	let w = app.window();
	w.set_size(slint::LogicalSize::new(
		settings.window_width.max(640.) as f32,
		settings.window_height.max(400.) as f32,
	));
	if let (Some(x), Some(y)) = (settings.window_left, settings.window_top) {
		w.set_position(slint::LogicalPosition::new(x as f32, y as f32));
	}
	w.set_maximized(settings.window_state == waveduderust_platform::settings::WindowState::Maximized);
}

/// Saves the window's size, corner and state; a maximized window keeps its normal size and corner.
#[cfg(not(target_arch = "wasm32"))]
fn keep_window(state: &mut State, app: &AppWindow) {
	use waveduderust_platform::settings::WindowState;
	let w = app.window();
	let s = &mut state.settings;
	if w.is_minimized() {
		// where it was before is unknown: the saved place stays
	} else if w.is_maximized() {
		s.window_state = WindowState::Maximized;
	} else {
		let scale = w.scale_factor();
		let (size, at) = (w.size().to_logical(scale), w.position().to_logical(scale));
		s.window_state = WindowState::Normal;
		(s.window_width, s.window_height) = (f64::from(size.width), f64::from(size.height));
		(s.window_left, s.window_top) = (Some(f64::from(at.x)), Some(f64::from(at.y)));
	}
	state.save_settings();
}

/// Puts `path` first in the recent files and saves the settings.
#[cfg(not(target_arch = "wasm32"))]
fn remember(state: &mut State, app: &AppWindow, path: &std::path::Path) {
	state.settings.push_recent(path);
	state.save_settings();
	show_recent(&state.settings, app);
}

/// Pushes the recent files into File > Open recent.
fn show_recent(settings: &waveduderust_platform::settings::Settings, app: &AppWindow) {
	// the native menus (muda) read `&` as an access key on Windows and drop it on macOS
	let native = cfg!(any(windows, target_os = "macos"));
	let recent: Vec<slint::SharedString> = settings
		.recent_files
		.iter()
		.map(|p| p.display().to_string())
		.map(|p| if native { p.replace('&', "&&") } else { p }.into())
		.collect();
	app.set_recent_files(ModelRc::from(recent.as_slice()));
}

/// Pushes the Examples dialog's topics and the rows of the topic, filtered by the search.
fn show_examples(state: &mut State, app: &AppWindow) {
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
	app.set_example_topic(state.example_topic);
	app.set_example_rows(ModelRc::new(VecModel::from(rows)));
}

/// A file dropped on the window: an image becomes the scene image, a project opens (after the unsaved check).
fn dropped(state: &mut State, app: &AppWindow, path: PathBuf) {
	let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
	if waveduderust::io::image_extensions().contains(&ext.as_str()) {
		adopt_image(state, &path);
	} else if ["wdproj", "xml", "json"].contains(&ext.as_str()) {
		act(state, app, Action::OpenFile(path));
	} else {
		tracing::warn!("cannot open {}: not a project or an image", path.display());
	}
}

/// Embeds the image at `path` as the scene image, an image scene, one undo step.
fn adopt_image(state: &mut State, path: &std::path::Path) {
	let bytes = match std::fs::read(path) {
		Ok(b) => b,
		Err(e) => return tracing::error!("cannot read {}: {e}", path.display()),
	};
	let size = match waveduderust::io::image_size(&bytes) {
		Ok(s) => s,
		Err(e) => return tracing::error!("cannot read {}: {e}", path.display()),
	};
	let folder = state.doc.path.as_deref().and_then(std::path::Path::parent);
	let stored = waveduderust::io::stored_image_path(folder, &path.to_string_lossy());
	let name = path.file_name().map_or_default(|n| n.to_string_lossy().into_owned());
	state.doc.edit(|p| {
		let s = &mut p.scene;
		s.mode = waveduderust::model::SceneMode::Image;
		(s.image, s.image_path) = (name.clone(), stored);
		(s.image_data, s.image_size) = (Some(bytes.into()), size);
		(s.width, s.height) = size;
	});
	tracing::info!("Scene image {name}");
}

/// Embeds `bytes` as the scene image, the canvas taking its size.
fn embed(state: &mut State, bytes: std::sync::Arc<[u8]>, size: (u32, u32)) {
	state.doc.edit(|p| {
		let s = &mut p.scene;
		(s.image_data, s.image_size) = (Some(bytes), size);
		(s.width, s.height) = size;
	});
}

/// Follows the scene image file (docs/ui/60, Files and drop): asks while no other question shows.
fn watch_scene_image(state: &mut State, app: &AppWindow) {
	use scene_watch::Event;
	let folder = state.doc.path.as_deref().and_then(std::path::Path::parent);
	match state.scene_watch.tick(&state.doc.project.scene, folder) {
		Some(Event::Withdraw) if app.get_modal().message == ui::Message::SceneImage => {
			app.set_modal(ui::Modal::default());
		}
		Some(Event::Take(bytes, size)) => {
			state.doc.settle(|p| (p.scene.image_data, p.scene.image_size) = (Some(bytes.clone()), size));
			state.show(app);
		}
		_ => {}
	}
	if state.scene_watch.asking() && app.get_modal().message == ui::Message::None {
		tell(app, ui::Message::SceneImage, &state.doc.project.scene.image);
	}
}

/// Sets the palette's scheme for a forced theme, or back to the desktop's after one; untouched, it follows the desktop.
fn apply_theme(state: &mut State, app: &AppWindow) {
	let palette = app.global::<ui::Palette<'_>>();
	match (settings::color_scheme(&state.settings), state.system_scheme) {
		(Some(forced), system) => {
			state.system_scheme = system.or_else(|| Some(palette.get_color_scheme()));
			palette.set_color_scheme(forced);
		}
		(None, Some(system)) => palette.set_color_scheme(system),
		(None, None) => {}
	}
}

/// The About window's version, licences and system lines.
fn about(state: &State) -> ui::AboutData {
	let threads = state.facts.threads;
	let gpu = match (state.live.has_gpu(), state.settings.use_gpu) {
		(false, _) => "none",
		(true, true) => "on",
		(true, false) => "off",
	};
	#[cfg(not(target_arch = "wasm32"))]
	let h264 = {
		let enabled = waveduderust_platform::codec::enabled(state.settings.use_openh264);
		let installed = waveduderust_platform::settings::AppDirs::platform()
			.and_then(|d| waveduderust_platform::codec::installed(&d.codec()))
			.is_some();
		match (enabled, installed) {
			(false, _) => "not enabled",
			(true, true) => "Cisco's OpenH264 2.6.0",
			(true, false) => "enabled, not downloaded",
		}
	};
	#[cfg(target_arch = "wasm32")]
	let h264 = "the browser's (WebCodecs)";
	let system = [
		format!("Program: WaveDude Rust {}", env!("CARGO_PKG_VERSION")),
		"Runtime: Rust, Slint".to_owned(),
		format!("Operating system: {} {}", std::env::consts::OS, std::env::consts::ARCH),
		format!("Processor: {threads} logical cores"),
		format!("GPU setting: {gpu}"),
		format!("Engine: {}", state.live.engine()),
		format!("H.264 encoder: {h264}"),
	]
	.join("\n");
	ui::AboutData {
		version: env!("CARGO_PKG_VERSION").into(),
		license: include_str!("../../../LICENSE.md").into(),
		openh264: include_str!("../../../crates/waveduderust/assets/OpenH264-BINARY_LICENSE.txt").into(),
		system: system.into(),
	}
}

/// The memory warning answered: Run applies the ticks to the settings and runs, Cancel leaves the frames as they
/// are and turns playback off; `mute` clears the setting.
fn answer_warning(state: &mut State, app: &AppWindow, answer: ui::WarningAnswer, mute: bool) {
	let Some(ticks) = state.warning_ticks.take() else { return };
	let quality = state.live.warning().map(|(_, w)| w.quality_of(ticks));
	let Some(ask) = state.live.take_ask() else { return };
	app.set_memory_warning_shown(false);
	if mute {
		state.settings.warn_when_ram_insufficient = false;
	}
	if answer == ui::WarningAnswer::Run {
		if let Some(q) = quality {
			state.settings.set_preview_quality(q);
		}
		apply_settings(state, app);
		// the new quality's re-run may ask again: this answer goes for it
		state.live.take_ask();
		state.live.go(ask.then);
		if ask.then == live::Then::Run {
			state.playback.frames = state.live.frames;
			state.playback.follow();
		}
	} else {
		state.live.warn = state.settings.warn_when_ram_insufficient;
		state.playback.pause();
		tracing::info!("Run cancelled: the frames exceed the RAM cache; the frames made stay");
	}
	state.save_settings();
	show_settings(state, app);
}

/// Applies the settings to the window, the history and the run.
fn apply_settings(state: &mut State, app: &AppWindow) {
	apply_theme(state, app);
	if let Err(e) = slint::select_bundled_translation(&settings::language(&state.settings)) {
		tracing::warn!("language: {e}");
	}
	let s = &state.settings;
	show_recent(s, app);
	app.set_title_full_path(s.title_full_path);
	app.set_preview_stacked(s.preview_orientation == waveduderust_platform::settings::Orientation::Vertical);
	app.global::<ui::Form<'_>>().set_described(s.part_description_expanded);
	state.doc.set_limit(s.undo_steps as usize);
	state.live.set_quality(s.preview_quality());
	state.live.auto_rerun = s.auto_rerun;
	state.live.warn = s.warn_when_ram_insufficient;
	state.live.set_dither(s.preview_dither_bits);
	// an export reads the run's frames: the run swaps once it ended
	if state.exporting.is_none() && state.live.set_gpu(s.use_gpu) {
		frames_dropped(&mut state.playback);
	}
	state.live.ram_cache = s.ram_cache();
	// another preview quality is a simulation edit
	if state.live.edited(&state.doc.project, &state.doc.view) == live::Change::Frames {
		state.playback.pause();
	}
}

/// The run's frames went and it starts over at frame 0: playback stops there.
fn frames_dropped(playback: &mut Playback) {
	playback.pause();
	playback.seek(0);
}

/// The memory bar; the RAM cache is the budget a run starting now gets.
fn ram_bar(state: &State) -> ui::RamBar {
	use waveduderust::run::Budget;
	#[cfg(not(target_arch = "wasm32"))]
	{
		let (available, physical) = waveduderust::run::Memory::default().read();
		settings::ram_bar(Some(available), physical, Budget::of(available, physical, state.settings.ram_cache(), 0))
	}
	#[cfg(target_arch = "wasm32")]
	settings::ram_bar(None, state.facts.physical_bytes, Budget::of_memory(web::BUDGET, state.settings.ram_cache()))
}

/// Pushes the settings to the Settings window and the Welcome page.
fn show_settings(state: &State, app: &AppWindow) {
	let data = settings::data(&state.settings);
	app.set_ram(ram_bar(state));
	app.set_settings(data);
	app.set_undo_spec(settings::undo_spec());
	app.set_welcome_levers(settings::levers(&state.settings));
	app.set_welcome_show_startup(state.settings.show_startup);
}

/// The project's file name, an untitled one's in the UI's language.
fn file_name(state: &State, app: &AppWindow) -> String {
	state.doc.name().unwrap_or_else(|| format!("{}.wdproj", app.global::<ui::Words<'_>>().get_untitled()))
}

/// Starts the export of `plan` from the live run, or waits for the run it needs (decision 0012).
fn begin_export(state: &mut State, app: &AppWindow, plan: &export::Plan) {
	app.set_export_shown(false);
	state.export_dialog = None;
	let needs = plan.needs(&state.doc.view);
	match state.live.request_export(&state.doc.project, &state.doc.view, needs) {
		live::Gate::Ready => write_export(state, plan, state.playback.position),
		live::Gate::Waiting => state.exporting = Some(export::Exporting::waiting(plan, state.playback.position)),
		live::Gate::Busy => return,
	}
	state.export_plan = Some(plan.clone());
	state.show(app);
}

/// Writes `plan` from the run the export holds, a still of the timeline frame `position`.
fn write_export(state: &mut State, plan: &export::Plan, position: u32) {
	let Some(frames) = state.live.export_frames() else {
		tracing::error!("the export has no frames to read");
		state.live.end_export(&state.doc.project, &state.doc.view);
		state.exporting = None;
		return;
	};
	let s = &*state;
	state.exporting = Some(export::start(plan, frames, &s.doc.project, &s.doc.view, position, &s.settings, &s.schemes));
}

/// The export ended or was stopped: the run it held goes on with the edits made meanwhile.
fn release_export(state: &mut State, app: &AppWindow) {
	state.exporting = None;
	app.set_export_progress(ui::ExportProgress::default());
	if state.live.end_export(&state.doc.project, &state.doc.view) == live::Change::Frames {
		state.playback.pause();
	}
	if state.live.set_gpu(state.settings.use_gpu) {
		frames_dropped(&mut state.playback);
	}
}

/// A completed export: where it wrote is remembered, a run's is what Export last repeats.
fn export_completed(state: &mut State) {
	let Some(plan) = state.export_plan.take() else { return };
	export::remember(&plan, state.doc.path.as_deref(), &mut state.settings);
	state.save_settings();
	if matches!(plan.kind, export::Kind::Run(_)) {
		state.last_export = Some(plan);
	}
}

/// Asks about the next output the export would replace, or starts it when none is left.
fn ask_replace(state: &mut State, app: &AppWindow) {
	let Some((plan, outputs)) = state.replacing.as_mut() else { return };
	match outputs.pop() {
		Some((path, folder)) => {
			let message = if folder { ui::Message::ReplaceThem } else { ui::Message::ReplaceIt };
			tell(app, message, &path.display().to_string());
		}
		None => {
			let plan = plan.clone();
			state.replacing = None;
			begin_export(state, app, &plan);
		}
	}
}

/// Shows `message`, `arg` the value its text names.
fn tell(app: &AppWindow, message: ui::Message, arg: &str) {
	app.set_modal(ui::Modal { message, arg: arg.into() });
}

/// Where Save writes: the project's file, asked for when it has none, or a file asked for (Save As).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SaveTo {
	File,
	Picked,
}

/// Saves the project, to a picked file for [`SaveTo::Picked`] or without a file; `false` when cancelled or failed.
#[cfg(target_arch = "wasm32")]
fn save(state: &mut State, app: &AppWindow, _: SaveTo) -> bool {
	let name = file_name(state, app);
	match files::download(&name, &state.doc.text(), "application/xml") {
		Ok(()) => {
			state.doc.mark_saved();
			tracing::info!("saved {name} as a download");
			true
		}
		Err(e) => {
			tracing::error!("cannot save {name}: {e}");
			tell(app, ui::Message::SaveFailed, &name);
			false
		}
	}
}

#[cfg(not(target_arch = "wasm32"))]
fn save(state: &mut State, app: &AppWindow, to: SaveTo) -> bool {
	let Some(path) = state
		.doc
		.path
		.clone()
		.filter(|_| to == SaveTo::File)
		.or_else(|| files::dialog(&app.global::<ui::Words<'_>>()).set_file_name(file_name(state, app)).save_file())
	else {
		return false;
	};
	match state.doc.save_as(&path) {
		Ok(()) => {
			tracing::info!("saved {}", path.display());
			remember(state, app, &path);
			true
		}
		Err(e) => {
			tracing::error!("cannot save {}: {e}", path.display());
			tell(app, ui::Message::SaveFailed, &path.display().to_string());
			false
		}
	}
}

/// The answer to the message window: for the unsaved edits prompt, Save, Don't save or Cancel.
fn answer(state: &mut State, app: &AppWindow, answer: ui::Answer) {
	let modal = app.get_modal();
	let message = modal.message;
	app.set_modal(ui::Modal::default());
	match (message, answer) {
		(ui::Message::ReplaceIt | ui::Message::ReplaceThem, ui::Answer::Yes) => ask_replace(state, app),
		(ui::Message::ReplaceIt | ui::Message::ReplaceThem, _) => state.replacing = None,
		(ui::Message::Crashed, ui::Answer::Copy) => {
			if let Err(e) = clipboard::write(modal.arg.to_string()) {
				tracing::error!("{e}");
			}
			// the window stays
			app.set_modal(modal);
		}
		(ui::Message::SceneImage, answer) => {
			if let Some((bytes, size)) = state.scene_watch.answer(answer == ui::Answer::Yes) {
				embed(state, bytes, size);
			}
		}
		(ui::Message::DiscardFrames, ui::Answer::Yes) if state.commands().can_discard => {
			state.live.discard();
			frames_dropped(&mut state.playback);
			tracing::info!("Frames discarded");
		}
		(ui::Message::ExportRunning, ui::Answer::Yes) => {
			if let Some(e) = state.exporting.as_mut() {
				// the run is held until the writer let go of it
				e.stop();
				release_export(state, app);
			}
			// on to the unsaved changes, then out
			if let Some(then) = state.pending.take() {
				act(state, app, then);
			}
			return;
		}
		(ui::Message::ExportRunning, _) => {
			state.pending = None;
			return;
		}
		// the click's own pass: a browser may drop a download a page starts without one (decision 0025)
		#[cfg(target_arch = "wasm32")]
		(ui::Message::ExportReady, answer) => {
			if let Some(download) = state.ready.take().filter(|_| answer == ui::Answer::Download)
				&& let Err(e) = download.start()
			{
				tracing::error!("cannot download {}: {e}", download.name());
			}
		}
		_ => {}
	}
	if let Some(then) = state.pending.take() {
		let go = match answer {
			ui::Answer::Save => save(state, app, SaveTo::File),
			ui::Answer::Discard => true,
			ui::Answer::Ok
			| ui::Answer::Cancel
			| ui::Answer::Yes
			| ui::Answer::No
			| ui::Answer::Copy
			| ui::Answer::Download => false,
		};
		if go {
			state.doc.mark_saved();
			act(state, app, then);
		}
	}
}

/// A transport bar or timeline action (docs/ui/50).
fn transport(state: &mut State, app: &AppWindow, action: TransportAction) {
	// an empty project has no frames to move through; the views stay switchable
	if !state.doc.project.has_enabled_source()
		&& !matches!(
			action,
			TransportAction::ShowField
				| TransportAction::ShowIntensity
				| TransportAction::Layout
				| TransportAction::PauseRun
				| TransportAction::ClearRange
		) {
		return;
	}
	let p = &mut state.playback;
	match action {
		TransportAction::Play if p.playing => p.pause(),
		TransportAction::Play => {
			p.play();
			match state.start_error() {
				Some(e) => tracing::error!("{e}"),
				None => {
					state.live.fill();
				}
			}
		}
		TransportAction::Prev => p.seek(i64::from(p.position) - 1),
		TransportAction::Next => p.seek(i64::from(p.position) + 1),
		TransportAction::SetIn => p.set_in(),
		TransportAction::SetOut => p.set_out(),
		TransportAction::ClearRange => (p.start, p.end) = (None, None),
		TransportAction::Loop => p.looping ^= true,
		TransportAction::Live => p.live ^= state.live.active(),
		TransportAction::PauseRun => state.live.toggle_pause(),
		TransportAction::ShowField | TransportAction::ShowIntensity => {
			use waveduderust::model::Layer;
			let layer = if action == TransportAction::ShowField { Layer::Field } else { Layer::Intensity };
			state.doc.edit_view_merging(None, |v| v.layer_mut(layer).preview ^= true);
			state.show(app);
		}
		TransportAction::Layout => {
			use waveduderust_platform::settings::Orientation;
			// the pictures are made anew: a gesture on them gets no release
			state.finish_gesture(app);
			state.settings.preview_orientation =
				if app.get_preview_stacked() { Orientation::Vertical } else { Orientation::Horizontal };
			state.save_settings();
		}
	}
}

fn transport_state(
	p: &Playback,
	shown: &live::Shown,
	view: &waveduderust::model::View,
	paused: bool,
	empty: bool,
) -> ui::Transport {
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
		paused,
		empty,
		step: shown.step.map_or_else(Default::default, |s| format!("step {s}").into()),
		field_shown: view.field.preview,
		intensity_shown: view.intensity.preview,
	}
}

#[cfg(not(target_arch = "wasm32"))]
fn open(state: &mut State, app: &AppWindow, path: PathBuf) {
	match state.doc.open(&path) {
		Ok(()) => {
			state.opened();
			remember(state, app, &path);
			tracing::info!("Opened {}", path.display());
		}
		Err(e) => {
			tracing::error!("cannot open {}: {e}", path.display());
			tell(app, ui::Message::OpenFailed, &path.display().to_string());
		}
	}
	state.show(app);
}

async fn start() -> Result<(), Box<dyn std::error::Error>> {
	let ring = log::init();
	let context = gpu::select().await?;
	let app = AppWindow::new()?;
	wire::wire_widgets(&app);
	let live = Live::new(context.as_ref())?;
	let store = waveduderust_platform::store::Store::open();
	let facts = waveduderust_platform::machine::machine_facts();
	let settings = store.as_ref().map_or_else(Default::default, |s| {
		waveduderust_platform::settings::Settings::load(s, false, || facts).settings
	});
	let state = Rc::new(RefCell::new(State {
		doc: Document::new(UNDO_STEPS),
		nodes: Vec::new(),
		tree_rows: Rc::default(),
		selection: Selection::default(),
		collapsed: Vec::new(),
		live,
		playback: Playback::default(),
		messages: Messages::new(ring),
		pending: None,
		settings,
		schemes: store.as_ref().map_or_else(Default::default, waveduderust_platform::user_schemes::load),
		store,
		facts,
		exporting: None,
		inbox: files::Inbox::default(),
		pasted: clipboard::Pasted::default(),
		scene_watch: scene_watch::SceneWatch::default(),
		example_topic: ui::ExampleTopic::default(),
		example_search: String::new(),
		example_rows: Vec::new(),
		last_export: None,
		export_dialog: None,
		export_plan: None,
		#[cfg(target_arch = "wasm32")]
		ready: None,
		focus_seen: false,
		unsaved: Rc::default(),
		replacing: None,
		ran: 0,
		styled: 0,
		pictures: (true, true),
		warning_ticks: None,
		graphs: graphs::Graphs::default(),
		system_scheme: None,
		form_seen: (0, 0),
		form_base: 0,
		typed_form: Shown::default(),
		overlays: canvas::Overlays::default(),
		gesture: None,
		drags: 0,
	}));
	app.set_tree_rows(ModelRc::from(state.borrow().tree_rows.clone()));
	app.set_native_access_keys(cfg!(windows));
	apply_settings(&mut state.borrow_mut(), &app);
	wire::wire_window_events(&state, &app);
	let layout = state.borrow().settings.layout.clone();
	let keeper = Rc::downgrade(&state);
	docking::wire(&app, layout.as_deref(), move |layout| {
		let Some(state) = keeper.upgrade() else { return };
		let s = &mut *state.borrow_mut();
		s.settings.layout = Some(layout);
		s.save_settings();
	});
	#[cfg(target_arch = "wasm32")]
	web::guard_unload(state.borrow().unsaved.clone());
	state.borrow_mut().opened();
	// a crash report comes first; the welcome page waits for the next start
	#[cfg(not(target_arch = "wasm32"))]
	let crashed = waveduderust_platform::settings::AppDirs::platform().and_then(|d| crash::take(&d.config));
	#[cfg(target_arch = "wasm32")]
	let crashed: Option<PathBuf> = None;
	if let Some(path) = &crashed {
		tell(&app, ui::Message::Crashed, &path.display().to_string());
	}
	if crashed.is_none() && state.borrow().settings.show_startup && std::env::args_os().nth(1).is_none() {
		show_settings(&state.borrow(), &app);
		app.set_welcome_shown(true);
	}
	#[cfg(not(target_arch = "wasm32"))]
	if let Some(path) = std::env::args_os().nth(1) {
		open(&mut state.borrow_mut(), &app, path.into());
	}
	#[cfg(not(target_arch = "wasm32"))]
	recovery::recover(&mut state.borrow_mut());
	state.borrow_mut().show(&app);

	wire::wire_tree(&state, &app);
	wire::wire_form(&state, &app);
	wire::wire_canvas(&state, &app);
	wire::wire_menu(&state, &app);
	wire::wire_dialogs(&state, &app);
	wire::wire_transport(&state, &app);
	wire::wire_log(&state, &app);
	wire::wire_export(&state, &app);

	let h = Handle::new(&state, &app);
	wake::set(ticker::wake);
	ticker::start(move || h.with(tick));
	// The browser's event loop outlives this function.
	#[cfg(target_arch = "wasm32")]
	std::mem::forget(app.clone_strong());
	#[cfg(not(target_arch = "wasm32"))]
	place_window(&state.borrow().settings, &app);
	let ran = app.run();
	#[cfg(not(target_arch = "wasm32"))]
	ticker::end();
	#[cfg(not(target_arch = "wasm32"))]
	if let Err(e) = &ran
		&& let Some(context) = &context
		&& gpu::lost(context)
	{
		return recovery::restart_after_loss(&mut state.borrow_mut(), e);
	}
	ran?;
	Ok(())
}

/// One tick of the window: the run's news, playback, the export, the inboxes; whether something still moves.
fn tick(s: &mut State, app: &AppWindow) -> bool {
	s.live.seek(s.playback.position);
	let shown = s.live.poll(&s.doc.project, &s.doc.view, &s.schemes);
	s.playback.frames = s.live.frames.max(s.playback.frames);
	if s.playback.live && !shown.running && shown.made.is_some_and(|m| m + 1 >= s.playback.frames) {
		s.playback.position = shown.made.unwrap_or_default();
		s.playback.live = false;
	}
	s.playback.tick(web_time::Instant::now(), s.doc.view.video.fps, shown.made);
	s.live.prefetch(|count| s.playback.ahead(count, shown.made));
	let empty = !s.doc.project.has_enabled_source();
	app.set_transport(transport_state(&s.playback, &shown, &s.doc.view, s.live.paused(), empty));
	app.set_commands(s.commands());
	match (shown.export, s.exporting.as_ref().and_then(export::Exporting::waits)) {
		(Some(live::ExportNews::Ready), Some(waiting)) => {
			let (plan, position) = waiting.clone();
			write_export(s, &plan, position);
		}
		// the warning says so in the status bar
		(Some(live::ExportNews::Stopped), Some(_)) => release_export(s, app),
		_ => {}
	}
	// the browser's late answers: the H.264 profiles it encodes, its session for the spool
	#[cfg(target_arch = "wasm32")]
	if let Some(dialog) = &s.export_dialog {
		let mut data = app.get_export_data();
		if dialog.refresh(&mut data) {
			app.set_export_data(data);
		}
	}
	if let Some(e) = &mut s.exporting {
		match e.poll() {
			Ok(progress) => app.set_export_progress(progress),
			Err(outcome) => {
				release_export(s, app);
				match outcome {
					export::Outcome::Written(written) => {
						tracing::info!("{}", written.line);
						export_completed(s);
						#[cfg(target_arch = "wasm32")]
						{
							tell(app, ui::Message::ExportReady, written.download.name());
							s.ready = Some(written.download);
						}
					}
					export::Outcome::Cancelled { done, total } => {
						tracing::info!("{}", export::cancelled_line(done, total));
					}
					export::Outcome::Failed(e) => {
						tracing::error!("{e}");
						tell(app, ui::Message::ExportFailed, "");
					}
				}
			}
		}
	}
	if !s.pasted.borrow().is_empty() {
		paste(s);
		s.show(app);
	}
	watch_scene_image(s, app);
	// a focus request no box took within a frame lapses
	let form = app.global::<ui::Form<'_>>();
	s.focus_seen = form.get_focus_pending() && !s.focus_seen;
	if !s.focus_seen {
		form.set_focus_pending(false);
	}
	let picked = s.inbox.take();
	for (layer, name, text) in picked {
		if let Some(layer) = layer {
			s.load_scheme(layer, &name, text);
			s.show(app);
			continue;
		}
		match s.doc.open_text(&name, &text) {
			Ok(()) => {
				s.opened();
				tracing::info!("Opened {name}");
				s.show(app);
			}
			Err(e) => {
				tracing::error!("cannot open {name}: {e}");
				tell(app, ui::Message::OpenFailed, &name);
			}
		}
	}
	if s.messages.take() {
		s.messages.show(app);
	}
	if let Some((why, full)) = &shown.failed {
		tell(app, if *full { ui::Message::StorageFull } else { ui::Message::RunFailed }, why);
	}
	if let Some((layer, fit)) = shown.fitted {
		s.fitted(layer, fit);
		s.show(app);
	}
	if shown.running || shown.changed || shown.field.is_some() || shown.intensity.is_some() {
		s.show_graphs();
	}
	if let Some(image) = shown.field {
		app.set_field(image);
	}
	if let Some(image) = shown.intensity {
		app.set_intensity(image);
	}
	app.set_run_status(shown.status);
	// Play and edits ask from here too
	s.show_warning(app);
	app.global::<ui::Canvas<'_>>().set_computing(shown.computing);
	// classic's `Shell::next_tick`: ticks while something moves, else news wakes the window
	shown.running
		|| shown.computing
		|| s.live.fit_pending()
		|| s.playback.playing
		|| s.exporting.is_some()
		|| s.scene_watch.pending()
		|| form.get_focus_pending()
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
	if let Some(dirs) = waveduderust_platform::settings::AppDirs::platform() {
		crash::install(dirs.config);
	}
	pollster::block_on(start())
}

#[cfg(target_arch = "wasm32")]
fn main() {
	// a panic ends the module: the page says so
	std::panic::set_hook(Box::new(|info| web::fail(&format!("WaveDude Rust stopped: {info}"))));
	wasm_bindgen_futures::spawn_local(async {
		if let Err(e) = start().await {
			web::fail(&format!("WaveDude Rust could not start: {e}"));
		}
	});
}

/// The window's tick: every 16 ms while the last one said something moves, else stopped until [`wake::wake`].
mod ticker {
	use std::sync::atomic::{AtomicBool, Ordering};

	const TICK: std::time::Duration = std::time::Duration::from_millis(16);

	thread_local! {
		static TIMER: std::cell::RefCell<Option<slint::Timer>> = const { std::cell::RefCell::new(None) };
	}

	/// A wake on its way to the event loop: the wakes before it lands are one.
	static WAKING: AtomicBool = AtomicBool::new(false);

	/// Starts ticking `tick`, which says whether to go on.
	pub(super) fn start(tick: impl Fn() -> bool + 'static) {
		let timer = slint::Timer::default();
		timer.start(slint::TimerMode::Repeated, TICK, move || {
			if !tick() {
				TIMER.with(|t| t.borrow().as_ref().map(slint::Timer::stop));
			}
		});
		TIMER.with(|t| t.replace(Some(timer)));
	}

	/// Drops the tick and what it holds, the app's state with its run and GPU buffers, while the thread's other
	/// locals (wgpu's) are alive: dropped by the thread-local destructors at exit, they may outlive them.
	#[cfg(not(target_arch = "wasm32"))]
	pub(super) fn end() {
		TIMER.with(std::cell::RefCell::take);
	}

	/// Ticks again; from any thread.
	pub(super) fn wake() {
		if WAKING.swap(true, Ordering::AcqRel) {
			return;
		}
		let queued = slint::invoke_from_event_loop(|| {
			WAKING.store(false, Ordering::Release);
			TIMER.with(|t| {
				if let Some(t) = t.borrow().as_ref()
					&& !t.running()
				{
					t.restart();
				}
			});
		});
		if queued.is_err() {
			// no event loop: a later wake tries again
			WAKING.store(false, Ordering::Release);
		}
	}
}

/// Writes probe `i`'s recording to a CSV file the user names (the browser: the export dialog names its download).
fn export_probe_csv(state: &mut State, app: &AppWindow, i: usize) {
	let Some(probe) = state.doc.project.probes.get(i) else { return };
	let Some(csv) = state.live.probe_csv(probe) else {
		tracing::info!("{}: no recording to export yet", probe.name);
		return;
	};
	#[cfg(not(target_arch = "wasm32"))]
	{
		let name = format!("{}.csv", probe.name);
		let words = app.global::<ui::Words<'_>>();
		let Some(path) = rfd::FileDialog::new()
			.set_title(words.get_export_probe_data().as_str())
			.add_filter(words.get_csv_files().as_str(), &["csv"])
			.set_file_name(name)
			.save_file()
		else {
			return;
		};
		match std::fs::write(&path, csv) {
			Ok(()) => tracing::info!("Exported {} to {}", probe.name, path.display()),
			Err(e) => tracing::error!("{}: {e}", path.display()),
		}
	}
	#[cfg(target_arch = "wasm32")]
	{
		let (dialog, data) = export::Dialog::csv(&probe.name, csv, &app.global::<ui::Words<'_>>());
		state.export_dialog = Some(dialog);
		app.set_export_data(data);
		app.set_export_shown(true);
	}
}

fn end_gesture(app: &AppWindow) {
	let canvas = app.global::<ui::Canvas<'_>>();
	canvas.set_dragging(false);
	canvas.set_marquee_shown(false);
}
