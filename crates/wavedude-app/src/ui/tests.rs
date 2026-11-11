//! The views driven headless: which memos a playback tick reaches, and every form's rows.

use std::path::Path;
use std::time::{Duration, Instant};

use freya::prelude::*;
use freya_testing::prelude::*;
use wavedude_core::model::{OutlineEdit, PartEdit, PartKindEdit, Point, ProbeDetector, SourceArray, SourceEdit};

use super::app::{Boot, BootInfo, CloseGate, root};
use super::shared::{Shared, runs};
use wavedude_app_state::commands::{Command, Edit, NewPart};
use wavedude_app_state::log::LogRing;
use wavedude_app_state::selection::{ItemKind, ItemRef, Row};
use wavedude_app_state::settings::AppDirs;
use wavedude_app_state::{AppState, Event as AppEvent, Guarded};

const EXAMPLE: &str = "../wavedude-core/tests/fixtures/examples/Example16_Wave_in_2_boxes.wdproj";

fn shell(dir: &Path, prepare: impl FnOnce(&mut super::shared::App)) -> (TestingRunner, Shared) {
	let dirs = AppDirs::under(dir);
	let mut app = AppState::start(&dirs, []).expect("state starts");
	app.first_start = false;
	app.settings.use_openh264 = Some(false);
	app.settings.check_for_updates = Some(false);
	app.handle(AppEvent::SetLanguage("en".into()), Instant::now());
	prepare(&mut app);
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
	let (mut test, ()) = TestingRunner::new(
		root,
		Size2D::new(1400., 900.),
		move |r| drop(r.provide_root_context(move || boot.clone())),
		1.0,
	);
	test.sync_and_update();
	(test, handle.shared().expect("the root provided the state"))
}

/// Drives the shell until the frame an open or edit asked for has arrived, so later revisions are the test's own.
fn settle(test: &mut TestingRunner, shared: &Shared) {
	let deadline = Instant::now() + Duration::from_secs(60);
	loop {
		test.sync_and_update();
		if !shared.peek(|a| a.live.is_working() || a.live.fit_pending()) {
			// The arrival's revisions reach the memos on the next pass.
			test.sync_and_update();
			return;
		}
		assert!(Instant::now() < deadline, "the frame arrives");
		std::thread::sleep(Duration::from_millis(10));
	}
}

#[test]
fn a_playback_tick_reaches_neither_the_menus_nor_the_tree() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, shared) = shell(dir.path(), |app| {
		app.settings.auto_rerun = false;
		app.handle(AppEvent::Guarded(Guarded::Open(EXAMPLE.into())), Instant::now());
	});
	settle(&mut test, &shared);
	let before = (runs("menu"), runs("tree"), runs("properties"), shared.revisions());
	assert!(before.0 > 0 && before.1 > 0, "the memos ran once");
	for frame in 1..6 {
		shared.update(|a| a.live.playback.set_position(frame));
		test.sync_and_update();
	}
	let after = shared.revisions();
	assert!(after.playback > before.3.playback, "the position moved");
	assert_eq!(after.document, before.3.document);
	assert_eq!((runs("menu"), runs("tree"), runs("properties")), (before.0, before.1, before.2));
	shared.update(|a| a.selection.select(Row::Scene));
	test.sync_and_update();
	assert!(runs("tree") > before.1 && runs("properties") > before.2, "a selection reaches them");
}

/// The OS clipboard is read when a menu opens, never while rendering (copypasta off the main thread segfaults
/// on macOS when several renders read it at once).
#[test]
fn the_clipboard_is_read_as_a_menu_opens_not_as_the_shell_renders() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, shared) = shell(dir.path(), |app| {
		app.handle(AppEvent::Guarded(Guarded::Open(EXAMPLE.into())), Instant::now());
	});
	settle(&mut test, &shared);
	shared.update(|a| a.selection.select(Row::Scene));
	test.sync_and_update();
	assert!(runs("menu") > 0, "the menu model was built");
	assert_eq!(runs("clipboard"), 0, "no render reads the clipboard");
	test.press_key(Key::Named(NamedKey::F10));
	test.sync_and_update();
	assert_eq!(runs("clipboard"), 1, "the bar opening looks once");
}

#[test]
fn every_form_is_built_for_every_item_kind() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, shared) = shell(dir.path(), |app| {
		let now = Instant::now();
		let mut apply = |c| app.apply(c, now).expect("the command applies");
		apply(Command::AddSource { at: None });
		apply(Command::Edit(Edit::Source(0, SourceEdit::SetArray(Some(SourceArray::default())))));
		apply(Command::Edit(Edit::Source(0, SourceEdit::DetachAfter(Some(100)))));
		apply(Command::AddSource { at: None });
		apply(Command::AddProbe { at: None, detector: ProbeDetector::Standard });
		apply(Command::AddScheme);
		for kind in NewPart::ALL {
			apply(Command::AddPart { kind, at: None });
		}
	});
	let (parts, sources, probes, schemes) = shared.peek(|a| {
		let p = &a.document.project;
		(p.parts.len(), p.sources.len(), p.probes.len(), p.color_schemes.len())
	});
	assert_eq!(parts, NewPart::ALL.len());
	let mut rows = vec![Row::Project, Row::Scene, Row::Simulation, Row::Display];
	for (kind, count) in
		[(ItemKind::Source, sources), (ItemKind::Part, parts), (ItemKind::Probe, probes), (ItemKind::Scheme, schemes)]
	{
		rows.extend((0..count).map(|i| Row::Item(ItemRef::new(kind, i))));
	}
	for row in rows {
		shared.update(|a| a.selection.select(row));
		for _ in 0..3 {
			test.sync_and_update();
		}
		let first = match row {
			Row::Project => "ProjectNewSeed",
			Row::Scene => "Scene.Mode",
			Row::Simulation => "Simulation.Steps",
			Row::Display => "DisplayLayer.Brightness",
			Row::Item(i) => match i.kind {
				ItemKind::Source => "Source.X",
				ItemKind::Part => "Part.X",
				ItemKind::Probe => "Probe.BoxWidth",
				ItemKind::Scheme => "ColorScheme.Kind",
			},
			Row::Folder(_) => unreachable!("the rows are objects"),
		};
		assert!(area_of(&test, first).is_some(), "{row:?} built no form: '{first}' is not drawn");
	}
}

/// The boxes sit in a zero-size layer over the picture; a press on one must still reach it.
#[test]
fn a_press_on_a_graph_box_selects_its_probe() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, shared) = shell(dir.path(), |app| {
		app.settings.auto_rerun = false;
		app.handle(AppEvent::Guarded(Guarded::Open(EXAMPLE.into())), Instant::now());
	});
	settle(&mut test, &shared);
	let area = test
		.find(|node, element| {
			Rect::try_downcast(element).filter(|r| r.accessibility.builder.author_id() == Some("ProbeGraphBox:0"))?;
			Some(node.layout().area)
		})
		.expect("the first probe's box is drawn");
	assert!(area.width() > 20. && area.height() > 20., "the box has its size: {area:?}");
	let at = (f64::from(area.center().x), f64::from(area.center().y));
	assert!(shared.peek(|a| !a.selection.contains(ItemRef::new(ItemKind::Probe, 0))));
	test.press_cursor(at);
	test.sync_and_update();
	test.release_cursor(at);
	test.sync_and_update();
	assert!(shared.peek(|a| a.selection.contains(ItemRef::new(ItemKind::Probe, 0))), "the press selected the probe");
}

/// The area of the rectangle carrying the test id `id`, if it is drawn.
fn area_of(test: &TestingRunner, id: &str) -> Option<Area> {
	test.find(|node, element| {
		Rect::try_downcast(element).filter(|r| r.accessibility.builder.author_id() == Some(id))?;
		Some(node.layout().area)
	})
}

#[test]
fn an_outline_point_row_fits_the_pane() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, shared) = shell(dir.path(), |app| {
		let now = Instant::now();
		app.apply(Command::AddPart { kind: NewPart::Outline, at: None }, now).expect("the part is added");
		let points = vec![Point::new(-40, -30), Point::new(40, -30), Point::new(0, 40)];
		let edit = PartEdit::Kind(PartKindEdit::Outline(OutlineEdit::Points(points)));
		app.apply(Command::Edit(Edit::Part(0, edit)), now).expect("points set");
	});
	shared.update(|a| a.selection.select(Row::Item(ItemRef::new(ItemKind::Part, 0))));
	for _ in 0..3 {
		test.sync_and_update();
	}
	let column = area_of(&test, "Coating.Depth").expect("the coating rows are drawn");
	// the points sit below the fold
	test.scroll((f64::from(column.center().x), f64::from(column.center().y)), (0., -2000.));
	for _ in 0..3 {
		test.sync_and_update();
	}
	let (x, y, remove) = (area_of(&test, "Point0.X"), area_of(&test, "Point0.Y"), area_of(&test, "Point0.Remove"));
	let [x, y, remove] = [x, y, remove].map(|a| a.expect("the point row is drawn"));
	assert!(x.width() > 20. && y.width() > 20., "both boxes share the row: {x:?} {y:?}");
	assert!(x.max_x() <= y.min_x() && y.max_x() <= remove.min_x(), "X, Y, remove in order");
	assert!(remove.max_x() <= column.max_x() + 1., "the remove button stays in the pane: {remove:?} {column:?}");
}

#[test]
fn gate_and_zone_plate_readouts_match_the_design() {
	let dir = tempfile::tempdir().unwrap();
	let (_test, shared) = shell(dir.path(), |app| {
		let now = Instant::now();
		app.apply(Command::AddPart { kind: NewPart::XorGate, at: None }, now).expect("the gate is added");
		app.apply(Command::AddPart { kind: NewPart::ZonePlate, at: None }, now).expect("the plate is added");
	});
	let readouts =
		|i| shared.peek(|a| super::properties_forms::readouts(a, Row::Item(ItemRef::new(ItemKind::Part, i))));
	let gate = readouts(0).gate.expect("the default gate has a design");
	assert_eq!(gate.spacing, 60, "the spacing the design uses, not the stored 2");
	assert!((gate.quarter_wave - 5.0).abs() < 0.05, "lambda / (4 sqrt(core index)): {}", gate.quarter_wave);
	assert_eq!(readouts(1).zones.expect("the plate has a wavelength").0, 8, "the zone index at the rim plus one");
}
