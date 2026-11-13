//! The tables of the menus of docs/ui/10-menus-and-shortcuts.md: the UI zooms, the part kinds, the
//! [`MenuAction`]s that the menu bar, the context menus and the shortcuts share, and Move up and Move down.

use wavedude_app_state::commands::NewPart;
use wavedude_app_state::selection::Row;
use wavedude_app_state::settings::Theme as ThemeSetting;
use wavedude_app_state::strings::Language;
use wavedude_core::engine::SampleFormat;
use wavedude_core::model::{ItemRef, ProbeDetector};

use crate::App;
use crate::window::Pane;

/// The UI zoom entries of View > UI zoom: factor and string key.
pub const ZOOMS: [(f64, &str); 8] = [
	(0.5, "MenuViewUiZoom50"),
	(0.75, "MenuViewUiZoom75"),
	(1.0, "MenuViewUiZoom100"),
	(1.25, "MenuViewUiZoom125"),
	(1.5, "MenuViewUiZoom150"),
	(2.0, "MenuViewUiZoom200"),
	(2.5, "MenuViewUiZoom250"),
	(3.0, "MenuViewUiZoom300"),
];

/// The Add part menu's entries: kind, string key, id.
pub const PART_KINDS: [(NewPart, &str, &str); 21] = [
	(NewPart::Lens, "MenuAddLens", "Lens"),
	(NewPart::FresnelLens, "MenuAddFresnelLens", "FresnelLens"),
	(NewPart::Fiber, "MenuAddFiber", "Fiber"),
	(NewPart::BraggReflector, "MenuAddBraggReflector", "BraggReflector"),
	(NewPart::GrinLens, "MenuAddGrinLens", "GrinLens"),
	(NewPart::ZonePlate, "MenuAddZonePlate", "ZonePlate"),
	(NewPart::Diffuser, "MenuAddDiffuser", "Diffuser"),
	(NewPart::InterferenceFilter, "MenuAddInterferenceFilter", "InterferenceFilter"),
	(NewPart::SimpleGrating, "MenuAddSimpleGrating", "SimpleGrating"),
	(NewPart::FacettedGrating, "MenuAddFacettedGrating", "FacettedGrating"),
	(NewPart::MetaLens, "MenuAddMetaLens", "MetaLens"),
	(NewPart::MetaPrism, "MenuAddMetaPrism", "MetaPrism"),
	(NewPart::MetaAxicon, "MenuAddMetaAxicon", "MetaAxicon"),
	(NewPart::Polygon, "MenuAddPolygon", "Polygon"),
	(NewPart::Rectangle, "MenuAddRectangle", "Rectangle"),
	(NewPart::Ellipse, "MenuAddEllipse", "Ellipse"),
	(NewPart::Triangle, "MenuAddTriangle", "Triangle"),
	(NewPart::Outline, "MenuAddOutline", "Outline"),
	(NewPart::XorGate, "MenuAddXorGate", "XorGate"),
	(NewPart::AndGate, "MenuAddAndGate", "AndGate"),
	(NewPart::OrGate, "MenuAddOrGate", "OrGate"),
];

/// What an export command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKindChoice {
	Field,
	Intensity,
	Both,
	FieldFrame,
	IntensityFrame,
}

/// What an item action applies to: the current item, or the row a context menu opened on (none: the empty tree).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum On {
	Current,
	Row(Option<Row>),
}

/// What a menu entry or a shortcut does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuAction {
	New,
	Open,
	/// Entry `n` of the recent files.
	Recent(usize),
	/// The Examples dialog.
	Examples,
	Save,
	SaveAs,
	Export(ExportKindChoice),
	ExportLast,
	Close,
	Exit,
	Undo,
	Redo,
	Cut,
	Copy,
	Paste,
	Delete,
	SelectAll,
	/// Shows or hides a pane.
	Pane(Pane),
	Theme(ThemeSetting),
	Zoom(f64),
	/// `None` follows the system.
	Language(Option<Language>),
	Resolution(u32),
	Frames(u32),
	Samples(SampleFormat),
	/// One of [`wavedude_app_state::settings::DITHER_BITS`].
	Dither(u32),
	Run,
	PauseRun,
	Stop,
	DiscardFrames,
	Play,
	UseGpu,
	AutoRerun,
	WarnMemory,
	CheckForUpdates,
	CheckAtLaunch,
	/// 0 is Auto.
	FrameMemory(i32),
	About,
	Manual,
	AddSource,
	AddPart(NewPart),
	AddProbe(ProbeDetector),
	ToggleSelected,
	ToggleEnabled,
	Duplicate,
	MoveUp,
	MoveDown,
	ExportProbeCsv,
	/// Adds a project colour scheme to the user's, or removes the user's of its name.
	ToggleUserScheme,
}

/// Whether Move up (`up`) or Move down shifts `target` and the items moving with it.
pub fn can_move(app: &App, target: ItemRef, up: bool) -> bool {
	!moves_of(app, target, up).is_empty()
}

/// The reorders `(index, to)` of Move up (`up`) or Move down on `target` and the selected items of its list.
pub fn moves_of(app: &App, target: ItemRef, up: bool) -> Vec<(usize, usize)> {
	let indices = app.selection.targets(target).into_iter().filter(|t| t.kind == target.kind).map(|t| t.index);
	block_moves(indices.collect(), target.kind.len(&app.document.project), up)
}

/// The reorders `(from, to)` shifting the items at `indices` of a list of `len` one place, the leading item first;
/// none when the leading item is at the list's edge, so the block keeps its order.
fn block_moves(mut indices: Vec<usize>, len: usize, up: bool) -> Vec<(usize, usize)> {
	indices.sort_unstable();
	if !up {
		indices.reverse();
	}
	let lead = indices.first().copied();
	if lead.is_none_or(|i| if up { i == 0 } else { i + 1 >= len }) {
		return Vec::new();
	}
	indices.into_iter().map(|i| (i, if up { i - 1 } else { i + 1 })).collect()
}

#[cfg(test)]
mod tests {
	use super::block_moves;

	#[test]
	fn a_block_at_the_list_edge_keeps_its_order() {
		assert_eq!(block_moves(vec![1, 0], 3, true), []);
		assert_eq!(block_moves(vec![1, 2], 3, false), []);
		assert_eq!(block_moves(vec![0, 2], 4, true), []);
	}

	#[test]
	fn a_block_moves_with_its_leading_item_first() {
		assert_eq!(block_moves(vec![2, 1], 4, true), [(1, 0), (2, 1)]);
		assert_eq!(block_moves(vec![1, 2], 4, false), [(2, 3), (1, 2)]);
		assert_eq!(block_moves(vec![1, 3], 5, true), [(1, 0), (3, 2)]);
		assert_eq!(block_moves(Vec::new(), 4, true), []);
	}
}
