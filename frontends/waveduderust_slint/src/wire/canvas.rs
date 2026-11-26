//! The pictures' callbacks.

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use super::Handle;
use crate::state::State;
use crate::tree::Node;
use crate::ui::{self, AppWindow};
use crate::{canvas, end_gesture, export_probe_csv, live, models, number};

/// The pictures' handles: selection by click, drags as one undo step each.
pub(crate) fn wire_canvas(state: &Rc<RefCell<State>>, app: &AppWindow) {
	let canvas = app.global::<ui::Canvas<'_>>();
	state.borrow().overlays.wire(&canvas);
	state.borrow().graphs.wire(&canvas);
	let (st, weak) = (state.clone(), app.as_weak());
	canvas.on_rescaled(move |layer, scale| {
		let Some(app) = weak.upgrade() else { return };
		// reported during a layout, which may run while the state is borrowed: taken up once it is free
		let st = st.clone();
		slint::Timer::single_shot(std::time::Duration::ZERO, move || {
			let s = &mut *st.borrow_mut();
			if s.overlays.rescale(layer, scale) {
				s.overlays.show(&app.global::<ui::Canvas<'_>>(), &s.doc.project, &s.doc.view, &s.selection);
				// the boxes are laid out in logical pixels
				s.show_graphs();
				s.show_box_focus(&app);
			}
		});
	});
	let h = Handle::new(state, app);
	canvas.on_press(move |layer, x, y, control, shift| {
		h.show(|s, app| {
			s.drags += 1;
			let boxes = s.graphs.placed(layer);
			let press = canvas::Press { layer, at: (x, y), control, shift };
			s.gesture = s.overlays.press(&s.doc.project, &mut s.selection, press, s.drags, boxes);
			app.global::<ui::Canvas<'_>>().set_dragging(s.gesture.is_some());
		});
	});
	let h = Handle::new(state, app);
	canvas.on_drag(move |dx, dy, shift| {
		h.with(|s, app| match &mut s.gesture {
			Some(canvas::Gesture::Handle(drag)) => {
				s.doc.edit_merging(Some(drag.key()), |p| drag.apply(p, (dx, dy), shift));
				s.show(app);
			}
			Some(canvas::Gesture::Marquee(m)) => {
				m.stretch((dx, dy));
				let canvas = app.global::<ui::Canvas<'_>>();
				canvas.set_marquee(m.rect());
				canvas.set_marquee_layer(m.layer());
				canvas.set_marquee_shown(true);
			}
			None => {}
		});
	});
	let h = Handle::new(state, app);
	canvas.on_release(move || h.show(State::finish_gesture));
	let h = Handle::new(state, app);
	canvas.on_brightness(move |layer, value| {
		h.show(|s, _| s.set_brightness(canvas::model_layer(layer), number::wide(value)));
	});
	let h = Handle::new(state, app);
	canvas.on_auto(move |layer| {
		h.show(|s, _| {
			let layer = canvas::model_layer(layer);
			if let live::Fit::Done(fit) = s.live.auto(layer) {
				s.fitted(layer, fit);
			}
		});
	});
	let h = Handle::new(state, app);
	canvas.on_hover(move |layer, x, y| {
		h.read(|s, app| {
			let canvas = app.global::<ui::Canvas<'_>>();
			let under = models::index_to_ui(s.graphs.under(layer, (x, y)));
			canvas.set_hover_layer(layer);
			canvas.set_hover_box(under);
			canvas.set_hover_tip(s.overlays.tip(&s.doc.project, layer, (x, y), s.graphs.placed(layer)));
		});
	});
	let h = Handle::new(state, app);
	canvas.on_export_probe_csv(move |probe| {
		h.with(|s, app| {
			if let Ok(i) = usize::try_from(probe) {
				s.selection.set(Node::Probe(i));
				export_probe_csv(s, app, i);
				s.show(app);
			}
		});
	});
	let h = Handle::new(state, app);
	canvas.on_key(move |layer, key, shift| {
		use ui::HandleKey;
		h.with(|s, app| {
			let step = if shift { 10. } else { 1. };
			// a lever turns counter-clockwise on Right and Up
			let (d, turn) = match key {
				HandleKey::Enter | HandleKey::Next | HandleKey::Previous => {
					let taken = if key == HandleKey::Enter {
						s.overlays.enter(&s.doc.project, &mut s.selection, layer, s.graphs.placed(layer))
					} else {
						let back = key == HandleKey::Previous;
						s.overlays.tab(&s.doc.project, &mut s.selection, layer, back, s.graphs.placed(layer))
					};
					s.show(app);
					return taken;
				}
				HandleKey::Left => ((-step, 0.), -step),
				HandleKey::Right => ((step, 0.), step),
				HandleKey::Up => ((0., -step), step),
				HandleKey::Down => ((0., step), -step),
			};
			let Some(key) = s.overlays.nudge_key(layer, &s.selection) else { return false };
			let boxes = s.graphs.placed(layer);
			if s.overlays.focused_box(layer, &s.doc.project, boxes).is_some() {
				s.doc.edit_merging(Some(key), |p| s.overlays.nudge_box(p, layer, d, boxes));
			} else {
				s.doc.edit_merging(Some(key), |p| s.overlays.nudge(p, &s.selection, layer, d, turn));
			}
			s.show(app);
			true
		})
	});
	let h = Handle::new(state, app);
	canvas.on_cancel(move || {
		h.show(|s, app| {
			end_gesture(app);
			if let Some(canvas::Gesture::Handle(drag)) = s.gesture.take() {
				s.doc.cancel(&drag.key());
			}
		});
	});
}
