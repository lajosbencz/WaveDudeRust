//! The Properties pane driven headless on an example: the number box's keys, Enter and Esc, its drag, typed text
//! applied before a shortcut and when its form goes, the focus that Enter on a tree row and an Add put into the
//! form, a check box, a drop-down, disabled rows, the colour picker and the gradient.

mod common;

use std::path::Path;

use eframe::egui::{self, Key, Modifiers, PointerButton, Pos2, Role, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use wavedude_app::App;
use wavedude_app::ui::{Boot, Shell};
use wavedude_app_state::document::Source;
use wavedude_app_state::log::LogRing;
use wavedude_app_state::platform::Instant;
use wavedude_app_state::settings::{AppDirs, Theme};
use wavedude_app_state::{Event, Guarded};
use wavedude_core::model::{Project, Rgb, SourceMode};

const EXAMPLE: &str = "../wavedude-core/tests/fixtures/examples/Example01_Source_Mirror_Probe.wdproj";
/// Frames for an input to reach the state and its result to show.
const SETTLE: usize = 4;

/// The window on `project` with nothing run, in the light theme, tall enough for a form.
fn window_on(dir: &Path, project: &Path) -> Harness<'static, Shell> {
	let dirs = AppDirs::under(dir);
	let mut app = common::started(&dirs, false);
	let open = Event::Guarded(Guarded::Open(Source::Path(project.into())));
	for event in [Event::SetTheme(Theme::Light), Event::SetAutoRerun(false), open] {
		app.handle(event, Instant::now());
	}
	let boot = Boot {
		store: dirs.store(),
		gpu: None,
		open: Default::default(),
		log: LogRing::new(),
		update: Default::default(),
	};
	let mut harness =
		Harness::builder().with_size(vec2(1280., 1200.)).build_eframe(move |cc| Shell::new(cc, app, boot));
	harness.run_steps(SETTLE);
	harness
}

fn window(dir: &Path) -> Harness<'static, Shell> {
	window_on(dir, Path::new(EXAMPLE))
}

fn settle(harness: &mut Harness<'_, Shell>) {
	harness.run_steps(SETTLE);
}

fn app<'h>(harness: &'h Harness<'_, Shell>) -> &'h App {
	harness.state().app()
}

fn project<'h>(harness: &'h Harness<'_, Shell>) -> &'h Project {
	&app(harness).document.project
}

/// Selects the tree row `label`.
fn select(harness: &mut Harness<'_, Shell>, label: &str) {
	harness.get_by_role_and_label(Role::TreeItem, label).click();
	settle(harness);
}

/// Clicks the editor `label` of `role`.
fn click(harness: &mut Harness<'_, Shell>, role: Role, label: &str) {
	harness.get_by_role_and_label(role, label).click();
	settle(harness);
}

fn typed(harness: &mut Harness<'_, Shell>, text: &str) {
	harness.event(egui::Event::Text(text.into()));
	settle(harness);
}

fn key(harness: &mut Harness<'_, Shell>, key: Key) {
	harness.key_press(key);
	settle(harness);
}

fn button(harness: &mut Harness<'_, Shell>, pos: Pos2, pressed: bool) {
	let modifiers = Default::default();
	harness.event(egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers });
	harness.run_steps(1);
}

#[test]
fn a_number_box_steps_by_its_keys_applies_on_enter_and_esc_reverts_without_stopping_the_run() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path());
	select(&mut harness, "Source 1");
	let x0 = project(&harness).sources[0].x;
	click(&mut harness, Role::SpinButton, "X");
	key(&mut harness, Key::ArrowUp);
	assert_eq!(project(&harness).sources[0].x, x0 + 1., "Up did not step");
	typed(&mut harness, "123");
	assert_eq!(project(&harness).sources[0].x, x0 + 1., "typing applied before Enter");
	key(&mut harness, Key::Enter);
	assert_eq!(project(&harness).sources[0].x, 123.);
	assert!(harness.get_by_role_and_label(Role::SpinButton, "X").is_focused(), "Enter took the focus away");
	key(&mut harness, Key::F5);
	assert!(app(&harness).live.run_is_active(), "F5 started no run");
	assert!(harness.get_by_role_and_label(Role::SpinButton, "X").is_focused(), "F5 took the focus away");
	typed(&mut harness, "456");
	key(&mut harness, Key::Escape);
	assert_eq!(project(&harness).sources[0].x, 123., "Esc applied the typed text");
	assert!(app(&harness).live.run_is_active(), "the Esc that reverted the text also stopped the run");
	key(&mut harness, Key::Escape);
	assert!(!app(&harness).live.run_is_active(), "Esc with nothing typed did not stop the run");
}

#[test]
fn a_drag_on_a_number_box_scrubs_its_value() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path());
	select(&mut harness, "Source 1");
	let x0 = project(&harness).sources[0].x;
	let at = harness.get_by_role_and_label(Role::SpinButton, "X").rect().left_center() + vec2(10., 0.);
	harness.hover_at(at);
	harness.run_steps(1);
	button(&mut harness, at, true);
	for i in 1..=5 {
		harness.hover_at(at + vec2(8. * i as f32, 0.));
		harness.run_steps(1);
	}
	button(&mut harness, at + vec2(40., 0.), false);
	settle(&mut harness);
	assert!(project(&harness).sources[0].x > x0, "the drag to the right did not raise X");
	assert!(!harness.get_by_role_and_label(Role::SpinButton, "X").is_focused(), "the drag focused the box");
}

#[test]
fn typed_text_applies_before_a_shortcut_and_when_its_form_goes() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path());
	select(&mut harness, "Source 1");
	click(&mut harness, Role::SpinButton, "X");
	typed(&mut harness, "77");
	// Stop: harmless with nothing running
	harness.key_press_modifiers(Modifiers::SHIFT, Key::F5);
	settle(&mut harness);
	assert_eq!(project(&harness).sources[0].x, 77., "the shortcut went before the typed text");
	click(&mut harness, Role::SpinButton, "Y");
	typed(&mut harness, "66");
	select(&mut harness, "Rectangle 1");
	assert_eq!(project(&harness).sources[0].y, 66., "the typed text was lost with its form");
}

#[test]
fn enter_on_a_tree_row_puts_the_caret_into_the_name_with_its_text_selected() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path());
	select(&mut harness, "Source 1");
	key(&mut harness, Key::Enter);
	assert!(harness.get_by_role_and_label(Role::TextInput, "Name").is_focused(), "the name box has no focus");
	typed(&mut harness, "Beam");
	assert_eq!(project(&harness).sources[0].name, "Beam", "the typed name did not replace the old one");
}

#[test]
fn an_added_item_puts_the_caret_into_its_form() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path());
	click(&mut harness, Role::Button, "Source");
	assert_eq!(project(&harness).sources.len(), 2);
	assert!(
		harness.get_by_role_and_label(Role::TextInput, "Name").is_focused(),
		"the added source's form has no focus"
	);
	typed(&mut harness, "Second");
	assert_eq!(project(&harness).sources[1].name, "Second");
}

#[test]
fn a_check_box_a_drop_down_and_the_rows_they_enable() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path());
	select(&mut harness, "Source 1");
	let on = project(&harness).sources[0].enabled;
	click(&mut harness, Role::CheckBox, "Enabled");
	assert_eq!(project(&harness).sources[0].enabled, !on);
	let mode = project(&harness).sources[0].mode;
	let other = if mode == SourceMode::Pulsed { "Continuous" } else { "Pulsed" };
	click(&mut harness, Role::ComboBox, "Mode");
	harness.get_by_label(other).click();
	settle(&mut harness);
	let pulsed = project(&harness).sources[0].mode == SourceMode::Pulsed;
	assert_ne!(project(&harness).sources[0].mode, mode, "the pick left the mode");
	let width = harness.get_by_role_and_label(Role::SpinButton, "Pulse width");
	assert_eq!(width.accesskit_node().is_disabled(), !pulsed, "Pulse width does not follow the mode");
}

/// The added scheme's first stop.
fn first_stop(harness: &Harness<'_, Shell>) -> Rgb {
	project(harness).color_schemes.last().expect("a scheme").stops[0].color
}

/// Adds a colour scheme by its folder's plus.
fn add_scheme(harness: &mut Harness<'_, Shell>) {
	select(harness, "Colour schemes");
	let folder = harness.get_by_role_and_label(Role::TreeItem, "Colour schemes").rect();
	let add = harness
		.get_all_by_label("Add an item to this folder")
		.find(|n| folder.y_range().contains(n.rect().center().y))
		.expect("the folder's plus");
	add.click();
	settle(harness);
}

const SWATCH: &str = "Pick the colour: Space opens the picker, Enter keeps it, Escape puts the old colour back";

#[test]
fn the_pickers_typed_text_applies_before_a_shortcut_and_when_the_picker_closes() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path());
	add_scheme(&mut harness);
	harness.get_all_by_role_and_label(Role::ColorWell, SWATCH).next().expect("a stop's swatch").click();
	settle(&mut harness);
	click(&mut harness, Role::SpinButton, "G");
	typed(&mut harness, "33");
	assert_ne!(first_stop(&harness).g, 33, "typing applied before Enter");
	// Stop: harmless with nothing running
	harness.key_press_modifiers(Modifiers::SHIFT, Key::F5);
	settle(&mut harness);
	assert_eq!(first_stop(&harness).g, 33, "the shortcut went before the typed channel");
	click(&mut harness, Role::TextInput, "#");
	typed(&mut harness, "#123456");
	let away = harness.get_by_role_and_label(Role::TreeItem, "Project").rect().center();
	button(&mut harness, away, true);
	button(&mut harness, away, false);
	settle(&mut harness);
	assert!(harness.query_by_role_and_label(Role::TextInput, "#").is_none(), "the click left the picker open");
	assert_eq!(first_stop(&harness).to_hex(), "#123456", "the typed hex was lost with the picker");
}

#[test]
fn the_picker_changes_a_stop_and_esc_puts_its_colour_back_and_the_gradient_adds_and_removes_stops() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path());
	add_scheme(&mut harness);
	assert!(
		harness.get_by_role_and_label(Role::TextInput, "Name").is_focused(),
		"the added scheme's form has no focus"
	);
	let before = first_stop(&harness);
	harness.get_all_by_role_and_label(Role::ColorWell, SWATCH).next().expect("a stop's swatch").click();
	settle(&mut harness);
	click(&mut harness, Role::SpinButton, "R");
	let r = if before.r == 10 { "20" } else { "10" };
	typed(&mut harness, r);
	key(&mut harness, Key::Enter);
	assert_eq!(first_stop(&harness).r.to_string(), r, "the picker's R box left the stop");
	key(&mut harness, Key::Escape);
	assert_eq!(first_stop(&harness), before, "Esc kept the picked colour");
	assert!(harness.query_by_role_and_label(Role::SpinButton, "R").is_none(), "Esc left the picker open");

	let stops = project(&harness).color_schemes.last().unwrap().stops.len();
	let tip = "Drag a marker to move its stop";
	let bar = harness
		.get_all_by_role(Role::Slider)
		.find(|n| n.accesskit_node().label().is_some_and(|l| l.starts_with(tip)))
		.unwrap();
	let at = bar.rect().left_top() + vec2(bar.rect().width() * 0.37, 8.);
	button(&mut harness, at, true);
	button(&mut harness, at, false);
	settle(&mut harness);
	assert_eq!(
		project(&harness).color_schemes.last().unwrap().stops.len(),
		stops + 1,
		"a click on the bar added no stop"
	);
	key(&mut harness, Key::Delete);
	assert_eq!(project(&harness).color_schemes.last().unwrap().stops.len(), stops, "Del kept the selected stop");
}

#[test]
fn a_project_scheme_joins_the_users_and_leaves_them_from_the_tree_and_the_picker() {
	let dir = tempfile::tempdir().unwrap();
	let mut harness = window(dir.path());
	add_scheme(&mut harness);
	let name = project(&harness).color_schemes.last().expect("a scheme").name.clone();
	click(&mut harness, Role::Button, "Add to my schemes");
	assert!(app(&harness).is_user_scheme(&name), "the button kept no user scheme");
	let file = AppDirs::under(dir.path()).store().path(&format!("ColorSchemes/{name}.wdscheme"));
	assert!(file.is_file(), "{file:?} not written");
	harness.get_by_role_and_label(Role::TreeItem, &name).click_secondary();
	settle(&mut harness);
	let entry = harness.get_all_by_label("Remove from my schemes").find(|n| n.accesskit_node().role() != Role::Button);
	entry.expect("the row menu's entry").click();
	settle(&mut harness);
	assert!(!file.exists(), "the file stayed");
	assert!(!app(&harness).is_user_scheme(&name), "the row menu removed no user scheme");
	click(&mut harness, Role::Button, "Add to my schemes");
	// the project's scheme goes: the layer that names it shows the user's
	select(&mut harness, &name);
	key(&mut harness, Key::Delete);
	assert!(project(&harness).color_schemes.is_empty(), "Del kept the project's scheme");
	select(&mut harness, "Display");
	harness.get_all_by_role_and_label(Role::ComboBox, "Colour scheme").next().expect("the field's picker").click();
	settle(&mut harness);
	assert!(harness.query_by_label("Remove from my schemes").is_none(), "a built-in scheme offers its removal");
	harness.get_by_label(&name).click();
	settle(&mut harness);
	assert_eq!(project(&harness).display.field.color_scheme, name, "the pick left the layer's scheme");
	harness.get_all_by_role_and_label(Role::ComboBox, "Colour scheme").next().unwrap().click();
	settle(&mut harness);
	harness.get_by_label("Remove from my schemes").click();
	settle(&mut harness);
	assert!(!app(&harness).is_user_scheme(&name), "the picker removed no user scheme");
}

#[test]
fn the_picker_removes_the_users_scheme_a_legacy_name_resolves_to() {
	let dir = tempfile::tempdir().unwrap();
	let mut flare = wavedude_core::io::schemes::built_in()
		.iter()
		.find(|s| s.name == "Flare inverted")
		.expect("the built-in")
		.clone();
	flare.description = "mine".into();
	let store = AppDirs::under(dir.path()).store();
	wavedude_app_state::user_schemes::add(&store, &flare).unwrap();
	let file = dir.path().join("legacy.wdproj");
	let text = std::fs::read_to_string(EXAMPLE).unwrap().replace("\"Ice and fire\"", "\"Flare_inv\"");
	std::fs::write(&file, text).unwrap();
	let mut harness = window_on(dir.path(), &file);
	assert_eq!(project(&harness).display.field.color_scheme, "Flare_inv");
	select(&mut harness, "Display");
	harness.get_all_by_role_and_label(Role::ComboBox, "Colour scheme").next().expect("the field's picker").click();
	settle(&mut harness);
	harness.get_by_label("Remove from my schemes").click();
	settle(&mut harness);
	assert!(!app(&harness).is_user_scheme("Flare inverted"), "the legacy name's user scheme stayed");
}
