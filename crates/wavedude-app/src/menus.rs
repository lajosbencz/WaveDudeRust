//! The tables of the menus of docs/ui/10-menus-and-shortcuts.md: the UI zooms, the preview levers, the part kinds,
//! the [`MenuAction`]s that the menu bar, the context menus and the shortcuts share, when each is enabled, and Move up
//! and Move down.

use wavedude_app_state::AppState;
use wavedude_app_state::clipboard::Copyable;
use wavedude_app_state::commands::NewPart;
use wavedude_app_state::live::Backend;
use wavedude_app_state::selection::Row;
use wavedude_app_state::settings::{DITHER_BITS, Theme as ThemeSetting};
use wavedude_app_state::strings::Language;
use wavedude_core::engine::SampleFormat;
use wavedude_core::model::{ItemKind, ItemRef, ProbeDetector};

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

/// View > Resolution and the startup page's lever: divisor and string key.
pub const RESOLUTIONS: [(u32, &str); 3] =
	[(1, "MenuViewResolutionFull"), (2, "MenuViewResolutionHalf"), (4, "MenuViewResolutionQuarter")];

/// View > Frames and the startup page's lever: divisor and string key.
pub const FRAME_STEPS: [(u32, &str); 3] =
	[(1, "MenuViewFramesFull"), (2, "MenuViewFramesHalf"), (4, "MenuViewFramesQuarter")];

/// View > Samples and the startup page's lever: format and string key.
pub const SAMPLES: [(SampleFormat, &str); 2] =
	[(SampleFormat::Half, "MenuViewSamples16"), (SampleFormat::Index, "MenuViewSamples8")];

/// View > Dither and the startup page's lever: bits and string key.
pub const DITHERS: [(u32, &str); DITHER_BITS.len()] = [
	(DITHER_BITS[0], "MenuViewDitherOff"),
	(DITHER_BITS[1], "MenuViewDither10"),
	(DITHER_BITS[2], "MenuViewDither12"),
	(DITHER_BITS[3], "MenuViewDither14"),
	(DITHER_BITS[4], "MenuViewDither16"),
];

/// The Add part menu's entries: kind, string key.
pub const PART_KINDS: [(NewPart, &str); 21] = [
	(NewPart::Lens, "MenuAddLens"),
	(NewPart::FresnelLens, "MenuAddFresnelLens"),
	(NewPart::Fiber, "MenuAddFiber"),
	(NewPart::BraggReflector, "MenuAddBraggReflector"),
	(NewPart::GrinLens, "MenuAddGrinLens"),
	(NewPart::ZonePlate, "MenuAddZonePlate"),
	(NewPart::Diffuser, "MenuAddDiffuser"),
	(NewPart::InterferenceFilter, "MenuAddInterferenceFilter"),
	(NewPart::SimpleGrating, "MenuAddSimpleGrating"),
	(NewPart::FacettedGrating, "MenuAddFacettedGrating"),
	(NewPart::MetaLens, "MenuAddMetaLens"),
	(NewPart::MetaPrism, "MenuAddMetaPrism"),
	(NewPart::MetaAxicon, "MenuAddMetaAxicon"),
	(NewPart::Polygon, "MenuAddPolygon"),
	(NewPart::Rectangle, "MenuAddRectangle"),
	(NewPart::Ellipse, "MenuAddEllipse"),
	(NewPart::Triangle, "MenuAddTriangle"),
	(NewPart::Outline, "MenuAddOutline"),
	(NewPart::XorGate, "MenuAddXorGate"),
	(NewPart::AndGate, "MenuAddAndGate"),
	(NewPart::OrGate, "MenuAddOrGate"),
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
	/// The startup page.
	Welcome,
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

/// The item an item action on `on` applies to: the current item, or the context menu's row when it is an item.
pub fn target<B: Backend>(app: &AppState<B>, on: On) -> Option<ItemRef> {
	match on {
		On::Current => app.selection.current_item(),
		On::Row(Some(Row::Item(i))) => Some(i),
		On::Row(_) => None,
	}
}

/// Whether `action` on `on` can run now (the Enabled columns of docs/ui/10): its entries are greyed out and the
/// window refuses it otherwise. Paste and Export last also need the clipboard's objects and a last export of the
/// project, which their entries and their actions check.
pub fn enabled<B: Backend>(app: &AppState<B>, action: MenuAction, on: On) -> bool {
	let live = &app.live;
	let history = &app.document.history;
	let project = &app.document.project;
	let target = target(app, on);
	match action {
		MenuAction::Close => !app.document.is_placeholder(),
		MenuAction::Export(_) | MenuAction::ExportLast => !live.export_busy(),
		MenuAction::Undo => history.can_undo() || history.has_pending(),
		MenuAction::Redo => history.can_redo(),
		MenuAction::Cut | MenuAction::Delete | MenuAction::Duplicate | MenuAction::ToggleSelected => target.is_some(),
		MenuAction::Copy => {
			let row = match on {
				On::Current => app.selection.current(),
				On::Row(row) => row,
			};
			(on == On::Current && !app.selection.items().is_empty()) || row.and_then(Copyable::of_row).is_some()
		}
		MenuAction::SelectAll => !(project.sources.is_empty() && project.parts.is_empty() && project.probes.is_empty()),
		MenuAction::Dither(_) => app.settings.preview_quality().format == SampleFormat::Half,
		MenuAction::Run => !live.run_is_active() && !live.export_busy(),
		MenuAction::PauseRun => live.run_is_active(),
		MenuAction::Stop => live.run_is_active() || live.export_busy(),
		MenuAction::DiscardFrames => live.has_frames() && !live.export_busy(),
		MenuAction::ToggleEnabled => target.is_some_and(|t| t.kind != ItemKind::Scheme),
		MenuAction::ToggleUserScheme => target.is_some_and(|t| t.kind == ItemKind::Scheme),
		MenuAction::MoveUp | MenuAction::MoveDown => {
			target.is_some_and(|t| can_move(app, t, action == MenuAction::MoveUp))
		}
		MenuAction::ExportProbeCsv => {
			let probe = target.filter(|t| t.kind == ItemKind::Probe).and_then(|t| project.probes.get(t.index));
			probe.is_some_and(|p| p.active) && !live.export_busy()
		}
		MenuAction::New
		| MenuAction::Open
		| MenuAction::Recent(_)
		| MenuAction::Examples
		| MenuAction::Welcome
		| MenuAction::Save
		| MenuAction::SaveAs
		| MenuAction::Exit
		| MenuAction::Paste
		| MenuAction::Pane(_)
		| MenuAction::Theme(_)
		| MenuAction::Zoom(_)
		| MenuAction::Language(_)
		| MenuAction::Resolution(_)
		| MenuAction::Frames(_)
		| MenuAction::Samples(_)
		| MenuAction::Play
		| MenuAction::UseGpu
		| MenuAction::AutoRerun
		| MenuAction::WarnMemory
		| MenuAction::CheckForUpdates
		| MenuAction::CheckAtLaunch
		| MenuAction::FrameMemory(_)
		| MenuAction::About
		| MenuAction::Manual
		| MenuAction::AddSource
		| MenuAction::AddPart(_)
		| MenuAction::AddProbe(_) => true,
	}
}

/// Whether Move up (`up`) or Move down shifts `target` and the items moving with it.
pub fn can_move<B: Backend>(app: &AppState<B>, target: ItemRef, up: bool) -> bool {
	!moves_of(app, target, up).is_empty()
}

/// The reorders `(index, to)` of Move up (`up`) or Move down on `target` and the selected items of its list.
pub fn moves_of<B: Backend>(app: &AppState<B>, target: ItemRef, up: bool) -> Vec<(usize, usize)> {
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
	use std::sync::Arc;
	use std::sync::atomic::AtomicBool;

	use wavedude_app_state::Event;
	use wavedude_app_state::commands::Command;
	use wavedude_app_state::live::RunNeeds;
	use wavedude_app_state::live::fake::FakeBackend;
	use wavedude_app_state::platform::Instant;
	use wavedude_app_state::settings::Settings;
	use wavedude_export::ExportFormat;

	use super::*;

	fn app() -> AppState<FakeBackend> {
		AppState::with_backend(Settings::default(), None, FakeBackend::default(), Arc::new(AtomicBool::new(false)))
	}

	#[test]
	fn an_item_action_needs_its_item() {
		let mut app = app();
		let scene = On::Row(Some(Row::Scene));
		let scheme = On::Row(Some(Row::Item(ItemRef::new(ItemKind::Scheme, 0))));
		for action in [MenuAction::Cut, MenuAction::Delete, MenuAction::Duplicate, MenuAction::ToggleEnabled] {
			assert!(!enabled(&app, action, On::Current), "{action:?} on the project row");
			assert!(!enabled(&app, action, scene), "{action:?} on the Scene row");
		}
		assert!(!enabled(&app, MenuAction::Copy, On::Current), "the project row copies nothing");
		assert!(enabled(&app, MenuAction::Copy, scene));
		assert!(!enabled(&app, MenuAction::SelectAll, On::Current), "an empty project has nothing to select");
		assert!(!enabled(&app, MenuAction::Close, On::Current), "the placeholder closes to itself");
		assert!(
			!enabled(&app, MenuAction::ToggleEnabled, scheme) && enabled(&app, MenuAction::ToggleUserScheme, scheme)
		);
		app.handle(Event::Command(Command::AddSource { at: None }), Instant::now());
		for action in [MenuAction::Cut, MenuAction::Copy, MenuAction::Delete, MenuAction::ToggleEnabled] {
			assert!(enabled(&app, action, On::Current), "{action:?} on the added source");
		}
		assert!(enabled(&app, MenuAction::SelectAll, On::Current) && enabled(&app, MenuAction::Close, On::Current));
		assert!(!enabled(&app, MenuAction::MoveUp, On::Current), "a lone source stays");
		assert!(!enabled(&app, MenuAction::ExportProbeCsv, On::Current), "a source has no probe data");
	}

	#[test]
	fn an_export_refuses_a_run_and_what_needs_its_frames_but_stops() {
		let mut app = app();
		let project = app.document.project.clone();
		assert!(enabled(&app, MenuAction::Run, On::Current) && !enabled(&app, MenuAction::Stop, On::Current));
		app.live.request_export(&project, RunNeeds::new(ExportFormat::Png, false), Instant::now());
		assert!(app.live.export_busy());
		for action in [
			MenuAction::Run,
			MenuAction::DiscardFrames,
			MenuAction::Export(ExportKindChoice::Field),
			MenuAction::ExportLast,
		] {
			assert!(!enabled(&app, action, On::Current), "{action:?} during an export");
		}
		assert!(enabled(&app, MenuAction::Stop, On::Current));
	}

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
