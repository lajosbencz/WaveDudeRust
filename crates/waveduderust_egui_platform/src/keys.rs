//! The window's keys of docs/ui/10 (Global and focus-scoped keys), each mapped to its [`MenuAction`].

use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use waveduderust_app::menus::{ExportKindChoice, MenuAction, ZoomStep};
use waveduderust_app::preview::PaneKey;

const fn key(modifiers: Modifiers, key: Key) -> KeyboardShortcut {
	KeyboardShortcut::new(modifiers, key)
}

const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);
const NONE: Modifiers = Modifiers::NONE;

/// The shortcuts; the first of an action is the one its menu entry shows.
pub const SHORTCUTS: [(KeyboardShortcut, MenuAction); 32] = [
	(key(CMD, Key::N), MenuAction::New),
	(key(CMD, Key::O), MenuAction::Open),
	(key(CMD_SHIFT, Key::S), MenuAction::SaveAs),
	(key(CMD, Key::S), MenuAction::Save),
	(key(CMD, Key::Comma), MenuAction::Settings),
	(key(CMD_SHIFT, Key::E), MenuAction::Export(ExportKindChoice::Field)),
	(key(CMD_SHIFT, Key::I), MenuAction::Export(ExportKindChoice::Intensity)),
	(key(CMD_SHIFT, Key::B), MenuAction::Export(ExportKindChoice::Both)),
	(key(CMD_SHIFT, Key::F), MenuAction::Export(ExportKindChoice::FieldFrame)),
	(key(CMD_SHIFT, Key::G), MenuAction::Export(ExportKindChoice::IntensityFrame)),
	(key(CMD, Key::E), MenuAction::ExportLast),
	(key(CMD, Key::Y), MenuAction::Redo),
	(key(CMD_SHIFT, Key::Z), MenuAction::Redo),
	(key(CMD, Key::Z), MenuAction::Undo),
	(key(CMD, Key::X), MenuAction::Cut),
	(key(Modifiers::SHIFT, Key::Delete), MenuAction::Cut),
	(key(CMD, Key::C), MenuAction::Copy),
	(key(Modifiers::CTRL, Key::Insert), MenuAction::Copy),
	(key(CMD, Key::V), MenuAction::Paste),
	(key(Modifiers::SHIFT, Key::Insert), MenuAction::Paste),
	(key(CMD, Key::A), MenuAction::SelectAll),
	(key(NONE, Key::F1), MenuAction::ProjectPage),
	(key(Modifiers::SHIFT, Key::F5), MenuAction::Stop),
	(key(NONE, Key::F5), MenuAction::Run),
	(key(NONE, Key::F6), MenuAction::PauseRun),
	(key(NONE, Key::F7), MenuAction::Play),
	(key(NONE, Key::Escape), MenuAction::Stop),
	// Ctrl+Plus is Ctrl+Shift+= on some layouts.
	(key(CMD, Key::Equals), MenuAction::Zoom(ZoomStep::In)),
	(key(CMD, Key::Plus), MenuAction::Zoom(ZoomStep::In)),
	(key(CMD_SHIFT, Key::Plus), MenuAction::Zoom(ZoomStep::In)),
	(key(CMD, Key::Minus), MenuAction::Zoom(ZoomStep::Out)),
	(key(CMD, Key::Num0), MenuAction::Zoom(ZoomStep::Reset)),
];

/// The tree's keys on its focused row that act on the selection.
pub const TREE_KEYS: [(KeyboardShortcut, MenuAction); 7] = [
	(key(Modifiers::ALT, Key::ArrowUp), MenuAction::MoveUp),
	(key(Modifiers::ALT, Key::ArrowDown), MenuAction::MoveDown),
	(key(CMD, Key::D), MenuAction::Duplicate),
	(key(NONE, Key::Insert), MenuAction::ToggleSelected),
	(key(CMD, Key::Space), MenuAction::ToggleSelected),
	(key(NONE, Key::Space), MenuAction::ToggleEnabled),
	(key(NONE, Key::Delete), MenuAction::Delete),
];

/// The pictures' playback keys, with no modifier or Shift alone, while the focus is on them.
pub const PANE_KEYS: [(Key, PaneKey); 10] = [
	(Key::Space, PaneKey::TogglePlay),
	(Key::Comma, PaneKey::StepBack),
	(Key::Period, PaneKey::StepForward),
	(Key::I, PaneKey::SetIn),
	(Key::O, PaneKey::SetOut),
	(Key::X, PaneKey::ClearRange),
	(Key::L, PaneKey::ToggleLoop),
	(Key::F, PaneKey::ToggleLive),
	(Key::Home, PaneKey::RangeStart),
	(Key::End, PaneKey::RangeEnd),
];
