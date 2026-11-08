//! The manual window (docs/ui/70): a contents tree of the headings beside the whole document rendered as
//! Markdown with the figures of `assets/manual`; taking a row scrolls its heading to the top, a 4 px splitter
//! between them. One window at a time; F1 opens it or brings it forward, Esc closes it. It follows the main
//! window's theme, language and zoom while open.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use freya::prelude::*;
use freya::winit::window::WindowId;
use wavedude_widgets::dock::Splitter;
use wavedude_widgets::{TestIdExt, Theme, use_init_wave_theme, use_wave_theme};

use super::about::markdown;
use super::app::{Cx, theme_kind};
use wavedude_app_state::strings::Strings;

const MANUAL: &str = include_str!("../../assets/manual.md");
const CONTENTS_WIDTH: f32 = 260.;
const CONTENTS_MIN: f32 = 120.;
const TEXT_MIN: f32 = 200.;

/// The figures the manual shows, by the name of its `<figure name="..."/>` tags.
pub mod figures {
	macro_rules! figures {
		($($name:literal),* $(,)?) => {
			/// Every figure: name and PNG bytes.
			pub const ALL: &[(&str, &[u8])] = &[$(($name, include_bytes!(concat!("../../assets/manual/", $name, ".png")))),*];
		};
	}
	figures!(
		"main-window",
		"project-tree",
		"preview",
		"timeline",
		"menu-file",
		"menu-file-export",
		"menu-edit",
		"menu-view",
		"menu-simulation",
		"menu-help",
		"menu-add-part",
		"properties-project",
		"properties-scene",
		"properties-simulation",
		"properties-display",
		"properties-source",
		"properties-composite-source",
		"properties-lens",
		"properties-polygon",
		"properties-rectangle",
		"properties-ellipse",
		"properties-triangle",
		"properties-outline",
		"properties-fibre",
		"properties-bragg-reflector",
		"properties-grin-lens",
		"properties-zone-plate",
		"properties-xor-gate",
		"properties-probe",
		"properties-line-probe",
		"properties-colour-scheme",
	);

	pub fn get(name: &str) -> Option<&'static [u8]> {
		ALL.iter().find(|(n, _)| *n == name).map(|(_, bytes)| *bytes)
	}
}

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

/// The figure names a `<figure name="..."/>` tag names in `html`.
pub fn figure_name(html: &str) -> Option<&str> {
	let rest = html.trim().strip_prefix("<figure")?;
	let start = rest.find("name=\"")? + 6;
	let end = rest[start..].find('"')? + start;
	Some(&rest[start..end])
}

/// GitHub's anchor of a heading: lower case, spaces as hyphens, punctuation dropped.
pub fn anchor(title: &str) -> String {
	title
		.chars()
		.filter_map(|c| match c {
			' ' | '-' => Some('-'),
			c if c.is_alphanumeric() => Some(c.to_ascii_lowercase()),
			_ => None,
		})
		.collect()
}

#[derive(Clone)]
struct ManualContext {
	theme: Theme,
	strings: Strings,
}

fn manual_root() -> impl IntoElement {
	let ctx = consume_context::<ManualContext>();
	let theme = ctx.theme.clone();
	use_init_wave_theme(move || theme);
	ManualView { strings: ctx.strings }
}

#[derive(PartialEq)]
struct ManualView {
	strings: Strings,
}

impl Component for ManualView {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let s = self.strings;
		let all = use_hook(|| Rc::new(sections(MANUAL)));
		let mut taken = use_state(|| 0usize);
		let mut contents_width = use_state(|| CONTENTS_WIDTH);
		let mut window_width = use_state(|| 0f32);
		let tops: Rc<RefCell<HashMap<usize, f32>>> = use_hook(|| Rc::new(RefCell::new(HashMap::new())));
		let mut scroll = use_scroll_controller(ScrollConfig::default);
		let current = *taken.read();
		let count = all.len();

		let go = {
			let tops = tops.clone();
			move |i: usize| {
				taken.set(i);
				if let Some(&top) = tops.borrow().get(&i) {
					scroll.scroll_to_y(-(top as i32));
				}
			}
		};
		let rows = all.iter().enumerate().map(|(i, sec)| {
			let selected = i == current;
			let mut go = go.clone();
			rect()
				.width(Size::fill())
				.padding((3., 6., 3., 6. + 12. * (sec.level.saturating_sub(2)) as f32))
				.background(if selected { theme.selection } else { Color::TRANSPARENT })
				.color(if selected { theme.selection_text } else { theme.text })
				.test_id(format!("ManualRow:{}", anchor(&sec.title)))
				.on_press(move |_| go(i))
				.child(label().text(sec.title.clone()).max_lines(1))
				.into_element()
		});
		let figure = move |html: String| -> Option<Element> {
			let name = figure_name(&html)?;
			let bytes = figures::get(name)?;
			let key: &'static str = figures::ALL.iter().find(|(n, _)| *n == name).map(|(n, _)| *n)?;
			Some(
				rect()
					.width(Size::fill())
					.padding((6., 0., 6., 0.))
					.child(ImageViewer::new((key, bytes)).width(Size::fill()).aspect_ratio(AspectRatio::Fit))
					.into_element(),
			)
		};
		// Each section's top within the scrolled column: its area's top less the column's.
		let column_top = use_state(|| 0f32);
		let body = all.iter().enumerate().map(|(i, sec)| {
			let tops = tops.clone();
			rect()
				.width(Size::fill())
				.on_sized(move |e: Event<SizedEventData>| {
					tops.borrow_mut().insert(i, e.area.min_y() - *column_top.peek() + scroll.position().y);
				})
				.child(markdown(sec.body.clone(), &theme).inline_element(figure))
				.into_element()
		});
		let total = *window_width.read();
		let max_contents = (total - TEXT_MIN - 4.).max(CONTENTS_MIN);
		rect()
			.expanded()
			.horizontal()
			.content(Content::flex())
			.background(theme.window_background)
			.color(theme.text)
			.font_size(theme.font_size)
			.on_sized(move |e: Event<SizedEventData>| window_width.set_if_modified(e.area.width()))
			.on_global_key_down({
				let mut go = go.clone();
				move |e: Event<KeyboardEventData>| match e.key {
					Key::Named(NamedKey::Escape) => Platform::get().close_window(Platform::window_id()),
					Key::Named(NamedKey::ArrowDown) => go((current + 1).min(count.saturating_sub(1))),
					Key::Named(NamedKey::ArrowUp) => go(current.saturating_sub(1)),
					_ => {}
				}
			})
			.child(
				rect()
					.width(Size::px(contents_width.read().clamp(CONTENTS_MIN, max_contents)))
					.height(Size::fill())
					.background(theme.tool_window_background)
					.child(label().text(s.get("ManualContents")).font_weight(FontWeight::BOLD).margin(6.))
					.child(ScrollView::new().expanded().child(rect().width(Size::fill()).children(rows))),
			)
			.child(
				TooltipContainer::new(Tooltip::new_text(s.get("TipManualSplitter"))).child(
					Splitter::new(false, move |delta: f32| {
						let w = (*contents_width.peek() + delta).clamp(CONTENTS_MIN, max_contents);
						contents_width.set(w);
					})
					.test_id("ManualSplitter"),
				),
			)
			.child(
				ScrollView::new_controlled(scroll).width(Size::flex(1.)).height(Size::fill()).child(
					rect()
						.width(Size::fill())
						.padding(16.)
						.on_sized({
							let mut column_top = column_top;
							move |e: Event<SizedEventData>| {
								column_top.set_if_modified(e.area.min_y() + scroll.position().y);
							}
						})
						.children(body),
				),
			)
	}
}

/// Watches F1 requests and opens the manual window or brings it forward; a theme, language or zoom change
/// relaunches it in place.
#[derive(PartialEq)]
pub struct ManualLauncher;

impl Component for ManualLauncher {
	fn render(&self) -> impl IntoElement {
		let cx = Cx::get();
		let open: Rc<Cell<Option<WindowId>>> = use_hook(|| Rc::new(Cell::new(None)));
		let request = *cx.ui.manual_request.read();
		let seen = use_hook(|| Rc::new(Cell::new(0u64)));
		let look = cx.shared.read(super::shared::Topics::SETTINGS, |a| {
			(a.settings.theme, a.settings.language.clone(), a.settings.ui_zoom.to_bits())
		});
		let mut last_look = use_state(|| look.clone());
		if request != seen.get() {
			seen.set(request);
			match open.get() {
				Some(id) => Platform::get().focus_window(id),
				None => launch(&cx, open.clone()),
			}
		} else if *last_look.peek() != look {
			last_look.set(look);
			if let Some(id) = open.get() {
				Platform::get().close_window(id);
				open.set(None);
				launch(&cx, open.clone());
			}
		}
		rect()
	}
}

fn launch(cx: &Cx, open: Rc<Cell<Option<WindowId>>>) {
	let (s, setting, zoom) = cx.shared.peek(|a| (a.strings, a.settings.theme, a.settings.ui_zoom));
	let ctx = ManualContext { theme: Theme::of(theme_kind(setting)), strings: s };
	let title: &'static str = Box::leak(s.get("ManualTitle").to_owned().into_boxed_str());
	let closed = open.clone();
	let config = WindowConfig::new(manual_root)
		.with_root_context(ctx)
		.with_title(title)
		.with_size(1200. * zoom, 820. * zoom)
		.with_min_size(480. * zoom, 300. * zoom)
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
		assert_eq!(titles.first(), Some(&"The main window"));
		assert!(titles.contains(&"The examples"));
		assert!(all.iter().all(|s| s.body.starts_with('#')));
	}

	#[test]
	fn every_figure_tag_names_a_bundled_figure_and_every_anchor_a_heading() {
		let all = sections(MANUAL);
		let anchors: Vec<String> = all.iter().map(|s| anchor(&s.title)).collect();
		for line in MANUAL.lines() {
			let mut rest = line;
			while let Some(start) = rest.find("<figure") {
				let tag = &rest[start..];
				let name = figure_name(tag).expect("a figure tag names its figure");
				assert!(figures::get(name).is_some(), "no figure '{name}'");
				rest = &tag[1..];
			}
			let mut rest = line;
			while let Some(start) = rest.find("](#") {
				let target = &rest[start + 3..];
				let end = target.find(')').expect("a link closes");
				assert!(anchors.contains(&target[..end].to_owned()), "no heading for #{}", &target[..end]);
				rest = &target[end..];
			}
		}
	}

	#[test]
	fn anchors_follow_github() {
		assert_eq!(anchor("The File menu"), "the-file-menu");
		assert_eq!(anchor("Lens/Mirror"), "lensmirror");
		assert_eq!(anchor("XOR gate"), "xor-gate");
	}
}
