//! The Examples dialog (docs/ui/70): the example projects in tabs by topic, a search over titles and descriptions
//! that dims the tabs without a match, a scrolled list; Open runs the guarded open of any project.

use eframe::egui::{
	self, FontId, Key, Modifiers, RichText, Role, ScrollArea, Sense, Stroke, TextEdit, Ui, WidgetInfo, vec2,
};
use wavedude_app_state::{Event, Guarded};
use wavedude_examples::Topic;

use super::modal::{Choice, Reply, Then, buttons};
use super::theme::{FONT_SIZE, Palette};
use crate::App;

const WIDTH: f32 = 860.;
const LIST_HEIGHT: f32 = 360.;
const TAB_HEIGHT: f32 = 26.;

/// The indices into [`wavedude_examples::all`] of the examples of `topic` that match `query`, in list order.
fn matching(topic: Topic, query: &str) -> Vec<usize> {
	let query = query.trim().to_lowercase();
	let all = wavedude_examples::all().iter().enumerate();
	all.filter(|(_, e)| {
		e.topic == topic
			&& (query.is_empty()
				|| e.title.to_lowercase().contains(&query)
				|| e.description.to_lowercase().contains(&query))
	})
	.map(|(i, _)| i)
	.collect()
}

/// The dialog's state.
pub(super) struct Examples {
	tab: usize,
	query: String,
	/// Each topic's matches for `query`.
	matches: [Vec<usize>; Topic::ALL.len()],
	chosen: Option<usize>,
	/// The chosen row is to be scrolled into view.
	reveal: bool,
}

impl Default for Examples {
	fn default() -> Self {
		Self { tab: 0, query: String::new(), matches: Topic::ALL.map(|t| matching(t, "")), chosen: None, reveal: false }
	}
}

impl Examples {
	fn search(&mut self) {
		self.matches = Topic::ALL.map(|t| matching(t, &self.query));
		if self.matches[self.tab].is_empty()
			&& let Some(t) = self.matches.iter().position(|m| !m.is_empty())
		{
			self.tab = t;
		}
		self.chosen = None;
	}

	fn show_tab(&mut self, tab: usize) {
		self.tab = tab;
		self.chosen = None;
		self.reveal = true;
	}

	pub(super) fn ui(&mut self, ui: &mut Ui, fresh: bool, app: &App) -> Option<Reply> {
		let s = &app.strings;
		let palette = Palette::of(ui.ctx().theme());
		ui.set_width(WIDTH);
		ui.label(RichText::new(s.get("ExamplesTitle")).size(FONT_SIZE * 1.2).family(wavedude_view::fonts::bold()));
		ui.horizontal(|ui| {
			ui.spacing_mut().item_spacing.x = 0.;
			for (n, topic) in Topic::ALL.iter().enumerate() {
				let active = n == self.tab;
				let ink = match (active, self.matches[n].is_empty()) {
					(true, _) => palette.accent,
					(false, false) => palette.text,
					(false, true) => palette.text_disabled,
				};
				let text = s.get(topic.title_key());
				let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(FONT_SIZE), ink);
				let size = vec2(galley.size().x + 16., TAB_HEIGHT);
				let (rect, response) = ui.allocate_exact_size(size, Sense::click());
				response.widget_info(|| WidgetInfo::selected(Role::Tab, true, active, text));
				if response.hovered() {
					ui.painter().rect_filled(rect, 0., palette.hover_background);
				}
				ui.painter().galley(rect.center() - galley.size() / 2., galley, ink);
				if active {
					let line = [rect.left_bottom() + vec2(0., -1.), rect.right_bottom() + vec2(0., -1.)];
					ui.painter().line_segment(line, Stroke::new(2., palette.accent));
				}
				if response.clicked() {
					self.show_tab(n);
				}
			}
		});
		let search =
			TextEdit::singleline(&mut self.query).id_salt("ExamplesSearch").hint_text(s.get("ExamplesSearchHint"));
		let search = ui.add(search.desired_width(f32::INFINITY));
		if search.changed() {
			self.search();
		}
		let rows = &self.matches[self.tab];
		let selected = self.chosen.filter(|i| rows.contains(i)).or_else(|| rows.first().copied());
		let at = selected.and_then(|i| rows.iter().position(|&r| r == i));
		let typing = search.has_focus();
		let (mut step, mut beside) = (0isize, 0isize);
		ui.input_mut(|i| {
			step = isize::from(i.consume_key(Modifiers::NONE, Key::ArrowDown))
				- isize::from(i.consume_key(Modifiers::NONE, Key::ArrowUp));
			if !typing {
				beside = isize::from(i.consume_key(Modifiers::NONE, Key::ArrowRight))
					- isize::from(i.consume_key(Modifiers::NONE, Key::ArrowLeft));
			}
		});
		let mut open = None;
		if step != 0
			&& let Some(at) = at
		{
			self.chosen = rows.get(at.saturating_add_signed(step).min(rows.len() - 1)).copied();
			self.reveal = true;
		}
		let selected = self.chosen.filter(|i| rows.contains(i)).or(selected);
		let frame = egui::Frame::new().stroke(Stroke::new(1., palette.border));
		frame.show(ui, |ui| {
			let mut area = ScrollArea::vertical().id_salt(("ExamplesList", self.tab)).auto_shrink(false);
			area = area.max_height(LIST_HEIGHT).min_scrolled_height(LIST_HEIGHT);
			area.show(ui, |ui| {
				ui.spacing_mut().item_spacing.y = 0.;
				if rows.is_empty() {
					ui.add_space(8.);
					ui.label(RichText::new(s.get("ExamplesNone")).color(palette.text_dim));
				}
				for &i in rows {
					let example = &wavedude_examples::all()[i];
					let on = selected == Some(i);
					let (title, detail) = if on {
						(palette.selection_text, palette.selection_text)
					} else {
						(palette.text, palette.text_dim)
					};
					let fill = if on { palette.selection } else { egui::Color32::TRANSPARENT };
					let shown = egui::Frame::new().fill(fill).inner_margin(egui::Margin {
						left: 8,
						right: 8,
						top: 4,
						bottom: 6,
					});
					let row = shown.show(ui, |ui| {
						ui.set_width(ui.available_width());
						ui.label(RichText::new(&example.title).color(title).family(wavedude_view::fonts::bold()));
						ui.add(egui::Label::new(RichText::new(&example.description).color(detail)).wrap());
					});
					let response = ui.interact(row.response.rect, ui.scope_id().with(("Example", i)), Sense::click());
					response.widget_info(|| WidgetInfo::selected(Role::ListItem, true, on, &example.title));
					if on && std::mem::take(&mut self.reveal) {
						response.scroll_to_me(None);
					}
					if response.clicked() {
						self.chosen = Some(i);
					}
					if response.double_clicked() {
						open = Some(i);
					}
				}
			});
		});
		if beside != 0 {
			self.show_tab(self.tab.saturating_add_signed(beside).min(Topic::ALL.len() - 1));
		}
		let choices = [
			Choice { enabled: selected.is_some(), ..Choice::default(s.get("ExamplesOpen")) },
			Choice::cancel(s.get("DialogCancel")),
		];
		let open = match buttons(ui, fresh, &choices) {
			Some(0) => selected,
			Some(_) => return Some(None),
			None => open,
		};
		open.map(|i| Some(Then::Send(Event::Guarded(Guarded::OpenExample(i)))))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_search_moves_to_the_first_tab_with_a_match() {
		let mut examples = Examples::default();
		let example = wavedude_examples::all().iter().find(|e| e.topic != Topic::ALL[0]).unwrap();
		examples.query = example.title.clone();
		examples.search();
		assert_eq!(Topic::ALL[examples.tab], example.topic);
	}
}
