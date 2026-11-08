//! The window shell driven headless: the tree, the menu bar and a property edit.

use std::path::Path;

use freya::prelude::*;
use freya_testing::TestingNode;
use freya_testing::prelude::*;
use wavedude_app::ui::app::{Boot, BootInfo, CloseGate, root};
use wavedude_app::ui::shared::Shared;
use wavedude_app_state::log::LogRing;
use wavedude_app_state::selection::{ItemKind, ItemRef, Row};
use wavedude_app_state::settings::AppDirs;
use wavedude_app_state::{AppState, Event as AppEvent, Guarded};

const EXAMPLE: &str = "../wavedude-core/tests/fixtures/examples/Example16_Wave_in_2_boxes.wdproj";

/// A returning user's window: the startup page answered.
fn shell(dir: &Path) -> (TestingRunner, Shared) {
	shell_at(dir, true)
}

fn shell_at(dir: &Path, answered: bool) -> (TestingRunner, Shared) {
	let dirs = AppDirs::under(dir);
	let mut app = AppState::start(&dirs, []).expect("state starts");
	if answered {
		app.first_start = false;
		app.settings.use_openh264 = Some(false);
		app.settings.check_for_updates = Some(false);
	}
	app.handle(AppEvent::SetLanguage("en".into()), std::time::Instant::now());
	app.handle(AppEvent::Guarded(Guarded::Open(EXAMPLE.into())), std::time::Instant::now());
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
	let (mut test, ()) = TestingRunner::new(
		root,
		Size2D::new(1400., 900.),
		move |r| drop(r.provide_root_context(move || boot.clone())),
		1.0,
	);
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

#[test]
fn drop_downs_size_to_their_words() {
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
	let open = words("Open");
	let (hidden, shown) = (open[0], open[1]);
	let reflective = words("Reflective")[0];
	assert!(shown.width() < 90., "the label fills the cell: {}", shown.width());
	assert!(shown.width() > hidden.width(), "the label is wider than its own word: the widest word sets it");
	assert!(
		(shown.width() - reflective.width()).abs() < 10.,
		"widest word {} shown {} hidden {}",
		reflective.width(),
		shown.width(),
		hidden.width()
	);
	assert!(shown.max_x() < 320., "the button stays inside the pane");
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
