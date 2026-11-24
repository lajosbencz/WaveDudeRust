//! WaveDude Rust's app: the Slint window over the core (decision 0032).

mod canvas;
mod clipboard;
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
mod messages;
mod number;
mod playback;
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
	/// The user's colour schemes.
	schemes: waveduderust::io::schemes::SchemeStore,
	facts: waveduderust::quality::MachineFacts,
	exporting: Option<export::Exporting>,
	inbox: files::Inbox,
	pasted: clipboard::Pasted,
	example_topic: ui::ExampleTopic,
	example_search: String,
	/// The examples the dialog lists, by index into [`examples::all`].
	example_rows: Vec<usize>,
	/// The last run export, for Export last.
	last_export: Option<export::Kind>,
	/// The document generation the run started from.
	ran: u64,
	/// The view generation the pictures are drawn with.
	styled: u64,
	graphs: graphs::Graphs,
	/// The desktop's colour scheme once a forced theme has written the palette's, which ends its system binding
	/// (docs/blockers.md, Slint's system colour scheme).
	system_scheme: Option<slint::language::ColorScheme>,
	/// The project's and the view's generations the Properties form last saw, and the count of changes made not
	/// through it (undo, a removal, a reorder, another project), which may put another object under the same node.
	form_seen: (u64, u64),
	form_base: u64,
	/// The typed form shown: its object's identity and the token that drops text typed for another.
	typed_form: Shown,
	overlays: canvas::Overlays,
	/// A handle drag or a marquee under way.
	gesture: Option<canvas::Gesture>,
	/// Presses on the pictures so far; each drag's undo key.
	drags: u64,
}

/// The typed form shown: its key, the token its editors carry and the lists it caches.
#[derive(Default)]
struct Shown {
	key: Option<String>,
	token: i32,
	node: Option<Node>,
	// the schemes Based on lists
	bases: Vec<waveduderust::model::ColorScheme>,
	// the stops the gradient bar shows, and whether the bar changed them since
	stops: Vec<waveduderust::model::Stop>,
	bar_edited: bool,
	// the Display layers' pickers
	field_schemes: Option<form::LayerPicker>,
	intensity_schemes: Option<form::LayerPicker>,
}

impl State {
	/// Applies the edits of the selected object's typed form; `typing`: a keystroke, joining the run of keystrokes
	/// into one undo step.
	fn edit_form(&mut self, form: &ui::Form<'_>, typing: bool) {
		let node = self.selection.current();
		let key = typing.then(|| format!("{node:?}"));
		if node == Node::Display {
			self.doc.edit_view_merging(key.as_deref(), |v| form::apply_view(form, v));
		} else if let Node::Scheme(i) = node {
			// a renamed scheme takes its layers along, in the same step
			self.doc.edit_both(key.as_deref(), |p, v| {
				let old = p.color_schemes.get(i).map(|s| s.name.clone());
				form::apply(form, p, node);
				if let (Some(old), Some(new)) = (old, p.color_schemes.get(i).map(|s| &s.name)) {
					v.rename_scheme(&old, new);
				}
			});
		} else {
			self.doc.edit_merging(key.as_deref(), |p| form::apply(form, p, node));
		}
		self.form_seen = (self.doc.generation, self.doc.view_generation);
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
		app.set_file_name(self.doc.name().unwrap_or_default().into());
		app.set_file_path(self.doc.path.as_deref().map_or_default(|p| p.display().to_string()).into());
		app.set_dirty(self.doc.dirty());
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
		if self.styled != self.doc.view_generation {
			self.styled = self.doc.view_generation;
			self.live.restyle(&self.doc.project, &self.doc.view);
		}
		let canvas = app.global::<ui::Canvas<'_>>();
		canvas.set_field_brightness(self.doc.view.field.brightness as f32);
		let scene = &self.doc.project.scene;
		canvas.set_edge(if scene.has_rim() {
			ui::EdgeLine::Rim
		} else if scene.edges == waveduderust::model::Edges::Reflective {
			ui::EdgeLine::Wall
		} else {
			ui::EdgeLine::Open
		});
		canvas.set_intensity_brightness(self.doc.view.intensity.brightness as f32);
		self.overlays.show(&app.global::<ui::Canvas<'_>>(), &self.doc.project, &self.doc.view, &self.selection);
		self.show_graphs();
		let canvas = app.global::<ui::Canvas<'_>>();
		let focus = [ui::Layer::Field, ui::Layer::Intensity].into_iter().find_map(|layer| {
			let (b, grip) = self.overlays.focused_box(layer, &self.doc.project, self.graphs.placed(layer))?;
			Some((layer, b.probe, grip))
		});
		canvas.set_focus_box(focus.map_or(-1, |(_, i, _)| i32::try_from(i).unwrap_or(-1)));
		if let Some((layer, _, grip)) = focus {
			canvas.set_focus_layer(layer);
			canvas.set_focus_grip(grip);
		}
		let node = self.selection.current();
		let kind = form::kind(&self.doc.project, node);
		app.set_props_kind(node.kind());
		app.set_props_heading(tree::name(&self.doc.project, node).into());
		let folder = tree::folder(&self.doc.project, node);
		app.set_props_entries(folder.map_or(-1, |n| i32::try_from(n).unwrap_or(i32::MAX)));
		let seen = (self.doc.generation, self.doc.view_generation);
		if seen != self.form_seen {
			self.form_seen = seen;
			self.form_base = self.form_base.wrapping_add(1);
		}
		let key = format!("{:?}@{}", self.selection.current(), self.form_base);
		let g = app.global::<ui::Form<'_>>();
		g.set_kind(kind);
		if self.typed_form.node != Some(node) {
			self.typed_form.node = Some(node);
			g.set_based_on(-1);
			g.set_object(g.get_object().wrapping_add(1));
		}
		if self.typed_form.key.as_ref() != Some(&key) {
			self.typed_form.key = Some(key.clone());
			self.typed_form.token = self.typed_form.token.wrapping_add(1);
			g.set_token(self.typed_form.token);
			g.set_invalid("".into());
		}
		form::show(&g, &self.doc.project, &self.doc.view, node);
		if node == Node::Display {
			let (project, view, user) = (&self.doc.project, &self.doc.view, &self.schemes);
			let shown = &mut self.typed_form;
			form::show_layer_schemes(&g, project, user, view, ui::Layer::Field, &mut shown.field_schemes);
			form::show_layer_schemes(&g, project, user, view, ui::Layer::Intensity, &mut shown.intensity_schemes);
		}
		// a pick names a base by its index, which another list makes another's
		g.set_mine(self.mine());
		if let Node::Scheme(i) = node {
			if form::show_bases(&g, &self.doc.project, &self.schemes, i, &mut self.typed_form.bases) {
				g.set_based_on(-1);
			}
			// the bar selects a stop by its index, which an edit from elsewhere may give another
			let stops = &self.doc.project.color_schemes[i].stops;
			if *stops != self.typed_form.stops {
				if !self.typed_form.bar_edited {
					g.set_stops_replaced(g.get_stops_replaced().wrapping_add(1));
				}
				self.typed_form.stops.clone_from(stops);
			}
			self.typed_form.bar_edited = false;
		}
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

	/// Whether the current scheme is among the user's: -1 for another row or no store to keep them, 0 not, 1 it is.
	fn mine(&self) -> i32 {
		match (self.selection.current(), &self.store) {
			(Node::Scheme(i), Some(_)) => self
				.doc
				.project
				.color_schemes
				.get(i)
				.map_or(-1, |s| i32::from(waveduderust_platform::user_schemes::has(&self.schemes, &s.name))),
			_ => -1,
		}
	}

	/// Adds the scheme of the file `name` holding `text` to the project and points `layer` at it.
	fn load_scheme(&mut self, layer: ui::Layer, name: &str, text: String) {
		let scheme = match form::scheme_file(name, text) {
			Ok(scheme) => scheme,
			Err(e) => return tracing::error!("The colour scheme could not be loaded from '{name}': {e}"),
		};
		let mut added = String::new();
		self.doc.edit_both(None, |p, v| {
			p.add_color_scheme(scheme);
			added.clone_from(&p.color_schemes[p.color_schemes.len() - 1].name);
			form::layer_settings(v, layer).color_scheme.clone_from(&added);
		});
		tracing::info!("Colour scheme '{added}' loaded from '{name}' into the project");
	}

	/// Whether `node` is an active probe, which a run records.
	fn active_probe(&self, node: Node) -> bool {
		matches!(node, Node::Probe(i) if self.doc.project.probes.get(i).is_some_and(|p| p.active))
	}

	/// Lays out the probe graph boxes at the frame shown.
	fn show_graphs(&mut self) {
		let scales = [self.overlays.scale(ui::Layer::Field), self.overlays.scale(ui::Layer::Intensity)];
		let selected: Vec<usize> = self
			.selection
			.group()
			.into_iter()
			.filter_map(|n| if let Node::Probe(i) = n { Some(i) } else { None })
			.collect();
		let (project, view, graphs) = (&self.doc.project, &self.doc.view, &mut self.graphs);
		if self.live.traces(|t| graphs.show(project, view, &selected, Some(t), scales)).is_none() {
			graphs.show(project, view, &selected, None, scales);
		}
		for layer in [ui::Layer::Field, ui::Layer::Intensity] {
			self.overlays.drop_lost_box(layer, project, self.graphs.placed(layer));
		}
	}

	/// Sets `layer`'s brightness, one undo step.
	fn set_brightness(&mut self, layer: waveduderust::model::Layer, value: f64) {
		self.doc.edit_view_merging(None, |v| v.layer_mut(layer).brightness = value);
	}

	/// Takes an Auto brightness fit.
	fn fitted(&mut self, layer: waveduderust::model::Layer, fit: Option<f64>) {
		match fit {
			Some(b) => self.set_brightness(layer, b),
			None => tracing::info!("Auto brightness: nothing to fit"),
		}
	}

	/// Ends a picture gesture as its release does.
	fn finish_gesture(&mut self, app: &AppWindow) {
		end_gesture(app);
		match self.gesture.take() {
			// a drag that ends where it began makes its item the selection
			Some(canvas::Gesture::Handle(drag)) if !drag.moved() => self.selection.set(drag.node()),
			Some(canvas::Gesture::Marquee(m)) => m.select(&self.overlays, &self.doc.project, &mut self.selection),
			_ => {}
		}
	}

	/// Keeps the current scheme among the user's, or removes the user's of its name.
	fn toggle_mine(&mut self) {
		use waveduderust_platform::user_schemes;
		let (Node::Scheme(i), Some(store)) = (self.selection.current(), &self.store) else { return };
		let Some(scheme) = self.doc.project.color_schemes.get(i) else { return };
		let (done, what) = if user_schemes::has(&self.schemes, &scheme.name) {
			(user_schemes::remove(store, &scheme.name), "removed from")
		} else {
			(user_schemes::add(store, scheme), "added to")
		};
		if done.is_ok() {
			tracing::info!("Colour scheme '{}' {what} my schemes", scheme.name);
		}
		self.schemes = user_schemes::load(store);
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
			mine: self.mine(),
			can_export_csv: !exporting && self.active_probe(self.selection.current()),
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

/// A tree row's handler: `f` on the row's node with the callback's argument and the
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
				state.exporting = export::start(
					kind,
					&state.doc.project,
					&state.doc.view,
					state.playback.position,
					&state.settings,
					&state.schemes,
					&app.global::<ui::Words<'_>>(),
				);
				if matches!(kind, export::Kind::Run(_)) && state.exporting.is_some() {
					state.last_export = Some(kind);
				}
			}
		}
		Command::Settings => {
			show_settings(state, app);
			app.set_settings_shown(true);
		}
		Command::About => tell(app, ui::Message::About, env!("CARGO_PKG_VERSION")),
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
				let current = state.selection.current();
				let mut last = None;
				state.doc.edit_both(None, |p, v| last = clipboard::paste(p, v, fragment, current));
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
	app.set_title_full_path(s.title_full_path);
	app.set_preview_stacked(s.preview_orientation == waveduderust_platform::settings::Orientation::Vertical);
	app.global::<ui::Form<'_>>().set_described(s.part_description_expanded);
	state.doc.set_limit(s.undo_steps as usize);
	state.live.quality = s.preview_quality();
	state.live.auto_rerun = s.auto_rerun;
}

/// Pushes the settings to the Settings window and the Welcome page.
fn show_settings(state: &State, app: &AppWindow) {
	app.set_settings(settings::data(&state.settings));
	app.set_undo_spec(settings::undo_spec());
	app.set_welcome_ram((state.facts.physical_bytes as f64 / f64::from(1u32 << 30)) as f32);
	app.set_welcome_levers(settings::levers(&state.settings));
	app.set_welcome_show_startup(state.settings.show_startup);
}

/// The project's file name, an untitled one's in the UI's language.
fn file_name(state: &State, app: &AppWindow) -> String {
	state.doc.name().unwrap_or_else(|| format!("{}.wdproj", app.global::<ui::Words<'_>>().get_untitled()))
}

/// Shows `message`, `arg` the value its text names.
fn tell(app: &AppWindow, message: ui::Message, arg: &str) {
	app.set_modal(ui::Modal { message, arg: arg.into() });
}

/// Saves the project, to a picked file for Save As or without a file; `false` when cancelled or failed.
#[cfg(target_arch = "wasm32")]
fn save(state: &mut State, app: &AppWindow, _: bool) -> bool {
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
fn save(state: &mut State, app: &AppWindow, pick: bool) -> bool {
	let Some(path) = state
		.doc
		.path
		.clone()
		.filter(|_| !pick)
		.or_else(|| files::dialog(&app.global::<ui::Words<'_>>()).set_file_name(file_name(state, app)).save_file())
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
			tell(app, ui::Message::SaveFailed, &path.display().to_string());
			false
		}
	}
}

/// The answer to the message window: for the unsaved edits prompt, Save, Don't save or Cancel.
fn answer(state: &mut State, app: &AppWindow, answer: ui::Answer) {
	app.set_modal(ui::Modal::default());
	if let Some(then) = state.pending.take() {
		let go = match answer {
			ui::Answer::Save => save(state, app, false),
			ui::Answer::Discard => true,
			ui::Answer::Ok | ui::Answer::Cancel => false,
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
			// the pictures are made anew: a gesture on them gets no release
			state.finish_gesture(app);
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
			tell(app, ui::Message::OpenFailed, &path.display().to_string());
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
	form::wire_colors(&app.global::<ui::ColorLogic<'_>>());
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
		schemes: store.as_ref().map_or_else(Default::default, waveduderust_platform::user_schemes::load),
		store,
		facts,
		exporting: None,
		inbox: files::Inbox::default(),
		pasted: clipboard::Pasted::default(),
		example_topic: ui::ExampleTopic::default(),
		example_search: String::new(),
		example_rows: Vec::new(),
		last_export: None,
		ran: 0,
		styled: 0,
		graphs: graphs::Graphs::default(),
		system_scheme: None,
		form_seen: (0, 0),
		form_base: 0,
		typed_form: Shown::default(),
		overlays: canvas::Overlays::default(),
		gesture: None,
		drags: 0,
	}));
	app.set_native_access_keys(cfg!(windows));
	apply_settings(&mut state.borrow_mut(), &app);
	follow_desktop_scheme(&state, &app);
	state.borrow_mut().opened();
	if state.borrow().settings.show_startup && std::env::args_os().nth(1).is_none() {
		show_settings(&state.borrow(), &app);
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
	let edit_rows = {
		let (st, weak) = (state.clone(), app.as_weak());
		move |edit: form::RowEdit| {
			let Some(app) = weak.upgrade() else { return };
			let mut s = st.borrow_mut();
			let node = s.selection.current();
			s.doc.edit(|p| form::edit_rows(p, node, edit));
			s.show(&app);
		}
	};
	let index = |k: i32| usize::try_from(k).unwrap_or(usize::MAX);
	let rows = app.global::<ui::Form<'_>>();
	rows.on_add_row({
		let edit = edit_rows.clone();
		move || edit(form::RowEdit::Add)
	});
	rows.on_remove_row({
		let edit = edit_rows.clone();
		move |k| edit(form::RowEdit::Remove(index(k)))
	});
	rows.on_move_row({
		let edit = edit_rows.clone();
		move |from, to| edit(form::RowEdit::Move { from: index(from), to: index(to) })
	});
	rows.on_clear_rows(move || edit_rows(form::RowEdit::Clear));
	// the gradient bar and the stop swatches: each edit on the scheme shown, the stop then selected handed back
	let bar_edit = {
		let (st, weak) = (state.clone(), app.as_weak());
		move |gesture: i32, edit: &dyn Fn(&mut waveduderust::model::ColorScheme) -> i32| {
			let Some(app) = weak.upgrade() else { return -1 };
			let mut s = st.borrow_mut();
			let Node::Scheme(i) = s.selection.current() else { return -1 };
			let mut selected = -1;
			let key = (gesture != 0).then(|| format!("stop gesture {gesture}"));
			s.typed_form.bar_edited = true;
			s.doc.edit_merging(key.as_deref(), |p| {
				if let Some(scheme) = p.color_schemes.get_mut(i) {
					selected = edit(scheme);
				}
			});
			s.show(&app);
			selected
		}
	};
	let bar = app.global::<ui::Form<'_>>();
	bar.on_add_stop_at({
		let edit = bar_edit.clone();
		move |at, gesture| edit(gesture, &|s| i32::try_from(s.add_stop_at(form::thousandths(at))).unwrap_or(-1))
	});
	bar.on_move_stop({
		let edit = bar_edit.clone();
		move |k, at, gesture| {
			edit(gesture, &|s| {
				let moved = usize::try_from(k).ok().and_then(|k| s.move_stop(k, form::thousandths(at)));
				moved.map_or(k, |k| i32::try_from(k).unwrap_or(-1))
			})
		}
	});
	bar.on_remove_stop({
		let edit = bar_edit.clone();
		move |k, gesture| edit(gesture, &|s| usize::try_from(k).map_or(k, |k| form::remove_stop(s, k)))
	});
	bar.on_recolour_stop(move |k, colour, gesture| {
		let colour = form::rgb(colour);
		bar_edit(gesture, &|s| {
			if let Some(stop) = usize::try_from(k).ok().and_then(|k| s.stops.get_mut(k)) {
				stop.color = colour;
			}
			k
		});
	});
	wire_canvas(&state, &app);
	let (st, weak) = (state.clone(), app.as_weak());
	app.global::<ui::Form<'_>>().on_load_scheme_file(move |layer| {
		let Some(app) = weak.upgrade() else { return };
		let s = st.borrow();
		files::pick_scheme(&s.inbox, &app.global::<ui::Words<'_>>(), layer);
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.global::<ui::Form<'_>>().on_toggle_mine(move || {
		let Some(app) = weak.upgrade() else { return };
		let s = &mut *st.borrow_mut();
		s.toggle_mine();
		s.show(&app);
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.global::<ui::Form<'_>>().on_remove_my_scheme(move |layer| {
		let Some(app) = weak.upgrade() else { return };
		let s = &mut *st.borrow_mut();
		let Some(store) = &s.store else { return };
		let name = form::layer_settings(&mut s.doc.view, layer).color_scheme.clone();
		if waveduderust_platform::user_schemes::remove(store, &name).is_ok() {
			tracing::info!("Colour scheme '{name}' removed from my schemes");
		}
		s.schemes = waveduderust_platform::user_schemes::load(store);
		s.show(&app);
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.global::<ui::Form<'_>>().on_edit_scheme_copy(move |layer| {
		let Some(app) = weak.upgrade() else { return };
		let s = &mut *st.borrow_mut();
		let name = form::layer_settings(&mut s.doc.view, layer).color_scheme.clone();
		let Some((scheme, own)) = form::find_scheme(&s.doc.project, &s.schemes, &name) else { return };
		let mut copy = scheme.clone();
		if own {
			copy.description = format!("copy of project scheme '{}'", scheme.name);
		}
		copy.name = format!("{} copy", scheme.name);
		let mut at = 0;
		s.doc.edit_both(None, |p, v| {
			p.add_color_scheme(copy);
			at = p.color_schemes.len() - 1;
			form::layer_settings(v, layer).color_scheme.clone_from(&p.color_schemes[at].name);
		});
		tracing::info!("Colour scheme '{}' copied into the project for editing", s.doc.project.color_schemes[at].name);
		s.selection.set(Node::Scheme(at));
		s.show(&app);
	});

	let weak = app.as_weak();
	bar.on_marker_at(move |at, reach, selected| {
		weak.upgrade().map_or(-1, |app| form::marker_at(&app.global::<ui::Form<'_>>().get_stops(), at, reach, selected))
	});
	let weak = app.as_weak();
	bar.on_stop_beside(move |k, step| {
		weak.upgrade().map_or(-1, |app| form::stop_beside(&app.global::<ui::Form<'_>>().get_stops(), k, step))
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.global::<ui::Form<'_>>().on_base_on(move |picked| {
		let Some(app) = weak.upgrade() else { return };
		let mut s = st.borrow_mut();
		if let (Node::Scheme(i), Ok(k)) = (s.selection.current(), usize::try_from(picked)) {
			let base = form::bases(&s.doc.project, &s.schemes, i).get(k).map(|&b| b.clone());
			if let Some(base) = base {
				s.doc.edit(|p| {
					if let Some(scheme) = p.color_schemes.get_mut(i) {
						form::base_on(scheme, &base);
					}
				});
				app.global::<ui::Form<'_>>().set_based_on(picked);
			}
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
			s.example_topic = i;
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
			show_settings(s, &app);
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
			show_settings(s, &app);
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	app.on_settings_edited(move || {
		if let Some(app) = weak.upgrade() {
			let s = &mut *st.borrow_mut();
			settings::apply(&app.get_settings(), &mut s.settings, s.store.as_ref());
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
	app.on_transport_speed(move |text| {
		let speed = &mut st.borrow_mut().playback.speed;
		if let Some(v) = number::parse(&text) {
			*speed = v.clamp(0.01, 100.0);
		}
		speed.to_string().into()
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
			let shown = s.live.poll(&s.doc.project, &s.doc.view, &s.schemes);
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
								tell(&app, ui::Message::ExportFailed, "");
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
			for (layer, name, text) in picked {
				if let Some(layer) = layer {
					s.load_scheme(layer, &name, text);
					s.show(&app);
					continue;
				}
				match s.doc.open_text(&name, &text) {
					Ok(()) => {
						s.opened();
						tracing::info!("Opened {name}");
					}
					Err(e) => {
						tracing::error!("cannot open {name}: {e}");
						tell(&app, ui::Message::OpenFailed, &name);
					}
				}
			}
			if s.messages.take() {
				s.messages.show(&app);
			}
			if let Some((layer, fit)) = shown.fitted {
				s.fitted(layer, fit);
				s.show(&app);
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
			app.global::<ui::Canvas<'_>>().set_computing(shown.computing);
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

/// The pictures' handles: selection by click, drags as one undo step each.
fn wire_canvas(state: &Rc<RefCell<State>>, app: &AppWindow) {
	let canvas = app.global::<ui::Canvas<'_>>();
	state.borrow().overlays.wire(&canvas);
	state.borrow().graphs.wire(&canvas);
	let (st, weak) = (state.clone(), app.as_weak());
	canvas.on_rescaled(move |layer, scale| {
		let Some(app) = weak.upgrade() else { return };
		// reported during a layout, which may run while the state is borrowed: taken up once it is free
		let st = st.clone();
		slint::Timer::single_shot(std::time::Duration::ZERO, move || {
			let s = &mut *st.borrow_mut();
			if s.overlays.rescale(layer, scale) {
				s.overlays.show(&app.global::<ui::Canvas<'_>>(), &s.doc.project, &s.doc.view, &s.selection);
			}
		});
	});
	let (st, weak) = (state.clone(), app.as_weak());
	canvas.on_press(move |layer, x, y, control, shift| {
		let Some(app) = weak.upgrade() else { return };
		let s = &mut *st.borrow_mut();
		s.drags += 1;
		let boxes = s.graphs.placed(layer);
		let press = canvas::Press { layer, at: (x, y), control, shift };
		s.gesture = s.overlays.press(&s.doc.project, &mut s.selection, press, s.drags, boxes);
		app.global::<ui::Canvas<'_>>().set_dragging(s.gesture.is_some());
		s.show(&app);
	});
	let (st, weak) = (state.clone(), app.as_weak());
	canvas.on_drag(move |dx, dy, shift| {
		let Some(app) = weak.upgrade() else { return };
		let s = &mut *st.borrow_mut();
		match &mut s.gesture {
			Some(canvas::Gesture::Handle(drag)) => {
				let key = drag.key().to_owned();
				s.doc.edit_merging(Some(&key), |p| drag.apply(p, (dx, dy), shift));
				s.show(&app);
			}
			Some(canvas::Gesture::Marquee(m)) => {
				m.stretch((dx, dy));
				let canvas = app.global::<ui::Canvas<'_>>();
				canvas.set_marquee(m.rect());
				canvas.set_marquee_layer(m.layer());
				canvas.set_marquee_shown(true);
			}
			None => {}
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	canvas.on_release(move || {
		let Some(app) = weak.upgrade() else { return };
		let s = &mut *st.borrow_mut();
		s.finish_gesture(&app);
		s.show(&app);
	});
	let (st, weak) = (state.clone(), app.as_weak());
	canvas.on_brightness(move |layer, value| {
		let Some(app) = weak.upgrade() else { return };
		let s = &mut *st.borrow_mut();
		s.set_brightness(canvas::model_layer(layer), number::wide(value));
		s.show(&app);
	});
	let (st, weak) = (state.clone(), app.as_weak());
	canvas.on_auto(move |layer| {
		let Some(app) = weak.upgrade() else { return };
		let s = &mut *st.borrow_mut();
		let layer = canvas::model_layer(layer);
		if let live::Fit::Done(fit) = s.live.auto(layer) {
			s.fitted(layer, fit);
		}
		s.show(&app);
	});
	let (st, weak) = (state.clone(), app.as_weak());
	canvas.on_hover(move |layer, x, y| {
		let Some(app) = weak.upgrade() else { return };
		let s = st.borrow();
		let canvas = app.global::<ui::Canvas<'_>>();
		let under = s.graphs.under(layer, (x, y)).map_or(-1, |i| i32::try_from(i).unwrap_or(-1));
		canvas.set_hover_layer(layer);
		canvas.set_hover_box(under);
	});
	let (st, weak) = (state.clone(), app.as_weak());
	canvas.on_export_probe_csv(move |probe| {
		let Some(app) = weak.upgrade() else { return };
		let s = &mut *st.borrow_mut();
		if let Ok(i) = usize::try_from(probe) {
			s.selection.set(Node::Probe(i));
			export_probe_csv(s, &app, i);
			s.show(&app);
		}
	});
	let (st, weak) = (state.clone(), app.as_weak());
	canvas.on_key(move |layer, key, shift| {
		use ui::HandleKey;
		let Some(app) = weak.upgrade() else { return false };
		let s = &mut *st.borrow_mut();
		let step = if shift { 10. } else { 1. };
		// a lever turns counter-clockwise on Right and Up
		let (d, turn) = match key {
			HandleKey::Enter | HandleKey::Next | HandleKey::Previous => {
				let taken = if key == HandleKey::Enter {
					s.overlays.enter(&s.doc.project, &mut s.selection, layer, s.graphs.placed(layer))
				} else {
					let back = key == HandleKey::Previous;
					s.overlays.tab(&s.doc.project, &mut s.selection, layer, back, s.graphs.placed(layer))
				};
				s.show(&app);
				return taken;
			}
			HandleKey::Left => ((-step, 0.), -step),
			HandleKey::Right => ((step, 0.), step),
			HandleKey::Up => ((0., -step), step),
			HandleKey::Down => ((0., step), -step),
		};
		let Some(key) = s.overlays.nudge_key(layer, &s.selection) else { return false };
		let boxes = s.graphs.placed(layer);
		if s.overlays.focused_box(layer, &s.doc.project, boxes).is_some() {
			s.doc.edit_merging(Some(&key), |p| s.overlays.nudge_box(p, layer, d, boxes));
		} else {
			s.doc.edit_merging(Some(&key), |p| s.overlays.nudge(p, &s.selection, layer, d, turn));
		}
		s.show(&app);
		true
	});
	let (st, weak) = (state.clone(), app.as_weak());
	canvas.on_cancel(move || {
		let Some(app) = weak.upgrade() else { return };
		let s = &mut *st.borrow_mut();
		end_gesture(&app);
		if let Some(canvas::Gesture::Handle(drag)) = s.gesture.take() {
			s.doc.cancel(drag.key());
		}
		s.show(&app);
	});
}

/// Writes probe `i`'s recording to a CSV file the user names (the browser: a download).
fn export_probe_csv(state: &State, app: &AppWindow, i: usize) {
	let Some(probe) = state.doc.project.probes.get(i) else { return };
	let Some(csv) = state.live.probe_csv(probe) else {
		tracing::info!("{}: no recording to export yet", probe.name);
		return;
	};
	let name = format!("{}.csv", probe.name);
	#[cfg(not(target_arch = "wasm32"))]
	{
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
		let _ = app;
		if let Err(e) = files::download(&name, &String::from_utf8_lossy(&csv), "text/csv") {
			tracing::error!("{e}");
		}
	}
}

fn end_gesture(app: &AppWindow) {
	let canvas = app.global::<ui::Canvas<'_>>();
	canvas.set_dragging(false);
	canvas.set_marquee_shown(false);
}
