//! The window's shortcuts (docs/ui/10): each key maps to a menu action run by [`menus::dispatch`]. A focused text
//! box keeps its own editing keys; a modal takes every key.

use freya::prelude::*;

use super::app::Cx;
use super::export_dialog::ExportKindChoice;
use super::menus::{self, MenuAction};

/// Whether the focused element edits text, so Ctrl+Z, C, X, V and A are its own.
fn text_box_focused() -> bool {
	let role = Platform::get().current_window().focused_accessibility_node.peek().role();
	matches!(
		role,
		AccessibilityRole::TextInput
			| AccessibilityRole::MultilineTextInput
			| AccessibilityRole::PasswordInput
			| AccessibilityRole::SpinButton
	)
}

/// The menu action of a key with its modifiers; `text` is true while a text box has the focus.
pub fn command_of(key: &Key, modifiers: Modifiers, text: bool) -> Option<MenuAction> {
	let ctrl = modifiers.contains(Modifiers::CONTROL) || modifiers.contains(Modifiers::META);
	let shift = modifiers.contains(Modifiers::SHIFT);
	let alt = modifiers.contains(Modifiers::ALT);
	if alt {
		return None;
	}
	match key {
		Key::Named(NamedKey::F1) if !ctrl && !shift => Some(MenuAction::Manual),
		Key::Named(NamedKey::F5) if !ctrl => Some(if shift { MenuAction::Stop } else { MenuAction::Run }),
		Key::Named(NamedKey::F6) if !ctrl && !shift => Some(MenuAction::PauseRun),
		Key::Named(NamedKey::F7) if !ctrl && !shift => Some(MenuAction::Play),
		Key::Character(c) if ctrl => {
			let c = c.to_lowercase();
			match (c.as_str(), shift) {
				("n", false) => Some(MenuAction::New),
				("o", false) => Some(MenuAction::Open),
				("s", false) => Some(MenuAction::Save),
				("s", true) => Some(MenuAction::SaveAs),
				("e", false) => Some(MenuAction::ExportLast),
				("e", true) => Some(MenuAction::Export(ExportKindChoice::Field)),
				("i", true) => Some(MenuAction::Export(ExportKindChoice::Intensity)),
				("b", true) => Some(MenuAction::Export(ExportKindChoice::Both)),
				("f", true) => Some(MenuAction::Export(ExportKindChoice::FieldFrame)),
				("g", true) => Some(MenuAction::Export(ExportKindChoice::IntensityFrame)),
				_ if text => None,
				("z", false) => Some(MenuAction::Undo),
				("z", true) | ("y", false) => Some(MenuAction::Redo),
				("x", false) => Some(MenuAction::Cut),
				("c", false) => Some(MenuAction::Copy),
				("v", false) => Some(MenuAction::Paste),
				("a", false) => Some(MenuAction::SelectAll),
				_ => None,
			}
		}
		_ => None,
	}
}

/// Esc, bubbled to the window root: what a text box, a drag, a menu or a dialog did not take cancels a running
/// export, else stops the run (the Stop command's second gesture).
pub fn on_escape(cx: &Cx, e: &Event<KeyboardEventData>) {
	if e.key != Key::Named(NamedKey::Escape) || !e.modifiers.is_empty() {
		return;
	}
	if cx.ui.modal.peek().is_some() || cx.ui.menu.peek().is_some() {
		return;
	}
	e.stop_propagation();
	menus::dispatch(cx, MenuAction::Stop);
}

/// The window's key handler.
pub fn on_global_key(cx: &Cx, e: &Event<KeyboardEventData>) {
	if cx.ui.modal.peek().is_some() || cx.ui.menu.peek().is_some() {
		return;
	}
	if let Some(action) = command_of(&e.key, e.modifiers, text_box_focused()) {
		e.stop_propagation();
		menus::dispatch(cx, action);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn text_box_keeps_its_editing_keys() {
		let z = Key::Character("z".into());
		assert_eq!(command_of(&z, Modifiers::CONTROL, false), Some(MenuAction::Undo));
		assert_eq!(command_of(&z, Modifiers::CONTROL, true), None);
		assert_eq!(
			command_of(&Key::Character("S".into()), Modifiers::CONTROL | Modifiers::SHIFT, true),
			Some(MenuAction::SaveAs)
		);
		assert_eq!(command_of(&Key::Named(NamedKey::F5), Modifiers::SHIFT, false), Some(MenuAction::Stop));
		assert_eq!(command_of(&Key::Named(NamedKey::F1), Modifiers::ALT, false), None);
	}
}
