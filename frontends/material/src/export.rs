//! The export dialog (docs/ui/70, Export Save dialog) over [`ExportForm`]; the exports' course is
//! [`waveduderust_app::shell::ShellState`]'s.

use egui_mineral::widget::SizeExt as _;
#[cfg(not(target_arch = "wasm32"))]
use egui_mineral_material::prelude::{ButtonVariant, IconButton, PushButton, icons};
use egui_mineral_material::prelude::{
	CheckButton, Elide, Select, SelectOption, TextField, TextFieldVariant, TypeScale,
};
use waveduderust::model::{Layer, Word as _};
use waveduderust::unstable::export::h264::Profile;
use waveduderust_app::Event;
use waveduderust_app::export::profile_key;
use waveduderust_app::export::session::{ExportForm, Notes, Target};

use crate::{App, Mui, Salt, form};

const ROW_GAP: f32 = 12.;

/// What the dialog's content asks for this pass.
pub(crate) enum Asked {
	Send(Event),
	/// The save dialog of the box of this target.
	#[cfg(not(target_arch = "wasm32"))]
	Browse(Target),
}

/// The export dialog's content over `form`: format, a name box per output, the archive, MP4's notes; what it asks
/// for.
pub(crate) fn dialog(ui: &mut Mui<'_>, form: &mut ExportForm, app: &App) -> Option<Asked> {
	let rt = ui.rt();
	ui.scope(|ui| {
		// room for each outlined box's floating label
		ui.spacing_mut().item_spacing.y = ROW_GAP;
		rows(&mut rt.ui(ui), form, app)
	})
	.inner
}

/// The format, the name boxes, the archive and the notes.
fn rows(ui: &mut Mui<'_>, form: &mut ExportForm, app: &App) -> Option<Asked> {
	let s = &app.strings;
	let w = ui.available_width();
	let mut asked = None;
	let formats = form.formats();
	if !formats.is_empty() {
		let options: Vec<SelectOption> = (0..formats.len())
			.map(|i| SelectOption::new(form.format_name(i)).disabled(form.unavailable(app, i).is_some()))
			.collect();
		let mut picked = Some(form.format);
		let select = Select::optional("export-format", s.get("ExportFileType"), &mut picked)
			.items(options.into_iter().enumerate());
		ui.mineral(select.preferred_width(w));
		if let Some(i) = picked.filter(|&i| i != form.format) {
			form.set_format(i);
		}
	}
	let archived = form.archived(app);
	let labelled = form.outputs.len() > 1 || archived;
	let title = s.get(form.title_key());
	for i in 0..form.outputs.len() {
		let label = form
			.layers()
			.get(i)
			.filter(|_| labelled)
			.map_or(title, |layer: &Layer| s.enum_word(Layer::TYPE_NAME, layer.name()));
		asked = asked.or(name_row(ui, form, Target::Output(i), label));
	}
	if form.writes_many() {
		let mut on = archived;
		let check = CheckButton::new("export-archive", s.get("ExportArchive"), &mut on);
		ui.mineral(check.disabled(!ExportForm::archive_chosen()));
		if on != archived {
			asked = Some(Asked::Send(Event::SetExportArchive(on)));
		}
		if archived {
			asked = asked.or(name_row(ui, form, Target::Archive, s.get("ExportArchiveName")));
		}
	}
	let shown = form.notes(app);
	asked.or(notes(ui, form, app, &shown))
}

/// The name box of `target` labelled `label`, natively with its browse button.
fn name_row(ui: &mut Mui<'_>, form: &mut ExportForm, target: Target, label: &str) -> Option<Asked> {
	let rt = ui.rt();
	let browse = if cfg!(target_arch = "wasm32") { 0. } else { 48. };
	let w = ui.available_width() - browse;
	let text = form.text_mut(target);
	ui.horizontal(|ui| {
		let ui = &mut rt.ui(ui);
		let field = TextField::new(("export-path", Salt(target)), label, text)
			.variant(TextFieldVariant::Outlined)
			.elide(Elide::Start);
		ui.mineral(field.preferred_width(w));
		#[cfg(not(target_arch = "wasm32"))]
		{
			let button = IconButton::new(("export-browse", Salt(target)), icons::ICON_FOLDER_OPEN, label);
			if ui.mineral(button).clicked() {
				return Some(Asked::Browse(target));
			}
		}
		None
	})
	.inner
}

/// What [`ExportForm::notes`] lists: MP4's profile, the codec's notice, consent and deletion, why the export
/// cannot start.
#[cfg_attr(target_arch = "wasm32", expect(unused_variables, reason = "the browser deletes no codec"))]
fn notes(ui: &mut Mui<'_>, form: &mut ExportForm, app: &App, notes: &Notes) -> Option<Asked> {
	let s = &app.strings;
	let mut asked = None;
	if let Some(row) = &notes.profile {
		let options: Vec<SelectOption> = Profile::ALL
			.iter()
			.map(|p| SelectOption::new(s.get(profile_key(*p))).disabled(!row.supported.contains(p)))
			.collect();
		let at = Profile::ALL.iter().position(|&p| p == row.current);
		let mut picked = at;
		let select = Select::optional("export-profile", s.get("ExportH264Profile"), &mut picked)
			.items(options.into_iter().enumerate());
		let w = ui.available_width();
		ui.mineral(select.preferred_width(w));
		if let Some(i) = picked.filter(|_| picked != at) {
			asked = Some(Asked::Send(Event::SetH264Profile(Profile::ALL[i])));
		}
		if let Some(note) = row.note() {
			form::text(ui, s.get(note), TypeScale::BodySmall, true);
		}
	}
	#[cfg(not(target_arch = "wasm32"))]
	if let Some(enabled) = notes.openh264 {
		form::text(ui, waveduderust::unstable::export::h264::NOTICE, TypeScale::BodySmall, true);
		let mut on = enabled;
		ui.mineral(CheckButton::new("export-openh264", s.get("ExportUseOpenH264"), &mut on));
		if on != enabled {
			asked = Some(Asked::Send(Event::SetUseOpenH264(on)));
		}
	}
	#[cfg(not(target_arch = "wasm32"))]
	if notes.delete_codec {
		let button = PushButton::new("export-delete-codec", s.get("ExportDeleteOpenH264")).variant(ButtonVariant::Text);
		if ui.mineral(button).clicked() {
			form.delete_codec(app);
		}
	}
	if let Some(key) = notes.blocked {
		form::text(ui, s.get(key), TypeScale::BodySmall, true);
	}
	asked
}
