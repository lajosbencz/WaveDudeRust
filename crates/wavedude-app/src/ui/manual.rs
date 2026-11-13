//! The manual window (docs/ui/70): a contents list of the headings beside the manual rendered as Markdown with its
//! figures; taking a row or following a `#anchor` link puts its heading at the top. A window of its own where the
//! platform has several, a window inside the page in the browser; F1 brings it forward, Esc closes it.
//! Sections out of sight are not laid out once their heights are known at the column's width, which they are
//! measured again for once it stops changing.

use eframe::egui::{
	self, Color32, CursorIcon, EventFilter, FontId, Key, LayerId, Modifiers, Order, Rect, RichText, ScrollArea, Sense,
	TextStyle, Ui, UiBuilder, ViewportBuilder, ViewportCommand, ViewportId, vec2,
};
use egui_commonmark::CommonMarkCache;

use super::markdown::{self, Piece, pieces};
use super::theme::{FONT_SIZE, Palette};
use crate::App;
use crate::manual::{MANUAL, anchor, sections};

const SIZE: [f32; 2] = [1200., 820.];
const MIN_SIZE: [f32; 2] = [480., 300.];
const CONTENTS_WIDTH: f32 = 260.;
const CONTENTS_MIN: f32 = 120.;
const TEXT_MIN: f32 = 200.;
const TEXT_PADDING: f32 = 16.;
const SPLITTER: f32 = 4.;
const SPLITTER_KEY_STEP: f32 = 10.;
const ROW_INDENT: f32 = 12.;
/// Seconds the column's width stays put before the sections are measured again.
const SETTLE: f64 = 0.25;

/// A heading and its text.
struct Section {
	level: usize,
	title: String,
	pieces: Vec<Piece>,
}

/// The window's state while it is open.
pub(super) struct Manual {
	/// The title's section first, which is no row.
	sections: Vec<Section>,
	/// Each section's `#anchor` link.
	links: Vec<String>,
	taken: usize,
	contents_width: f32,
	/// A section to bring to the top once its place is known.
	go: Option<usize>,
	/// Each section's top and height in the text column, at `width`.
	places: Vec<(f32, f32)>,
	width: f32,
	/// When the column's width last changed, while `places` waits to be measured again.
	resized: Option<f64>,
	markdown: CommonMarkCache,
	/// F1 asked for it again: it comes forward.
	raise: bool,
}

impl Default for Manual {
	fn default() -> Self {
		let sections: Vec<Section> = sections(MANUAL)
			.into_iter()
			.map(|s| Section { level: s.level, title: s.title, pieces: pieces(&s.body) })
			.collect();
		let links: Vec<String> = sections.iter().map(|s| format!("#{}", anchor(&s.title))).collect();
		let mut markdown = CommonMarkCache::default();
		let taken = sections.iter().position(|s| s.level > 1).unwrap_or_default();
		for link in &links {
			markdown.add_link_hook(link.as_str());
		}
		Self {
			sections,
			links,
			taken,
			contents_width: CONTENTS_WIDTH,
			go: None,
			places: Vec::new(),
			width: 0.,
			resized: None,
			markdown,
			raise: false,
		}
	}
}

impl Manual {
	/// Brings the open window forward.
	pub(super) fn raise(&mut self) {
		self.raise = true;
	}

	/// Shows the window; false once it was closed.
	pub(super) fn show(&mut self, ctx: &egui::Context, app: &App) -> bool {
		let title = app.strings.get("ManualTitle");
		let mut open = true;
		if ctx.embed_viewports() {
			let id = egui::Id::unique("ManualWindow");
			// keys go to the window with the focus, or on top with nothing focused
			let layer = LayerId::new(Order::Middle, id);
			let here = match ctx.memory(egui::Memory::focused).and_then(|f| ctx.read_response(f)) {
				Some(focused) => focused.layer_id == layer,
				None => ctx.top_layer_id() == Some(layer),
			};
			let window = egui::Window::new(title)
				.id(id)
				.open(&mut open)
				.default_size(SIZE)
				.min_size(MIN_SIZE)
				.collapsible(false);
			if std::mem::take(&mut self.raise) {
				ctx.move_to_top(layer);
			}
			window.show(ctx, |ui| self.body(ui, app, here));
			return open && !(here && escaped(ctx));
		}
		let id = ViewportId::from_hash_of("manual");
		let builder = ViewportBuilder::default().with_title(title).with_inner_size(SIZE).with_min_inner_size(MIN_SIZE);
		if std::mem::take(&mut self.raise) {
			ctx.send_viewport_cmd_to(id, ViewportCommand::Focus);
		}
		ctx.show_viewport_immediate(id, builder, |ui, _| {
			egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| self.body(ui, app, true));
			let ctx = ui.ctx();
			!ctx.input(|i| i.viewport().close_requested()) && !escaped(ctx)
		})
	}

	/// The contents beside the text, a splitter between them; `keys`: the arrows move through the contents.
	fn body(&mut self, ui: &mut Ui, app: &App, keys: bool) {
		let palette = Palette::of(ui.ctx().theme());
		let room = ui.available_rect_before_wrap();
		let max = (room.width() - TEXT_MIN - SPLITTER).max(CONTENTS_MIN);
		self.contents_width = self.contents_width.clamp(CONTENTS_MIN, max).round();
		let (contents, rest) = room.split_left_right_at_x(room.left() + self.contents_width);
		let (bar, text) = rest.split_left_right_at_x(rest.left() + SPLITTER);
		ui.painter().rect_filled(contents, 0., palette.tool_window_background);
		ui.scope_builder(UiBuilder::new().max_rect(contents.shrink2(vec2(0., 6.))), |ui| {
			ui.label(RichText::new(app.strings.get("ManualContents")).family(wavedude_view::fonts::bold()));
			ScrollArea::vertical().id_salt("ManualRows").auto_shrink(false).show(ui, |ui| self.rows(ui, palette));
		});
		self.splitter(ui, bar, app.strings.get("TipManualSplitter"), palette);
		let (down, up) = ui.input_mut(|i| {
			(
				keys && i.consume_key(Modifiers::NONE, Key::ArrowDown),
				keys && i.consume_key(Modifiers::NONE, Key::ArrowUp),
			)
		});
		let row = |i: &usize| self.sections[*i].level > 1;
		let step = if down {
			(self.taken + 1..self.sections.len()).find(row)
		} else if up {
			(0..self.taken).rev().find(row)
		} else {
			None
		};
		if let Some(i) = step {
			self.take(i);
		}
		ui.painter().rect_filled(text, 0., palette.window_background);
		ui.scope_builder(UiBuilder::new().max_rect(text), |ui| self.text(ui));
		ui.advance_cursor_after_rect(room);
	}

	/// The bar between the contents and the text: dragged, or Left and Right with the focus, it moves.
	fn splitter(&mut self, ui: &mut Ui, bar: Rect, tip: &str, palette: &Palette) {
		let id = ui.scope_id().with("ManualSplitter");
		let response = ui.interact(bar, id, Sense::click_and_drag()).on_hover_text(tip);
		let response = response.on_hover_cursor(CursorIcon::ResizeHorizontal);
		response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Splitter, true, tip));
		if response.dragged() {
			self.contents_width += response.drag_delta().x;
		}
		if response.has_focus() {
			ui.memory_mut(|m| {
				m.set_focus_lock_filter(id, EventFilter { horizontal_arrows: true, ..Default::default() });
			});
			ui.input_mut(|i| {
				let n = i.count_and_consume_key(Modifiers::NONE, Key::ArrowRight) as f32
					- i.count_and_consume_key(Modifiers::NONE, Key::ArrowLeft) as f32;
				self.contents_width += n * SPLITTER_KEY_STEP;
			});
		}
		let color = if response.has_focus() || response.dragged() { palette.accent } else { palette.border };
		ui.painter().rect_filled(bar, 0., color);
	}

	fn take(&mut self, i: usize) {
		self.taken = i;
		self.go = Some(i);
	}

	fn rows(&mut self, ui: &mut Ui, palette: &Palette) {
		ui.spacing_mut().item_spacing.y = 0.;
		let mut taken = None;
		for (i, section) in self.sections.iter().enumerate().filter(|(_, s)| s.level > 1) {
			let on = i == self.taken;
			let indent = 6. + ROW_INDENT * section.level.saturating_sub(2) as f32;
			let height = ui.text_style_height(&TextStyle::Body) + 6.;
			let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
			if on {
				ui.painter().rect_filled(rect, 0., palette.selection);
			}
			let ink = if on { palette.selection_text } else { palette.text };
			let text = rect.left_center() + vec2(indent, 0.);
			let galley = ui.painter().layout_no_wrap(section.title.clone(), FontId::proportional(FONT_SIZE), ink);
			ui.painter().with_clip_rect(rect).galley(
				text - vec2(0., galley.size().y / 2.),
				galley,
				Color32::PLACEHOLDER,
			);
			response.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, on, &section.title));
			if response.clicked() {
				taken = Some(i);
			}
		}
		if let Some(i) = taken {
			self.take(i);
		}
	}

	fn text(&mut self, ui: &mut Ui) {
		let width = ui.available_width() - 2. * TEXT_PADDING;
		let now = ui.input(|i| i.time);
		if width != self.width {
			self.width = width;
			self.resized = Some(now);
		}
		if let Some(since) = self.resized {
			if self.places.is_empty() || now - since >= SETTLE {
				self.resized = None;
				self.places.clear();
			} else {
				ui.ctx().request_repaint_after_secs((SETTLE - (now - since)) as f32);
			}
		}
		let measured = self.places.len() == self.sections.len();
		let mut scroll = ScrollArea::vertical().id_salt("ManualText").auto_shrink(false);
		if measured && let Some(i) = self.go.take() {
			scroll = scroll.vertical_scroll_offset(self.places[i].0);
		}
		let Self { sections, places, markdown: cache, .. } = self;
		let mut followed = None;
		let clicked = ui.input(|i| i.pointer.any_click());
		scroll.show_viewport(ui, |ui, view| {
			let followed = &mut followed;
			ui.add_space(TEXT_PADDING);
			let origin = ui.min_rect().top();
			for (i, section) in sections.iter().enumerate() {
				if measured {
					let (top, height) = places[i];
					if top + height < view.min.y || top > view.max.y {
						ui.allocate_space(vec2(width, height));
						continue;
					}
				}
				let top = ui.cursor().top() - origin;
				egui::Frame::new().inner_margin(egui::Margin::symmetric(TEXT_PADDING as i8, 0)).show(ui, |ui| {
					ui.set_width(width);
					markdown::show(ui, cache, &section.pieces);
				});
				if !measured {
					places.push((top, ui.cursor().top() - origin - top));
				}
				if clicked {
					*followed = followed
						.take()
						.or_else(|| cache.link_hooks().iter().find(|(_, on)| **on).map(|(l, _)| l.clone()));
				}
			}
		});
		if let Some(link) = followed
			&& let Some(i) = self.links.iter().position(|l| *l == link)
		{
			self.take(i);
		}
	}
}

fn escaped(ctx: &egui::Context) -> bool {
	ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
}
