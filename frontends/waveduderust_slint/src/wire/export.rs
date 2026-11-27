//! The export dialog's callbacks.

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use super::Handle;
use crate::state::State;
#[cfg(target_arch = "wasm32")]
use crate::tell;
use crate::ui::{self, AppWindow};
use crate::{ask_replace, export};

/// The export dialog: format, paths, codec and OK.
pub(crate) fn wire_export(state: &Rc<RefCell<State>>, app: &AppWindow) {
	let ui_export = app.global::<ui::Export<'_>>();
	let h = Handle::new(state, app);
	ui_export.on_format(move |index| h.read(|s, app| s.edit_export(app, |d, data| d.format(data, index))));
	let h = Handle::new(state, app);
	ui_export.on_browse(move || {
		h.read(|s, app| s.edit_export(app, |d, data| d.browse(data, &app.global::<ui::Words<'_>>())));
	});
	let h = Handle::new(state, app);
	ui_export.on_path_edited(move || {
		h.read(|s, app| {
			s.edit_export(app, |d, data| {
				data.error = Default::default();
				d.follow(data);
			});
		});
	});
	let h = Handle::new(state, app);
	ui_export.on_browse_archive(move || {
		h.read(|s, app| s.edit_export(app, |d, data| d.browse_archive(data, &app.global::<ui::Words<'_>>())));
	});
	let weak = app.as_weak();
	ui_export.on_delete_codec(move || {
		let Some(app) = weak.upgrade() else { return };
		let ui_export = app.global::<ui::Export<'_>>();
		let mut data = ui_export.get_data();
		export::delete_codec(&mut data);
		ui_export.set_data(data);
	});
	let h = Handle::new(state, app);
	ui_export.on_ok(move || {
		h.with(|s, app| {
			let Some(dialog) = &s.export_dialog else { return };
			let ui_export = app.global::<ui::Export<'_>>();
			let mut data = ui_export.get_data();
			#[cfg(target_arch = "wasm32")]
			if let Some(download) = dialog.csv_download(&data) {
				match download {
					Ok(download) => {
						ui_export.set_shown(false);
						s.export_dialog = None;
						tracing::info!("Exported the probe data as {}", download.name());
						tell(app, ui::Message::ExportReady, download.name());
						s.ready = Some(download);
					}
					Err(e) => {
						data.error = e.into();
						ui_export.set_data(data);
					}
				}
				return;
			}
			match dialog.plan(&data, &mut s.settings) {
				Ok(plan) => {
					// asked in order, popped from the end
					let mut outputs = plan.existing();
					outputs.reverse();
					s.replacing = Some((plan, outputs));
					ask_replace(s, app);
				}
				Err(e) => {
					data.error = e.into();
					ui_export.set_data(data);
				}
			}
		});
	});
}
