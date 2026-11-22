//! A number box over a [`NumberSpec`] on `NumberField` (docs/ui/30, Number box): typed text keeps its digits,
//! clamped; steps, the wheel and scrubs round to the spec's precision.

use std::fmt::Debug;
use std::hash::Hash;

use egui_mineral::layout::Edge;
use egui_mineral::semantic::KeyedSalt;
use egui_mineral::widget::WidgetResponse;
use egui_mineral_material::extras::{NumberField, NumberFieldOutput};
use egui_mineral_material::prelude::TextFieldVariant;
use waveduderust_app::number::NumberSpec;
use waveduderust_egui_platform::number::range_of;

use crate::Mui;

/// What a pass of [`show`] asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Change {
	pub(super) value: Option<f64>,
	/// A scrub began or ended this pass: one history step spans it.
	pub(super) scrub_began: bool,
	pub(super) scrub_ended: bool,
}

/// A box showing `value`, none when `blank`; `place` sets the field up and shows it.
pub(super) fn show<S: Into<KeyedSalt> + Hash + Debug + Copy>(
	ui: &mut Mui<'_>,
	salt: S,
	label: &str,
	spec: &NumberSpec,
	value: f64,
	blank: bool,
	place: impl FnOnce(&mut Mui<'_>, NumberField<'_>) -> WidgetResponse<NumberFieldOutput>,
) -> (WidgetResponse<NumberFieldOutput>, Change) {
	let id = ui.make_persistent_id(("number-box", salt));
	let mut scrubbing = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or_default();
	// the runtime's separator, which the field parses
	let shown = spec.clone().with_decimal_separator(ui.rt().i18n().format().decimal_separator());
	let mut v = value;
	let field = NumberField::new(salt, label, range_of(spec), &mut v)
		.variant(TextFieldVariant::Outlined)
		.commit_on_submit(true)
		.snap_typed(false)
		.step_buttons(false)
		.text_align(Edge::End)
		.tabular_figures(true)
		.format(move |x| if blank { String::new() } else { shown.format(x) });
	let r = place(ui, field);
	let out = *r.output();
	let mut change = Change::default();
	if out.scrub_started {
		scrubbing = true;
		change.scrub_began = true;
	}
	change.value = (v != value).then_some(v);
	if scrubbing && (out.edit.is_some() || !ui.input(|i| i.pointer.primary_down())) {
		scrubbing = false;
		change.scrub_ended = true;
	}
	ui.data_mut(|d| d.insert_temp(id, scrubbing));
	(r, change)
}
