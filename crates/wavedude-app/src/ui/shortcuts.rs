//! The window's keys of docs/ui/10 (Global and focus-scoped keys): each maps to a [`MenuAction`] and fires with
//! exactly its modifiers. An open menu takes every key; a focused text box keeps its editing keys. Copy, cut and
//! paste arrive as egui's clipboard events (the system's and the browser's gestures alike) and are taken before the
//! widgets see them; the other keys are taken after the widgets, so Esc stops the run only where no widget used it.

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

use crate::menus::{ExportKindChoice, MenuAction};

const fn key(modifiers: Modifiers, key: Key) -> KeyboardShortcut {
	KeyboardShortcut::new(modifiers, key)
}

const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);
const NONE: Modifiers = Modifiers::NONE;

/// The shortcuts; the first of an action is the one its menu entry shows.
const SHORTCUTS: [(KeyboardShortcut, MenuAction); 26] = [
	(key(CMD, Key::N), MenuAction::New),
	(key(CMD, Key::O), MenuAction::Open),
	(key(CMD_SHIFT, Key::S), MenuAction::SaveAs),
	(key(CMD, Key::S), MenuAction::Save),
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
	(key(NONE, Key::F1), MenuAction::Manual),
	(key(Modifiers::SHIFT, Key::F5), MenuAction::Stop),
	(key(NONE, Key::F5), MenuAction::Run),
	(key(NONE, Key::F6), MenuAction::PauseRun),
	(key(NONE, Key::F7), MenuAction::Play),
	(key(NONE, Key::Escape), MenuAction::Stop),
];

/// A focused text box's own keys.
fn edits_text(action: MenuAction) -> bool {
	matches!(
		action,
		MenuAction::Undo
			| MenuAction::Redo
			| MenuAction::Cut
			| MenuAction::Copy
			| MenuAction::Paste
			| MenuAction::SelectAll
	)
}

/// The clipboard's gestures, which the system and the browser turn into clipboard events.
fn is_clipboard(action: MenuAction) -> bool {
	matches!(action, MenuAction::Cut | MenuAction::Copy | MenuAction::Paste)
}

/// The action of a key pressed with `modifiers`.
fn action_of(key: Key, modifiers: Modifiers) -> Option<MenuAction> {
	SHORTCUTS.iter().find(|(s, _)| s.logical_key == key && modifiers.matches_exact(s.modifiers)).map(|(_, a)| *a)
}

/// The text a menu entry shows for its action's shortcut.
pub(super) fn text(ctx: &egui::Context, action: MenuAction) -> Option<String> {
	match action {
		MenuAction::Exit => Some("Alt+F4".into()),
		MenuAction::Delete => Some("Del".into()),
		_ => SHORTCUTS.iter().find(|(_, a)| *a == action).map(|(s, _)| ctx.format_shortcut(s)),
	}
}

/// What the user asked for with the keyboard in one frame; pasted text comes with [`MenuAction::Paste`].
pub(super) struct Keyboard {
	menu_open: bool,
	taken: Vec<(MenuAction, Option<String>)>,
}

impl Keyboard {
	/// Takes the clipboard's events, unless a menu or a text box has them.
	pub(super) fn before_ui(ctx: &egui::Context) -> Self {
		let menu_open = egui::Popup::is_any_open(ctx);
		let mut taken = Vec::new();
		if !menu_open && !ctx.text_edit_focused() {
			ctx.input_mut(|input| {
				input.events.retain(|event| {
					let action = match event {
						egui::Event::Copy => (MenuAction::Copy, None),
						egui::Event::Cut => (MenuAction::Cut, None),
						egui::Event::Paste(text) => (MenuAction::Paste, Some(text.clone())),
						_ => return true,
					};
					taken.push(action);
					false
				});
			});
		}
		Self { menu_open, taken }
	}

	/// Takes the shortcuts' keys the widgets left; all that was asked for this frame.
	pub(super) fn after_ui(mut self, ctx: &egui::Context) -> Vec<(MenuAction, Option<String>)> {
		if self.menu_open {
			return self.taken;
		}
		let text = ctx.text_edit_focused();
		ctx.input_mut(|input| {
			input.events.retain(|event| {
				let egui::Event::Key { key, pressed: true, modifiers, .. } = event else { return true };
				let Some(action) = action_of(*key, *modifiers).filter(|a| !(text && edits_text(*a))) else {
					return true;
				};
				// A clipboard key comes with its clipboard event where the system or the browser makes one.
				if !(is_clipboard(action) && self.taken.iter().any(|(a, _)| *a == action)) {
					self.taken.push((action, None));
				}
				false
			});
		});
		self.taken
	}
}

/// Whether the browser's own action for a key is to be prevented: a shortcut of the table, but for the
/// clipboard's, whose default makes the browser's clipboard events.
#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn is_reserved(key: Key, modifiers: Modifiers) -> bool {
	action_of(key, modifiers).is_some_and(|action| !is_clipboard(action))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_shortcut_fires_with_exactly_its_modifiers() {
		assert_eq!(action_of(Key::S, Modifiers::COMMAND), Some(MenuAction::Save));
		assert_eq!(action_of(Key::S, Modifiers::COMMAND | Modifiers::SHIFT), Some(MenuAction::SaveAs));
		assert_eq!(action_of(Key::F5, Modifiers::SHIFT), Some(MenuAction::Stop));
		assert_eq!(action_of(Key::Escape, Modifiers::SHIFT), None);
		assert_eq!(action_of(Key::N, Modifiers::COMMAND | Modifiers::SHIFT), None);
		assert_eq!(action_of(Key::F1, Modifiers::ALT), None);
	}

	#[test]
	fn the_clipboard_keys_stay_the_browsers() {
		assert!(!is_reserved(Key::V, Modifiers::COMMAND));
		assert!(is_reserved(Key::F5, Modifiers::NONE));
		assert!(!is_reserved(Key::F6, Modifiers::SHIFT));
		assert!(is_reserved(Key::S, Modifiers::COMMAND | Modifiers::SHIFT));
	}
}
