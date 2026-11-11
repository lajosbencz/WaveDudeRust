//! Pictures of the window driven headless: whole-window shots for checking a change by eye, and the manual's
//! figures cropped to the control they show.
//!
//! `WAVEDUDE_SHOTS=<dir> cargo test -p wavedude-app --test screenshots -- --ignored window_states`
//! `WAVEDUDE_FIGURES=$PWD/crates/wavedude-app/assets/manual cargo test -p wavedude-app --test screenshots -- --ignored manual_figures`

use std::path::PathBuf;
use std::time::{Duration, Instant};

use freya::prelude::*;
use freya_testing::prelude::*;
use wavedude_app::ui::app::{Boot, BootInfo, CloseGate, root};
use wavedude_app::ui::shared::Shared;
use wavedude_app_state::commands::{Command, Edit, NewPart};
use wavedude_app_state::log::LogRing;
use wavedude_app_state::selection::{ItemKind, ItemRef, Row};
use wavedude_app_state::settings::{AppDirs, Theme};
use wavedude_app_state::{AppState, Event as AppEvent, Guarded};
use wavedude_core::model::{DisplayEdit, DisplayLayerEdit, Layer, ProbeDetector, ProjectEdit, SourceArray, SourceEdit};

const EXAMPLE: &str = "../wavedude-core/tests/fixtures/examples/Example16_Wave_in_2_boxes.wdproj";

fn out_dir(var: &str) -> Option<PathBuf> {
	let dir = PathBuf::from(std::env::var_os(var)?);
	std::fs::create_dir_all(&dir).expect("the output folder");
	Some(dir)
}

struct Shots {
	test: TestingRunner,
	shared: Shared,
	dir: PathBuf,
	_tmp: tempfile::TempDir,
}

impl Shots {
	fn start(dir: PathBuf, size: (f32, f32)) -> Self {
		Self::start_with(dir, size, EXAMPLE)
	}

	/// The shell at `size` with `example` open.
	fn start_with(dir: PathBuf, size: (f32, f32), example: &str) -> Self {
		Self::start_in(dir, size, example, None, Theme::Light)
	}

	/// As [`start_with`](Self::start_with) with the panes `hidden` tucked away and the window in `theme`.
	fn start_in(dir: PathBuf, size: (f32, f32), example: &str, hidden: Option<&[&str]>, theme: Theme) -> Self {
		let tmp = tempfile::tempdir().unwrap();
		let dirs = AppDirs::under(tmp.path());
		let mut app = AppState::start(&dirs, []).expect("state starts");
		app.settings.theme = theme;
		if let Some(hidden) = hidden {
			let mut layout = wavedude_app::ui::window::default_layout();
			for id in hidden {
				layout.hide(id);
			}
			app.settings.layout = Some(serde_json::to_string(&layout).expect("a dock layout serialises"));
		}
		app.first_start = false;
		app.settings.use_openh264 = Some(false);
		app.settings.check_for_updates = Some(false);
		app.settings.warn_when_ram_insufficient = false;
		app.handle(AppEvent::SetLanguage("en".into()), Instant::now());
		let quality = wavedude_core::run::PreviewQuality {
			resolution_divisor: 2,
			frame_divisor: 2,
			format: wavedude_core::engine::SampleFormat::Index,
			..wavedude_core::run::PreviewQuality::DEFAULT
		};
		app.handle(AppEvent::SetPreviewQuality(quality), Instant::now());
		app.handle(AppEvent::Guarded(Guarded::Open(example.into())), Instant::now());
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
			Size2D::new(size.0, size.1),
			move |r| drop(r.provide_root_context(move || boot.clone())),
			1.0,
		);
		test.sync_and_update();
		let shared = handle.shared().expect("the root provided the state");
		Self { test, shared, dir, _tmp: tmp }
	}

	/// Lets the state and the presenter settle: ticks until the run stepped to its frame, then a little more.
	fn settle(&mut self) {
		for _ in 0..200 {
			self.test.poll(Duration::from_millis(16), Duration::from_millis(32));
			if !self.shared.peek(|a| a.live.is_working() || a.live.fit_pending()) {
				break;
			}
		}
		self.test.poll(Duration::from_millis(16), Duration::from_millis(250));
	}

	/// Runs the simulation until `frames` are there (or it ends), then shows `frame`.
	fn run_to(&mut self, frames: u32, frame: u32) {
		self.shared.send(AppEvent::Run);
		let started = Instant::now();
		while started.elapsed() < Duration::from_secs(60) {
			self.test.poll(Duration::from_millis(16), Duration::from_millis(100));
			let done = self.shared.peek(|a| {
				let ended = a.live.run_status().is_some_and(|(state, ..)| state.is_finished());
				ended || a.live.present_spans().first().is_some_and(|&(from, to)| from == 0 && to + 1 >= frames)
			});
			if done {
				break;
			}
		}
		self.shared.send(AppEvent::StopRun);
		self.test.poll(Duration::from_millis(16), Duration::from_millis(500));
		let status = self.shared.peek(|a| a.live.run_status());
		assert!(status.as_ref().is_some_and(|(state, ..)| state.is_finished()), "the run did not stop: {status:?}");
		self.shared.send(AppEvent::Scrub(frame));
		self.settle();
	}

	fn shot(&mut self, name: &str) {
		self.settle();
		self.test.render_to_file(self.dir.join(format!("{name}.png")));
	}

	fn area_of(&self, id: &str) -> Area {
		self.test
			.find(|node, element| {
				Rect::try_downcast(element).filter(|r| r.accessibility.builder.author_id() == Some(id))?;
				Some(node.layout().area)
			})
			.unwrap_or_else(|| panic!("no element with the test id {id}"))
	}

	/// The union of the areas of the elements whose test id starts with `prefix`.
	fn area_under(&self, prefix: &str) -> Area {
		let areas = self.test.find_many(|node, element| {
			let r = Rect::try_downcast(element)?;
			let under = r.accessibility.builder.author_id().is_some_and(|id| id.starts_with(prefix));
			under.then(|| node.layout().area)
		});
		assert!(!areas.is_empty(), "no element with a test id under {prefix}");
		areas.into_iter().reduce(|a, b| a.union(&b)).unwrap()
	}

	/// A pane's area cut below its lowest content, so a short form makes a short figure.
	fn pane_content(&self, id: &str) -> Area {
		let pane = self.area_of(id);
		let bottom = self
			.test
			.find_many(|node, element| {
				Rect::try_downcast(element).filter(|r| r.accessibility.builder.author_id().is_some())?;
				let a = node.layout().area;
				let inside = a.max_y() <= pane.max_y() && a.min_y() >= pane.min_y() && a.min_x() >= pane.min_x();
				(inside && a.width() > 0. && a.height() < pane.height() - 1.).then_some(a.max_y())
			})
			.into_iter()
			.fold(pane.min_y() + 60., f32::max);
		Area::new(pane.origin, (pane.width(), (bottom + 10. - pane.min_y()).min(pane.height())).into())
	}

	/// Crops the window to `area` plus `pad` pixels and writes it as a figure.
	fn figure(&mut self, name: &str, area: Area, pad: f32) {
		self.settle();
		let png = self.test.render();
		let img = image::load_from_memory(png.as_bytes()).expect("the render decodes");
		let x = (area.min_x() - pad).max(0.) as u32;
		let y = (area.min_y() - pad).max(0.) as u32;
		let w = ((area.width() + 2. * pad) as u32).min(img.width() - x);
		let h = ((area.height() + 2. * pad) as u32).min(img.height() - y);
		img.crop_imm(x, y, w, h).save(self.dir.join(format!("{name}.png"))).expect("the figure saves");
	}

	fn select(&mut self, row: Row) {
		self.shared.update(|a| a.selection.select(row));
		self.settle();
	}

	fn command(&mut self, c: Command) {
		self.shared.send(AppEvent::Command(c));
		self.settle();
	}

	/// Opens menu `index` of the bar from the keyboard.
	fn open_menu(&mut self, index: usize) {
		self.test.press_key(Key::Named(NamedKey::F10));
		self.test.sync_and_update();
		for _ in 0..index {
			self.test.press_key(Key::Named(NamedKey::ArrowRight));
			self.test.sync_and_update();
		}
		self.test.press_key(Key::Named(NamedKey::ArrowDown));
		self.test.sync_and_update();
		self.test.sync_and_update();
	}

	fn close_menu(&mut self) {
		self.test.press_key(Key::Named(NamedKey::Escape));
		self.test.press_key(Key::Named(NamedKey::Escape));
		self.test.sync_and_update();
	}

	/// Hovers the submenu entry `id` of an open menu until it unfolds.
	fn hover_submenu(&mut self, id: &str) {
		let entry = centre(self.area_of(id));
		self.test.move_cursor(entry);
		self.test.poll(Duration::from_millis(16), Duration::from_millis(600));
	}

	/// The last part added, selected.
	fn add_part(&mut self, kind: NewPart) -> ItemRef {
		self.command(Command::AddPart { kind, at: None });
		let index = self.shared.peek(|a| a.document.project.parts.len() - 1);
		let item = ItemRef::new(ItemKind::Part, index);
		self.select(Row::Item(item));
		item
	}

	/// The last probe added, selected.
	fn add_probe(&mut self, detector: ProbeDetector) -> ItemRef {
		self.command(Command::AddProbe { at: None, detector });
		let index = self.shared.peek(|a| a.document.project.probes.len() - 1);
		let item = ItemRef::new(ItemKind::Probe, index);
		self.select(Row::Item(item));
		item
	}
}

fn centre(a: Area) -> (f64, f64) {
	(f64::from(a.center().x), f64::from(a.center().y))
}

#[test]
#[ignore = "writes pictures; run with WAVEDUDE_SHOTS set"]
fn window_states() {
	let Some(dir) = out_dir("WAVEDUDE_SHOTS") else { return };
	let mut s = Shots::start(dir, (1400., 900.));
	s.shot("01-open");
	s.run_to(300, 300);
	s.shot("02-frame-300");
	s.select(Row::Item(ItemRef::new(ItemKind::Source, 0)));
	s.shot("03-source-form");
	s.select(Row::Item(ItemRef::new(ItemKind::Part, 1)));
	s.shot("04-part-form");
	s.open_menu(0);
	s.shot("05-file-menu");
	s.close_menu();
	// A drag of the Properties title bar onto the middle of the Project pane: the drop overlay, then the tab.
	let title = centre(s.area_of("DockTitle:properties"));
	s.test.press_cursor(title);
	s.test.move_cursor((title.0 + 40., title.1 + 40.));
	s.test.sync_and_update();
	let tree = centre(s.area_of("DockPane:project"));
	s.test.move_cursor(tree);
	s.test.sync_and_update();
	s.shot("06-dock-drag");
	s.test.release_cursor(tree);
	s.test.sync_and_update();
	s.shot("07-dock-dropped");
	assert!(wavedude_testing::tagged(&s.test, "DockTab:properties").is_some(), "the dropped pane shows as a tab");
}

/// The Examples dialog on its first tab and on a search.
#[test]
#[ignore = "writes pictures; run with WAVEDUDE_SHOTS set"]
fn examples_dialog() {
	let Some(dir) = out_dir("WAVEDUDE_SHOTS") else { return };
	let mut s = Shots::start(dir, (1400., 900.));
	s.open_menu(0);
	let entry = centre(s.area_of("file:examples"));
	s.test.click_cursor(entry);
	s.shot("08-examples-dialog");
	let search = centre(s.area_of("ExamplesSearch"));
	s.test.click_cursor(search);
	s.test.write_text("lens");
	s.shot("09-examples-search");
}

/// The window at common display sizes, with a long form (meta lens), a list editor (interference filter) and a
/// detector selected, and the add-part menu open: where panes crowd or clip.
#[test]
#[ignore = "writes pictures; run with WAVEDUDE_SHOTS set"]
fn window_sizes() {
	let Some(dir) = out_dir("WAVEDUDE_SHOTS") else { return };
	for (w, h) in [(1024., 600.), (1280., 720.), (1366., 768.), (1920., 1080.), (2560., 1440.)] {
		let tag = format!("{w}x{h}");
		let mut s = Shots::start(dir.clone(), (w, h));
		s.run_to(200, 200);
		s.add_part(NewPart::MetaLens);
		s.shot(&format!("size-{tag}-meta-lens"));
		s.add_part(NewPart::InterferenceFilter);
		s.shot(&format!("size-{tag}-filter"));
		s.add_probe(ProbeDetector::Imaging);
		s.shot(&format!("size-{tag}-imaging"));
		let part_button = s.area_of("ToolAddPart");
		s.test.click_cursor(centre(part_button));
		s.test.sync_and_update();
		s.test.sync_and_update();
		s.hover_submenu("AddPart:MetaSurfaces");
		s.shot(&format!("size-{tag}-add-part"));
		s.close_menu();
	}
}

/// The README's animation: the logic gates example played through, one picture per frame of the sweep, for
/// ffmpeg to join (docs/README.md, figures). Run with `WAVEDUDE_GIF` set to the output folder.
#[test]
#[ignore = "writes the README animation's frames; run with WAVEDUDE_GIF set"]
fn readme_animation() {
	let Some(dir) = out_dir("WAVEDUDE_GIF") else { return };
	let example = "../wavedude-core/tests/fixtures/examples/Example18_XOR_AND_OR_logic.wdproj";
	let mut s = Shots::start_in(dir, (1100., 790.), example, Some(&["project", "properties"]), Theme::Dark);
	let brightness = |layer: Layer, v: f64| {
		let edit = DisplayLayerEdit::Brightness(v);
		AppEvent::Command(Command::Edit(Edit::Project(ProjectEdit::Display(match layer {
			Layer::Field => DisplayEdit::Field(edit),
			Layer::Intensity => DisplayEdit::Intensity(edit),
		}))))
	};
	// Full resolution: the pictures are shown at the scene's own pixels.
	let full = wavedude_core::run::PreviewQuality {
		resolution_divisor: 1,
		frame_divisor: 2,
		format: wavedude_core::engine::SampleFormat::Index,
		..wavedude_core::run::PreviewQuality::DEFAULT
	};
	s.shared.send(AppEvent::SetPreviewQuality(full));
	s.run_to(2000, 0);
	// After the open's brightness fit: both layers dimmer than the fit, so the guides keep their structure.
	s.shared.send(brightness(Layer::Field, 5.0));
	s.shared.send(brightness(Layer::Intensity, 2.5));
	s.run_to(2000, 0);
	// The middle of the run: the pulses have reached the gates and the outputs answer.
	let frames = 24;
	for i in 0..frames {
		let frame = 700 + i * 8;
		s.shared.send(AppEvent::Scrub(frame));
		s.shot(&format!("frame-{i:02}"));
	}
}

#[test]
#[ignore = "writes the manual's figures; run with WAVEDUDE_FIGURES set"]
fn manual_figures() {
	let Some(dir) = out_dir("WAVEDUDE_FIGURES") else { return };
	let mut s = Shots::start(dir.clone(), (1400., 900.));
	s.run_to(400, 400);
	s.select(Row::Item(ItemRef::new(ItemKind::Part, 1)));
	s.figure("main-window", Area::new((0., 0.).into(), (1400., 900.).into()), 0.);
	s.figure("project-tree", s.area_of("DockPane:project"), 0.);
	let pictures = s.area_of("PreviewCell:Field").union(&s.area_of("PreviewCell:Intensity"));
	s.figure("preview", pictures, 0.);
	let timeline = s.area_of("PreviewTimeline").union(&s.area_under("PreviewStacked"));
	s.figure("timeline", timeline, 0.);
	let headers = ["MenuFile", "MenuEdit", "MenuView", "MenuSimulation", "MenuHelp"];
	for (index, name, prefix) in [
		(0, "menu-file", "file:"),
		(1, "menu-edit", "edit:"),
		(2, "menu-view", "view:"),
		(3, "menu-simulation", "sim:"),
		(4, "menu-help", "help:"),
	] {
		s.open_menu(index);
		let area = s.area_under(prefix).union(&s.area_of(headers[index]));
		s.figure(name, area, 2.);
		if index == 0 {
			let export = s.area_of("file:export");
			s.test.move_cursor(centre(export));
			s.test.poll(Duration::from_millis(16), Duration::from_millis(600));
			let area = s.area_under("file:export:").union(&export);
			s.figure("menu-file-export", area, 2.);
		}
		s.close_menu();
	}
	let part_button = s.area_of("ToolAddPart");
	s.test.click_cursor(centre(part_button));
	s.test.sync_and_update();
	s.test.sync_and_update();
	s.figure("menu-add-part", s.area_under("AddPart:").union(&part_button), 2.);
	s.close_menu();
	let probe_button = s.area_of("ToolAddProbe");
	s.test.click_cursor(centre(probe_button));
	s.test.sync_and_update();
	s.test.sync_and_update();
	s.hover_submenu("AddProbe:Probabilistic");
	s.figure("menu-add-probe", s.area_under("AddProbe:").union(&probe_button), 2.);
	s.close_menu();

	// The forms, in a taller window so each fits.
	let mut s = Shots::start(dir, (1400., 1500.));
	s.settle();
	let pane = |s: &Shots| s.pane_content("DockPane:properties");
	for (row, name) in [
		(Row::Project, "properties-project"),
		(Row::Scene, "properties-scene"),
		(Row::Simulation, "properties-simulation"),
		(Row::Display, "properties-display"),
	] {
		s.select(row);
		s.figure(name, pane(&s), 0.);
	}
	let source = ItemRef::new(ItemKind::Source, 0);
	s.select(Row::Item(source));
	s.command(Command::Edit(Edit::Source(0, SourceEdit::SetArray(None))));
	s.figure("properties-source", pane(&s), 0.);
	let array = SourceArray { size_x: 7, size_y: 3, ..SourceArray::default() };
	s.command(Command::Edit(Edit::Source(0, SourceEdit::SetArray(Some(array)))));
	s.figure("properties-composite-source", pane(&s), 0.);
	s.select(Row::Item(ItemRef::new(ItemKind::Part, 1)));
	s.figure("properties-rectangle", pane(&s), 0.);
	for (kind, name) in [
		(NewPart::Lens, "properties-lens"),
		(NewPart::FresnelLens, "properties-fresnel-lens"),
		(NewPart::Polygon, "properties-polygon"),
		(NewPart::Ellipse, "properties-ellipse"),
		(NewPart::Triangle, "properties-triangle"),
		(NewPart::Outline, "properties-outline"),
		(NewPart::Fiber, "properties-fibre"),
		(NewPart::BraggReflector, "properties-bragg-reflector"),
		(NewPart::GrinLens, "properties-grin-lens"),
		(NewPart::ZonePlate, "properties-zone-plate"),
		(NewPart::Diffuser, "properties-diffuser"),
		(NewPart::InterferenceFilter, "properties-interference-filter"),
		(NewPart::SimpleGrating, "properties-simple-grating"),
		(NewPart::FacettedGrating, "properties-facetted-grating"),
		(NewPart::MetaLens, "properties-meta-lens"),
		(NewPart::MetaPrism, "properties-meta-prism"),
		(NewPart::MetaAxicon, "properties-meta-axicon"),
		(NewPart::XorGate, "properties-xor-gate"),
	] {
		s.add_part(kind);
		s.figure(name, pane(&s), 0.);
	}
	s.select(Row::Item(ItemRef::new(ItemKind::Probe, 0)));
	s.figure("properties-line-probe", pane(&s), 0.);
	for (detector, name) in [
		(ProbeDetector::Standard, "properties-probe"),
		(ProbeDetector::Counting, "properties-counting-detector"),
		(ProbeDetector::Imaging, "properties-imaging-detector"),
	] {
		s.add_probe(detector);
		s.figure(name, pane(&s), 0.);
	}
	s.command(Command::AddScheme);
	let schemes = s.shared.peek(|a| a.document.project.color_schemes.len());
	s.select(Row::Item(ItemRef::new(ItemKind::Scheme, schemes - 1)));
	s.figure("properties-colour-scheme", pane(&s), 0.);
}
