//! Form rows (DESIGN.md, Overlay pages): a [`SettingRow`] per setting, and the texts around them.

use eframe::egui::{self, Frame, Margin, RichText};
use egui_mineral_material::prelude::{MaterialExt as _, SettingRow, TypeScale};

use crate::Mui;

/// The widest a row's control gets.
const CONTROL_W: f32 = 280.;
/// A `SettingRow`'s inset of its label.
const ROW_INSET: i8 = 16;
/// The space below each row.
const ROW_GAP: f32 = 8.;

/// Text in `scale`, dimmed with `dim`, wrapping at the row's width.
pub(crate) fn text(ui: &mut Mui<'_>, text: &str, scale: TypeScale, dim: bool) {
	let c = ui.tokens().color;
	let color = if dim { c.on_surface_variant } else { c.on_surface };
	let font = scale.font(ui.ctx());
	ui.add(egui::Label::new(RichText::new(text).font(font).color(color)).wrap());
}

/// A group's heading.
pub(crate) fn heading(ui: &mut Mui<'_>, title: &str) {
	let c = ui.tokens().color;
	let font = TypeScale::TitleSmall.font(ui.ctx());
	ui.add(egui::Label::new(RichText::new(title).font(font).color(c.primary)).wrap());
}

/// A row whose control needs the row's width, below its label at any width.
pub(crate) fn stacked<R>(
	ui: &mut Mui<'_>,
	label: &str,
	supporting: Option<&str>,
	control: impl FnOnce(&mut Mui<'_>) -> R,
) -> R {
	let rt = ui.rt();
	let inset = Frame::NONE.inner_margin(Margin { left: ROW_INSET, ..Margin::ZERO });
	let r = inset
		.show(ui, |ui| {
			let ui = &mut rt.ui(ui);
			text(ui, label, TypeScale::BodyLarge, false);
			if let Some(supporting) = supporting {
				text(ui, supporting, TypeScale::BodyMedium, true);
			}
			control(ui)
		})
		.inner;
	ui.add_space(ROW_GAP);
	r
}

fn setting_row(key: &'static str, label: &str, supporting: Option<&str>) -> SettingRow {
	let row = SettingRow::new(("form-row", key), label);
	match supporting {
		Some(text) => row.supporting(text),
		None => row,
	}
}

/// A row keyed `key`, labelled `label` with `supporting` text; `control` gets the width it may take.
pub(crate) fn row<R>(
	ui: &mut Mui<'_>,
	key: &'static str,
	label: &str,
	supporting: Option<&str>,
	control: impl FnOnce(&mut Mui<'_>, f32) -> R,
) -> R {
	let r = setting_row(key, label, supporting)
		.show(ui, |ui, _| {
			let w = ui.available_width().min(CONTROL_W);
			control(ui, w)
		})
		.inner;
	ui.add_space(ROW_GAP);
	r
}

/// A read-only row keyed `key` showing `value`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn value(ui: &mut Mui<'_>, key: &'static str, label: &str, value: &str) {
	setting_row(key, label, None).show_value(ui, value);
	ui.add_space(ROW_GAP);
}
