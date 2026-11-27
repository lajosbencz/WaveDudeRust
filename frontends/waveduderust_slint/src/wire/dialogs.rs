//! The Examples, Welcome, Settings, memory warning and About windows' callbacks.

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use super::Handle;
use crate::state::State;
use crate::ui::{self, AppWindow};
use crate::{
	Action, about, act, answer_warning, apply_settings, clipboard, memory, models, ram_bar, settings, show_examples,
	show_settings,
};

/// The Examples, Welcome, Settings, memory warning and About windows.
pub(crate) fn wire_dialogs(state: &Rc<RefCell<State>>, app: &AppWindow) {
	let h = Handle::new(state, app);
	app.global::<ui::Examples<'_>>().on_pick_topic(move |i| {
		h.with(|s, app| {
			s.example_topic = i;
			show_examples(s, app);
		});
	});
	let h = Handle::new(state, app);
	app.global::<ui::Examples<'_>>().on_search(move |t| {
		h.with(|s, app| {
			s.example_search = t.into();
			show_examples(s, app);
		});
	});
	let h = Handle::new(state, app);
	app.global::<ui::Examples<'_>>().on_open(move |row| {
		h.with(|s, app| {
			if let Some(&i) = models::index_from_ui(row).and_then(|r| s.example_rows.get(r)) {
				app.global::<ui::Examples<'_>>().set_shown(false);
				act(s, app, Action::Example(i));
			}
		});
	});
	let preset = |h: Handle| {
		move |quality| {
			h.with(|s, app| {
				let facts = s.facts;
				settings::preset(&mut s.settings, s.store.as_ref(), quality, facts);
				apply_settings(s, app);
			});
		}
	};
	app.global::<ui::Welcome<'_>>().on_preset(preset(Handle::new(state, app)));
	app.global::<ui::Settings<'_>>().on_preset(preset(Handle::new(state, app)));
	let h = Handle::new(state, app);
	app.global::<ui::Ram<'_>>().on_refresh(move || h.read(|s, app| app.global::<ui::Ram<'_>>().set_bar(ram_bar(s))));
	let h = Handle::new(state, app);
	app.global::<ui::Welcome<'_>>().on_show_startup_toggled(move |on| {
		h.with(|s, app| {
			s.settings.show_startup = on;
			s.save_settings();
			show_settings(s, app);
		});
	});
	let h = Handle::new(state, app);
	app.global::<ui::Settings<'_>>().on_edited(move || {
		h.with(|s, app| {
			settings::apply(&app.global::<ui::Settings<'_>>().get_data(), &mut s.settings, s.store.as_ref());
			apply_settings(s, app);
		});
	});
	let h = Handle::new(state, app);
	app.global::<ui::MemoryWarning<'_>>().on_ticked(
		move |ui::WarningTicks { cache: cache_auto, resolution, frames, samples }| {
			h.with(|s, app| {
				let ticks = waveduderust::run::Levers { cache_auto, resolution, frames, samples };
				if let Some((_, w)) = s.live.warning() {
					app.global::<ui::MemoryWarning<'_>>().set_data(memory::data(&w, ticks));
					s.warning_ticks = Some(ticks);
				}
			});
		},
	);
	let h = Handle::new(state, app);
	app.global::<ui::MemoryWarning<'_>>()
		.on_answer(move |answer, mute| h.show(|s, app| answer_warning(s, app, answer, mute)));
	app.global::<ui::About<'_>>().on_open_link(|url| crate::open_link(&url));
	let st = state.clone();
	app.global::<ui::About<'_>>().on_copy(move || {
		let text = about(&st.borrow()).system.to_string();
		match clipboard::write(text) {
			Ok(()) => tracing::info!("System information copied to the clipboard"),
			Err(e) => tracing::warn!("clipboard: {e}"),
		}
	});
}
