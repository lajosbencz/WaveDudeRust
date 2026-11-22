//! The menus' icons in Material Symbols.

use egui_material_icons::MaterialIcon;
use egui_material_icons::icons::{
	ICON_CLOSE, ICON_CONTENT_COPY, ICON_CONTENT_CUT, ICON_CONTENT_PASTE, ICON_DELETE, ICON_DELETE_SWEEP,
	ICON_FOLDER_OPEN, ICON_HISTORY, ICON_IMAGE, ICON_INFO, ICON_IOS_SHARE, ICON_LIST_ALT, ICON_LOGOUT, ICON_NOTE_ADD,
	ICON_OPEN_IN_NEW, ICON_PAUSE, ICON_PLAY_ARROW, ICON_REDO, ICON_SAVE, ICON_SAVE_AS, ICON_SELECT_ALL, ICON_SETTINGS,
	ICON_STOP, ICON_UNDO, ICON_VIEW_SIDEBAR, ICON_WAVING_HAND,
};
use waveduderust_app::menus::Icon;

/// The Material symbol of a menu entry's `icon`.
pub fn material(icon: Icon) -> MaterialIcon {
	match icon {
		Icon::Close => ICON_CLOSE,
		Icon::Copy => ICON_CONTENT_COPY,
		Icon::Cut => ICON_CONTENT_CUT,
		Icon::Paste => ICON_CONTENT_PASTE,
		Icon::Delete => ICON_DELETE,
		Icon::DeleteSweep => ICON_DELETE_SWEEP,
		Icon::FolderOpen => ICON_FOLDER_OPEN,
		Icon::History => ICON_HISTORY,
		Icon::Image => ICON_IMAGE,
		Icon::Info => ICON_INFO,
		Icon::Share => ICON_IOS_SHARE,
		Icon::ListAlt => ICON_LIST_ALT,
		Icon::Logout => ICON_LOGOUT,
		Icon::NoteAdd => ICON_NOTE_ADD,
		Icon::OpenInNew => ICON_OPEN_IN_NEW,
		Icon::Pause => ICON_PAUSE,
		Icon::Play => ICON_PLAY_ARROW,
		Icon::Redo => ICON_REDO,
		Icon::Save => ICON_SAVE,
		Icon::SaveAs => ICON_SAVE_AS,
		Icon::SelectAll => ICON_SELECT_ALL,
		Icon::Settings => ICON_SETTINGS,
		Icon::Stop => ICON_STOP,
		Icon::Undo => ICON_UNDO,
		Icon::Sidebar => ICON_VIEW_SIDEBAR,
		Icon::WavingHand => ICON_WAVING_HAND,
	}
}
