//! The app's state and how it shows in the window.

use std::path::PathBuf;
use std::rc::Rc;

use slint::{ComponentHandle, VecModel};

use crate::document::{Document, MergeKey};
use crate::live::Live;
use crate::messages::Messages;
use crate::playback::Playback;
use crate::selection::Selection;
use crate::tree::Node;
use crate::ui::{self, AppWindow};
use crate::{
	Action, canvas, clipboard, end_gesture, export, files, form, graphs, live, memory, models, scene_watch, selection,
	settings, tree, wake,
};

/// What the window shows.
pub(crate) struct State {
	pub(crate) doc: Document,
	pub(crate) scene_watch: scene_watch::SceneWatch,
	pub(crate) nodes: Vec<Node>,
	/// The window's tree rows, one per entry of `nodes`.
	pub(crate) tree_rows: Rc<VecModel<ui::TreeRow>>,
	pub(crate) selection: Selection,
	/// Tree rows whose children are hidden.
	pub(crate) collapsed: Vec<Node>,
	pub(crate) live: Live,
	pub(crate) playback: Playback,
	pub(crate) messages: Messages,
	/// The menu action the unsaved edits prompt holds.
	pub(crate) pending: Option<Action>,
	pub(crate) settings: waveduderust_platform::settings::Settings,
	pub(crate) store: Option<waveduderust_platform::store::Store>,
	/// The user's colour schemes.
	pub(crate) schemes: waveduderust::io::schemes::SchemeStore,
	pub(crate) facts: waveduderust::quality::MachineFacts,
	pub(crate) exporting: Option<export::Exporting>,
	pub(crate) inbox: files::Inbox,
	pub(crate) pasted: clipboard::Pasted,
	pub(crate) example_topic: ui::ExampleTopic,
	pub(crate) example_search: String,
	/// The examples the dialog lists, by index into [`examples::all`].
	pub(crate) example_rows: Vec<usize>,
	/// The last run export, for Export last.
	pub(crate) last_export: Option<export::Plan>,
	pub(crate) export_dialog: Option<export::Dialog>,
	/// Unsaved edits, for the browser's question on leaving the page.
	pub(crate) unsaved: Rc<std::cell::Cell<bool>>,
	/// A form focus request was seen pending at the last tick.
	pub(crate) focus_seen: bool,
	/// The plan of the export running.
	pub(crate) export_plan: Option<export::Plan>,
	/// The export asked about and the outputs it would replace, asked one by one.
	pub(crate) replacing: Option<(export::Plan, Vec<(PathBuf, bool)>)>,
	/// A completed export's file, downloaded when its message window's Download is clicked.
	#[cfg(target_arch = "wasm32")]
	pub(crate) ready: Option<files::Download>,
	/// The document generation the run started from.
	pub(crate) ran: u64,
	/// The view generation the pictures are drawn with.
	pub(crate) styled: u64,
	/// Display > Field / Intensity > Preview as last shown.
	pub(crate) pictures: (bool, bool),
	/// The memory warning's boxes while it is open.
	pub(crate) warning_ticks: Option<waveduderust::run::Levers>,
	pub(crate) graphs: graphs::Graphs,
	/// The desktop's colour scheme once a forced theme has written the palette's, which ends its system binding
	/// (docs/blockers.md, Slint's system colour scheme).
	pub(crate) system_scheme: Option<slint::language::ColorScheme>,
	/// The project's and the view's generations the Properties form last saw, and the count of changes made not
	/// through it (undo, a removal, a reorder, another project), which may put another object under the same node.
	pub(crate) form_seen: (u64, u64),
	pub(crate) form_base: u64,
	/// The typed form shown: its object's identity and the token that drops text typed for another.
	pub(crate) typed_form: Shown,
	pub(crate) overlays: canvas::Overlays,
	/// A handle drag or a marquee under way.
	pub(crate) gesture: Option<canvas::Gesture>,
	/// Presses on the pictures so far; each drag's undo key.
	pub(crate) drags: u64,
}

/// The typed form shown: its key, the token its editors carry and the lists it caches.
#[derive(Default)]
pub(crate) struct Shown {
	key: Option<(Node, u64)>,
	token: i32,
	node: Option<Node>,
	// the schemes Based on lists
	bases: Vec<waveduderust::model::ColorScheme>,
	// the stops the gradient bar shows, and whether the bar changed them since
	stops: Vec<waveduderust::model::Stop>,
	pub(crate) bar_edited: bool,
	// the Display layers' pickers
	field_schemes: Option<form::LayerPicker>,
	intensity_schemes: Option<form::LayerPicker>,
}

impl State {
	/// Applies the edits of the selected object's typed form; `typing`: a keystroke, joining the run of keystrokes
	/// into one undo step.
	pub(crate) fn edit_form(&mut self, form: &ui::Form<'_>, typing: bool) {
		let node = self.selection.current();
		let key = typing.then_some(MergeKey::Form(node));
		if node == Node::Display {
			self.doc.edit_view_merging(key, |v| form::apply_view(form, v));
		} else if let Node::Scheme(i) = node {
			// a renamed scheme takes its layers along, in the same step
			self.doc.edit_both(key, |p, v| {
				let old = p.color_schemes.get(i).map(|s| s.name.clone());
				form::apply(form, p, node);
				if let (Some(old), Some(new)) = (old, p.color_schemes.get(i).map(|s| &s.name)) {
					v.rename_scheme(&old, new);
				}
			});
		} else {
			self.doc.edit_merging(key, |p| form::apply(form, p, node));
		}
		self.form_seen = (self.doc.generation, self.doc.view_generation);
	}

	/// Expands `node`'s row or collapses it; a selection it hides moves up to it (WPF's TreeView).
	pub(crate) fn expand(&mut self, node: Node, open: bool) {
		self.collapsed.retain(|&n| n != node);
		if !open {
			self.collapsed.push(node);
			if self.selection.group().iter().any(|&n| n != node && tree::within(n, node)) {
				self.selection.set(node);
			}
		}
	}

	/// Opens the memory warning the run asks, with the proposal ticked; closes one no longer asked.
	pub(crate) fn show_warning(&mut self, app: &AppWindow) {
		match (self.live.warning(), self.warning_ticks) {
			(Some((_, w)), None) => {
				let ticks = w.proposal();
				self.warning_ticks = Some(ticks);
				app.set_memory_warning(memory::data(&w, ticks));
				app.set_memory_warning_shown(true);
			}
			// the request may have changed under it
			(Some((_, w)), Some(ticks)) => app.set_memory_warning(memory::data(&w, ticks)),
			(None, Some(_)) => {
				self.warning_ticks = None;
				app.set_memory_warning_shown(false);
			}
			(None, None) => {}
		}
	}

	/// `edit` on the export dialog's data with the dialog open; none open, nothing.
	pub(crate) fn edit_export(&self, app: &AppWindow, edit: impl FnOnce(&export::Dialog, &mut ui::ExportData)) {
		let Some(dialog) = &self.export_dialog else { return };
		let mut data = app.get_export_data();
		edit(dialog, &mut data);
		app.set_export_data(data);
	}

	/// Saves the settings; a failure is logged.
	pub(crate) fn save_settings(&self) {
		settings::save(&self.settings, self.store.as_ref());
	}

	/// Pushes the state into the window's properties.
	pub(crate) fn show(&mut self, app: &AppWindow) {
		wake::wake();
		self.unsaved.set(self.doc.dirty());
		if self.export_dialog.is_none() {
			app.set_export_shown(false);
		}
		self.selection.retain(&self.doc.project);
		// a row selected elsewhere (an add, a paste) shows its folder
		let current = self.selection.current();
		self.collapsed.retain(|&n| n == current || !tree::within(current, n));
		let (nodes, rows) = tree::rows(&self.doc.project, &self.selection, &self.collapsed);
		self.nodes = nodes;
		models::update(&self.tree_rows, rows);
		let current = self.nodes.iter().position(|&n| n == self.selection.current());
		app.set_tree_current(models::index_to_ui(current));
		app.set_file_name(self.doc.name().unwrap_or_default().into());
		app.set_file_path(self.doc.path.as_deref().map_or_default(|p| p.display().to_string()).into());
		app.set_dirty(self.doc.dirty());
		app.set_commands(self.commands());
		let mut change = live::Change::None;
		if self.ran != self.doc.generation {
			self.ran = self.doc.generation;
			change = self.live.edited(&self.doc.project, &self.doc.view);
		}
		if self.styled != self.doc.view_generation {
			self.styled = self.doc.view_generation;
			change = change.max(self.live.restyle(&self.doc.project, &self.doc.view));
		}
		// a simulation edit: the frames go, playback stops (docs/ui/60, Edit classes)
		if change == live::Change::Frames {
			self.playback.pause();
		}
		self.show_warning(app);
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
		let pictures = (self.doc.view.field.preview, self.doc.view.intensity.preview);
		if pictures != self.pictures {
			// a picture came or went (the toggles, the form, undo): a gesture on it gets no release
			self.pictures = pictures;
			self.finish_gesture(app);
			for (layer, shown) in [(ui::Layer::Field, pictures.0), (ui::Layer::Intensity, pictures.1)] {
				if !shown {
					self.overlays.forget(layer);
				}
			}
		}
		self.overlays.show(&app.global::<ui::Canvas<'_>>(), &self.doc.project, &self.doc.view, &self.selection);
		self.show_graphs();
		self.show_box_focus(app);
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
		let key = (node, self.form_base);
		let g = app.global::<ui::Form<'_>>();
		g.set_kind(kind);
		if self.typed_form.node != Some(node) {
			self.typed_form.node = Some(node);
			g.set_based_on(-1);
			g.set_object(g.get_object().wrapping_add(1));
		}
		if self.typed_form.key != Some(key) {
			self.typed_form.key = Some(key);
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
	pub(crate) fn opened(&mut self) {
		self.scene_watch = scene_watch::SceneWatch::default();
		self.find_scene_image();
		self.selection.set(Node::Project);
		self.last_export = None;
		(self.export_dialog, self.replacing) = (None, None);
		self.playback.reset();
		self.live.load(&self.doc.project, &self.doc.view);
		self.playback.frames = self.live.frames;
		self.ran = self.doc.generation;
	}

	/// A scene image without a path is looked for by name beside the project and in its `scene_images` folder; the
	/// path found is adopted, relative.
	fn find_scene_image(&mut self) {
		let scene = &self.doc.project.scene;
		let Some(folder) = self.doc.path.as_deref().and_then(std::path::Path::parent) else { return };
		if !scene.image_path.is_empty() || scene.image.is_empty() {
			return;
		}
		let found = [folder.join(&scene.image), folder.join("scene_images").join(&scene.image)]
			.into_iter()
			.find(|p| p.is_file());
		if let Some(found) = found {
			let path = waveduderust::io::stored_image_path(Some(folder), &found.to_string_lossy());
			self.doc.settle(|p| p.scene.image_path.clone_from(&path));
		}
	}

	/// Whether the current scheme is among the user's.
	fn mine(&self) -> ui::Mine {
		match (self.selection.current(), &self.store) {
			(Node::Scheme(i), Some(_)) => self.doc.project.color_schemes.get(i).map_or(ui::Mine::Hidden, |s| {
				if waveduderust_platform::user_schemes::has(&self.schemes, &s.name) {
					ui::Mine::Yes
				} else {
					ui::Mine::No
				}
			}),
			_ => ui::Mine::Hidden,
		}
	}

	/// Adds the scheme of the file `name` holding `text` to the project and points `layer` at it.
	pub(crate) fn load_scheme(&mut self, layer: ui::Layer, name: &str, text: String) {
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

	/// Why a run cannot start (docs/ui/60, Run start errors): an image scene without its image.
	pub(crate) fn start_error(&self) -> Option<&'static str> {
		let scene = &self.doc.project.scene;
		if scene.mode != waveduderust::model::SceneMode::Image || scene.image_data.is_some() {
			return None;
		}
		let relative = !scene.image_path.is_empty() && std::path::Path::new(&scene.image_path).is_relative();
		Some(if relative && self.doc.path.is_none() {
			"Save the project first, so the scene image can be found relative to it."
		} else {
			"The scene uses an image, but no image file is set."
		})
	}

	/// Run is offered: no run or export under way, and a source enabled to drive one.
	pub(crate) fn can_run(&self, exporting: bool) -> bool {
		!self.live.active() && !exporting && self.doc.project.has_enabled_source()
	}

	/// Whether `node` is an active probe, which a run records.
	fn active_probe(&self, node: Node) -> bool {
		matches!(node, Node::Probe(i) if self.doc.project.probes.get(i).is_some_and(|p| p.active))
	}

	/// Lays out the probe graph boxes at the frame shown.
	pub(crate) fn show_graphs(&mut self) {
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

	/// Marks the graph box with the keyboard focus.
	pub(crate) fn show_box_focus(&self, app: &AppWindow) {
		let canvas = app.global::<ui::Canvas<'_>>();
		let focus = [ui::Layer::Field, ui::Layer::Intensity].into_iter().find_map(|layer| {
			let (b, grip) = self.overlays.focused_box(layer, &self.doc.project, self.graphs.placed(layer))?;
			Some((layer, b.probe, grip))
		});
		canvas.set_focus_box(models::index_to_ui(focus.map(|(_, i, _)| i)));
		if let Some((layer, _, grip)) = focus {
			canvas.set_focus_layer(layer);
			canvas.set_focus_grip(grip);
		}
	}

	/// Sets `layer`'s brightness, one undo step.
	pub(crate) fn set_brightness(&mut self, layer: waveduderust::model::Layer, value: f64) {
		self.doc.edit_view_merging(None, |v| v.layer_mut(layer).brightness = value);
	}

	/// Takes an Auto brightness fit.
	pub(crate) fn fitted(&mut self, layer: waveduderust::model::Layer, fit: Option<f64>) {
		match fit {
			Some(b) => self.set_brightness(layer, b),
			None => tracing::info!("Auto brightness: nothing to fit"),
		}
	}

	/// Ends a picture gesture as its release does.
	pub(crate) fn finish_gesture(&mut self, app: &AppWindow) {
		end_gesture(app);
		match self.gesture.take() {
			// a drag that ends where it began makes its item the selection
			Some(canvas::Gesture::Handle(drag)) if !drag.moved() => self.selection.set(drag.node()),
			Some(canvas::Gesture::Marquee(m)) => m.select(&self.overlays, &self.doc.project, &mut self.selection),
			_ => {}
		}
	}

	/// Keeps the current scheme among the user's, or removes the user's of its name.
	pub(crate) fn toggle_mine(&mut self) {
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
	pub(crate) fn commands(&self) -> ui::Commands {
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
			can_run: self.can_run(exporting),
			run_active: self.live.active(),
			paused: self.live.paused(),
			can_stop: self.live.active() || exporting,
			can_discard: self.live.has_frames() && !exporting,
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

	pub(crate) fn node(&self, row: i32) -> Option<Node> {
		models::index_from_ui(row).and_then(|r| self.nodes.get(r).copied())
	}
}
