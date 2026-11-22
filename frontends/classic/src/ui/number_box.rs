//! The number box of docs/ui/30 (Number box) over a [`NumberSpec`]: the kit's box on the spec's range, drag rate and
//! text, and the pending edit of its typed text.

use eframe::egui::{self, Id, Rect, Ui};
use waveduderust_app::number::NumberSpec;
use waveduderust_egui_platform::number::range_of;

use super::edit;
use crate::kit::number::{self, Edited, Number, Turned};

impl Edited {
	/// Offers the typed value's `event` as the pending edit of box `id` while it has or just lost the focus.
	pub(super) fn offer(&self, ctx: &egui::Context, id: Id, event: impl Fn(f64) -> waveduderust_app::Event) {
		self.offer_opt(ctx, id, |v| Some(event(v)));
	}

	/// As [`offer`](Self::offer); `event` may decline a value.
	pub(super) fn offer_opt(
		&self,
		ctx: &egui::Context,
		id: Id,
		event: impl Fn(f64) -> Option<waveduderust_app::Event>,
	) {
		if self.response.has_focus() || self.response.lost_focus() {
			edit::offer(ctx, id, self.typed.and_then(event));
		}
	}
}

/// Runs `show` on the kit's description of `spec`.
fn with_spec<R>(spec: &NumberSpec, label: &str, show: impl FnOnce(&Number<'_>) -> R) -> R {
	let format = |v: f64| spec.format(v);
	let parse = |text: &str| spec.parse(text);
	show(&Number { range: range_of(spec), scrub: spec.drag_pixels() as f32, format: &format, parse: &parse, label })
}

/// The box at `rect` showing `value` (nothing when `blank`), `id` its text's; `label` names it for assistive
/// technology. Disabled with its `Ui`.
pub(super) fn number_box(
	ui: &mut Ui,
	rect: Rect,
	id: Id,
	value: f64,
	spec: &NumberSpec,
	blank: bool,
	label: &str,
) -> Edited {
	with_spec(spec, label, |n| number::flat(ui, rect, id, (!blank).then_some(value), n))
}

/// The box `id` turned in `rect`, edited upright by [`number_box`] while `upright`.
pub(super) fn turned(ui: &Ui, rect: Rect, id: Id, value: f64, spec: &NumberSpec, label: &str, upright: bool) -> Turned {
	with_spec(spec, label, |n| number::turned(ui, rect, id, value, n, upright))
}

#[cfg(test)]
mod tests {
	use eframe::egui::{Event, Key, vec2};

	use super::*;

	#[test]
	fn a_step_applies_once_and_leaves_nothing_pending() {
		use egui_kittest::kittest::Queryable as _;

		/// The value shown, the values applied, and the passes that also left a value pending.
		#[derive(Default)]
		struct Seen {
			value: f64,
			applied: Vec<f64>,
			both: usize,
		}
		let id = Id::unique("Box");
		let spec = NumberSpec::integer(0., 100.);
		let builder = egui_kittest::Harness::builder().with_context_setup(crate::kit::install);
		let mut harness = builder.build_ui_state(
			|ui, seen: &mut Seen| {
				let rect = Rect::from_min_size(ui.cursor().min, vec2(80., 22.));
				let edited = number_box(ui, rect, id, seen.value, &spec, false, "Box");
				seen.both += usize::from(edited.applied.is_some() && edited.typed.is_some());
				if let Some(v) = edited.applied {
					seen.applied.push(v);
					seen.value = v;
				}
			},
			Seen { value: 5., ..Seen::default() },
		);
		harness.get_by_label("Box").click();
		harness.run();
		for key in [Key::ArrowUp, Key::PageUp] {
			harness.key_press(key);
			harness.run();
		}
		harness.event(Event::Text("42".into()));
		harness.run();
		harness.key_press(Key::Enter);
		harness.run();
		assert_eq!(harness.state().applied, [6., 16., 42.]);
		assert_eq!(harness.state().both, 0, "an applied value was also left pending");
	}
}
