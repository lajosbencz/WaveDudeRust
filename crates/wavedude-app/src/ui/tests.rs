//! The views driven headless: which memos a playback tick reaches, and every form's paths.

use std::path::Path;
use std::time::{Duration, Instant};

use freya::prelude::*;
use freya_testing::prelude::*;
use serde_json::json;

use super::app::{Boot, BootInfo, CloseGate, root};
use super::properties::BOUND;
use super::shared::{Shared, runs};
use wavedude_app_state::commands::{Command, NewPart, Target};
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
	let info =
		BootInfo { ring: LogRing::new(), dirs, close: CloseGate::default(), open: None, update: Default::default() };
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

/// Drives the shell until the scratch frame an open or edit requested has arrived, so later revisions are the test's own.
fn settle(test: &mut TestingRunner, shared: &Shared) {
	let deadline = Instant::now() + Duration::from_secs(60);
	loop {
		test.sync_and_update();
		if !shared.peek(|a| a.live.scratch_pending()) {
			// The arrival's revisions reach the memos on the next pass.
			test.sync_and_update();
			return;
		}
		assert!(Instant::now() < deadline, "the scratch frame arrives");
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

#[test]
fn every_form_binds_paths_its_object_has() {
	let dir = tempfile::tempdir().unwrap();
	let (mut test, shared) = shell(dir.path(), |app| {
		let now = Instant::now();
		let mut apply = |c| app.apply(c, now).expect("the command applies");
		apply(Command::AddSource { at: None });
		let source = Target::Item(ItemRef::new(ItemKind::Source, 0));
		let array = serde_json::to_value(wavedude_core::model::SourceArray::default()).unwrap();
		apply(Command::Set { target: source, path: "array".into(), value: array });
		apply(Command::Set { target: source, path: "detach_after".into(), value: json!(100) });
		apply(Command::AddSource { at: None });
		apply(Command::AddProbe { at: None });
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
		BOUND.with_borrow_mut(Vec::clear);
		shared.update(|a| a.selection.select(row));
		for _ in 0..3 {
			test.sync_and_update();
		}
		assert!(BOUND.with_borrow(|b| !b.is_empty()), "{row:?} built no form");
	}
}
