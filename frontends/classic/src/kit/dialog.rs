//! Next's dialog windows (docs/ui/70; Dialogs/MessageWindow.xaml) on egui-mineral's modal layer: the frame and title,
//! the message body, the button row with Enter (default), Esc (cancel) and mnemonics with or without Alt, and the
//! focus trap.

use eframe::egui::{
	self, Align, Align2, Color32, FontId, Frame, Id, Key, Layout, Margin, Modifiers, Rect, Response, ScrollArea, Sense,
	Stroke, Ui, Vec2, pos2, vec2,
};
use egui_mineral::runtime::Core;
use waveduderust_app::strings::{mnemonic, strip_mnemonic};

use super::tokens::{BORDER, FONT_SIZE, Palette, TITLE_HEIGHT};

pub(crate) const PADDING: f32 = 16.;
const SPACING: f32 = 12.;
/// Between a body's items.
pub(crate) const ITEM_SPACING: Vec2 = Vec2::new(8., SPACING);
/// A message window's body: the window 440 wide (MessageWindow.xaml:25).
pub(crate) const MESSAGE_WIDTH: f32 = 440. - 2. * PADDING;
const DISC: f32 = 32.;
const BUTTON: Vec2 = Vec2::new(75., 23.);
const BACKDROP: Color32 = Color32::from_black_alpha(110);
/// A message's text scrolls beyond this height.
const TEXT_HEIGHT: f32 = 240.;

/// A modal window on the core's modal layer, its body `width` wide (bounded by the screen): the title over `add`'s
/// body; what `add` returns.
pub(crate) fn window<R>(core: &Core, id: Id, title: &str, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
	let ctx = core.ctx();
	let palette = Palette::of(ctx.theme());
	let frame = Frame::new().fill(palette.window_background).stroke(Stroke::new(BORDER, palette.border));
	let modal = egui_mineral::kit::layer::modal(core, id).frame(frame).backdrop_color(BACKDROP);
	let width = width.min(ctx.content_rect().width() - 4. * PADDING);
	modal
		.show(ctx, |ui| {
			ui.spacing_mut().item_spacing = Vec2::ZERO;
			ui.set_width(width + 2. * PADDING);
			let (title_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), TITLE_HEIGHT), Sense::hover());
			let inner = Frame::new()
				.inner_margin(Margin::same(PADDING as i8))
				.show(ui, |ui| {
					ui.set_width(width);
					ui.spacing_mut().item_spacing = ITEM_SPACING;
					add(ui)
				})
				.inner;
			// as wide as a body wider than asked
			let rect = Rect::from_min_size(title_rect.min, vec2(ui.min_rect().width(), TITLE_HEIGHT));
			title_bar(ui, palette, rect, title);
			inner
		})
		.inner
}

fn title_bar(ui: &Ui, palette: &Palette, rect: Rect, title: &str) {
	let painter = ui.painter();
	painter.rect_filled(rect, 0., palette.tool_window_background);
	let rule = Rect::from_min_max(pos2(rect.left(), rect.bottom() - BORDER), rect.max);
	painter.rect_filled(rule, 0., palette.border);
	let at = pos2(rect.left() + PADDING / 2., rect.center().y);
	painter.text(at, Align2::LEFT_CENTER, title, FontId::proportional(FONT_SIZE), palette.text);
}

/// What a message window's disc shows.
#[derive(Clone, Copy)]
pub(crate) enum Disc {
	Error,
	Question,
	Info,
}

/// A button of a dialog: its text with the mnemonic, and whether Enter (`default`) or Esc (`cancel`) press it.
#[derive(Clone, Copy)]
pub(crate) struct Choice<'a> {
	pub text: &'a str,
	pub default: bool,
	pub cancel: bool,
	pub enabled: bool,
	pub tip: Option<&'a str>,
	/// Drawn in the accent colour.
	pub primary: bool,
}

impl<'a> Choice<'a> {
	pub(crate) fn plain(text: &'a str) -> Self {
		Self { text, default: false, cancel: false, enabled: true, tip: None, primary: false }
	}

	pub(crate) fn default(text: &'a str) -> Self {
		Self { default: true, ..Self::plain(text) }
	}

	pub(crate) fn cancel(text: &'a str) -> Self {
		Self { cancel: true, ..Self::plain(text) }
	}

	/// The only button: Enter and Esc both press it.
	pub(crate) fn both(text: &'a str) -> Self {
		Self { default: true, cancel: true, ..Self::plain(text) }
	}
}

/// A message window's body: [`head`], then `choices` at the bottom right; the index of the one pressed.
pub(crate) fn message(ui: &mut Ui, fresh: bool, disc: Disc, text: &str, choices: &[Choice<'_>]) -> Option<usize> {
	head(ui, disc, text);
	buttons(ui, fresh, choices)
}

/// The disc with `text` wrapped beside it, both against the top (MessageWindow.xaml:38-45); a long text scrolls.
pub(crate) fn head(ui: &mut Ui, disc: Disc, text: &str) {
	ui.horizontal_top(|ui| {
		ui.spacing_mut().item_spacing.x = SPACING;
		paint_disc(ui, disc);
		ScrollArea::vertical().max_height(TEXT_HEIGHT).show(ui, |ui| {
			ui.add(egui::Label::new(text).wrap());
		});
	});
}

fn paint_disc(ui: &mut Ui, disc: Disc) {
	let palette = Palette::of(ui.ctx().theme());
	let (fill, glyph) = match disc {
		Disc::Error => (palette.status_error, "!"),
		Disc::Question => (palette.accent, "?"),
		Disc::Info => (palette.accent, "i"),
	};
	let (rect, _) = ui.allocate_exact_size(Vec2::splat(DISC), Sense::hover());
	let painter = ui.painter();
	painter.circle_filled(rect.center(), DISC / 2., fill);
	let font = FontId::new(18., waveduderust::unstable::view::fonts::bold());
	painter.text(rect.center(), Align2::CENTER_CENTER, glyph, font, palette.status_text);
}

/// A dialog's button with its mnemonic stripped.
pub(crate) fn button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
	ui.add_enabled(enabled, egui::Button::new(strip_mnemonic(text)).min_size(BUTTON))
}

fn primary_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
	let palette = Palette::of(ui.ctx().theme());
	let label = egui::RichText::new(strip_mnemonic(text)).color(palette.status_text);
	ui.add_enabled(enabled, egui::Button::new(label).fill(palette.accent).min_size(BUTTON))
}

/// The row of `choices` at the bottom right; the index of the one pressed by the pointer or a key. The default one
/// takes the focus when the dialog is `fresh`, and whenever the focus is outside the dialog.
pub(crate) fn buttons(ui: &mut Ui, fresh: bool, choices: &[Choice<'_>]) -> Option<usize> {
	let mut pressed = None;
	ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
		for (i, choice) in choices.iter().enumerate().rev() {
			let mut response = if choice.primary {
				primary_button(ui, choice.text, choice.enabled)
			} else {
				button(ui, choice.text, choice.enabled)
			};
			if let Some(tip) = choice.tip {
				response = response.on_hover_text(tip);
			}
			if choice.default && (fresh || focus_outside(ui)) {
				response.request_focus();
			}
			if response.clicked() {
				pressed = Some(i);
			}
		}
	});
	pressed.or_else(|| pressed_key(ui, choices))
}

/// The focus trap: egui's modal layer keeps Tab inside; focus left on a widget below it comes back. Popups the
/// dialog opens sit above it and keep theirs.
fn focus_outside(ui: &Ui) -> bool {
	let ctx = ui.ctx();
	let Some(layer) = ctx.memory(egui::Memory::focused).and_then(|id| ctx.read_response(id)).map(|r| r.layer_id) else {
		return false;
	};
	!ctx.memory(|m| m.is_above_modal_layer(layer))
}

/// The letter or digit a key types, lower-cased.
fn letter_of(key: Key) -> Option<char> {
	let mut chars = key.name().chars();
	match (chars.next(), chars.next()) {
		(Some(c), None) if c.is_alphanumeric() => Some(c.to_ascii_lowercase()),
		_ => None,
	}
}

/// Whether the mnemonic of `text` was typed, with or without Alt, outside a text box; the key is taken.
pub(crate) fn mnemonic_pressed(ui: &Ui, text: &str) -> bool {
	pressed_key(ui, &[Choice::plain(text)]).is_some()
}

/// Whether Esc was pressed; the key is taken.
pub(crate) fn escape_pressed(ui: &Ui) -> bool {
	egui_mineral::kit::layer::consume_escape(ui.ctx())
}

/// The choice a key picks: Enter, Esc or a mnemonic letter; the key is taken.
fn pressed_key(ui: &Ui, choices: &[Choice<'_>]) -> Option<usize> {
	let typing = ui.ctx().text_edit_focused();
	let pick = |key: Key| -> Option<usize> {
		let find = |f: &dyn Fn(&Choice<'_>) -> bool| choices.iter().position(|c| c.enabled && f(c));
		match key {
			Key::Enter => find(&|c| c.default),
			Key::Escape => find(&|c| c.cancel),
			key if !typing => letter_of(key).and_then(|l| find(&|c| mnemonic(c.text) == Some(l))),
			_ => None,
		}
	};
	let mut picked = None;
	ui.input_mut(|input| {
		input.events.retain(|event| {
			let egui::Event::Key { key, pressed: true, modifiers, .. } = event else { return true };
			if picked.is_some() || !(modifiers.is_none() || *modifiers == Modifiers::ALT) {
				return true;
			}
			picked = pick(*key);
			picked.is_none()
		});
	});
	picked
}
