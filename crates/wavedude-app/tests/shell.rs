//! The window shell driven headless: the tree, the menu bar and a property edit.

use std::path::Path;
use std::time::Instant;

use freya::prelude::*;
use freya_testing::TestingNode;
use freya_testing::prelude::*;
use wavedude_app::ui::app::{Boot, BootInfo, CloseGate, root};
use wavedude_app::ui::shared::Shared;
use wavedude_app_state::log::LogRing;
use wavedude_app_state::selection::{ItemKind, ItemRef, Row};
use wavedude_app_state::settings::AppDirs;
use wavedude_app_state::{AppState, Event as AppEvent, Guarded};
use wavedude_core::model::{ProbeDetector, ProbeKind};

const EXAMPLE: &str = "../wavedude-core/tests/fixtures/examples/Example16_Wave_in_2_boxes.wdproj";

/// A returning user's window: the startup page answered.
fn shell(dir: &Path) -> (TestingRunner, Shared) {
	shell_at(dir, true)
}

fn shell_at(dir: &Path, answered: bool) -> (TestingRunner, Shared) {
	shell_sized(dir, answered, Size2D::new(1400., 900.))
}

/// A window tall enough for the whole probe form: its Axes section lies below a 900 px window.
fn tall_shell(dir: &Path) -> (TestingRunner, Shared) {
	shell_sized(dir, true, Size2D::new(1400., 1400.))
}

fn shell_sized(dir: &Path, answered: bool, size: Size2D) -> (TestingRunner, Shared) {
	let dirs = AppDirs::under(dir);
	let mut app = AppState::start(&dirs, []).expect("state starts");
	if answered {
		app.first_start = false;
		app.settings.use_openh264 = Some(false);
		app.settings.check_for_updates = Some(false);
	}
	app.handle(AppEvent::SetLanguage("en".into()), Instant::now());
	app.handle(AppEvent::Guarded(Guarded::Open(EXAMPLE.into())), Instant::now());
	let info = BootInfo {
		ring: LogRing::new(),
		dirs,
		close: CloseGate::default(),
		open: None,
		update: Default::default(),
		placement: Default::default(),
	};
	let boot = Boot::new(app, info);
	let handle = boot.clone();
	let (mut test, ()) =
		TestingRunner::new(root, size, move |r| drop(r.provide_root_context(move || boot.clone())), 1.0);
	test.sync_and_update();
	let shared = handle.shared().expect("the root provided the state");
	(test, shared)
}

fn find_text(test: &TestingRunner, text: &str) -> Option<TestingNode> {
	test.find(|node, element| {
		let label = Label::try_downcast(element).is_some_and(|l| l.text.as_ref() == text);
		let spans = Paragraph::try_downcast(element)
			.is_some_and(|p| p.spans.iter().map(|s| s.text.as_ref()).collect::<String>() == text);
		(label || spans).then_some(node)
	})
}

#[test]
fn a_first_start_shows_the_startup_page_and_continue_saves_it() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, state) = shell_at(dir.path(), false);
	test.poll(std::time::Duration::from_millis(16), std::time::Duration::from_millis(100));
	assert!(find_text(&test, "Welcome to WaveDude Rust").is_some(), "no startup page");
	test.press_key(Key::Named(NamedKey::Enter));
	test.sync_and_update();
	assert!(find_text(&test, "Welcome to WaveDude Rust").is_none(), "the page stayed");
	if !wavedude_export::h264::BUILT_IN {
		assert_eq!(state.peek(|a| a.settings.use_openh264), Some(false), "the codec is never enabled unasked");
	}
}

#[test]
fn tree_lists_the_example_items() {
	let dir = tempfile::tempdir().unwrap();
	let (test, _) = shell(dir.path());
	for name in ["Vertical source", "Horizontal source", "Left box", "Right box"] {
		assert!(find_text(&test, name).is_some(), "no tree row {name}");
	}
}

#[test]
fn f10_enters_the_menu_bar_and_down_opens_file() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, _) = shell(dir.path());
	assert!(find_text(&test, "New Project").is_none());
	test.press_key(Key::Named(NamedKey::F10));
	test.sync_and_update();
	test.press_key(Key::Named(NamedKey::ArrowDown));
	test.sync_and_update();
	assert!(find_text(&test, "New Project").is_some(), "File menu did not open");
}

#[test]
fn a_property_edit_reaches_the_document() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, state) = shell(dir.path());
	let source = ItemRef::new(ItemKind::Source, 0);
	state.update(|a| a.selection.select(Row::Item(source)));
	test.sync_and_update();
	test.sync_and_update();
	let before = state.peek(|a| a.document.project.sources[0].enabled);
	let toggle = find_text(&test, "Enabled").expect("the source form shows");
	let area = toggle.layout().area;
	test.click_cursor((f64::from(area.min_x()) + 2., f64::from(area.center().y)));
	test.sync_and_update();
	assert_ne!(state.peek(|a| a.document.project.sources[0].enabled), before, "the check box did not edit the source");
}

/// A row greyed by another setting takes no edit: the line probe's step axis is a point probe's.
#[test]
fn a_disabled_row_takes_no_edit() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, state) = tall_shell(dir.path());
	let probe = ItemRef::new(ItemKind::Probe, 0);
	state.update(|a| a.selection.select(Row::Item(probe)));
	test.sync_and_update();
	test.sync_and_update();
	assert_eq!(state.peek(|a| a.document.project.probes[0].kind), ProbeKind::Line);
	let before = state.peek(|a| a.document.project.probes[0].auto_scale_x);
	let toggle = find_text(&test, "Auto-scale steps").expect("the probe form shows");
	let area = toggle.layout().area;
	assert!(area.max_y() < 1400., "the row is on screen: {area:?}");
	test.click_cursor((f64::from(area.min_x()) + 2., f64::from(area.center().y)));
	test.sync_and_update();
	assert_eq!(state.peek(|a| a.document.project.probes[0].auto_scale_x), before, "a disabled check box toggled");
	// the combo measures its words in a hidden copy, so the menu adds a second visible "Full"
	let words = |test: &TestingRunner, w: &str| {
		test.find_many(|node, element| {
			Label::try_downcast(element).is_some_and(|l| l.text.as_ref() == w).then_some(node.layout().area)
		})
	};
	let shown = words(&test, "Running").into_iter().max_by(|a, b| a.width().total_cmp(&b.width())).unwrap();
	let before = words(&test, "Full").len();
	test.click_cursor((f64::from(shown.center().x), f64::from(shown.center().y)));
	test.sync_and_update();
	test.sync_and_update();
	assert_eq!(words(&test, "Full").len(), before, "a disabled combo box opened its menu");
}

/// A counting detector's graph runs over the steps, so its step axis rows take edits although its kind is Line.
#[test]
fn a_counting_detectors_step_axis_takes_an_edit() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, state) = tall_shell(dir.path());
	let probe = ItemRef::new(ItemKind::Probe, 0);
	state.update(|a| {
		a.document.project.probes[0].detector = ProbeDetector::Counting;
		a.document.edited(Instant::now());
		a.selection.select(Row::Item(probe));
	});
	test.sync_and_update();
	test.sync_and_update();
	assert_eq!(state.peek(|a| a.document.project.probes[0].kind), ProbeKind::Line);
	let before = state.peek(|a| a.document.project.probes[0].auto_scale_x);
	let toggle = find_text(&test, "Auto-scale steps").expect("the probe form shows");
	let area = toggle.layout().area;
	assert!(area.max_y() < 1400., "the row is on screen: {area:?}");
	test.click_cursor((f64::from(area.min_x()) + 2., f64::from(area.center().y)));
	test.sync_and_update();
	assert_ne!(
		state.peek(|a| a.document.project.probes[0].auto_scale_x),
		before,
		"the check box did not edit the detector"
	);
}

#[test]
fn drop_downs_fill_their_column() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, state) = shell(dir.path());
	state.update(|a| a.selection.select(Row::Scene));
	for _ in 0..3 {
		test.sync_and_update();
	}
	let words = |w: &str| {
		test.find_many(|node, element| {
			Label::try_downcast(element).is_some_and(|l| l.text.as_ref() == w).then_some(node.layout().area)
		})
	};
	// the hidden measuring copy lists every word first
	let open = words("Open");
	let (hidden, shown) = (open[0], open[1]);
	let reflective = words("Reflective")[0];
	assert!(reflective.width() > hidden.width(), "the measuring copy holds the widest word");
	assert!(
		shown.width() > reflective.width() + 40.,
		"the button fills the column past its widest word: shown {} widest {}",
		shown.width(),
		reflective.width()
	);
	let column = test
		.find(|node, element| {
			Rect::try_downcast(element).filter(|r| r.accessibility.builder.author_id() == Some("Scene.Width"))?;
			Some(node.layout().area)
		})
		.expect("the width box is drawn");
	assert!(shown.max_x() < column.max_x(), "the button stays in the editor column: {shown:?} {column:?}");
}

#[test]
fn hovering_export_opens_its_submenu_inside_the_window() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, _) = shell(dir.path());
	test.press_key(Key::Named(NamedKey::F10));
	test.sync_and_update();
	test.press_key(Key::Named(NamedKey::ArrowDown));
	test.sync_and_update();
	let export = find_text(&test, "Export").expect("the File menu shows Export").layout().area;
	test.move_cursor((f64::from(export.center().x), f64::from(export.center().y)));
	test.sync_and_update();
	test.sync_and_update();
	let row = |id: &str| {
		test.find(|node, element| {
			Rect::try_downcast(element).filter(|r| r.accessibility.builder.author_id() == Some(id))?;
			Some(node.layout().area)
		})
		.unwrap_or_else(|| panic!("no menu row {id}"))
	};
	let field = row("file:export:field");
	let new = row("file:new");
	assert!(field.min_x() >= new.max_x() - 4., "the submenu opens beside its parent: {field:?} vs {new:?}");
	assert!(field.max_x() <= 1400., "the submenu stays inside the window: {field:?}");
	assert!(new.width() < 320., "the File menu is {} px wide", new.width());
}

/// The brightness box of a strip on its side: a click on its likeness focuses it, typed text and Enter set the value.
#[test]
fn the_turned_brightness_box_takes_clicks_and_keys() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, state) = shell(dir.path());
	let id = "PreviewFieldImageBrightness";
	let area = test
		.find(|node, element| {
			Rect::try_downcast(element).filter(|r| r.accessibility.builder.author_id() == Some(id))?;
			Some(node.layout().area)
		})
		.expect("the box's stand-in");
	assert!(area.height() > area.width(), "the strip is on its side: {area:?}");
	test.click_cursor((f64::from(area.center().x), f64::from(area.center().y)));
	test.sync_and_update();
	test.press_key(Key::Named(NamedKey::End));
	for _ in 0..8 {
		test.press_key(Key::Named(NamedKey::Backspace));
	}
	test.write_text("42");
	test.press_key(Key::Named(NamedKey::Enter));
	test.sync_and_update();
	test.sync_and_update();
	assert_eq!(state.peek(|a| a.document.project.display.field.brightness), 42.);
}

/// The area of the element tagged `id`.
fn tagged_area(test: &TestingRunner, id: &str) -> Option<Area> {
	test.find(|node, element| {
		Rect::try_downcast(element).filter(|r| r.accessibility.builder.author_id() == Some(id))?;
		Some(node.layout().area)
	})
}

/// Whether the focused element lies inside the element tagged `id`.
fn focus_in(test: &mut TestingRunner, id: &str) -> bool {
	let focus = test.sync_and_update().focus;
	let focused = test.find(|node, element| {
		let a11y = Rect::try_downcast(element)
			.map(|r| r.accessibility.a11y_id)
			.or_else(|| Paragraph::try_downcast(element).map(|p| p.accessibility.a11y_id))?;
		(a11y == Some(focus)).then(|| node.layout().area)
	});
	let tag = tagged_area(test, id).unwrap_or_else(|| panic!("no element {id}"));
	focused.is_some_and(|f| tag.contains(f.center()))
}

fn settle(test: &mut TestingRunner) {
	test.poll(std::time::Duration::from_millis(16), std::time::Duration::from_millis(16));
	test.sync_and_update();
}

fn click_tagged(test: &mut TestingRunner, id: &str) {
	let area = tagged_area(test, id).unwrap_or_else(|| panic!("no element {id}"));
	test.click_cursor((f64::from(area.center().x), f64::from(area.center().y)));
	settle(test);
}

#[test]
fn an_opened_project_shows_its_root() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, state) = shell(dir.path());
	settle(&mut test);
	assert_eq!(state.peek(|a| a.selection.current()), Some(Row::Project));
	assert!(find_text(&test, "No selection").is_none(), "the Properties pane shows the Project form");
	state.update(|a| drop(a.handle(AppEvent::Guarded(Guarded::New), Instant::now())));
	settle(&mut test);
	assert_eq!(state.peek(|a| a.selection.current()), Some(Row::Project), "a new project too");
	assert!(focus_in(&mut test, "ProjectTree"), "the tree takes the focus");
}

/// `KeyboardCheck` step 2: the move and remove buttons are disabled with no item selected and skipped.
#[test]
fn tab_walks_the_project_toolbar_into_the_tree() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, _) = shell(dir.path());
	settle(&mut test);
	for _ in 0..40 {
		if focus_in(&mut test, "ToolAddSource") {
			break;
		}
		test.press_key(Key::Named(NamedKey::Tab));
	}
	assert!(focus_in(&mut test, "ToolAddSource"), "Tab never reached Add source");
	assert!(!focus_in(&mut test, "ToolAddPart"));
	for id in ["ToolAddProbe", "ToolAddPart", "ProjectTree"] {
		test.press_key(Key::Named(NamedKey::Tab));
		assert!(focus_in(&mut test, id), "Tab skipped {id}");
	}
}

/// `KeyboardCheck` step 3: the new source is selected and its name box has the caret.
#[test]
fn an_added_source_is_selected_with_the_caret_in_its_name() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, state) = shell(dir.path());
	settle(&mut test);
	let count = state.peek(|a| a.document.project.sources.len());
	click_tagged(&mut test, "ToolAddSource");
	let added = ItemRef::new(ItemKind::Source, count);
	assert_eq!(state.peek(|a| a.selection.current()), Some(Row::Item(added)));
	assert!(focus_in(&mut test, "Source.Name"), "the name box has no focus");
	test.write_text("Lamp");
	settle(&mut test);
	let name = state.peek(|a| a.document.project.sources[count].name.clone());
	assert_eq!(name, "Lamp", "the focused name is selected, typing replaces it");
}

/// `KeyboardCheck` step 7: Remove takes the selected source away and selects its neighbour.
#[test]
fn removing_the_last_row_selects_the_one_above() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, state) = shell(dir.path());
	let count = state.peek(|a| a.document.project.sources.len());
	assert!(count >= 2, "the example has two sources");
	state.update(|a| a.selection.select(Row::Item(ItemRef::new(ItemKind::Source, count - 1))));
	settle(&mut test);
	click_tagged(&mut test, "ToolRemove");
	assert_eq!(state.peek(|a| a.document.project.sources.len()), count - 1);
	assert_eq!(state.peek(|a| a.selection.current()), Some(Row::Item(ItemRef::new(ItemKind::Source, count - 2))));
	assert!(focus_in(&mut test, "ProjectTree"), "the tree takes the focus from the button");
}

/// `KeyboardCheck` step 6: a move keeps the selection and gives the tree the focus.
#[test]
fn moving_a_row_keeps_it_selected_and_focuses_the_tree() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, state) = shell(dir.path());
	let name = state.peek(|a| a.document.project.sources[1].name.clone());
	state.update(|a| a.selection.select(Row::Item(ItemRef::new(ItemKind::Source, 1))));
	settle(&mut test);
	click_tagged(&mut test, "ToolMoveUp");
	assert_eq!(state.peek(|a| a.document.project.sources[0].name.clone()), name);
	assert_eq!(state.peek(|a| a.selection.current()), Some(Row::Item(ItemRef::new(ItemKind::Source, 0))));
	assert!(focus_in(&mut test, "ProjectTree"), "the tree takes the focus from the button");
}

/// The elements tagged with `prefix` and a tail, drawn now.
fn count_tagged(test: &TestingRunner, prefix: &str) -> usize {
	let count = std::cell::Cell::new(0);
	test.find(|_, element| {
		let tagged = Rect::try_downcast(element)
			.is_some_and(|r| r.accessibility.builder.author_id().is_some_and(|id| id.starts_with(prefix)));
		count.set(count.get() + usize::from(tagged));
		None::<()>
	});
	count.get()
}

fn open_examples_dialog(test: &mut TestingRunner) {
	test.press_key(Key::Named(NamedKey::F10));
	test.sync_and_update();
	test.press_key(Key::Named(NamedKey::ArrowDown));
	test.sync_and_update();
	click_tagged(test, "file:examples");
	assert!(tagged_area(test, "ExamplesDialog").is_some(), "File > Examples opens the dialog");
}

#[test]
fn the_examples_dialog_lists_every_example_by_topic() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, _) = shell(dir.path());
	open_examples_dialog(&mut test);
	for (tab, expected) in [8, 10, 7, 5, 18, 5].into_iter().enumerate() {
		click_tagged(&mut test, &format!("ExamplesTab:{tab}"));
		let rows = count_tagged(&test, "Example:");
		assert_eq!(rows, expected, "tab {tab}");
	}
	test.press_key(Key::Named(NamedKey::Escape));
	settle(&mut test);
	assert!(tagged_area(&test, "ExamplesDialog").is_none(), "Esc closes the dialog");
}

#[test]
fn the_examples_search_narrows_the_rows() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, _) = shell(dir.path());
	open_examples_dialog(&mut test);
	click_tagged(&mut test, "ExamplesSearch");
	test.write_text("fibre");
	settle(&mut test);
	assert!(tagged_area(&test, "Example:Example23_Fibres").is_some(), "the search follows the matches to their tab");
	assert!(tagged_area(&test, "Example:Example24_Fibre_cross_over").is_some());
	assert!(tagged_area(&test, "Example:Example01_Source_Mirror_Probe").is_none());
	let rows = count_tagged(&test, "Example:");
	assert!((2..18).contains(&rows), "{rows} rows match 'fibre'");
}

#[test]
fn enter_opens_the_selected_example() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, state) = shell(dir.path());
	open_examples_dialog(&mut test);
	click_tagged(&mut test, "ExamplesTab:2");
	click_tagged(&mut test, "Example:Example13_Bragg_reflector");
	test.press_key(Key::Named(NamedKey::Enter));
	settle(&mut test);
	assert!(tagged_area(&test, "ExamplesDialog").is_none(), "opening closes the dialog");
	let name = state.peek(|a| a.document.display_name(""));
	assert!(name.contains("Example13_Bragg_reflector"), "{name}");
}

/// Types `text` into the number box tagged `id` after a click on it, without leaving it.
fn type_into_number_box(test: &mut TestingRunner, id: &str, text: &str) {
	let area = tagged_area(test, id).unwrap_or_else(|| panic!("no element {id}"));
	test.click_cursor((f64::from(area.min_x()) + 6., f64::from(area.center().y)));
	test.sync_and_update();
	test.press_key(Key::Named(NamedKey::End));
	for _ in 0..12 {
		test.press_key(Key::Named(NamedKey::Backspace));
	}
	test.write_text(text);
	test.sync_and_update();
}

/// A value typed into a box and not yet confirmed is in the model when a command reads it.
#[test]
fn a_typed_value_is_committed_before_a_command_reads_the_model() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, state) = shell(dir.path());
	state.update(|a| a.selection.select(Row::Item(ItemRef::new(ItemKind::Source, 0))));
	test.sync_and_update();
	test.sync_and_update();
	type_into_number_box(&mut test, "Source.Frequency", "77");
	assert_ne!(state.peek(|a| a.document.project.sources[0].frequency), 77., "typing alone does not apply");
	state.send(AppEvent::SaveAs(dir.path().join("typed.wdproj")));
	assert_eq!(state.peek(|a| a.document.project.sources[0].frequency), 77.);
}

/// Focus leaving the box commits its text: a click on another tree row.
#[test]
fn a_typed_value_is_committed_when_another_tree_row_is_clicked() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, state) = shell(dir.path());
	state.update(|a| a.selection.select(Row::Item(ItemRef::new(ItemKind::Source, 0))));
	test.sync_and_update();
	test.sync_and_update();
	type_into_number_box(&mut test, "Source.Frequency", "66");
	assert_ne!(state.peek(|a| a.document.project.sources[0].frequency), 66.);
	let other = state.peek(|a| a.document.project.sources[1].name.clone());
	let row = find_text(&test, &other).expect("the tree row").layout().area;
	test.click_cursor((f64::from(row.center().x), f64::from(row.center().y)));
	test.sync_and_update();
	assert_eq!(state.peek(|a| a.document.project.sources[0].frequency), 66.);
	assert_eq!(state.peek(|a| a.selection.current()), Some(Row::Item(ItemRef::new(ItemKind::Source, 1))));
}
