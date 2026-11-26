//! The Examples, Welcome, Settings, memory warning and About windows' callbacks.

use std::cell::RefCell;
use std::rc::Rc;

use super::Handle;
use crate::state::State;
use crate::ui::AppWindow;
use crate::{
	Action, about, act, answer_warning, apply_settings, clipboard, memory, models, ram_bar, settings, show_examples,
	show_settings,
};

/// The Examples, Welcome, Settings, memory warning and About windows.
pub(crate) fn wire_dialogs(state: &Rc<RefCell<State>>, app: &AppWindow) {
	let h = Handle::new(state, app);
	app.on_example_topic_picked(move |i| {
		h.with(|s, app| {
			s.example_topic = i;
			show_examples(s, app);
		});
	});
	let h = Handle::new(state, app);
	app.on_example_search(move |t| {
		h.with(|s, app| {
			s.example_search = t.into();
			show_examples(s, app);
		});
	});
	let h = Handle::new(state, app);
	app.on_example_open(move |row| {
		h.with(|s, app| {
			if let Some(&i) = models::index_from_ui(row).and_then(|r| s.example_rows.get(r)) {
				app.set_examples_shown(false);
				act(s, app, Action::Example(i));
			}
		});
	});
	let h = Handle::new(state, app);
	app.on_welcome_preset(move |i| {
		h.with(|s, app| {
			let facts = s.facts;
			settings::preset(&mut s.settings, s.store.as_ref(), i, facts);
			apply_settings(s, app);
			show_settings(s, app);
		});
	});
	let h = Handle::new(state, app);
	app.on_ram_refresh(move || h.read(|s, app| app.set_ram(ram_bar(s))));
	let h = Handle::new(state, app);
	app.on_welcome_show_startup_toggled(move |on| {
		h.with(|s, app| {
			s.settings.show_startup = on;
			s.save_settings();
			show_settings(s, app);
		});
	});
	let h = Handle::new(state, app);
	app.on_settings_edited(move || {
		h.with(|s, app| {
			settings::apply(&app.get_settings(), &mut s.settings, s.store.as_ref());
			apply_settings(s, app);
			show_settings(s, app);
		});
	});
	let h = Handle::new(state, app);
	app.on_memory_warning_ticked(move |cache_auto, resolution, frames, samples| {
		h.with(|s, app| {
			let ticks = waveduderust::run::Levers { cache_auto, resolution, frames, samples };
			if let Some((_, w)) = s.live.warning() {
				app.set_memory_warning(memory::data(&w, ticks));
				s.warning_ticks = Some(ticks);
			}
		});
	});
	let h = Handle::new(state, app);
	app.on_memory_warning_answer(move |answer, mute| h.show(|s, app| answer_warning(s, app, answer, mute)));
	app.on_about_link(|url| crate::open_link(&url));
	let st = state.clone();
	app.on_about_copy(move || {
		let text = about(&st.borrow()).system.to_string();
		match clipboard::write(text) {
			Ok(()) => tracing::info!("System information copied to the clipboard"),
			Err(e) => tracing::warn!("clipboard: {e}"),
		}
	});
}
