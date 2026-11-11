//! Helpers for the headless Freya tests (`freya_testing`), shared by the widget and the application tests. A click
//! goes through [`click`], which asserts the target is on screen first: a click at a point nothing is drawn at
//! would leave a "nothing changed" assertion true for the wrong reason.

use freya::prelude::*;
use freya_testing::TestingNode;
use freya_testing::prelude::*;

/// The node showing exactly `text`: a label, or a paragraph whose spans join to it.
pub fn find_text(test: &TestingRunner, text: &str) -> Option<TestingNode> {
	test.find(|node, element| {
		let label = Label::try_downcast(element).is_some_and(|l| l.text.as_ref() == text);
		let spans = Paragraph::try_downcast(element)
			.is_some_and(|p| p.spans.iter().map(|s| s.text.as_ref()).collect::<String>() == text);
		(label || spans).then_some(node)
	})
}

/// Every label showing exactly `text`, by area; a combo box lists each word once more in its hidden measuring copy.
pub fn label_areas(test: &TestingRunner, text: &str) -> Vec<Area> {
	test.find_many(|node, element| {
		Label::try_downcast(element).is_some_and(|l| l.text.as_ref() == text).then(|| node.layout().area)
	})
}

/// The rect tagged `id` (`test_id`).
pub fn tagged(test: &TestingRunner, id: &str) -> Option<TestingNode> {
	test.find(|node, element| {
		Rect::try_downcast(element).filter(|r| r.accessibility.builder.author_id() == Some(id))?;
		Some(node)
	})
}

pub fn tagged_area(test: &TestingRunner, id: &str) -> Option<Area> {
	tagged(test, id).map(|n| n.layout().area)
}

/// How many rects carry a test id starting with `prefix`.
pub fn count_tagged(test: &TestingRunner, prefix: &str) -> usize {
	test.find_many(|_, element| {
		Rect::try_downcast(element)
			.filter(|r| r.accessibility.builder.author_id().is_some_and(|id| id.starts_with(prefix)))?;
		Some(())
	})
	.len()
}

/// The window: the root node's area.
pub fn window(test: &TestingRunner) -> Area {
	test.find(|node, _| Some(node.layout().area)).expect("a root node")
}

/// A frame's worth of time and an update, for effects and focus moves to land.
pub fn settle(test: &mut TestingRunner) {
	test.poll(std::time::Duration::from_millis(16), std::time::Duration::from_millis(16));
	test.sync_and_update();
}

/// Asserts `node` (`what`, for the message) lies inside the window with a size, so a click at its centre can
/// reach it. (A clipping ancestor is not checked: the runner's `is_visible` has no effect state for menu entries.)
pub fn assert_on_screen(test: &TestingRunner, node: &TestingNode, what: &str) {
	let area = node.layout().area;
	let window = window(test);
	assert!(
		area.width() > 0. && area.height() > 0. && window.contains(area.center()),
		"{what} lies outside the {window:?} window: {area:?}"
	);
}

/// Clicks the centre of `node` after [`assert_on_screen`].
pub fn click(test: &mut TestingRunner, node: &TestingNode, what: &str) {
	assert_on_screen(test, node, what);
	let c = node.layout().area.center();
	test.click_cursor((f64::from(c.x), f64::from(c.y)));
	test.sync_and_update();
}

/// Clicks `at` (window coordinates) inside `node` after [`assert_on_screen`]; for a check box's mark left of its label.
pub fn click_at(test: &mut TestingRunner, node: &TestingNode, at: (f64, f64), what: &str) {
	assert_on_screen(test, node, what);
	let area = node.layout().area;
	assert!(area.contains((at.0 as f32, at.1 as f32).into()), "{what}: {at:?} is not inside {area:?}");
	test.click_cursor(at);
	test.sync_and_update();
}

/// Clicks the element showing `text`.
pub fn click_text(test: &mut TestingRunner, text: &str) -> TestingNode {
	let node = find_text(test, text).unwrap_or_else(|| panic!("nothing shows '{text}'"));
	click(test, &node, text);
	node
}

/// Clicks the element tagged `id`.
pub fn click_tagged(test: &mut TestingRunner, id: &str) -> TestingNode {
	let node = tagged(test, id).unwrap_or_else(|| panic!("nothing is tagged '{id}'"));
	click(test, &node, id);
	node
}
