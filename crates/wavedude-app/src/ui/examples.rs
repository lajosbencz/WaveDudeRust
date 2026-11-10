//! The Examples dialog (docs/ui/70): the example projects in tabs by topic, a search over titles and
//! descriptions, a scrolled list; Open runs the same guarded open as any project.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use freya::prelude::*;
use wavedude_app_state::examples::{self, Example, Topic};
use wavedude_app_state::{Event as AppEvent, Guarded};
use wavedude_widgets::status_bar::is_double_click;
use wavedude_widgets::{Button, TestIdExt, TextBox, use_wave_theme};

use super::app::Cx;
use super::dialogs::{button, button_row, frame};
use super::properties::tagged;

const LIST_HEIGHT: f32 = 360.;

/// The indices into [`examples::all`] of the examples of `topic` that match `query`, in list order.
fn visible(topic: Topic, query: &str) -> Vec<usize> {
	let query = query.trim().to_lowercase();
	examples::all()
		.iter()
		.enumerate()
		.filter(|(_, e)| {
			e.topic == topic
				&& (query.is_empty()
					|| e.title.to_lowercase().contains(&query)
					|| e.description.to_lowercase().contains(&query))
		})
		.map(|(i, _)| i)
		.collect()
}

/// The tab to show for `query`: `current` while it has matches, else the first topic that has some.
fn tab_for(current: usize, query: &str) -> usize {
	let has = |t: usize| !visible(Topic::ALL[t], query).is_empty();
	if has(current) { current } else { (0..Topic::ALL.len()).find(|&t| has(t)).unwrap_or(current) }
}

/// Row heights by list position, for scrolling the selection into view.
type Heights = Rc<RefCell<Vec<f32>>>;

#[derive(PartialEq)]
pub struct ExamplesDialog;

impl Component for ExamplesDialog {
	fn render(&self) -> impl IntoElement {
		let cx = Cx::get();
		let theme = use_wave_theme();
		let s = cx.strings();
		let ui = cx.ui;
		let mut tab = use_state(|| 0usize);
		let mut query = use_state(String::new);
		let mut chosen = use_state(|| None::<usize>);
		let mut scroll = use_scroll_controller(ScrollConfig::default);
		let mut view_height = use_state(|| LIST_HEIGHT);
		let heights: Heights = use_hook(|| Rc::new(RefCell::new(Vec::new())));
		let search_id = use_hook(AccessibilityId::new_unique);
		let mut last_press = use_state(|| None::<(usize, Instant)>);

		let topic = Topic::ALL[*tab.read()];
		let rows = visible(topic, &query.read());
		let selected = chosen.read().filter(|i| rows.contains(i)).or_else(|| rows.first().copied());

		let open = {
			let cx = cx.clone();
			move |index: usize| {
				cx.ui.close_modal();
				cx.send(AppEvent::Guarded(Guarded::OpenExample(index)));
			}
		};
		let show = {
			let heights = heights.clone();
			move |position: usize| {
				let heights = heights.borrow();
				let top: f32 = heights.iter().take(position).sum();
				let bottom = top + heights.get(position).copied().unwrap_or(0.);
				let (offset, view) = (scroll.position().y.abs(), *view_height.peek());
				if top < offset {
					scroll.scroll_to_y(-(top as i32));
				} else if bottom > offset + view {
					scroll.scroll_to_y(-((bottom - view).ceil() as i32));
				}
			}
		};
		let on_key = {
			let (rows, open, mut show) = (rows.clone(), open.clone(), show.clone());
			move |e: Event<KeyboardEventData>| {
				let at = selected.and_then(|i| rows.iter().position(|&r| r == i)).unwrap_or(0);
				let beside = |delta: isize| (*tab.peek()).saturating_add_signed(delta).min(Topic::ALL.len() - 1);
				let walk = |delta: isize| at.saturating_add_signed(delta).min(rows.len().saturating_sub(1));
				match e.key {
					Key::Named(NamedKey::Escape) => ui.close_modal(),
					Key::Named(NamedKey::Enter) => match selected {
						Some(i) => open(i),
						None => return,
					},
					Key::Named(key @ (NamedKey::ArrowDown | NamedKey::ArrowUp)) => {
						let to = walk(if key == NamedKey::ArrowDown { 1 } else { -1 });
						if let Some(&i) = rows.get(to) {
							chosen.set(Some(i));
							show(to);
						}
					}
					Key::Named(key @ (NamedKey::ArrowLeft | NamedKey::ArrowRight)) if !search_id.is_focused() => {
						tab.set(beside(if key == NamedKey::ArrowRight { 1 } else { -1 }));
						chosen.set(None);
						scroll.scroll_to_y(0);
					}
					_ => return,
				}
				e.stop_propagation();
			}
		};

		let tabs = Topic::ALL.iter().enumerate().map(|(n, &t)| {
			let active = n == *tab.read();
			let has = !visible(t, &query.read()).is_empty();
			let ink = if active {
				theme.tab_active_text
			} else if has {
				theme.tab_text
			} else {
				theme.text_disabled
			};
			rect()
				.test_id(format!("ExamplesTab:{n}"))
				.border(Border::new().fill(if active { theme.accent } else { Color::TRANSPARENT }).width(BorderWidth {
					top: 0.,
					right: 0.,
					bottom: 2.,
					left: 0.,
				}))
				.child(
					Button::new()
						.flat()
						.on_press(move |_| {
							tab.set(n);
							chosen.set(None);
							scroll.scroll_to_y(0);
						})
						.child(label().text(s.get(t.title_key()).to_owned()).color(ink).max_lines(1)),
				)
				.into_element()
		});

		let list = rows.iter().enumerate().map(|(position, &i)| {
			let example: &Example = &examples::all()[i];
			let on = selected == Some(i);
			let (title_ink, detail_ink) =
				if on { (theme.selection_text, theme.selection_text) } else { (theme.text, theme.text_dim) };
			let (heights, open) = (heights.clone(), open.clone());
			rect()
				.test_id(example.test_id())
				.width(Size::fill())
				.padding((4., 8., 6., 8.))
				.background(if on { theme.selection } else { Color::TRANSPARENT })
				.on_sized(move |e: Event<SizedEventData>| {
					let mut heights = heights.borrow_mut();
					if heights.len() <= position {
						heights.resize(position + 1, 0.);
					}
					heights[position] = e.area.height();
				})
				.on_pointer_down(move |e: Event<PointerEventData>| {
					if e.button() != Some(MouseButton::Left) {
						return;
					}
					let now = Instant::now();
					let previous = last_press.peek().filter(|(at, _)| *at == i).map(|(_, t)| t);
					chosen.set(Some(i));
					if is_double_click(previous, now) {
						last_press.set(None);
						open(i);
					} else {
						last_press.set(Some((i, now)));
					}
				})
				.child(label().text(example.title.clone()).color(title_ink).font_weight(FontWeight::BOLD))
				.child(paragraph().width(Size::fill()).color(detail_ink).span(Span::new(example.description.clone())))
				.into_element()
		});
		let list: Vec<Element> = list.collect();
		let empty = list.is_empty();

		let on_open = {
			let open = open.clone();
			move |_: Event<PressEventData>| {
				if let Some(i) = selected {
					open(i);
				}
			}
		};
		frame(720.)
			.test_id("ExamplesDialog")
			.on_global_key_down(on_key)
			.child(
				label()
					.text(s.get("ExamplesTitle").to_owned())
					.font_weight(FontWeight::BOLD)
					.font_size(theme.font_size * 1.2),
			)
			.child(rect().width(Size::fill()).horizontal().children(tabs))
			.child(tagged(
				TextBox::new(query.read().clone(), move |text: String| {
					let at = tab_for(*tab.peek(), &text);
					if at != *tab.peek() {
						tab.set(at);
					}
					chosen.set(None);
					scroll.scroll_to_y(0);
					query.set(text);
				})
				.live()
				.id(Some(search_id)),
				"ExamplesSearch",
			))
			.child(
				rect()
					.width(Size::fill())
					.height(Size::px(LIST_HEIGHT))
					.border(Border::new().fill(theme.input_border).width(1.).alignment(BorderAlignment::Inner))
					.on_sized(move |e: Event<SizedEventData>| view_height.set_if_modified(e.area.height()))
					.child(
						ScrollView::new_controlled(scroll)
							.width(Size::fill())
							.height(Size::fill())
							.child(rect().width(Size::fill()).children(list))
							.maybe(empty, |v| {
								v.child(label().text(s.get("ExamplesNone").to_owned()).color(theme.text_dim).margin(8.))
							}),
					),
			)
			.child(button_row(vec![
				Button::new()
					.enabled(selected.is_some())
					.on_press(on_open)
					.child(label().text(wavedude_app_state::strings::strip_mnemonic(s.get("ExamplesOpen"))))
					.into_element(),
				button(s.get("DialogCancel"), true, move |_| ui.close_modal()),
			]))
			.into_element()
	}
}
