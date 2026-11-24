//! WaveDude Rust's app: the Slint window over the core (decision 0032).

mod clipboard;
mod document;
mod examples;
mod export;
mod files;
mod form;
mod gpu;
mod icons;
mod live;
mod log;
mod messages;
mod number;
mod playback;
mod rows;
mod selection;
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
use std::path::PathBuf;
use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, VecModel};
use ui::AppWindow;

use crate::document::Document;
use crate::live::Live;
use crate::messages::Messages;
use crate::playback::Playback;
use crate::selection::{Pick, Selection};
use crate::tree::Node;
use crate::ui::{Command, TransportAction};
use waveduderust::model::PartKind;

/// Undo steps until the settings arrive (decision 0030).
const UNDO_STEPS: usize = 1000;

/// What the window shows.
struct State {
	doc: Document,
	nodes: Vec<Node>,
	selection: Selection,
	/// Tree rows whose children are hidden.
	collapsed: Vec<Node>,
	live: Live,
	playback: Playback,
	messages: Messages,
	/// The menu action the unsaved edits prompt holds.
	pending: Option<Action>,
	settings: waveduderust_platform::settings::Settings,
	store: Option<waveduderust_platform::store::Store>,
	settings_section: usize,
	facts: waveduderust::quality::MachineFacts,
	exporting: Option<export::Exporting>,
	inbox: files::Inbox,
	pasted: clipboard::Pasted,
	example_topic: usize,
	example_search: String,
	/// The examples the dialog lists, by index into [`examples::all`].
	example_rows: Vec<usize>,
	/// The last run export, for Export last.
	last_export: Option<export::Kind>,
	/// The document generation the run started from.
	ran: u64,
	/// The desktop's colour scheme once a forced theme has written the palette's, which ends its system binding
	/// (docs/blockers.md, Slint's system colour scheme).
	system_scheme: Option<slint::language::ColorScheme>,
	/// The forms the Properties pane and the settings show, for [`sync`].
	props: Shown,
	settings_rows: Shown,
	/// The generation the Properties form last saw, and the one its object was last placed at: a change not made
	/// through the form (undo, a removal, a reorder, another project) may put another object under the same node.
	form_seen: u64,
	form_base: u64,
	draft: Option<Draft>,
	/// The typed form shown: its object's identity and the token that drops text typed for another.
	typed_form: Shown,
}

/// Text typed into a Properties row and not committed: the tree commits it before it acts, since the editor's own
/// commit on losing the focus comes after the rows changed (Slint runs `changed` handlers late).
struct Draft {
	token: i32,
	row: usize,
	text: String,
	/// The row's value the text was typed over.
	over: slint::SharedString,
}

/// A form the Properties pane or the settings show: its key and the token its rows carry.
#[derive(Default)]
struct Shown {
	key: Option<String>,
	token: i32,
}

/// Shows `rows` of the form `key`. The same form's rows are updated in place, and only those that changed, so the
/// editors, and the focus and typing in one, survive an edit; another form gets a new model and token, so no editor
/// carries text over to it and a late commit from a dropped editor is told apart.
fn sync(
	model: &ModelRc<ui::PropRow>,
	shown: &mut Shown,
	key: String,
	mut rows: Vec<ui::PropRow>,
	set: impl FnOnce(ModelRc<ui::PropRow>),
) {
	use slint::Model;
	let model = model.as_any().downcast_ref::<VecModel<ui::PropRow>>();
	let same = |a: &ui::PropRow, b: &ui::PropRow| a.label == b.label && a.kind == b.kind;
	match model {
		Some(model)
			if shown.key.as_ref() == Some(&key)
				&& model.row_count() == rows.len()
				&& rows.iter().zip(model.iter()).all(|(a, b)| same(a, &b)) =>
		{
			for (i, mut row) in rows.into_iter().enumerate() {
				let old = model.row_data(i).unwrap_or_default();
				row.form = shown.token;
				if old.choices.iter().eq(row.choices.iter()) {
					row.choices = old.choices.clone();
				}
				if row != old {
					model.set_row_data(i, row);
				}
			}
		}
		_ => {
			shown.key = Some(key);
			shown.token = shown.token.wrapping_add(1);
			for row in &mut rows {
				row.form = shown.token;
			}
			set(ModelRc::new(VecModel::from(rows)));
		}
	}
}

impl State {
	/// Sets row `row` of the selected object's form to `text`.
	fn set_prop(&mut self, row: usize, text: &str) -> Result<(), String> {
		self.draft = None;
		let node = self.selection.current();
		let mut result = Ok(());
		if node == Node::Display {
			let doc = &mut self.doc;
			result = rows::set_selected(&mut doc.project, &mut doc.view, node, row, text);
		} else {
			let view = &mut self.doc.view.clone();
			self.doc.edit(|p| result = rows::set_selected(p, view, node, row, text));
		}
		// the form's own edit keeps it
		self.form_seen = self.doc.generation;
		result
	}

	/// Applies the edits of the selected object's typed form; `typing`: a keystroke, joining the run of keystrokes
	/// into one undo step.
	fn edit_form(&mut self, form: &ui::Form<'_>, typing: bool) {
		let node = self.selection.current();
		if node == Node::Display {
			form::apply_view(form, &mut self.doc.view);
		} else {
			let key = typing.then(|| format!("{node:?}"));
			self.doc.edit_merging(key.as_deref(), |p| form::apply(form, p, node));
		}
		self.form_seen = self.doc.generation;
	}

	/// Commits the [`State::draft`] while its editor is still the one shown.
	fn commit_draft(&mut self, app: &AppWindow) {
		use slint::Model;
		let Some(draft) = self.draft.take() else { return };
		let shown = app.get_props_rows().row_data(draft.row).map(|r| r.value);
		if draft.token == self.props.token && shown.is_some_and(|v| v == draft.over) {
			// the form leaves the view, so would its error
			let _ = self.set_prop(draft.row, &draft.text);
		}
	}

	/// Expands `node`'s row or collapses it; a selection it hides moves up to it (WPF's TreeView).
	fn expand(&mut self, node: Node, open: bool) {
		self.collapsed.retain(|&n| n != node);
		if !open {
			self.collapsed.push(node);
			if self.selection.group().iter().any(|&n| n != node && tree::within(n, node)) {
				self.selection.set(node);
			}
		}
	}

	/// Pushes the state into the window's properties.
	fn show(&mut self, app: &AppWindow) {
		self.selection.retain(&self.doc.project);
		// a row selected elsewhere (an add, a paste) shows its folder
		let current = self.selection.current();
		self.collapsed.retain(|&n| n == current || !tree::within(current, n));
		let (nodes, rows) = tree::rows(&self.doc.project, &self.selection, &self.collapsed);
		self.nodes = nodes;
		app.set_tree_rows(ModelRc::new(VecModel::from(rows)));
		let current = self.nodes.iter().position(|&n| n == self.selection.current());
		app.set_tree_current(current.and_then(|i| i32::try_from(i).ok()).unwrap_or(-1));
		app.set_window_title(self.doc.title().into());
		app.set_commands(self.commands());
		// the native menus (muda) read `&` as an access key on Windows and drop it on macOS
		let native = cfg!(any(windows, target_os = "macos"));
		let recent: Vec<slint::SharedString> = self
			.settings
			.recent_files
			.iter()
			.map(|p| p.display().to_string())
			.map(|p| if native { p.replace('&', "&&") } else { p }.into())
			.collect();
		app.set_recent_files(ModelRc::from(recent.as_slice()));
		if self.ran != self.doc.generation {
			self.ran = self.doc.generation;
			self.live.edited(&self.doc.project, &self.doc.view);
		}
		let node = self.selection.current();
		let kind = form::kind(&self.doc.project, node);
		let (mut heading, rows) = if kind == ui::FormKind::None {
			rows::of_selected(&self.doc.project, &self.doc.view, node)
		} else {
			Default::default()
		};
		let folder = tree::folder(&self.doc.project, self.selection.current());
		if let Some((label, _)) = folder {
			heading = label.into();
		}
		app.set_props_heading(heading.into());
		app.set_props_entries(folder.map_or(-1, |(_, n)| i32::try_from(n).unwrap_or(i32::MAX)));
		if self.doc.generation != self.form_seen {
			self.form_seen = self.doc.generation;
			self.form_base = self.doc.generation;
		}
		let key = format!("{:?}@{}", self.selection.current(), self.form_base);
		let g = app.global::<ui::Form<'_>>();
		g.set_kind(kind);
		if self.typed_form.key.as_ref() != Some(&key) {
			self.typed_form.key = Some(key.clone());
			self.typed_form.token = self.typed_form.token.wrapping_add(1);
			g.set_token(self.typed_form.token);
			g.set_invalid("".into());
		}
		form::show(&g, &self.doc.project, &self.doc.view, node);
		sync(&app.get_props_rows(), &mut self.props, key, rows, |m| {
			app.set_props_rows(m);
		});
	}

	/// A project replaced the one shown: the selection and the timeline start over, frame 0 is made.
	fn opened(&mut self) {
		self.selection.set(Node::Project);
		self.last_export = None;
		self.playback.reset();
		self.live.load(&self.doc.project, &self.doc.view);
		self.playback.frames = self.live.frames;
		self.ran = self.doc.generation;
	}

	/// Which menu commands can run (Next's CanExecute rules).
	fn commands(&self) -> ui::Commands {
		let exporting = self.exporting.is_some();
		let can_export = !exporting && self.doc.project.simulation.steps > 0;
		ui::Commands {
			can_undo: self.doc.can_undo(),
			can_redo: self.doc.can_redo(),
			can_delete: self.node_exists(),
			can_toggle: selection::groupable(self.selection.current()) && self.node_exists(),
			can_select_all: !(self.doc.project.sources.is_empty()
				&& self.doc.project.parts.is_empty()
				&& self.doc.project.probes.is_empty()),
			can_move_up: tree::can_shift(&self.doc.project, self.selection.current(), -1),
			can_move_down: tree::can_shift(&self.doc.project, self.selection.current(), 1),
			can_copy: matches!(self.selection.current(), Node::Scene | Node::Simulation | Node::Display)
				|| self.node_exists(),
			can_run: !self.live.active() && !exporting,
			run_active: self.live.active(),
			paused: self.live.paused(),
			can_stop: self.live.active() || exporting,
			can_export,
			can_export_last: can_export && self.last_export.is_some(),
			can_close: self.doc.path.is_some() || self.doc.suggested.is_some() || self.doc.dirty(),
			can_play: self.playback.frames > 1,
			playing: self.playback.playing,
		}
	}

	/// Whether the selected node is a list item that exists.
	fn node_exists(&self) -> bool {
		let p = &self.doc.project;
		match self.selection.current() {
			Node::Source(i) => i < p.sources.len(),
			Node::Part(i) => i < p.parts.len(),
			Node::Probe(i) => i < p.probes.len(),
			Node::Scheme(i) => i < p.color_schemes.len(),
			_ => false,
		}
	}

	fn node(&self, row: i32) -> Option<Node> {
		usize::try_from(row).ok().and_then(|r| self.nodes.get(r).copied())
	}
}

/// Moves the current row `by` places in its list, the selection following it.
fn shift(state: &mut State, by: isize) {
	let (node, mut to) = (state.selection.current(), state.selection.current());
	state.doc.edit(|p| to = tree::shift(p, node, by));
	state.selection.set(to);
}

/// A tree row's handler: `f` on the row's node with the callback's argument, typed text committed first and the
/// window refreshed after.
fn on_row<A: 'static>(
	state: &Rc<RefCell<State>>,
	app: &AppWindow,
	f: impl Fn(&mut State, Node, A) + 'static,
) -> impl Fn(i32, A) + 'static {
	let (state, weak) = (state.clone(), app.as_weak());
	move |row, arg| {
		let Some(app) = weak.upgrade() else { return };
		let s = &mut *state.borrow_mut();
		let Some(node) = s.node(row) else { return };
		s.commit_draft(&app);
		f(s, node, arg);
		s.show(&app);
	}
}

/// The tree row a drag carries.
fn dragged(data: &slint::DataTransfer) -> Option<Node> {
	data.user_data().and_then(|d| d.downcast_ref::<Node>().copied())
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
	if action.replaces() && state.doc.dirty() {
		state.pending = Some(action);
		let name = state.doc.name();
		app.set_modal(modal(
			ui::ModalKind::Question,
			"WaveDude Rust",
			&format!("Save the changes to {name}?"),
			&["Save", "Don't save", "Cancel"],
			0,
			2,
		));
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
			state.live.run(&state.doc.project, &state.doc.view);
			state.playback.frames = state.live.frames;
			state.playback.follow();
		}
		Command::Pause => state.live.toggle_pause(),
		// the menu, Shift+F5 and Esc: a running export first, else the run
		Command::Stop => {
			if let Some(e) = &state.exporting {
				e.cancel();
			} else {
				state.live.stop();
				state.playback.live = false;
			}
		}
		Command::Welcome => {
			show_welcome(state, app);
			app.set_welcome_shown(true);
		}
		Command::RandomExample => {
			let nanos = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
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
			if let Some(path) = files::dialog().pick_file() {
				open(state, app, path);
			}
			#[cfg(target_arch = "wasm32")]
			files::pick(&state.inbox);
		}
		Command::Github => {
			if let Err(e) = webbrowser::open(env!("CARGO_PKG_REPOSITORY")) {
				tracing::error!("cannot open the project page: {e}");
			}
		}
		Command::Save | Command::SaveAs => {
			save(state, app, command == Command::SaveAs);
		}
		Command::ExportField
		| Command::ExportIntensity
		| Command::ExportBoth
		| Command::ExportFieldFrame
		| Command::ExportIntensityFrame
		| Command::ExportLast => {
			let kind = export::Kind::of(command).or(state.last_export);
			if state.exporting.is_none()
				&& let Some(kind) = kind
			{
				state.exporting = export::start(kind, &state.doc.project, &state.doc.view, state.playback.position, &state.settings);
				if matches!(kind, export::Kind::Run(_)) && state.exporting.is_some() {
					state.last_export = Some(kind);
				}
			}
		}
		Command::Settings => {
			show_settings(state, app);
			app.set_settings_shown(true);
		}
		Command::About => app.set_modal(modal(
			ui::ModalKind::Info,
			"About WaveDude Rust",
			&format!("WaveDude Rust {}\nA 2D wave simulator\n\nBased on the work of WaveDude Next; Jeroen Vleggaar, Huygens Optics, author of Wave Dude 2D, Copyright (C) 2026; 0x23 (Diffraction Limited), author of WaveSimulator2D.\n\nThis program is free software under the GNU General Public License, version 3 or later.", env!("CARGO_PKG_VERSION")),
			&["Close"],
			0,
			0,
		)),
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
	let texts: Vec<String> = state.pasted.borrow_mut().drain(..).collect();
	for text in texts {
		match waveduderust::io::wdproj::read_fragment(&text) {
			Ok(fragment) if !fragment.is_empty() => {
				let (current, view) = (state.selection.current(), &mut state.doc.view.clone());
				let mut last = None;
				state.doc.edit(|p| last = clipboard::paste(p, view, fragment, current));
				state.doc.view = view.clone();
				state.selection.set(last.unwrap_or(current));
			}
			_ => tracing::info!("the clipboard holds no project objects"),
		}
	}
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

/// After a forced theme, keeps the System theme on the desktop's scheme from winit's theme event, which Linux lacks
/// (docs/blockers.md, Slint's system colour scheme).
fn follow_desktop_scheme(state: &Rc<RefCell<State>>, app: &AppWindow) {
	use slint::language::ColorScheme;
	use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
	let (state, weak) = (state.clone(), app.as_weak());
	app.window().on_winit_window_event(move |_, event| {
		if let (winit::event::WindowEvent::ThemeChanged(theme), Some(app)) = (event, weak.upgrade()) {
			let mut s = state.borrow_mut();
			if s.system_scheme.is_some() {
				s.system_scheme = Some(match theme {
					winit::window::Theme::Dark => ColorScheme::Dark,
					winit::window::Theme::Light => ColorScheme::Light,
				});
				apply_theme(&mut s, &app);
			}
		}
		EventResult::Propagate
	});
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

/// Applies the settings to the window, the history and the run.
fn apply_settings(state: &mut State, app: &AppWindow) {
	apply_theme(state, app);
	if let Err(e) = slint::select_bundled_translation(&settings::language(&state.settings)) {
		tracing::warn!("language: {e}");
	}
	let s = &state.settings;
	app.set_preview_stacked(s.preview_orientation == waveduderust_platform::settings::Orientation::Vertical);
	app.global::<ui::Form<'_>>().set_described(s.part_description_expanded);
	state.doc.set_limit(s.undo_steps as usize);
	state.live.quality = s.preview_quality();
	state.live.auto_rerun = s.auto_rerun;
}

/// Pushes the Settings window's sections and rows.
fn show_settings(state: &mut State, app: &AppWindow) {
	let sections: Vec<slint::SharedString> = settings::SECTIONS.iter().map(|s| (*s).into()).collect();
	app.set_settings_sections(ModelRc::from(sections.as_slice()));
	app.set_settings_section(i32::try_from(state.settings_section).unwrap_or_default());
	let rows = settings::rows(&state.settings, state.settings_section);
	let key = state.settings_section.to_string();
	sync(&app.get_settings_rows(), &mut state.settings_rows, key, rows, |m| {
		app.set_settings_rows(m);
	});
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
			act(state, app, then);
		}
	}
}

/// A transport bar or timeline action (docs/ui/50).
fn transport(state: &mut State, app: &AppWindow, action: TransportAction) {
	let p = &mut state.playback;
	match action {
		TransportAction::Play if p.playing => p.pause(),
		TransportAction::Play => {
			p.play();
			state.live.fill();
		}
		TransportAction::Prev => p.seek(i64::from(p.position) - 1),
		TransportAction::Next => p.seek(i64::from(p.position) + 1),
		TransportAction::SetIn => p.set_in(),
		TransportAction::SetOut => p.set_out(),
		TransportAction::ClearRange => (p.start, p.end) = (None, None),
		TransportAction::Loop => p.looping ^= true,
		TransportAction::Live => p.live ^= state.live.active(),
		TransportAction::PauseRun => state.live.toggle_pause(),
		TransportAction::Layout => {
			use waveduderust_platform::settings::Orientation;
			state.settings.preview_orientation =
				if app.get_preview_stacked() { Orientation::Vertical } else { Orientation::Horizontal };
			if let Some(Err(e)) = state.store.as_ref().map(|s| state.settings.save(s)) {
				tracing::error!("{e}");
			}
		}
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
	let numbers = app.global::<ui::NumberLogic<'_>>();
	numbers.on_step(|spec, text, steps| number::step(&spec, &text, steps).into());
	numbers.on_scrub(|spec, start, dx, scale| number::scrub(&spec, &start, f64::from(dx), f64::from(scale)).into());
	let live = Live::new(&context)?;
	let store = waveduderust_platform::store::Store::open();
	let facts = waveduderust_platform::machine::machine_facts();
	let settings = store.as_ref().map_or_else(Default::default, |s| {
		waveduderust_platform::settings::Settings::load(s, false, || facts).settings
	});
	let state = Rc::new(RefCell::new(State {
		doc: Document::new(UNDO_STEPS),
		nodes: Vec::new(),
		selection: Selection::default(),
		collapsed: Vec::new(),
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
		pasted: clipboard::Pasted::default(),
		example_topic: 0,
		example_search: String::new(),
		example_rows: Vec::new(),
		last_export: None,
		ran: 0,
		system_scheme: None,
		props: Shown::default(),
		settings_rows: Shown::default(),
		form_seen: 0,
		form_base: 0,
		draft: None,
		typed_form: Shown::default(),
	}));
	app.set_native_access_keys(cfg!(windows));
	apply_settings(&mut state.borrow_mut(), &app);
	follow_desktop_scheme(&state, &app);
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

	app.on_tree_select(on_row(&state, &app, |s, node, pick| s.selection.pick(&s.doc.project, node, pick)));
	// a row's own check box and buttons act on the group it belongs to, else on the row
	let toggle = on_row(&state, &app, |s, node, ()| {
		let group = s.selection.group_of(node);
		s.doc.edit(|p| group.iter().for_each(|&n| tree::toggle(p, n)));
	});
	app.on_tree_toggle(move |row| toggle(row, ()));
	let expand = on_row(&state, &app, |s, node, ()| {
		let open = s.collapsed.contains(&node);
		s.expand(node, open);
	});
	app.on_tree_expand(move |row| expand(row, ()));
	let st = state.clone();
	app.on_tree_drag_data(move |row| {
		let mut data = slint::DataTransfer::default();
		// read lazily, at the drag start, and anew per show(), which replaces the rows; empty while the state is borrowed
		if let Some(node) = st.try_borrow().ok().and_then(|s| s.node(row)) {
			data.set_user_data(Rc::new(node));
		}
		data
	});
	let st = state.clone();
	app.on_tree_drop_place(move |row, data, before| {
		let s = st.borrow();
		let place = dragged(&data).zip(s.node(row)).and_then(|(n, t)| tree::drop_target(&s.doc.project, n, t, before));
		place.map_or(ui::DropPlace::None, |(place, _)| place)
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_tree_drop(move |row, data, before| {
		if let Some(app) = weak.upgrade() {
			let mut s = st.borrow_mut();
			let target = dragged(&data).zip(s.node(row));
			let Some((node, (_, to))) =
				target.and_then(|(n, t)| Some((n, tree::drop_target(&s.doc.project, n, t, before)?)))
			else {
				return;
			};
			s.commit_draft(&app);
			let mut moved = node;
			s.doc.edit(|p| moved = tree::move_to(p, node, to));
			s.selection.set(moved);
			s.show(&app);
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_tree_navigate(move |key, extend| {
		if let Some(app) = weak.upgrade() {
			let mut s = st.borrow_mut();
			let s = &mut *s;
			match tree::navigate(&s.doc.project, &s.nodes, &s.collapsed, s.selection.current(), key) {
				Some(tree::Step::To(node)) => {
					s.commit_draft(&app);
					s.selection.pick(&s.doc.project, node, if extend { Pick::Range } else { Pick::Alone });
				}
				Some(tree::Step::Expand(node, open)) => s.expand(node, open),
				None => return,
			}
			s.show(&app);
		}
	});
	app.on_tree_shift(on_row(&state, &app, |s, node, by: i32| {
		let mut to = node;
		s.doc.edit(|p| to = tree::shift(p, node, by as isize));
		s.selection.set(to);
	}));
	let remove_row = on_row(&state, &app, |s, node, ()| remove(s, &s.selection.group_of(node)));
	app.on_tree_remove(move |row| remove_row(row, ()));
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_props_set(move |form, row, text| {
		if let Some(app) = weak.upgrade() {
			let mut s = st.borrow_mut();
			if form != s.props.token {
				// from an editor of a form no longer shown
				return false;
			}
			let result = s.set_prop(usize::try_from(row).unwrap_or(usize::MAX), &text);
			let took = result.is_ok();
			app.set_props_error(result.err().unwrap_or_default().into());
			s.show(&app);
			return took;
		}
		false
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_props_typed(move |token, row, text| {
		use slint::Model;
		let (Some(app), Ok(row)) = (weak.upgrade(), usize::try_from(row)) else { return };
		let over = app.get_props_rows().row_data(row).map_or_else(Default::default, |r| r.value);
		st.borrow_mut().draft = Some(Draft { token, row, text: text.into(), over });
	});
	let parse = app.global::<ui::NumberParse<'_>>();
	parse.on_int(|spec, text| {
		let value = number::commit(&spec, &text);
		// `as` saturates: the box's f32 bounds round i32::MAX up a step
		ui::ParsedInt { ok: value.is_some(), value: value.unwrap_or_default() as i32 }
	});
	parse.on_float(|spec, text| {
		let value = number::commit(&spec, &text);
		ui::ParsedFloat { ok: value.is_some(), value: value.unwrap_or_default() as f32 }
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.global::<ui::Form<'_>>().on_edited(move |typing| {
		let Some(app) = weak.upgrade() else { return };
		let mut s = st.borrow_mut();
		let form = app.global::<ui::Form<'_>>();
		form.set_invalid("".into());
		s.edit_form(&form, typing);
		s.show(&app);
	});
	let st = state.clone();
	app.global::<ui::Form<'_>>().on_describe(move |open| {
		let s = &mut *st.borrow_mut();
		s.settings.part_description_expanded = open;
		if let Some(Err(e)) = s.store.as_ref().map(|store| s.settings.save(store)) {
			tracing::error!("{e}");
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.global::<ui::Form<'_>>().on_new_seed(move || {
		let Some(app) = weak.upgrade() else { return };
		let mut s = st.borrow_mut();
		let seed = waveduderust::model::Project::new_random_seed().seed;
		s.doc.edit(|p| p.seed = seed);
		s.show(&app);
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.global::<ui::Form<'_>>().on_add_point(move || {
		let Some(app) = weak.upgrade() else { return };
		let mut s = st.borrow_mut();
		if let Node::Part(i) = s.selection.current() {
			s.doc.edit(|p| {
				if let Some(PartKind::Outline(o)) = p.parts.get_mut(i).map(|p| &mut p.kind) {
					form::add_point(&mut o.points);
				}
			});
		}
		s.show(&app);
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.global::<ui::Form<'_>>().on_remove_point(move |k| {
		let Some(app) = weak.upgrade() else { return };
		let mut s = st.borrow_mut();
		if let (Node::Part(i), Ok(k)) = (s.selection.current(), usize::try_from(k)) {
			s.doc.edit(|p| {
				if let Some(PartKind::Outline(o)) = p.parts.get_mut(i).map(|p| &mut p.kind)
					&& k < o.points.len()
				{
					o.points.remove(k);
				}
			});
		}
		s.show(&app);
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_command(move |c| {
		if let Some(app) = weak.upgrade() {
			command(&mut st.borrow_mut(), &app, c);
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_add(move |item| {
		if let Some(app) = weak.upgrade() {
			let s = &mut *st.borrow_mut();
			add(s, item);
			s.show(&app);
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_open_recent(move |i| {
		if let Some(app) = weak.upgrade() {
			let s = &mut *st.borrow_mut();
			// the path now: saving first may move it in the list
			if let Some(path) = usize::try_from(i).ok().and_then(|i| s.settings.recent_files.get(i)).cloned() {
				act(s, &app, Action::OpenFile(path));
			}
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
				act(s, &app, Action::Example(i));
			}
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_welcome_preset(move |i| {
		if let Some(app) = weak.upgrade() {
			let s = &mut *st.borrow_mut();
			let facts = s.facts;
			settings::preset(&mut s.settings, s.store.as_ref(), i, facts);
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
			return true;
		}
		false
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
			command(s, &app, Command::Exit);
			slint::CloseRequestResponse::KeepWindowShown
		} else {
			slint::CloseRequestResponse::HideWindow
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_transport_action(move |action| {
		if let Some(app) = weak.upgrade() {
			transport(&mut st.borrow_mut(), &app, action);
		}
	});
	let st = state.clone();
	app.on_seek(move |frame| st.borrow_mut().playback.seek(i64::from(frame)));
	let st = state.clone();
	app.on_transport_speed(move |speed| st.borrow_mut().playback.speed = f64::from(speed).clamp(0.01, 100.0));
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
			app.set_commands(s.commands());
			if let Some(e) = &mut s.exporting {
				match e.poll() {
					Ok(progress) => app.set_export_progress(progress),
					Err(outcome) => {
						s.exporting = None;
						app.set_export_progress(ui::ExportProgress::default());
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
			if !s.pasted.borrow().is_empty() {
				paste(s);
				s.show(&app);
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
			app.set_run_status(shown.status);
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
