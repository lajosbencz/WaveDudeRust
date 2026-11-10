use freya::prelude::*;
use freya_testing::prelude::*;
use wavedude_widgets::{NumberBox, NumberSpec, TextBox};

fn find_text(test: &TestingRunner, text: &str) -> bool {
	test.find(|_, element| Label::try_downcast(element).filter(|l| l.text.as_ref() == text)).is_some()
}

#[test]
fn number_box_up_increments() {
	fn app() -> impl IntoElement {
		wavedude_widgets::theme::use_init_wave_theme(wavedude_widgets::theme::Theme::dark);
		let value = use_state(|| 1.0f64);
		rect()
			.child(NumberBox::new(value, NumberSpec::linear(0., 10., 0.5)).width(Size::px(120.)))
			.child(label().text(format!("value={}", value.read())))
	}
	let mut test = launch_test(app);
	test.sync_and_update();
	test.click_cursor((20., 10.));
	test.press_key(Key::Named(NamedKey::ArrowUp));
	test.sync_and_update();
	assert!(find_text(&test, "value=1.5"));
	test.press_key(Key::Named(NamedKey::PageUp));
	test.sync_and_update();
	assert!(find_text(&test, "value=6.5"));
}

#[test]
fn tree_shift_down_extends_selection() {
	use wavedude_widgets::tree::{RowKind, TreeRow, TreeState, TreeView};
	fn app() -> impl IntoElement {
		wavedude_widgets::theme::use_init_wave_theme(wavedude_widgets::theme::Theme::dark);
		let state = use_state(TreeState::<u32>::default);
		let rows = vec![
			TreeRow::new(0, 0, RowKind::Folder, "Sources").group(0),
			TreeRow::new(1, 1, RowKind::Item, "One").group(0).enabled(true),
			TreeRow::new(2, 1, RowKind::Item, "Two").group(0).enabled(true),
			TreeRow::new(3, 1, RowKind::Item, "Three").group(0).enabled(true),
		];
		let selected = state.read().selection.selected.clone();
		rect()
			.width(Size::px(300.))
			.height(Size::px(200.))
			.child(rect().height(Size::px(120.)).child(TreeView::new(rows, state, |_| {})))
			.child(label().text(format!("selected={selected:?}")))
	}
	let mut test = launch_test(app);
	test.sync_and_update();
	test.click_cursor((100., 23. + 11.));
	assert!(find_text(&test, "selected=[1]"));
	test.send_event(PlatformEvent::Keyboard {
		name: KeyboardEventName::KeyDown,
		key: Key::Named(NamedKey::ArrowDown),
		code: Code::ArrowDown,
		modifiers: Modifiers::SHIFT,
	});
	test.sync_and_update();
	assert!(find_text(&test, "selected=[1, 2]"));
}

#[test]
fn menu_panels_fit_their_entries_and_stay_inside_the_window() {
	use wavedude_widgets::menu::{MenuBar, MenuEntry, MenuItem, MenuModel, Submenu};
	fn app() -> impl IntoElement {
		wavedude_widgets::theme::use_init_wave_theme(wavedude_widgets::theme::Theme::dark);
		let model = MenuModel::new(vec![
			Submenu::new(
				"file",
				"_File",
				vec![
					MenuItem::new("file.open", "_Open").shortcut("Ctrl+O").tooltip("Opens a project").into(),
					MenuItem::new("file.long", "A much longer entry than the others").into(),
					MenuEntry::Separator,
					Submenu::new("file.recent", "_Recent", vec![MenuItem::new("file.r1", "One").into()]).into(),
				],
			)
			.into(),
			Submenu::new("edit", "_Edit", vec![MenuItem::new("edit.x", "Thing").into()]).into(),
			Submenu::new("view", "_View", vec![MenuItem::new("view.x", "Thing").into()]).into(),
			Submenu::new("simulation", "_Simulation", vec![MenuItem::new("simulation.x", "Thing").into()]).into(),
			Submenu::new("window", "_Window", vec![MenuItem::new("window.x", "Thing").into()]).into(),
			Submenu::new(
				"tools",
				"_Tools",
				vec![
					MenuItem::new("tools.x", "An entry much too long under the Tools header").into(),
					Submenu::new("tools.more", "_More", vec![MenuItem::new("tools.m1", "One").into()]).into(),
				],
			)
			.into(),
		]);
		rect().width(Size::fill()).height(Size::fill()).child(MenuBar::new(model, |_: String| {}))
	}
	const WINDOW: f32 = 600.;
	let (mut test, ()) = TestingRunner::new(app, Size2D::new(WINDOW, 400.), |_| {}, 1.0);
	test.sync_and_update();
	let area_of = |test: &TestingRunner, id: &str| {
		test.find(|node, element| {
			Rect::try_downcast(element).filter(|r| r.accessibility.builder.author_id() == Some(id))?;
			Some(node.layout().area)
		})
		.unwrap_or_else(|| panic!("{id} not found"))
	};
	let text_of = |test: &TestingRunner, text: &str| {
		test.find(|node, element| {
			Label::try_downcast(element).filter(|l| l.text.as_ref() == text)?;
			Some(node.layout().area)
		})
		.unwrap_or_else(|| panic!("label {text} not found"))
	};
	let hover = |test: &mut TestingRunner, id: &str| {
		let a = area_of(test, id);
		test.move_cursor((f64::from(a.center().x), f64::from(a.center().y)));
		for _ in 0..3 {
			test.sync_and_update();
		}
	};

	test.click_cursor((20., 10.));
	test.sync_and_update();
	let widths =
		[area_of(&test, "file.open"), area_of(&test, "file.long"), area_of(&test, "file.recent")].map(|a| a.width());
	assert!(widths[0] > 200. && widths[0] < 400., "menu row spans {} px", widths[0]);
	assert_eq!(widths[0], widths[1]);
	assert_eq!(widths[0], widths[2]);
	let row_right = area_of(&test, "file.open").max_x();
	let shortcut_right = text_of(&test, "Ctrl+O").max_x();
	assert!(shortcut_right <= row_right, "shortcut ends at {shortcut_right}, row at {row_right}");

	hover(&mut test, "file.recent");
	let recent = area_of(&test, "file.recent");
	let one = area_of(&test, "file.r1");
	assert!(
		one.min_x() >= recent.max_x() - 4. && one.min_x() < recent.max_x() + 8.,
		"submenu at {one:?} beside {recent:?}"
	);
	assert!((one.min_y() - recent.min_y()).abs() < 4., "submenu's first row level with its entry: {one:?} {recent:?}");

	let tools = area_of(&test, "tools");
	test.click_cursor((f64::from(tools.center().x), 10.));
	test.sync_and_update();
	test.sync_and_update();
	let long = area_of(&test, "tools.x");
	assert!(long.width() > WINDOW - tools.min_x(), "the entry must not fit under its header: {long:?}");
	assert!(long.max_x() <= WINDOW && long.min_x() >= 0., "the drop-down ends inside the window: {long:?}");

	hover(&mut test, "tools.more");
	let more = area_of(&test, "tools.more");
	let m1 = area_of(&test, "tools.m1");
	assert!(m1.max_x() <= more.min_x() + 4. && m1.min_x() >= 0., "submenu flipped to the left: {m1:?} of {more:?}");
}

#[test]
fn arrows_walk_a_menu_opened_by_a_click() {
	use wavedude_widgets::menu::{MenuBar, MenuItem, MenuModel, Submenu};
	fn app() -> impl IntoElement {
		wavedude_widgets::theme::use_init_wave_theme(wavedude_widgets::theme::Theme::dark);
		let mut last = use_state(String::new);
		let model = MenuModel::new(vec![
			Submenu::new(
				"file",
				"_File",
				vec![
					MenuItem::new("file.open", "_Open").into(),
					MenuItem::new("file.save", "_Save").into(),
					Submenu::new("file.recent", "_Recent", vec![MenuItem::new("file.r1", "One").into()]).into(),
				],
			)
			.into(),
			Submenu::new("edit", "_Edit", vec![MenuItem::new("edit.x", "Thing").into()]).into(),
		]);
		rect()
			.width(Size::fill())
			.height(Size::fill())
			.child(MenuBar::new(model, move |a: String| last.set(a)))
			.child(label().text(format!("last={}", last.read())))
	}
	let key = |test: &mut TestingRunner, k: NamedKey| {
		test.press_key(Key::Named(k));
		test.sync_and_update();
	};
	let mut test = launch_test(app);
	test.sync_and_update();
	test.click_cursor((15., 10.));
	test.sync_and_update();
	key(&mut test, NamedKey::ArrowDown);
	key(&mut test, NamedKey::ArrowDown);
	key(&mut test, NamedKey::Enter);
	assert!(find_text(&test, "last=file.save"));

	test.click_cursor((15., 10.));
	test.sync_and_update();
	key(&mut test, NamedKey::ArrowUp);
	key(&mut test, NamedKey::ArrowRight);
	key(&mut test, NamedKey::Enter);
	assert!(find_text(&test, "last=file.r1"));

	test.click_cursor((15., 10.));
	test.sync_and_update();
	key(&mut test, NamedKey::ArrowRight);
	key(&mut test, NamedKey::ArrowDown);
	key(&mut test, NamedKey::Enter);
	assert!(find_text(&test, "last=edit.x"));
}

fn text_app_body(select_all: bool) -> Element {
	{
		wavedude_widgets::theme::use_init_wave_theme(wavedude_widgets::theme::Theme::dark);
		let mut value = use_state(|| "Source 1".to_string());
		let committed = value.read().clone();
		rect()
			.child(
				TextBox::new(committed.clone(), move |t: String| value.set(t)).select_all_on_focus(select_all).live(),
			)
			.child(label().text(format!("value={committed}")))
			.into()
	}
}

#[test]
fn typing_into_a_focused_text_box_inserts_at_the_caret() {
	let mut test = launch_test(|| text_app_body(false));
	test.sync_and_update();
	test.click_cursor((200., 10.));
	test.write_text("!");
	test.sync_and_update();
	assert!(find_text(&test, "value=Source 1!"));
}

#[test]
fn focusing_selects_the_whole_text_so_typing_replaces_it() {
	let mut test = launch_test(|| text_app_body(true));
	test.sync_and_update();
	test.click_cursor((20., 10.));
	test.sync_and_update();
	test.sync_and_update();
	test.write_text("Lamp");
	test.sync_and_update();
	test.sync_and_update();
	assert!(find_text(&test, "value=Lamp"));
	test.click_cursor((10., 10.));
	test.sync_and_update();
	test.sync_and_update();
	test.write_text("x");
	test.sync_and_update();
	let shown =
		test.find(|_, e| Label::try_downcast(e).filter(|l| l.text.starts_with("value")).map(|l| l.text.to_string()));
	assert!(shown.is_some_and(|t| t.contains("amp") && t.contains('x')), "a later click places the caret");
}

#[test]
fn escape_puts_the_old_text_back_in_a_text_box() {
	fn app() -> impl IntoElement {
		wavedude_widgets::theme::use_init_wave_theme(wavedude_widgets::theme::Theme::dark);
		let mut value = use_state(|| "Source 1".to_string());
		let committed = value.read().clone();
		rect()
			.child(TextBox::new(committed.clone(), move |t: String| value.set(t)))
			.child(label().text(format!("value={committed}")))
	}
	let mut test = launch_test(app);
	test.sync_and_update();
	test.click_cursor((200., 10.));
	test.write_text("abc");
	test.sync_and_update();
	test.press_key(Key::Named(NamedKey::Escape));
	test.sync_and_update();
	test.press_key(Key::Named(NamedKey::Enter));
	test.sync_and_update();
	assert!(find_text(&test, "value=Source 1"));
}
