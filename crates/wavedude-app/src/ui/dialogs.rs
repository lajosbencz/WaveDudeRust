//! The in-window modals (the message window's look: a disc, the text, buttons at the bottom right), the
//! state's responses that open them, and the platform file dialogs (`rfd`).

use std::path::{Path, PathBuf};
use std::rc::Rc;

use freya::prelude::*;
use wavedude_widgets::use_wave_theme;

use wavedude_app_state::strings::{Strings, mnemonic, strip_mnemonic};
use wavedude_app_state::{Event as AppEvent, Guarded, Response, SaveChoice};

use super::app::{Cx, Ui};
use super::export_dialog::{self, ExportDialog, ExportRequest};
use super::{about, memory_warning};

/// The modal shown over the window.
#[derive(Clone)]
pub enum Modal {
	/// "Save the changes to {name}?"
	AskSave {
		name: String,
	},
	/// One line; the detail is in the log.
	Error(String),
	/// A notice with OK.
	Info(String),
	/// The crash report an earlier session left, with its path to copy.
	Crash(PathBuf),
	/// The first start's page: preview quality, MP4's codec, updates.
	Startup,
	MemoryWarning,
	About,
	Export(ExportRequest),
	/// A yes/no question; `yes` runs on Yes.
	Confirm {
		text: String,
		yes: Rc<dyn Fn()>,
	},
}

/// Acts on what the state asks for after an event.
pub fn respond(cx: &Cx, response: Response) {
	match response {
		Response::None => {}
		Response::AskSave { name } => cx.ui.show_modal(Modal::AskSave { name }),
		Response::AskSavePath { proposed } => save_as(cx, proposed),
		Response::Clipboard(text) => {
			if let Err(e) = Clipboard::set(text) {
				tracing::warn!("the clipboard could not be written: {e:?}");
			}
		}
		Response::MemoryWarning => cx.ui.show_modal(Modal::MemoryWarning),
		Response::Error(message) => cx.ui.show_modal(Modal::Error(message)),
		Response::Exit => quit(cx, false),
		Response::Restart => quit(cx, true),
	}
}

/// Ends the program, as the restart into a downloaded update when `restart`. A running export is stopped first on
/// the user's word, and the exit waits for it to end.
fn quit(cx: &Cx, restart: bool) {
	let finish = move |cx: &Cx| {
		if restart && !cx.info.update.confirm_restart() {
			tracing::warn!("a restart without a downloaded update");
		}
		cx.exit();
	};
	if cx.ui.export.peek().is_none() {
		finish(cx);
		return;
	}
	let stop_cx = cx.clone();
	let yes = Rc::new(move || export_dialog::stop_then(&stop_cx, finish));
	cx.ui.show_modal(Modal::Confirm { text: cx.strings().get("ExitStopsExport").to_owned(), yes });
}

/// The filters of a string table's `Name (*.a)|*.a;*.b|...` list for `rfd`; the tables are ours, so a malformed
/// list is a bug.
pub fn filters(spec: &str) -> Vec<(String, Vec<String>)> {
	let parts: Vec<&str> = spec.split('|').collect();
	parts
		.chunks(2)
		.map(|pair| {
			let [name, patterns] = pair else { panic!("the filter list '{spec}' pairs each name with patterns") };
			let ext = |p: &str| {
				let (_, e) = p.trim().rsplit_once('.').unwrap_or_else(|| panic!("the filter '{p}' has no extension"));
				e.to_owned()
			};
			((*name).to_owned(), patterns.split(';').map(ext).collect())
		})
		.collect()
}

/// The scene image filter: the string table's name over the extensions this build decodes.
pub fn scene_image_filter(s: &Strings) -> String {
	let exts: Vec<String> = wavedude_core::io::image_extensions().iter().map(|e| format!("*.{e}")).collect();
	let (name, rest) = s.get("SceneImageFilter").split_once('|').expect("the filter names the images first");
	let all = rest.split_once('|').map_or("", |(_, all)| all);
	format!("{name}|{}|{all}", exts.join(";"))
}

fn dialog(spec: &str, start: Option<&Path>) -> rfd::AsyncFileDialog {
	let mut d = rfd::AsyncFileDialog::new();
	for (name, exts) in filters(spec) {
		d = d.add_filter(name, &exts);
	}
	if let Some(start) = start {
		if start.is_dir() {
			d = d.set_directory(start);
		} else {
			if let Some(dir) = start.parent().filter(|p| p.is_dir()) {
				d = d.set_directory(dir);
			}
			if let Some(name) = start.file_name() {
				d = d.set_file_name(name.to_string_lossy());
			}
		}
	}
	d
}

/// Shows a platform Open dialog and calls `then` with the picked file.
pub fn pick_file(spec: &str, start: Option<PathBuf>, then: impl FnOnce(PathBuf) + 'static) {
	let d = dialog(spec, start.as_deref());
	spawn_in_window(async move {
		if let Some(f) = d.pick_file().await {
			then(f.path().to_owned());
		}
	});
}

/// Shows a platform Save dialog and calls `then` with the chosen path, or `None` when cancelled.
pub fn save_file(spec: &str, proposed: Option<PathBuf>, then: impl FnOnce(Option<PathBuf>) + 'static) {
	let d = dialog(spec, proposed.as_deref());
	spawn_in_window(async move {
		then(d.save_file().await.map(|f| f.path().to_owned()));
	});
}

/// File > Open: asks about unsaved edits after the pick.
pub fn open_project(cx: &Cx) {
	let s = cx.strings();
	let start = cx.shared.peek(|a| a.document.folder().map(Path::to_owned));
	let cx = cx.clone();
	pick_file(s.get("OpenFileDialogFilter"), start, move |path| cx.send(AppEvent::Guarded(Guarded::Open(path))));
}

/// Save As, from the menu or the state's request; a cancel aborts a pending action.
pub fn save_as(cx: &Cx, proposed: Option<PathBuf>) {
	let s = cx.strings();
	let proposed = proposed.or_else(|| cx.shared.peek(|a| a.document.proposed_save_path()));
	let cx = cx.clone();
	save_file(s.get("SaveFileDialogFilter"), proposed, move |path| match path {
		Some(p) => cx.send(AppEvent::SaveAs(p)),
		None => cx.send(AppEvent::SaveAsCancelled),
	});
}

#[derive(Clone, Copy, PartialEq)]
enum Disc {
	Error,
	Question,
	Info,
}

/// One button of a message window: its label (with mnemonic), what it does, whether Enter or Esc pick it.
struct DialogButton {
	text: String,
	action: Rc<dyn Fn()>,
	default: bool,
	cancel: bool,
}

/// The modal host: shows [`Ui::modal`] over the window.
#[derive(PartialEq)]
pub struct ModalHost;

impl Component for ModalHost {
	fn render(&self) -> impl IntoElement {
		let ui = consume_context::<Ui>();
		let modal = ui.modal.read().clone();
		let Some(modal) = modal else {
			return rect().into_element();
		};
		let cx = Cx::get();
		let s = cx.strings();
		let body = match modal {
			Modal::AskSave { name } => message(&s, Disc::Question, s.format("SaveChangesFormat", &[&name]), {
				let pick = |choice: SaveChoice| {
					let cx = cx.clone();
					Rc::new(move || {
						cx.ui.close_modal();
						cx.send(AppEvent::SaveChoice(choice));
					}) as Rc<dyn Fn()>
				};
				vec![
					DialogButton {
						text: s.get("DialogSave").into(),
						action: pick(SaveChoice::Save),
						default: true,
						cancel: false,
					},
					DialogButton {
						text: s.get("DialogDiscard").into(),
						action: pick(SaveChoice::DontSave),
						default: false,
						cancel: false,
					},
					DialogButton {
						text: s.get("DialogCancel").into(),
						action: pick(SaveChoice::Cancel),
						default: false,
						cancel: true,
					},
				]
			}),
			Modal::Error(text) => notice(&s, ui, Disc::Error, text),
			Modal::Info(text) => notice(&s, ui, Disc::Info, text),
			Modal::Crash(report) => crash(&s, ui, &report),
			Modal::Startup => super::startup::StartupPage.into_element(),
			Modal::Confirm { text, yes } => {
				let no = Rc::new(move || ui.close_modal()) as Rc<dyn Fn()>;
				let yes = Rc::new(move || {
					ui.close_modal();
					yes();
				}) as Rc<dyn Fn()>;
				message(
					&s,
					Disc::Question,
					text,
					vec![
						DialogButton { text: s.get("DialogYes").into(), action: yes, default: true, cancel: false },
						DialogButton { text: s.get("DialogNo").into(), action: no, default: false, cancel: true },
					],
				)
			}
			Modal::MemoryWarning => memory_warning::MemoryWarningDialog.into_element(),
			Modal::About => about::AboutDialog.into_element(),
			Modal::Export(request) => ExportDialog { request }.into_element(),
		};
		backdrop(body)
	}
}

/// The dimmed window behind a modal, the modal centred on it.
pub fn backdrop(body: Element) -> Element {
	rect()
		.layer(Layer::Overlay)
		.position(Position::new_global().left(0.).top(0.))
		.width(Size::window_percent(100.))
		.height(Size::window_percent(100.))
		.background(Color::from_argb(110, 0, 0, 0))
		.center()
		.on_pointer_down(|e: Event<PointerEventData>| e.stop_propagation())
		.child(body)
		.into_element()
}

/// A dialog frame in the theme: border, background, padding, a fixed width.
pub fn frame(width: f32) -> Rect {
	let theme = use_wave_theme();
	rect()
		.width(Size::px(width))
		.max_height(Size::window_percent(95.))
		.background(theme.tool_window_background)
		.color(theme.text)
		.font_size(theme.font_size)
		.border(Border::new().fill(theme.border).width(1.).alignment(BorderAlignment::Inner))
		.padding(16.)
		.spacing(12.)
}

/// A row of buttons at the bottom right.
pub fn button_row(buttons: Vec<Element>) -> Rect {
	rect().width(Size::fill()).horizontal().main_align(Alignment::end()).spacing(8.).children(buttons)
}

/// A dialog button with its mnemonic stripped.
pub fn button(text: &str, enabled: bool, on_press: impl FnMut(Event<PressEventData>) + 'static) -> Element {
	Button::new().enabled(enabled).on_press(on_press).child(label().text(strip_mnemonic(text))).into_element()
}

/// A message with OK.
fn notice(s: &Strings, ui: Ui, disc: Disc, text: String) -> Element {
	let close = Rc::new(move || ui.close_modal()) as Rc<dyn Fn()>;
	message(
		s,
		disc,
		text,
		vec![DialogButton { text: s.get("DialogOk").into(), action: close, default: true, cancel: true }],
	)
}

fn crash(s: &Strings, ui: Ui, report: &Path) -> Element {
	let path = report.display().to_string();
	let copied = s.get("CrashPathCopied").to_owned();
	let text = s.format("CrashReportFormat", &[&path]);
	let copy = Rc::new(move || match Clipboard::set(path.clone()) {
		Ok(()) => tracing::info!("{copied}"),
		Err(e) => tracing::warn!("the clipboard could not be written: {e:?}"),
	}) as Rc<dyn Fn()>;
	let close = Rc::new(move || ui.close_modal()) as Rc<dyn Fn()>;
	message(
		s,
		Disc::Error,
		text,
		vec![
			DialogButton { text: s.get("CrashCopyPath").into(), action: copy, default: false, cancel: false },
			DialogButton { text: s.get("DialogOk").into(), action: close, default: true, cancel: true },
		],
	)
}

/// A button's mnemonic, whether Enter and Esc pick it, and its action.
type KeyBinding = (Option<char>, bool, bool, Rc<dyn Fn()>);

fn message(s: &Strings, disc: Disc, text: String, buttons: Vec<DialogButton>) -> Element {
	let theme = use_wave_theme();
	let (fill, glyph) = match disc {
		Disc::Error => (theme.status_error, "!"),
		Disc::Question => (theme.accent, "?"),
		Disc::Info => (theme.accent, "i"),
	};
	let _ = s;
	let keys: Vec<KeyBinding> =
		buttons.iter().map(|b| (mnemonic(&b.text), b.default, b.cancel, b.action.clone())).collect();
	let on_key = move |e: Event<KeyboardEventData>| {
		let pick = |f: &dyn Fn(&KeyBinding) -> bool| keys.iter().find(|k| f(k)).map(|k| k.3.clone());
		let action = match &e.key {
			Key::Named(NamedKey::Enter) => pick(&|k| k.1),
			Key::Named(NamedKey::Escape) => pick(&|k| k.2),
			Key::Character(c) if !e.modifiers.contains(Modifiers::CONTROL) => {
				let letter = c.chars().next().and_then(|c| c.to_lowercase().next());
				pick(&|k| k.0.is_some() && k.0 == letter)
			}
			_ => None,
		};
		if let Some(a) = action {
			e.stop_propagation();
			a();
		}
	};
	let views: Vec<Element> = buttons
		.into_iter()
		.map(|b| {
			let action = b.action.clone();
			button(&b.text, true, move |_| action())
		})
		.collect();
	frame(440.)
		.on_global_key_down(on_key)
		.child(
			rect()
				.horizontal()
				.spacing(12.)
				.width(Size::fill())
				.child(
					rect()
						.width(Size::px(32.))
						.height(Size::px(32.))
						.corner_radius(16.)
						.background(fill)
						.center()
						.child(label().text(glyph).color(Color::WHITE).font_size(18.).font_weight(FontWeight::BOLD)),
				)
				.child(paragraph().width(Size::flex(1.)).span(Span::new(text))),
		)
		.child(button_row(views))
		.into_element()
}
