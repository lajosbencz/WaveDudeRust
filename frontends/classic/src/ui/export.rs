//! The export dialog (docs/ui/70, Export Save dialog): format and outputs, then the export, whose course is the
//! shell's. Natively each output is a path, typed or browsed; in the browser a file name.

use std::borrow::Cow;

use eframe::egui::{self, Align, Id, Key, Layout, RichText, Ui};
use waveduderust::model::{Layer, Word};
use waveduderust::unstable::export::h264::Profile;
use waveduderust_app::export::session::{ExportForm, Notes, ProfileRow, Step, Target};
use waveduderust_app::modal::Reply;
use waveduderust_app::strings::Strings;
use waveduderust_app::{App, Event};

use super::modal::{Answer, Then};
#[cfg(not(target_arch = "wasm32"))]
use super::properties;
use super::properties::{Choices, check_box, combo};
use super::text_box::{Kind, text_box};
use crate::kit::dialog::{Choice, buttons};
use crate::kit::{INPUT_HEIGHT, Palette};

pub(super) const WIDTH: f32 = 528.;
/// Width of a name box's label.
const LABEL: f32 = 80.;
#[cfg(not(target_arch = "wasm32"))]
const BROWSE: f32 = 28.;

/// The dialog of `form`; events it sends go to `out`, what the window does while it stays (a browse) to `stays`.
pub(super) fn dialog(
	ui: &mut Ui,
	fresh: bool,
	form: &mut ExportForm,
	app: &App,
	out: &mut Vec<Event>,
	stays: &mut Option<Then>,
) -> Option<Answer> {
	let s = &app.strings;
	let title = s.get(form.title_key());
	// Enter in a name box, which takes the key
	let enter = ui.input(|i| i.key_pressed(Key::Enter));
	let formats = form.formats();
	if !formats.is_empty() {
		let shown = &*form;
		let word = |i: usize| Cow::Borrowed(shown.format_name(i));
		let disabled = |i: usize| shown.unavailable(app, i).map(|key| s.get(key));
		let choices = Choices {
			len: formats.len(),
			current: Some(shown.format),
			shown: shown.format_name(shown.format),
			word: &word,
			swatch: None,
			disabled: Some(&disabled),
			actions: &[],
		};
		if let Some(i) = combo(ui, Id::unique("ExportFormat"), title, &choices) {
			form.set_format(i);
		}
	}
	let typing = name_boxes(ui, form, app, title, out, stays);
	let plan = form.plan(app);
	let notes = form.notes(app);
	show_notes(ui, form, app, &notes, out);
	let ready = notes.ready && plan.is_some();
	let choices =
		[Choice { enabled: ready, ..Choice::default(s.get("DialogOk")) }, Choice::cancel(s.get("DialogCancel"))];
	let pressed = buttons(ui, fresh, &choices).or((enter && typing && ready).then_some(0));
	pressed.map(|i| plan.filter(|_| i == 0).map(|plan| Then::Reply(Reply::Export(Step::Chosen(plan)))))
}

/// The name boxes: each output's, labelled by its layer while there are several boxes, then, for an export of several
/// files, the archive's check box and, while checked, the archive's box; whether a box has the focus.
fn name_boxes(
	ui: &mut Ui,
	form: &mut ExportForm,
	app: &App,
	title: &str,
	out: &mut Vec<Event>,
	stays: &mut Option<Then>,
) -> bool {
	let s = &app.strings;
	let archived = form.archived(app);
	let labelled = form.outputs.len() > 1 || archived;
	let mut typing = false;
	for i in 0..form.outputs.len() {
		let label = form.layers().get(i).filter(|_| labelled).map(|layer| s.enum_word(Layer::TYPE_NAME, layer.name()));
		typing |= name_row(ui, form, Target::Output(i), label, label.unwrap_or(title), stays);
	}
	if form.writes_many() {
		let text = s.get("ExportArchive");
		let picked = ui
			.add_enabled_ui(ExportForm::archive_chosen(), |ui| {
				check_box(ui, Id::unique("ExportArchive"), text, archived)
			})
			.inner;
		if let Some(on) = picked {
			out.push(Event::SetExportArchive(on));
		}
		if archived {
			let label = s.get("ExportArchiveName");
			typing |= name_row(ui, form, Target::Archive, Some(label), label, stays);
		}
	}
	typing
}

/// The row of the name box of `target`: `label` left of it when shown, the platform's browse button right of it;
/// `name` names the box for assistive technology. Whether the box has the focus.
fn name_row(
	ui: &mut Ui,
	form: &mut ExportForm,
	target: Target,
	label: Option<&str>,
	name: &str,
	stays: &mut Option<Then>,
) -> bool {
	let id = Id::unique("ExportPath").with(target);
	let mut field = |ui: &mut Ui| {
		let row = egui::vec2(ui.available_width(), INPUT_HEIGHT);
		ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
			ui.spacing_mut().item_spacing.x = 4.;
			browse_button(ui, target, stays);
			let kind = Kind { live: true, ..Kind::default() };
			let edited = text_box(ui, id, form.text_mut(target), ui.available_width(), kind, name);
			if let Some(typed) = edited.applied {
				*form.text_mut(target) = typed;
			}
			edited.response.has_focus()
		})
		.inner
	};
	match label {
		Some(label) => crate::kit::form::labelled(ui, label, LABEL, Some(id), field),
		None => field(ui),
	}
}

/// The save dialog's button of the box of `target`, at the path typed: the chosen format's, or the archive's.
#[cfg(not(target_arch = "wasm32"))]
fn browse_button(ui: &mut Ui, target: Target, stays: &mut Option<Then>) {
	if ui.add(egui::Button::new("...").min_size(egui::vec2(BROWSE, 0.))).clicked() {
		*stays = Some(Then::Reply(Reply::Browse(target)));
	}
}

/// Nothing: the browser has no browse dialog.
#[cfg(target_arch = "wasm32")]
fn browse_button(_: &mut Ui, _: Target, _: &mut Option<Then>) {}

/// What [`ExportForm::notes`] lists: MP4's profile, the codec's notice, consent and deletion, why the export cannot
/// start.
#[cfg_attr(target_arch = "wasm32", expect(unused_variables, reason = "the browser downloads no codec"))]
fn show_notes(ui: &mut Ui, form: &mut ExportForm, app: &App, notes: &Notes, out: &mut Vec<Event>) {
	let s = &app.strings;
	let dim = Palette::of(ui.ctx().theme()).text_dim;
	if let Some(row) = &notes.profile
		&& let Some(profile) = profile_row(ui, s, row)
	{
		out.push(Event::SetH264Profile(profile));
	}
	#[cfg(not(target_arch = "wasm32"))]
	if let Some(enabled) = notes.openh264 {
		ui.add(egui::Label::new(RichText::new(waveduderust::unstable::export::h264::NOTICE).color(dim)).wrap());
		if let Some(on) = check_box(ui, Id::unique("ExportUseOpenH264"), s.get("ExportUseOpenH264"), enabled) {
			out.push(Event::SetUseOpenH264(on));
		}
	}
	#[cfg(not(target_arch = "wasm32"))]
	if notes.delete_codec && properties::button(ui, s.get("ExportDeleteOpenH264"), true).clicked() {
		form.delete_codec(app);
	}
	if let Some(key) = notes.blocked {
		ui.add(egui::Label::new(RichText::new(s.get(key)).color(dim)).wrap());
	}
}

/// The H.264 profile row: every profile, those not offered greyed with the reason, which also shows below while the
/// current one is among them; the profile picked.
fn profile_row(ui: &mut Ui, s: &Strings, row: &ProfileRow) -> Option<Profile> {
	let reason = s.get(row.reason);
	let picked = ui
		.horizontal(|ui| {
			ui.label(s.get("ExportH264Profile"));
			profile_combo(ui, Id::unique("ExportH264Profile"), s, row.current, &row.supported, reason)
		})
		.inner;
	profile_note(ui, s, row);
	picked
}

/// The H.264 profile drop-down: every profile, those not in `supported` greyed with `reason`; the profile picked.
pub(super) fn profile_combo(
	ui: &mut Ui,
	id: Id,
	s: &Strings,
	current: Profile,
	supported: &[Profile],
	reason: &str,
) -> Option<Profile> {
	let name = |p: Profile| s.get(waveduderust_app::export::profile_key(p));
	let word = |i: usize| Cow::Borrowed(name(Profile::ALL[i]));
	let disabled = |i: usize| (!supported.contains(&Profile::ALL[i])).then_some(reason);
	let choices = Choices {
		len: Profile::ALL.len(),
		current: Profile::ALL.iter().position(|&p| p == current),
		shown: name(current),
		word: &word,
		swatch: None,
		disabled: Some(&disabled),
		actions: &[],
	};
	combo(ui, id, s.get("ExportH264Profile"), &choices).map(|i| Profile::ALL[i])
}

/// [`ProfileRow::note`], dimmed.
pub(super) fn profile_note(ui: &mut Ui, s: &Strings, row: &ProfileRow) {
	if let Some(note) = row.note() {
		let dim = Palette::of(ui.ctx().theme()).text_dim;
		ui.add(egui::Label::new(RichText::new(s.get(note)).color(dim)).wrap());
	}
}
