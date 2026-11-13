//! The commit policy of docs/ui/30 (Text box): the one pending edit, the event the focused box's typed text
//! would apply, kept in egui's memory beside the typed text. The window applies it before any event it sends and
//! when its box was not shown in a pass (unmounted with typed text); applying it drops the typed text, so the box
//! shows the model's value again.

use eframe::egui::{Context, Id};
use wavedude_app_state::Event;

/// The pending edit and the ones a newer box pushed out before the window took them.
#[derive(Clone, Default)]
struct Slot {
	pending: Option<Pending>,
	due: Vec<Event>,
}

#[derive(Clone)]
struct Pending {
	id: Id,
	event: Event,
	/// Offered again in this pass.
	seen: bool,
}

fn slot_id() -> Id {
	Id::unique("PendingEdit")
}

fn with<R>(ctx: &Context, f: impl FnOnce(&mut Slot) -> R) -> R {
	ctx.data_mut(|d| f(d.get_temp_mut_or_default::<Slot>(slot_id())))
}

/// Where box `id` keeps its typed text while it has the focus.
pub(super) fn typed_id(id: Id) -> Id {
	id.with("typed")
}

/// Box `id`'s typed text applies as `event`; `None`: it applies nothing (not typed, applied or reverted). Another
/// box's pending edit is applied first.
pub(super) fn offer(ctx: &Context, id: Id, event: Option<Event>) {
	with(ctx, |s| match event {
		Some(event) => {
			if let Some(other) = s.pending.take_if(|p| p.id != id) {
				s.due.push(other.event);
			}
			s.pending = Some(Pending { id, event, seen: true });
		}
		None => {
			s.pending.take_if(|p| p.id == id);
		}
	});
}

/// The start of a pass: the pending edit's box has to offer it again.
pub(super) fn begin_pass(ctx: &Context) {
	with(ctx, |s| {
		if let Some(p) = &mut s.pending {
			p.seen = false;
		}
	});
}

/// The edits to apply now, in order: all of them when `everything` (before the window sends an event), else those
/// whose box is gone.
pub(super) fn take(ctx: &Context, everything: bool, out: &mut Vec<Event>) {
	let gone = with(ctx, |s| {
		out.append(&mut s.due);
		s.pending.take_if(|p| everything || !p.seen)
	});
	if let Some(p) = gone {
		ctx.data_mut(|d| d.remove::<String>(typed_id(p.id)));
		out.push(p.event);
	}
}
