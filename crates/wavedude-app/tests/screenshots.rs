//! The manual's figures: the window on an example driven headless on Mesa's lavapipe, each figure cropped to the
//! control it shows.
//!
//! `WAVEDUDE_FIGURES=$PWD/crates/wavedude-app/assets/manual cargo test -p wavedude-app --test screenshots -- --ignored`

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use eframe::egui::{self, Key, Rect, Role, pos2, vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::{Harness, HarnessBuilder};
use wavedude_app::App;
use wavedude_app::ui::{Boot, Shell};
use wavedude_app_state::document::Source;
use wavedude_app_state::log::LogRing;
use wavedude_app_state::platform::Instant;
use wavedude_app_state::settings::{AppDirs, Theme};
use wavedude_app_state::{Event, Guarded};
use wavedude_core::engine::SampleFormat;
use wavedude_core::quality::PreviewQuality;

const EXAMPLE: &str = "../wavedude-core/tests/fixtures/examples/Example16_Wave_in_2_boxes.wdproj";
/// Frames for an input to reach the state and its result to show.
const SETTLE: usize = 6;
/// A form figure's height, the Properties pane's in a window 1500 high.
const FORM_HEIGHT: f32 = 967.;

/// The window at `size` on Example16 in the light theme at half resolution, rendered on lavapipe.
fn window(builder: HarnessBuilder<Shell>, dir: &Path, size: egui::Vec2) -> Harness<'static, Shell> {
	let dirs = AppDirs::under(dir);
	let mut app = common::started(&dirs, false);
	let quality = PreviewQuality {
		resolution_divisor: 2,
		frame_divisor: 2,
		format: SampleFormat::Index,
		..PreviewQuality::DEFAULT
	};
	let open = Event::Guarded(Guarded::Open(Source::Path(EXAMPLE.into())));
	for event in
		[Event::SetTheme(Theme::Light), Event::SetLanguage("en".into()), Event::SetPreviewQuality(quality), open]
	{
		app.handle(event, Instant::now());
	}
	let boot = Boot { store: dirs.store(), open: Default::default(), log: LogRing::new(), update: Default::default() };
	let mut harness = builder.with_size(size).build_eframe(move |cc| Shell::new(cc, app, boot));
	harness.run_steps(SETTLE);
	harness
}

fn settle(harness: &mut Harness<'_, Shell>) {
	harness.run_steps(SETTLE);
}

fn app<'h>(harness: &'h Harness<'_, Shell>) -> &'h App {
	harness.state().app()
}

/// Steps the window until `done`, at most two minutes.
fn wait_for(harness: &mut Harness<'_, Shell>, done: impl Fn(&App) -> bool) {
	let given_up = Instant::now() + Duration::from_secs(120);
	while !done(app(harness)) {
		assert!(Instant::now() < given_up, "the window did not get there: {}", app(harness).run_status());
		std::thread::sleep(Duration::from_millis(10));
		harness.run_steps(1);
	}
}

fn out_dir() -> Option<PathBuf> {
	let dir = PathBuf::from(std::env::var_os("WAVEDUDE_FIGURES")?);
	std::fs::create_dir_all(&dir).expect("the figures' folder");
	Some(dir)
}

/// Every node with its bounds: role, label and rectangle.
fn nodes(harness: &Harness<'_, Shell>) -> Vec<(String, Rect)> {
	harness
		.query_all_by(|_| true)
		.filter_map(|n| {
			let a = n.accesskit_node();
			let b = a.bounding_box()?;
			let rect = Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32));
			Some((format!("{:?} {:?}", a.role(), a.label()), rect))
		})
		.collect()
}

/// The union of the nodes in `after` that `before` lacks: what a menu opening added.
fn added(before: &[(String, Rect)], after: &[(String, Rect)]) -> Rect {
	after.iter().filter(|n| !before.contains(n)).fold(Rect::NOTHING, |r, (_, b)| r.union(*b))
}

fn rect_of(harness: &Harness<'_, Shell>, role: Role, label: &str) -> Rect {
	harness.get_by_role_and_label(role, label).rect()
}

/// Writes the window cropped to `area` grown by `pad`, with no pointer over it.
fn figure(harness: &mut Harness<'_, Shell>, dir: &Path, name: &str, area: Rect, pad: f32) {
	harness.event(egui::Event::PointerGone);
	settle(harness);
	let img = harness.render().expect("the window renders");
	// a pointer back over the window, where the next click finds it
	harness.hover_at(pos2(harness.ctx.content_rect().right() - 2., 12.));
	settle(harness);
	let scale = img.width() as f32 / harness.ctx.content_rect().width();
	let area = area.expand(pad);
	let x = (area.min.x * scale).max(0.).round() as u32;
	let y = (area.min.y * scale).max(0.).round() as u32;
	let w = ((area.width() * scale).round() as u32).min(img.width() - x);
	let h = ((area.height() * scale).round() as u32).min(img.height() - y);
	image::imageops::crop_imm(&img, x, y, w, h)
		.to_image()
		.save(dir.join(format!("{name}.png")))
		.expect("the figure saves");
}

/// Hovers `label` of an open menu until its submenu unfolds.
fn unfold(harness: &mut Harness<'_, Shell>, label: &str) {
	harness.get_by_label(label).hover();
	for _ in 0..10 {
		std::thread::sleep(Duration::from_millis(50));
		harness.run_steps(1);
	}
}

/// Clicks `label` in the Part button's menu, unfolding `submenu` first.
fn add_part(harness: &mut Harness<'_, Shell>, submenu: Option<&str>, label: &str) {
	harness.get_by_role_and_label(Role::Button, "Part").click();
	settle(harness);
	if let Some(submenu) = submenu {
		unfold(harness, submenu);
	}
	harness.get_by_label(label).click();
	settle(harness);
}

/// Clicks `label` in the Probe button's menu.
fn add_probe(harness: &mut Harness<'_, Shell>, label: &str) {
	harness.get_by_role_and_label(Role::Button, "Probe").click();
	settle(harness);
	if label != "Standard probe" {
		unfold(harness, "Probabilistic");
	}
	harness.get_by_label(label).click();
	settle(harness);
}

/// Types `value` into the number box `label` and applies it.
fn set_number(harness: &mut Harness<'_, Shell>, label: &str, value: &str) {
	harness.get_by_role_and_label(Role::SpinButton, label).click();
	settle(harness);
	harness.key_press(Key::End);
	for _ in 0..8 {
		harness.key_press(Key::Backspace);
	}
	harness.event(egui::Event::Text(value.into()));
	harness.key_press(Key::Enter);
	settle(harness);
}

#[test]
#[ignore = "writes the manual's figures; run with WAVEDUDE_FIGURES set"]
fn manual_figures() {
	let Some(dir) = out_dir() else { return };
	let Some(builder) = common::lavapipe() else { return };
	let tmp = tempfile::tempdir().unwrap();
	let mut h = window(builder, tmp.path(), vec2(1400., 900.));
	h.key_press(Key::F5);
	wait_for(&mut h, |app| app.live.present_spans().first().is_some_and(|&(from, to)| from == 0 && to >= 400));
	h.key_press(Key::Escape);
	settle(&mut h);
	h.get_by_role_and_label(Role::TreeItem, "Left box").click();
	settle(&mut h);
	common::preview_idle(&mut h);
	figure(&mut h, &dir, "main-window", Rect::from_min_size(pos2(0., 0.), vec2(1400., 900.)), 0.);
	let project = rect_of(&h, Role::Button, "Project");
	let properties = rect_of(&h, Role::Button, "Properties");
	let column = rect_of(&h, Role::Tree, "Project").right();
	figure(&mut h, &dir, "project-tree", Rect::from_x_y_ranges(0.0..=column, project.top()..=properties.top()), 0.);
	let timeline = rect_of(
		&h,
		Role::Slider,
		"Click or drag to scrub; drag a bracket to move the start or end of the export range. Left/Right step a frame, PageUp/PageDown ten, Home/End jump to the start/end of the range",
	);
	let intensity = rect_of(&h, Role::Image, "Intensity");
	figure(
		&mut h,
		&dir,
		"preview",
		Rect::from_x_y_ranges(timeline.x_range(), project.top()..=intensity.bottom() + 4.),
		0.,
	);
	let stacked = rect_of(&h, Role::RadioButton, "Field above intensity");
	figure(&mut h, &dir, "timeline", timeline.union(stacked), 0.);
	for (header, name) in [
		("File", "menu-file"),
		("Edit", "menu-edit"),
		("View", "menu-view"),
		("Simulation", "menu-simulation"),
		("Help", "menu-help"),
	] {
		let before = nodes(&h);
		h.get_by_role_and_label(Role::Button, header).click();
		settle(&mut h);
		let menu = added(&before, &nodes(&h)).union(rect_of(&h, Role::Button, header));
		figure(&mut h, &dir, name, menu, 2.);
		if header == "File" {
			let before = nodes(&h);
			unfold(&mut h, "Export");
			let export = h.get_by_label("Export").rect();
			let menu = added(&before, &nodes(&h)).union(export);
			figure(&mut h, &dir, "menu-file-export", menu, 2.);
		}
		h.key_press(Key::Escape);
		h.key_press(Key::Escape);
		settle(&mut h);
	}
	for (button, name, submenu) in [("Part", "menu-add-part", None), ("Probe", "menu-add-probe", Some("Probabilistic"))]
	{
		let before = nodes(&h);
		h.get_by_role_and_label(Role::Button, button).click();
		settle(&mut h);
		if let Some(submenu) = submenu {
			unfold(&mut h, submenu);
		}
		let menu = added(&before, &nodes(&h)).union(rect_of(&h, Role::Button, button));
		figure(&mut h, &dir, name, menu, 2.);
		h.key_press(Key::Escape);
		h.key_press(Key::Escape);
		settle(&mut h);
	}

	// The forms, in a taller window so each fits.
	let mut h = window(common::lavapipe().expect("lavapipe was there"), tmp.path(), vec2(1400., 1500.));
	let form = |h: &Harness<'_, Shell>| {
		let title = rect_of(h, Role::Button, "Properties");
		Rect::from_min_size(pos2(0., title.top()), vec2(rect_of(h, Role::Tree, "Project").right(), FORM_HEIGHT))
	};
	// without the focus an added item's form takes, as a reader sees a form
	let shot = |h: &mut Harness<'_, Shell>, name: &str| {
		h.ctx.memory_mut(|m| {
			if let Some(id) = m.focused() {
				m.surrender_focus(id);
			}
		});
		settle(h);
		let area = form(h);
		figure(h, &dir, name, area, 0.);
	};
	for (row, name) in [
		("Project", "properties-project"),
		("Scene", "properties-scene"),
		("Simulation", "properties-simulation"),
		("Display", "properties-display"),
	] {
		h.get_by_role_and_label(Role::TreeItem, row).click();
		settle(&mut h);
		shot(&mut h, name);
	}
	// the example's source is composite: the plain one first, then a composite of 7 x 3
	h.get_by_role_and_label(Role::TreeItem, "Vertical source").click();
	settle(&mut h);
	h.get_by_role_and_label(Role::CheckBox, "Composite source").click();
	settle(&mut h);
	shot(&mut h, "properties-source");
	h.get_by_role_and_label(Role::CheckBox, "Composite source").click();
	settle(&mut h);
	set_number(&mut h, "Columns", "7");
	set_number(&mut h, "Rows", "3");
	shot(&mut h, "properties-composite-source");
	h.get_by_role_and_label(Role::TreeItem, "Left box").click();
	settle(&mut h);
	shot(&mut h, "properties-rectangle");
	h.get_by_role_and_label(Role::TreeItem, "Field amplitude diagonal left box").click();
	settle(&mut h);
	shot(&mut h, "properties-line-probe");
	for (label, name) in [
		("Standard probe", "properties-probe"),
		("Counting detector", "properties-counting-detector"),
		("Imaging detector", "properties-imaging-detector"),
	] {
		add_probe(&mut h, label);
		shot(&mut h, name);
	}
	let schemes = h.get_by_label_contains("olour schemes").rect();
	h.get_all_by_role_and_label(Role::Button, "Add an item to this folder")
		.find(|b| schemes.contains(b.rect().center()))
		.expect("the colour schemes' add button")
		.click();
	settle(&mut h);
	shot(&mut h, "properties-colour-scheme");
	for (submenu, label, name) in [
		(None, "Lens/Mirror", "properties-lens"),
		(None, "Fresnel lens", "properties-fresnel-lens"),
		(None, "Polygon", "properties-polygon"),
		(None, "Ellipse", "properties-ellipse"),
		(None, "Triangle", "properties-triangle"),
		(None, "Outline", "properties-outline"),
		(None, "Fibre", "properties-fibre"),
		(None, "Bragg reflector", "properties-bragg-reflector"),
		(None, "GRIN lens", "properties-grin-lens"),
		(None, "Zone plate", "properties-zone-plate"),
		(None, "Diffuser", "properties-diffuser"),
		(None, "Interference filter", "properties-interference-filter"),
		(Some("Diffraction gratings"), "Simple 2D grating", "properties-simple-grating"),
		(Some("Diffraction gratings"), "Facetted grating", "properties-facetted-grating"),
		(Some("Meta surfaces"), "Meta lens", "properties-meta-lens"),
		(Some("Meta surfaces"), "Meta prism", "properties-meta-prism"),
		(Some("Meta surfaces"), "Meta axicon", "properties-meta-axicon"),
		(Some("Optical logic"), "XOR (Mach-Zehnder)", "properties-xor-gate"),
	] {
		add_part(&mut h, submenu, label);
		shot(&mut h, name);
	}
}
