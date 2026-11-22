//! Pointer and key input the preview's widgets share: presses, pinches, arrow steps and wheel notches.

use eframe::egui::{Context, Event, EventFilter, Id, Key, Modifiers, Rect, Response, TouchPhase, Ui};

/// Whether a button went down on `r` this frame; a press and its release can come in one.
pub fn pressed_on(r: &Response) -> bool {
	r.hovered() && r.ctx.input(|i| i.pointer.any_pressed())
}

/// Whether a second finger is down: the touches are a pinch or a pan, no longer a press.
pub fn pinching(ui: &Ui) -> bool {
	ui.input(|i| i.multi_touch().is_some())
}

#[derive(Clone, Copy, Default)]
struct PinchedPress(bool);

/// Whether the touches now down include a pinch or pan, from the moment the second finger moved until the last one
/// lifts: egui restarts its long-press timer while the first finger rests, which would open a context menu.
pub fn pinched_press(ctx: &Context) -> bool {
	let (pinch, touching) = ctx.input(|i| (i.multi_touch().is_some(), i.any_touches()));
	ctx.data_mut(|d| {
		let flag = d.get_temp_mut_or_default::<PinchedPress>(Id::NULL);
		if pinch {
			flag.0 = true;
		} else if !touching {
			flag.0 = false;
		}
		flag.0
	})
}

/// Whether a finger went down this frame while another may be resting: egui-winit ends the pointer press of the first
/// then, so a second finger is seen here before (or without) a multi-touch gesture.
pub fn touch_started(ui: &Ui) -> bool {
	ui.input(|i| i.events.iter().any(|e| matches!(e, Event::Touch { phase: TouchPhase::Start, .. })))
}

/// The arrow keys pressed, one or ten (Shift) a key, as a step right and down; consumed.
pub fn arrow_steps(ui: &Ui) -> (i32, i32) {
	ui.input_mut(|i| {
		let mut d = (0, 0);
		for (modifiers, n) in [(Modifiers::NONE, 1), (Modifiers::SHIFT, 10)] {
			let mut count = |key| i.count_and_consume_key(modifiers, key) as i32 * n;
			d.0 += count(Key::ArrowRight) - count(Key::ArrowLeft);
			d.1 += count(Key::ArrowDown) - count(Key::ArrowUp);
		}
		d
	})
}

/// Keeps the arrow keys with the focused widget `id`.
pub fn lock_arrows(ui: &Ui, id: Id) {
	let filter = EventFilter { horizontal_arrows: true, vertical_arrows: true, ..Default::default() };
	ui.memory_mut(|m| m.set_focus_lock_filter(id, filter));
}

/// Wheel notches over `rect`, up positive.
pub fn wheel_notches(ui: &Ui, rect: Rect) -> i32 {
	if !ui.rect_contains_pointer(rect) {
		return 0;
	}
	ui.input(|i| {
		i.events
			.iter()
			.map(|e| match e {
				Event::MouseWheel { delta, .. } if delta.y != 0. => delta.y.signum() as i32,
				_ => 0,
			})
			.sum()
	})
}
