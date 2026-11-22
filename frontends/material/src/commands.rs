//! One command vocabulary behind every entry point (DESIGN.md, Commands, menus, keys): the [`MenuAction`]s of the
//! menu bar and this window's [`ViewCommand`]s, each an app command (`shortcut::ActionId`) that menu items, palette
//! entries and keys fire; the window takes the fired ones each pass.

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use egui_mineral::semantic::KeySalt;
use egui_mineral::shortcut::{self, ActionId, Scope};
use waveduderust_app::menus::{self, MenuAction, On};
use waveduderust_app::preview::PaneKey;
use waveduderust_app::settings::Orientation;
use waveduderust_app::window::Pane;
use waveduderust_egui_platform::keys::{PANE_KEYS, SHORTCUTS};

use crate::App;

/// Which pictures the canvas shows (= Display layer Preview).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, KeySalt)]
pub(crate) enum Layers {
	Field,
	Both,
	Intensity,
}

impl Layers {
	pub(crate) const ALL: [Self; 3] = [Self::Field, Self::Both, Self::Intensity];

	/// What the project shows; `None` when both pictures are hidden.
	pub(crate) fn of(app: &App) -> Option<Self> {
		let d = &app.document.view;
		match (d.field.preview, d.intensity.preview) {
			(true, true) => Some(Self::Both),
			(true, false) => Some(Self::Field),
			(false, true) => Some(Self::Intensity),
			(false, false) => None,
		}
	}

	/// The string key of its name.
	pub(crate) fn label_key(self) -> &'static str {
		match self {
			Self::Field => "MenuViewField",
			Self::Both => "MenuViewBoth",
			Self::Intensity => "MenuViewIntensity",
		}
	}

	/// Whether the field and the intensity show.
	pub(crate) fn shows(self) -> (bool, bool) {
		(self != Self::Intensity, self != Self::Field)
	}
}

/// What this window does beyond the menu bar's [`MenuAction`]s.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum ViewCommand {
	Outline,
	Inspector,
	Messages,
	/// Hides outline and inspector; again restores them.
	CanvasOnly,
	Layers(Layers),
	Orientation(Orientation),
	Fit,
	ZoomIn,
	ZoomOut,
	/// Display: every item's handles, or the selected ones' alone.
	Handles,
	/// Display: every item's label, or the selected ones' alone.
	Labels,
	/// Entry of `settings_table::ZOOMS`.
	UiZoom(usize),
	Palette,
	Shortcuts,
	Pane(PaneKey),
	/// Leaves the page shown.
	Back,
}

/// A command of this window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Cmd {
	Act(MenuAction),
	View(ViewCommand),
}

impl Cmd {
	/// The app command its views fire.
	pub(crate) fn id(self) -> ActionId {
		ActionId::from(self.text())
	}

	fn text(self) -> String {
		match self {
			Self::Act(action) => action.command_id(),
			Self::View(view) => view_key(view),
		}
	}
}

/// Keyed by its app command's id, so items keep their agent keys across menus and the palette.
impl KeySalt for Cmd {
	fn write_key(&self, out: &mut String) {
		out.push_str(&self.text());
	}
}

/// `prefix` joined with a fixed `leaf`.
fn key(prefix: &str, leaf: &str) -> String {
	format!("{prefix}.{leaf}")
}

fn view_key(view: ViewCommand) -> String {
	use ViewCommand as V;
	let fixed = match view {
		V::Outline => "window.outline",
		V::Inspector => "window.inspector",
		V::Messages => "window.messages",
		V::CanvasOnly => "window.canvas_only",
		V::Layers(layers) => {
			return key(
				"window.layers",
				match layers {
					Layers::Field => "field",
					Layers::Both => "both",
					Layers::Intensity => "intensity",
				},
			);
		}
		V::Orientation(o) => {
			return key(
				"window.orientation",
				match o {
					Orientation::Horizontal => "horizontal",
					Orientation::Vertical => "vertical",
				},
			);
		}
		V::Fit => "window.fit",
		V::ZoomIn => "window.zoom_in",
		V::ZoomOut => "window.zoom_out",
		V::Handles => "window.handles",
		V::Labels => "window.labels",
		V::UiZoom(i) => return format!("window.ui_zoom.{i}"),
		V::Palette => "window.palette",
		V::Shortcuts => "window.shortcuts",
		V::Pane(k) => return key("window.pane", pane_key_name(k)),
		V::Back => "window.back",
	};
	fixed.to_owned()
}

fn pane_key_name(key: PaneKey) -> &'static str {
	match key {
		PaneKey::TogglePlay => "toggle_play",
		PaneKey::StepBack => "step_back",
		PaneKey::StepForward => "step_forward",
		PaneKey::SetIn => "set_in",
		PaneKey::SetOut => "set_out",
		PaneKey::ClearRange => "clear_range",
		PaneKey::ToggleLoop => "toggle_loop",
		PaneKey::ToggleLive => "toggle_live",
		PaneKey::RangeStart => "range_start",
		PaneKey::RangeEnd => "range_end",
	}
}

/// This window's window-wide keys beyond [`SHORTCUTS`].
pub(crate) const VIEW_KEYS: [(KeyboardShortcut, ViewCommand); 5] = [
	(KeyboardShortcut::new(Modifiers::ALT, Key::Num1), ViewCommand::Outline),
	(KeyboardShortcut::new(Modifiers::ALT, Key::Num2), ViewCommand::Inspector),
	(KeyboardShortcut::new(Modifiers::ALT, Key::Num3), ViewCommand::Messages),
	(KeyboardShortcut::new(Modifiers::ALT, Key::Num0), ViewCommand::CanvasOnly),
	(KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::P), ViewCommand::Palette),
];

const ESCAPE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Escape);

/// The key a view of `cmd` shows: its first window-wide one.
pub(crate) fn key_of(cmd: Cmd) -> Option<KeyboardShortcut> {
	match cmd {
		Cmd::Act(action) => SHORTCUTS.iter().find(|(_, a)| *a == action).map(|(k, _)| *k),
		Cmd::View(view) => VIEW_KEYS.iter().find(|(_, v)| *v == view).map(|(k, _)| *k),
	}
}

/// This build can look for updates.
const fn updater_built() -> bool {
	#[cfg(not(target_arch = "wasm32"))]
	return waveduderust_app::update::AVAILABLE;
	#[cfg(target_arch = "wasm32")]
	false
}

/// Whether this window offers `action` yet.
pub(crate) fn offered(action: MenuAction) -> bool {
	match action {
		MenuAction::New
		| MenuAction::Open
		| MenuAction::Recent(_)
		| MenuAction::Save
		| MenuAction::SaveAs
		| MenuAction::Close
		| MenuAction::Exit
		| MenuAction::Examples
		| MenuAction::Welcome
		| MenuAction::Settings
		| MenuAction::About
		| MenuAction::Zoom(_)
		| MenuAction::Run
		| MenuAction::PauseRun
		| MenuAction::Stop
		| MenuAction::DiscardFrames
		| MenuAction::Play
		| MenuAction::ProjectPage
		| MenuAction::Pane(Pane::Project | Pane::Properties | Pane::Messages)
		| MenuAction::Export(_)
		| MenuAction::ExportLast
		| MenuAction::ExportProbeCsv
		| MenuAction::Undo
		| MenuAction::Redo
		| MenuAction::Cut
		| MenuAction::Copy
		| MenuAction::Paste
		| MenuAction::Delete
		| MenuAction::SelectAll
		| MenuAction::AddSource
		| MenuAction::AddPart(_)
		| MenuAction::AddProbe(_)
		| MenuAction::ToggleSelected
		| MenuAction::ToggleEnabled
		| MenuAction::Duplicate
		| MenuAction::MoveUp
		| MenuAction::MoveDown
		| MenuAction::ToggleUserScheme => true,
		MenuAction::CheckForUpdates => updater_built(),
		MenuAction::Pane(Pane::Preview) => false,
	}
}

/// Whether `cmd` can run now: offered here and enabled by the state.
pub(crate) fn runs(app: &App, cmd: Cmd) -> bool {
	match cmd {
		Cmd::Act(action) => offered(action) && menus::enabled(app, action, On::Current),
		Cmd::View(ViewCommand::Pane(PaneKey::ToggleLive)) => app.live.run_is_active(),
		Cmd::View(ViewCommand::Pane(PaneKey::ClearRange)) => app.live.playback.has_range(),
		Cmd::View(_) => true,
	}
}

/// Binds the window-wide keys of the commands that run now; on a page Escape leaves it instead of stopping a run,
/// while the canvas takes it (`canvas`: a drag, an open menu) the canvas does.
pub(crate) fn bind_keys(ctx: &egui::Context, app: &App, page: bool, canvas: bool) {
	let acts = SHORTCUTS.iter().map(|&(k, a)| (k, Cmd::Act(a)));
	let views = VIEW_KEYS.iter().map(|&(k, v)| (k, Cmd::View(v)));
	for (key, cmd) in acts.chain(views) {
		if !((page || canvas) && key == ESCAPE) && runs(app, cmd) {
			shortcut::bind(ctx, Scope::Global, key, cmd.id());
		}
	}
	if page {
		shortcut::bind(ctx, Scope::Global, ESCAPE, Cmd::View(ViewCommand::Back).id());
	}
}

/// Binds the pictures' playback keys, bare or with Shift alone, while the focus is on `widget`.
pub(crate) fn bind_pane_keys(ctx: &egui::Context, app: &App, widget: egui::Id) {
	for (key, pane) in PANE_KEYS {
		let cmd = Cmd::View(ViewCommand::Pane(pane));
		if runs(app, cmd) {
			for modifiers in [Modifiers::NONE, Modifiers::SHIFT] {
				shortcut::bind(ctx, Scope::Focus(widget), KeyboardShortcut::new(modifiers, key), cmd.id());
			}
		}
	}
}

/// The commands of `offered` fired this pass, each once.
pub(crate) fn take_fired(ctx: &egui::Context, offered: impl IntoIterator<Item = Cmd>) -> Vec<Cmd> {
	let mut fired = Vec::new();
	for cmd in offered {
		if !fired.contains(&cmd) && shortcut::take_action(ctx, &cmd.id()) {
			fired.push(cmd);
		}
	}
	fired
}

/// Every command a key may fire: the window-wide keys', the pictures' and the outline's.
pub(crate) fn keyed() -> impl Iterator<Item = Cmd> {
	let acts = SHORTCUTS.iter().map(|&(_, a)| Cmd::Act(a));
	let views = VIEW_KEYS.iter().map(|&(_, v)| Cmd::View(v));
	let panes = PANE_KEYS.iter().map(|&(_, p)| Cmd::View(ViewCommand::Pane(p)));
	acts.chain(views).chain(panes).chain([Cmd::View(ViewCommand::Back)]).chain(crate::workspace::outline_keys())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn each_command_has_its_own_id() {
		let cmds: Vec<Cmd> = keyed().collect();
		for (i, a) in cmds.iter().enumerate() {
			for b in &cmds[i + 1..] {
				assert!(a == b || a.id() != b.id(), "{a:?} and {b:?}");
			}
		}
	}

	#[test]
	fn a_key_names_one_command() {
		let keys: Vec<(KeyboardShortcut, Cmd)> = SHORTCUTS
			.iter()
			.map(|&(k, a)| (k, Cmd::Act(a)))
			.chain(VIEW_KEYS.iter().map(|&(k, v)| (k, Cmd::View(v))))
			.collect();
		for (i, (k, a)) in keys.iter().enumerate() {
			for (l, b) in &keys[i + 1..] {
				assert!(k != l || a == b, "{k:?}: {a:?} and {b:?}");
			}
		}
	}

	#[test]
	fn a_shown_key_is_the_first_of_its_command() {
		let redo = SHORTCUTS.iter().find(|(_, a)| *a == MenuAction::Redo).map(|p| p.0);
		assert_eq!(key_of(Cmd::Act(MenuAction::Redo)), redo);
		assert_eq!(key_of(Cmd::View(ViewCommand::CanvasOnly)), Some(VIEW_KEYS[3].0));
		assert_eq!(key_of(Cmd::View(ViewCommand::Fit)), None);
	}

	#[test]
	fn the_layers_name_what_shows() {
		assert_eq!(Layers::Both.shows(), (true, true));
		assert_eq!(Layers::Field.shows(), (true, false));
		assert_eq!(Layers::Intensity.shows(), (false, true));
	}
}
