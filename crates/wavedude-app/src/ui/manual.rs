//! The manual window: a contents list of the headings beside the chapter taken, rendered as Markdown. One window
//! at a time; F1 opens it or brings it forward, Esc closes it.

use std::cell::Cell;
use std::rc::Rc;

use freya::prelude::*;
use freya::winit::window::WindowId;
use wavedude_widgets::{Theme, use_init_wave_theme, use_wave_theme};

use super::about::markdown;
use super::app::{Cx, theme_kind};

const MANUAL: &str = include_str!("../../assets/manual.md");

/// One heading and the text up to the next heading.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
	pub level: usize,
	pub title: String,
	pub body: String,
}

/// The manual split at its headings; the title (`#`) is no row.
pub fn sections(text: &str) -> Vec<Section> {
	let mut out: Vec<Section> = Vec::new();
	for line in text.lines() {
		let level = line.chars().take_while(|&c| c == '#').count();
		if level > 0 && line[level..].starts_with(' ') {
			out.push(Section { level, title: line[level..].trim().to_owned(), body: format!("{line}\n") });
		} else if let Some(last) = out.last_mut() {
			last.body.push_str(line);
			last.body.push('\n');
		}
	}
	out.retain(|s| s.level > 1 || !s.body.lines().skip(1).all(str::is_empty));
	out
}

#[derive(Clone)]
struct ManualContext {
	theme: Theme,
	contents: String,
}

fn manual_root() -> impl IntoElement {
	let ctx = consume_context::<ManualContext>();
	let theme = ctx.theme.clone();
	use_init_wave_theme(move || theme);
	ManualView { contents: ctx.contents }
}

#[derive(PartialEq)]
struct ManualView {
	contents: String,
}

impl Component for ManualView {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let all = use_hook(|| Rc::new(sections(MANUAL)));
		let mut taken = use_state(|| 0usize);
		let current = *taken.read();
		let rows = all.iter().enumerate().map(|(i, s)| {
			let selected = i == current;
			rect()
				.width(Size::fill())
				.padding((3., 6., 3., 6. + 12. * (s.level.saturating_sub(2)) as f32))
				.background(if selected { theme.selection } else { Color::TRANSPARENT })
				.color(if selected { theme.selection_text } else { theme.text })
				.on_press(move |_| taken.set(i))
				.child(label().text(s.title.clone()).max_lines(1))
				.into_element()
		});
		let body = all.get(current).map(|s| s.body.clone()).unwrap_or_default();
		let count = all.len();
		rect()
			.expanded()
			.horizontal()
			.content(Content::flex())
			.background(theme.window_background)
			.color(theme.text)
			.font_size(theme.font_size)
			.on_global_key_down(move |e: Event<KeyboardEventData>| match e.key {
				Key::Named(NamedKey::Escape) => Platform::get().close_window(Platform::window_id()),
				Key::Named(NamedKey::ArrowDown) => taken.set((current + 1).min(count.saturating_sub(1))),
				Key::Named(NamedKey::ArrowUp) => taken.set(current.saturating_sub(1)),
				_ => {}
			})
			.child(
				rect()
					.width(Size::px(240.))
					.height(Size::fill())
					.background(theme.tool_window_background)
					.child(label().text(self.contents.clone()).font_weight(FontWeight::BOLD).margin(6.))
					.child(ScrollView::new().expanded().child(rect().width(Size::fill()).children(rows))),
			)
			.child(rect().width(Size::px(1.)).height(Size::fill()).background(theme.border))
			.child(
				ScrollView::new()
					.width(Size::flex(1.))
					.height(Size::fill())
					.child(rect().width(Size::fill()).padding(16.).child(markdown(body, &theme))),
			)
	}
}

/// Watches F1 requests and opens the manual window or brings it forward.
#[derive(PartialEq)]
pub struct ManualLauncher;

impl Component for ManualLauncher {
	fn render(&self) -> impl IntoElement {
		let cx = Cx::get();
		let open: Rc<Cell<Option<WindowId>>> = use_hook(|| Rc::new(Cell::new(None)));
		let request = *cx.ui.manual_request.read();
		let seen = use_hook(|| Rc::new(Cell::new(0u64)));
		if request != seen.get() {
			seen.set(request);
			match open.get() {
				Some(id) => Platform::get().focus_window(id),
				None => launch(&cx, open.clone()),
			}
		}
		rect()
	}
}

fn launch(cx: &Cx, open: Rc<Cell<Option<WindowId>>>) {
	let (s, setting, zoom) = cx.shared.peek(|a| (a.strings, a.settings.theme, a.settings.ui_zoom));
	let ctx = ManualContext { theme: Theme::of(theme_kind(setting)), contents: s.get("ManualContents").to_owned() };
	let title: &'static str = Box::leak(s.get("ManualTitle").to_owned().into_boxed_str());
	let closed = open.clone();
	let config = WindowConfig::new(manual_root)
		.with_root_context(ctx)
		.with_title(title)
		.with_size(900. * zoom, 700. * zoom)
		.with_custom_scale_factor(zoom)
		.with_background(Theme::of(theme_kind(setting)).window_background)
		.with_on_close(move |_, _| {
			closed.set(None);
			CloseDecision::Close
		});
	spawn_in_window(async move {
		let id = Platform::get().launch_window(config).await;
		open.set(Some(id));
	});
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn manual_splits_at_its_chapters() {
		let all = sections(MANUAL);
		let titles: Vec<&str> = all.iter().filter(|s| s.level == 2).map(|s| s.title.as_str()).collect();
		assert_eq!(titles.first(), Some(&"The main window and its panes"));
		assert!(titles.contains(&"Examples"));
		assert!(all.iter().all(|s| s.body.starts_with('#')));
	}
}
