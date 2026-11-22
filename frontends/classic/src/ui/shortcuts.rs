//! The window's keys (docs/ui/10, Global and focus-scoped keys): each maps to a [`MenuAction`] and fires with exactly
//! its modifiers. Copy, cut and paste arrive as egui's clipboard events and are taken before the widgets see them; the
//! other keys are taken after, so Esc stops the run only where no widget used it.

use eframe::egui::{self, Key, Modifiers};
use waveduderust_app::menus::MenuAction;
use waveduderust_egui_platform::keys::SHORTCUTS;

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
		let menu_open = crate::kit::menu::any_open(ctx);
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

/// Whether the browser's own action for a key is to be prevented: a shortcut of the table, the zoom keys' page zoom
/// included (decision 0027), but for the clipboard's, whose default makes the browser's clipboard events.
#[cfg(target_arch = "wasm32")]
pub(super) fn is_reserved(key: Key, modifiers: Modifiers) -> bool {
	action_of(key, modifiers).is_some_and(|action| !is_clipboard(action))
}
