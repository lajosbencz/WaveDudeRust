//! The Properties form's callbacks, and the number boxes' and search boxes' logic.

use std::cell::RefCell;
use std::rc::Rc;

use slint::ComponentHandle;

use super::Handle;
#[cfg(not(target_arch = "wasm32"))]
use crate::adopt_image;
use crate::document::MergeKey;
use crate::state::State;
use crate::tree::Node;
use crate::ui::{self, AppWindow};
use crate::{files, form, models, number};

/// The window-wide logic the boxes call, which needs no state.
pub(crate) fn wire_widgets(app: &AppWindow) {
	let form_ui = app.global::<ui::Form<'_>>();
	let numbers = app.global::<ui::NumberLogic<'_>>();
	numbers.on_step(|spec, text, steps| number::step(&spec, &text, steps).into());
	numbers.on_scrub(|spec, start, dx, scale| number::scrub(&spec, &start, f64::from(dx), f64::from(scale)).into());
	form::wire_colors(&app.global::<ui::ColorLogic<'_>>());
	form_ui.set_pick_files(cfg!(not(target_arch = "wasm32")));
	form_ui.on_next_enabled(|enabled, count, from, step| {
		let enabled: Vec<bool> = slint::Model::iter(&enabled).collect();
		form::next_enabled(&enabled, usize::try_from(count).unwrap_or_default(), from, step)
	});
	app.global::<ui::Search<'_>>()
		.on_matches(|text, query| text.to_lowercase().contains(query.trim().to_lowercase().as_str()));
	let parse = app.global::<ui::NumberParse<'_>>();
	parse.on_int(|spec, text| {
		let value = number::commit(&spec, &text);
		// `as` saturates: the box's f32 bounds round i32::MAX up a step
		ui::ParsedInt { ok: value.is_some(), value: value.unwrap_or_default() as i32 }
	});
	parse.on_float(|spec, text| {
		let value = number::commit(&spec, &text);
		ui::ParsedFloat { ok: value.is_some(), value: value.unwrap_or_default() as f32 }
	});
}

/// The Properties form: edits, list rows, the gradient bar, colour schemes and the scene image.
pub(crate) fn wire_form(state: &Rc<RefCell<State>>, app: &AppWindow) {
	let form_ui = app.global::<ui::Form<'_>>();
	let h = Handle::new(state, app);
	form_ui.on_edited(move |typing| {
		h.show(|s, app| {
			let form = app.global::<ui::Form<'_>>();
			form.set_invalid("".into());
			s.edit_form(&form, typing);
		});
	});
	let st = state.clone();
	form_ui.on_describe(move |open| {
		let s = &mut *st.borrow_mut();
		s.settings.part_description_expanded = open;
		s.save_settings();
	});
	let h = Handle::new(state, app);
	form_ui.on_new_seed(move || {
		h.show(|s, _| {
			let seed = waveduderust::model::Project::new_random_seed().seed;
			s.doc.edit(|p| p.seed = seed);
		});
	});
	let edit_rows = {
		let h = Handle::new(state, app);
		move |edit: form::RowEdit| {
			h.show(|s, _| {
				let node = s.selection.current();
				s.doc.edit(|p| form::edit_rows(p, node, edit));
			});
		}
	};
	let index = |k: i32| models::index_from_ui(k).unwrap_or(usize::MAX);
	form_ui.on_add_row({
		let edit = edit_rows.clone();
		move || edit(form::RowEdit::Add)
	});
	form_ui.on_remove_row({
		let edit = edit_rows.clone();
		move |k| edit(form::RowEdit::Remove(index(k)))
	});
	form_ui.on_move_row({
		let edit = edit_rows.clone();
		move |from, to| edit(form::RowEdit::Move { from: index(from), to: index(to) })
	});
	form_ui.on_clear_rows(move || edit_rows(form::RowEdit::Clear));
	// the gradient bar and the stop swatches: each edit on the scheme shown, the stop then selected handed back
	let bar_edit = {
		let (st, weak) = (state.clone(), app.as_weak());
		move |gesture: i32, edit: &dyn Fn(&mut waveduderust::model::ColorScheme) -> i32| {
			let Some(app) = weak.upgrade() else { return -1 };
			let mut s = st.borrow_mut();
			let Node::Scheme(i) = s.selection.current() else { return -1 };
			let mut selected = -1;
			let key = (gesture != 0).then_some(MergeKey::Stops(gesture));
			s.typed_form.bar_edited = true;
			s.doc.edit_merging(key, |p| {
				if let Some(scheme) = p.color_schemes.get_mut(i) {
					selected = edit(scheme);
				}
			});
			s.show(&app);
			selected
		}
	};
	form_ui.on_add_stop_at({
		let edit = bar_edit.clone();
		move |at, gesture| edit(gesture, &|s| models::index_to_ui(Some(s.add_stop_at(form::thousandths(at)))))
	});
	form_ui.on_move_stop({
		let edit = bar_edit.clone();
		move |k, at, gesture| {
			edit(gesture, &|s| {
				let moved = models::index_from_ui(k).and_then(|k| s.move_stop(k, form::thousandths(at)));
				moved.map_or(k, |k| models::index_to_ui(Some(k)))
			})
		}
	});
	form_ui.on_remove_stop({
		let edit = bar_edit.clone();
		move |k, gesture| edit(gesture, &|s| models::index_from_ui(k).map_or(k, |k| form::remove_stop(s, k)))
	});
	form_ui.on_recolour_stop(move |k, colour, gesture| {
		let colour = form::rgb(colour);
		bar_edit(gesture, &|s| {
			if let Some(stop) = models::index_from_ui(k).and_then(|k| s.stops.get_mut(k)) {
				stop.color = colour;
			}
			k
		});
	});
	let h = Handle::new(state, app);
	form_ui.on_load_scheme_file(move |layer| {
		h.read(|s, app| files::pick_scheme(&s.inbox, &app.global::<ui::Words<'_>>(), layer));
	});
	#[cfg(not(target_arch = "wasm32"))]
	{
		let (st, weak) = (state.clone(), app.as_weak());
		form_ui.on_pick_image(move || {
			let Some(app) = weak.upgrade() else { return };
			let words = app.global::<ui::Words<'_>>();
			let picked = rfd::FileDialog::new()
				.set_title(words.get_pick_scene_image().as_str())
				.add_filter(words.get_image_files().as_str(), &waveduderust::io::image_extensions())
				.pick_file();
			if let Some(path) = picked {
				let s = &mut *st.borrow_mut();
				adopt_image(s, &path);
				s.show(&app);
			}
		});
		let st = state.clone();
		form_ui.on_save_image(move || {
			let (Some(bytes), name) = ({
				let scene = &st.borrow().doc.project.scene;
				(scene.image_data.clone(), scene.image.clone())
			}) else {
				return;
			};
			if let Some(path) = rfd::FileDialog::new().set_file_name(&name).save_file() {
				match std::fs::write(&path, &bytes) {
					Ok(()) => tracing::info!("Saved {}", path.display()),
					Err(e) => tracing::error!("cannot save {}: {e}", path.display()),
				}
			}
		});
	}
	let h = Handle::new(state, app);
	form_ui.on_drop_image(move || h.show(|s, _| s.doc.edit(|p| p.scene.remove_image())));
	let h = Handle::new(state, app);
	form_ui.on_toggle_mine(move || h.show(|s, _| s.toggle_mine()));
	let h = Handle::new(state, app);
	form_ui.on_remove_my_scheme(move |layer| {
		h.with(|s, app| {
			let Some(store) = &s.store else { return };
			let name = form::layer_settings(&mut s.doc.view, layer).color_scheme.clone();
			if waveduderust_platform::user_schemes::remove(store, &name).is_ok() {
				tracing::info!("Colour scheme '{name}' removed from my schemes");
			}
			s.schemes = waveduderust_platform::user_schemes::load(store);
			s.show(app);
		});
	});
	let h = Handle::new(state, app);
	form_ui.on_edit_scheme_copy(move |layer| {
		h.with(|s, app| {
			let name = form::layer_settings(&mut s.doc.view, layer).color_scheme.clone();
			let Some((scheme, own)) = form::find_scheme(&s.doc.project, &s.schemes, &name) else { return };
			let mut copy = scheme.clone();
			if own {
				copy.description = format!("copy of project scheme '{}'", scheme.name);
			}
			copy.name = format!("{} copy", scheme.name);
			let mut at = 0;
			s.doc.edit_both(None, |p, v| {
				p.add_color_scheme(copy);
				at = p.color_schemes.len() - 1;
				form::layer_settings(v, layer).color_scheme.clone_from(&p.color_schemes[at].name);
			});
			tracing::info!(
				"Colour scheme '{}' copied into the project for editing",
				s.doc.project.color_schemes[at].name
			);
			s.selection.set(Node::Scheme(at));
			s.show(app);
		});
	});

	let weak = app.as_weak();
	form_ui.on_marker_at(move |at, reach, selected| {
		weak.upgrade().map_or(-1, |app| form::marker_at(&app.global::<ui::Form<'_>>().get_stops(), at, reach, selected))
	});
	let weak = app.as_weak();
	form_ui.on_stop_beside(move |k, step| {
		weak.upgrade().map_or(-1, |app| form::stop_beside(&app.global::<ui::Form<'_>>().get_stops(), k, step))
	});
	let h = Handle::new(state, app);
	form_ui.on_base_on(move |picked| {
		h.show(|s, app| {
			if let (Node::Scheme(i), Ok(k)) = (s.selection.current(), usize::try_from(picked)) {
				let base = form::bases(&s.doc.project, &s.schemes, i).get(k).map(|&b| b.clone());
				if let Some(base) = base {
					s.doc.edit(|p| {
						if let Some(scheme) = p.color_schemes.get_mut(i) {
							form::base_on(scheme, &base);
						}
					});
					app.global::<ui::Form<'_>>().set_based_on(picked);
				}
			}
		});
	});
}
