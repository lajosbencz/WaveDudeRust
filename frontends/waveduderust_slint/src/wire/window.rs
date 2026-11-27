//! The window's own events.

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use crate::state::State;
use crate::ui::{self, AppWindow};
use crate::{apply_theme, dropped, wake};

/// The window's own events: input wakes the tick, a dropped file opens, and after a forced theme the System theme
/// keeps the desktop's scheme from winit's theme event, which Linux lacks (docs/blockers.md, Slint's system colour
/// scheme).
pub(crate) fn wire_window_events(state: &Rc<RefCell<State>>, app: &AppWindow) {
	use slint::language::ColorScheme;
	use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
	let (state, weak) = (state.clone(), app.as_weak());
	let mut cursor = None;
	app.window().on_winit_window_event(move |window, event| {
		use winit::event::{ElementState, TouchPhase, WindowEvent};
		let pressed = match event {
			WindowEvent::CursorMoved { position, .. } => {
				cursor = Some(*position);
				None
			}
			WindowEvent::CursorLeft { .. } => {
				cursor = None;
				None
			}
			WindowEvent::MouseInput { state: ElementState::Pressed, .. } => cursor,
			WindowEvent::Touch(t) if t.phase == TouchPhase::Started => Some(t.location),
			_ => None,
		};
		if let (Some(at), Some(app)) = (pressed, weak.upgrade()) {
			let at = at.to_logical::<f32>(f64::from(window.scale_factor()));
			app.global::<ui::Dock<'_>>().invoke_pressed(at.x, at.y);
		}
		// input ticks once after Slint handled it, as classic's repaint on input did: what a callback changed is shown
		if matches!(
			event,
			WindowEvent::KeyboardInput { .. }
				| WindowEvent::MouseInput { .. }
				| WindowEvent::MouseWheel { .. }
				| WindowEvent::Touch(_)
				| WindowEvent::Ime(_)
		) {
			wake::wake();
		}
		if let (WindowEvent::DroppedFile(path), Some(app)) = (event, weak.upgrade()) {
			// a native dialog's nested loop may deliver it while the state is borrowed
			let Ok(mut s) = state.try_borrow_mut() else { return EventResult::Propagate };
			let s = &mut *s;
			dropped(s, &app, path.clone());
			s.show(&app);
		}
		if let (WindowEvent::ThemeChanged(theme), Some(app)) = (event, weak.upgrade()) {
			let mut s = state.borrow_mut();
			if s.system_scheme.is_some() {
				s.system_scheme = Some(match theme {
					winit::window::Theme::Dark => ColorScheme::Dark,
					winit::window::Theme::Light => ColorScheme::Light,
				});
				apply_theme(&mut s, &app);
			}
		}
		EventResult::Propagate
	});
}
