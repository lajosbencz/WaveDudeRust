//! The window shell driven headless: the tree, the menu bar and a property edit.

use std::path::Path;
use std::time::Instant;

use freya::prelude::*;
use freya_testing::prelude::*;
use wavedude_app::ui::app::{Boot, BootInfo, CloseGate, root};
use wavedude_app::ui::shared::Shared;
use wavedude_app_state::commands::{Command, NewPart};
use wavedude_app_state::log::LogRing;
use wavedude_app_state::selection::{ItemKind, ItemRef, Row};
use wavedude_app_state::settings::{AppDirs, Settings};
use wavedude_app_state::{AppState, Event as AppEvent, Guarded};
use wavedude_core::model::{FilterLayer, PartKind, ProbeDetector, ProbeKind, Project};
use wavedude_examples::Topic;
use wavedude_testing::{click_at, click_tagged, count_tagged, find_text, label_areas, settle, tagged, tagged_area};

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
		open: Default::default(),
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
	// Continue sends the page's answers as settings events, each of which writes the file: read back through the
	// loader, the file holds what the state holds, and the next start is not a first one
	let saved = Settings::load(&AppDirs::under(dir.path()).settings_file(), || panic!("the file exists"));
	assert!(!saved.first_start);
	assert_eq!(saved.settings, state.peek(|a| a.settings.clone()));
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
	click_at(&mut test, &toggle, (f64::from(area.min_x()) + 2., f64::from(area.center().y)), "Enabled");
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
	click_at(&mut test, &toggle, (f64::from(area.min_x()) + 2., f64::from(area.center().y)), "Auto-scale steps");
	assert_eq!(state.peek(|a| a.document.project.probes[0].auto_scale_x), before, "a disabled check box toggled");
	assert!(!open_step_axis_combo(&mut test), "a disabled combo box opened its menu");
}

/// Clicks the Step axis combo (the widest "Running": the combo measures its words in a hidden copy) and says
/// whether its menu opened, which adds a second visible "Full".
fn open_step_axis_combo(test: &mut TestingRunner) -> bool {
	let shown = label_areas(test, "Running").into_iter().max_by(|a, b| a.width().total_cmp(&b.width())).unwrap();
	let before = label_areas(test, "Full").len();
	test.click_cursor((f64::from(shown.center().x), f64::from(shown.center().y)));
	test.sync_and_update();
	test.sync_and_update();
	label_areas(test, "Full").len() > before
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
	click_at(&mut test, &toggle, (f64::from(area.min_x()) + 2., f64::from(area.center().y)), "Auto-scale steps");
	assert_ne!(
		state.peek(|a| a.document.project.probes[0].auto_scale_x),
		before,
		"the check box did not edit the detector"
	);
	assert!(open_step_axis_combo(&mut test), "the enabled combo box opens its menu");
}

/// A drag by a layer's grip moves it through the list: the first layer dragged one row down swaps with the second.
#[test]
fn a_layer_dragged_by_its_grip_moves_down_the_list() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, state) = tall_shell(dir.path());
	state.update(|a| {
		drop(a.apply(Command::AddPart { kind: NewPart::InterferenceFilter, at: None }, Instant::now()));
	});
	settle(&mut test);
	let part = state.peek(|a| a.document.project.parts.len() - 1);
	fn layers_of(project: &Project, part: usize) -> Vec<FilterLayer> {
		match &project.parts[part].kind {
			PartKind::InterferenceFilter(f) => f.layers.clone(),
			_ => unreachable!("the part added is a filter"),
		}
	}
	let before = state.peek(|a| layers_of(&a.document.project, part));
	assert!(before.len() >= 2 && before[0] != before[1], "a new filter alternates two layers");
	let first = tagged(&test, "Layer0.Grip").expect("the first layer's grip");
	let second = tagged_area(&test, "Layer1.Grip").expect("the second layer's grip");
	wavedude_testing::assert_on_screen(&test, &first, "Layer0.Grip");
	let start = first.layout().area.center();
	let pitch = f64::from(second.center().y - start.y);
	test.press_cursor((f64::from(start.x), f64::from(start.y)));
	test.move_cursor((f64::from(start.x), f64::from(start.y) + pitch * 0.3));
	test.move_cursor((f64::from(start.x), f64::from(start.y) + pitch));
	settle(&mut test);
	test.release_cursor((f64::from(start.x), f64::from(start.y) + pitch));
	settle(&mut test);
	let after = state.peek(|a| layers_of(&a.document.project, part));
	assert_eq!((&after[0], &after[1]), (&before[1], &before[0]), "the first two layers changed places");
	assert_eq!(after[2..], before[2..], "the rest stayed");
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

#[test]
fn an_opened_project_shows_its_root() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, state) = shell(dir.path());
	settle(&mut test);
	assert_eq!(state.peek(|a| a.selection.current()), Some(Row::Project));
	assert!(find_text(&test, "No selection").is_none(), "the Properties pane shows the Project form");
	assert!(count_tagged(&test, "Project.") > 0, "the Project form's rows are shown");
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
	let matches = |t: Topic| {
		wavedude_examples::all()
			.iter()
			.filter(|e| {
				e.topic == t
					&& (e.title.to_lowercase().contains("fibre") || e.description.to_lowercase().contains("fibre"))
			})
			.count()
	};
	// the search lands on the first tab with a match and shows that tab's matches
	let expected = Topic::ALL.iter().map(|&t| matches(t)).find(|&n| n > 0).unwrap();
	assert!(expected > 1, "more than one example names a fibre");
	assert_eq!(count_tagged(&test, "Example:"), expected);
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
