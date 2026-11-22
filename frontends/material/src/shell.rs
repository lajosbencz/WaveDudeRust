//! The window over [`App`] (DESIGN.md, State wiring): each pass takes the tasks' answers and the fired commands,
//! ticks the state, draws the top bar, the status line, the workspace or the page over it, the modal, the palette and
//! the snackbars, then applies what they asked for in order.

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{
	self, Align, CentralPanel, Color32, Frame, Key, KeyboardShortcut, Layout, Modifiers, Panel, Ui, UiBuilder, vec2,
};
use egui_mineral::nav::Router;
use egui_mineral::runtime::InitError;
use egui_mineral::shortcut;
use egui_mineral::toast::ToastQueue;
use egui_mineral_material::adaptive::WindowClass;
use egui_mineral_material::prelude::{
	CommandPalette, ContainerVariant, Dialog, IconAction, IconButton, MaterialExt as _, Menu, Rt, SideSheet, Snackbar,
	SnackbarHost, SplitButton, TopAppBar, TypeScale, icons, snackbar_queue,
};
use egui_mineral_material::scroll;
use waveduderust_app::document::Source;
use waveduderust_app::form::{Dialog as FormDialog, Request};
use waveduderust_app::jobs::Job;
use waveduderust_app::menus::{self as app_menus, ExportKindChoice, MenuAction, On};
use waveduderust_app::modal::{Modal, Reply};
use waveduderust_app::platform::Instant;
#[cfg(not(target_arch = "wasm32"))]
use waveduderust_app::platform::show_in_folder;
use waveduderust_app::settings_table::{ZOOMS, welcome_due};
use waveduderust_app::shell::{Acted, Boot, Host as _, ShellState};
use waveduderust_app::status::CacheLine;
use waveduderust_app::strings::{Language, strip_mnemonic};
use waveduderust_app::window::Pane;
use waveduderust_app::{Event, Guarded};
use waveduderust_egui_platform::jobs::Jobs;
use waveduderust_egui_platform::keys::PANE_KEYS;
use waveduderust_egui_platform::shown::Shown;

use crate::commands::{self, Cmd, ViewCommand, key_of};
use crate::look;
use crate::menus::{self, Group, Palette, Row, Target};
use crate::messages::MessagesView;
use crate::modal;
use crate::pages::{self, Pages};
use crate::route::Route;
use crate::status;
use crate::workspace::Workspace;
use crate::{App, Mui};

const PALETTE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::K);
const PAGE_MAX_W: f32 = 1040.;
/// The least space either side of a page.
const PAGE_MARGIN: f32 = 16.;
/// The Messages panel's first height.
const MESSAGES_H: f32 = 220.;
const MESSAGES_SHEET_W: f32 = 400.;
/// The Export split button's width beside its label: icon, paddings, the menu half.
const EXPORT_BUTTON_EXTRA: f32 = 120.;
/// The most of the menu bar's width the title takes.
const TITLE_SHARE: f32 = 0.4;

/// What a view asks the window for; applied in order after the views.
pub(crate) enum Intent {
	Send(Event),
	Act(MenuAction),
	/// An action on an outline row, or the current item.
	On(MenuAction, On),
	/// A command fired this pass, with the text a paste key carried.
	Cmd(Cmd, Option<String>),
	/// What a form asks for beyond the state's events.
	Ask(Request),
	/// Shows a page over the workspace, or the workspace; the same page in another section replaces it.
	Go(Route),
	/// Shows a page on top of the current one.
	Push(Route),
	Back,
	/// Puts `text` on the clipboard and tells so with the string `note`.
	Copy {
		text: String,
		note: &'static str,
	},
	/// A modal's answer.
	Reply(Reply),
	/// The clipboard's text the browser read for a paste on the target.
	#[cfg(target_arch = "wasm32")]
	Paste(String, On),
}

/// What a snackbar's action does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, egui_mineral::semantic::KeySalt)]
enum Toast {
	/// Shows an export's output in its folder.
	#[cfg_attr(target_arch = "wasm32", expect(dead_code, reason = "the web has no folders to show"))]
	Reveal,
}

/// The [`eframe::App`].
pub(crate) struct Shell {
	rt: Arc<Rt>,
	app: App,
	boot: Boot,
	shown: Shown,
	router: Router<Route>,
	pages: Pages,
	workspace: Workspace,
	palette: Palette,
	toasts: ToastQueue<Snackbar<Toast>>,
	/// Whether the top bar's menu is open.
	menu_open: bool,
	/// The snackbar offering to show an export's output in its folder.
	#[cfg(not(target_arch = "wasm32"))]
	reveal: Option<(egui_mineral::toast::ToastId, PathBuf)>,
	cache: CacheLine,
	shell: ShellState,
	jobs: Jobs,
	messages: MessagesView,
	leave: waveduderust_egui_platform::LeaveGuard,
}

impl Shell {
	/// # Errors
	/// When the UI runtime cannot start.
	pub(crate) fn new(cc: &eframe::CreationContext<'_>, mut app: App, boot: Boot) -> Result<Self, InitError> {
		let ctx = &cc.egui_ctx;
		let rt = look::install(ctx, app.settings.touch_first)?;
		// English only (DESIGN.md, Strings); the Language setting stays as the other window keeps it.
		app.strings.set_language(Language::English);
		let shell = ShellState::new(&app, &boot);
		let mut router = Router::new(Route::Workspace);
		if welcome_due(&app) && boot.open.is_empty() {
			router.push(Route::Examples);
		}
		Ok(Self {
			rt,
			shown: Shown::new(ctx, &app),
			router,
			pages: Pages::default(),
			workspace: Workspace::new(cc.wgpu_render_state.clone()),
			palette: Palette::default(),
			toasts: snackbar_queue(),
			menu_open: false,
			#[cfg(not(target_arch = "wasm32"))]
			reveal: None,
			cache: CacheLine::default(),
			shell,
			jobs: Jobs::new(ctx),
			messages: MessagesView::default(),
			leave: waveduderust_egui_platform::LeaveGuard::default(),
			app,
			boot,
		})
	}

	fn send(&mut self, event: Event) {
		self.shell.send(&mut self.app, &self.jobs, event);
	}

	/// Runs `action` on `on` unless this window does not offer it or it cannot run now; `pasted` is the text a paste
	/// key carried.
	fn act(&mut self, ctx: &egui::Context, action: MenuAction, on: On, pasted: Option<String>) {
		if matches!(on, On::Current) && self.messages.act(ctx, action) {
			return;
		}
		if !commands::offered(action) {
			tracing::debug!("{action:?} refused");
			return;
		}
		let focus = app_menus::focuses_form(&self.app, action, on);
		let read = || pasted.or_else(|| waveduderust_egui_platform::read_clipboard(ctx, on));
		match self.shell.act(&mut self.app, &self.jobs, action, on, read) {
			Acted::Edited if focus => self.workspace.focus_form(),
			Acted::Show(action) => self.show(ctx, action),
			Acted::Refused | Acted::Ran | Acted::Edited => {}
		}
	}

	/// Shows the page or the pane of `action`.
	fn show(&mut self, ctx: &egui::Context, action: MenuAction) {
		match action {
			MenuAction::Examples | MenuAction::Welcome => self.toggle(Route::Examples),
			MenuAction::Settings => self.toggle(Route::Settings { section: None }),
			MenuAction::About => self.toggle(Route::About),
			MenuAction::Pane(Pane::Project) => self.view(ctx, ViewCommand::Outline),
			MenuAction::Pane(Pane::Properties) => self.view(ctx, ViewCommand::Inspector),
			MenuAction::Pane(Pane::Messages) => self.view(ctx, ViewCommand::Messages),
			_ => {}
		}
	}

	/// Shows a form's file dialog.
	fn dialog(&self, dialog: FormDialog) {
		if let Some(job) = Job::of_dialog(&self.app, dialog) {
			self.jobs.start(job, self.app.document_id(), self.app.strings);
		}
	}

	/// Runs a view command.
	fn view(&mut self, ctx: &egui::Context, view: ViewCommand) {
		match view {
			ViewCommand::Palette => self.palette.open = !self.palette.open,
			ViewCommand::Shortcuts => self.shell.modals.show(Modal::Shortcuts(self.shortcuts(ctx))),
			ViewCommand::UiZoom(i) => self.send(Event::SetMenuZoom(ZOOMS[i].0)),
			ViewCommand::Back => self.go(Route::Workspace),
			ViewCommand::Messages => {
				if self.messages.open {
					self.messages.open = false;
				} else {
					self.messages.show_focused();
				}
			}
			_ => {
				let mut events = Vec::new();
				self.workspace.view(&self.app, view, &mut events);
				for event in events {
					self.send(event);
				}
			}
		}
	}

	fn command(&mut self, ctx: &egui::Context, cmd: Cmd, pasted: Option<String>) {
		match cmd {
			Cmd::Act(action) => self.act(ctx, action, On::Current, pasted),
			Cmd::View(view) if commands::runs(&self.app, cmd) => self.view(ctx, view),
			Cmd::View(view) => tracing::debug!("{view:?} refused"),
		}
	}

	/// Shows `route` over the workspace; the same page again returns to the workspace.
	fn toggle(&mut self, route: Route) {
		if self.router.current().same_page(route) {
			self.go(Route::Workspace);
		} else {
			self.go(route);
		}
	}

	fn go(&mut self, route: Route) {
		let current = *self.router.current();
		if current == route {
			return;
		}
		if route == Route::Workspace {
			self.router.reset_to(Route::Workspace);
		} else if current.same_page(route) {
			self.router.replace(route);
		} else {
			self.router.reset_to(Route::Workspace);
			self.router.push(route);
		}
	}

	fn apply(&mut self, ctx: &egui::Context, intent: Intent) {
		match intent {
			Intent::Send(event) => self.send(event),
			Intent::Act(action) => self.act(ctx, action, On::Current, None),
			Intent::On(action, on) => self.act(ctx, action, on, None),
			Intent::Cmd(cmd, pasted) => self.command(ctx, cmd, pasted),
			Intent::Ask(Request::DescriptionOpen(open)) => self.app.settings.part_description_expanded = open,
			Intent::Ask(Request::Dialog(dialog)) => self.dialog(dialog),
			Intent::Go(route) => self.go(route),
			Intent::Push(route) => self.router.push(route),
			Intent::Back => {
				self.router.back();
			}
			#[cfg(target_arch = "wasm32")]
			Intent::Paste(text, on) => self.act(ctx, MenuAction::Paste, on, Some(text)),
			Intent::Reply(reply) => {
				if let Some(note) = self.shell.reply(&mut self.app, &self.jobs, reply) {
					self.toasts.push(Snackbar::new(self.app.strings.get(note)));
				}
			}
			Intent::Copy { text, note } => {
				ctx.copy_text(text);
				self.toasts.push(Snackbar::new(self.app.strings.get(note)));
			}
		}
	}

	/// The close box asks for Exit: modals but its questions go, and Exit asks once none shows.
	fn close_box(&mut self, ctx: &egui::Context, intents: &mut Vec<Intent>) {
		if ctx.input(|i| i.viewport().close_requested()) && self.shell.close_pressed() {
			ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
			self.palette.open = false;
		}
		if let Some(exit) = self.shell.exit_due() {
			intents.push(Intent::Send(exit));
		}
	}

	/// A snackbar when a run ends where no status line shows.
	fn run_ended(&mut self, compact: bool) {
		if self.shell.run_ended(&self.app) && compact {
			self.toasts.push(Snackbar::new(self.app.run_status()));
		}
	}

	/// The last file dropped on the window: an image becomes the scene's, another file opens.
	fn drop_in(&self, ctx: &egui::Context) {
		if let Some(file) = ctx.input(|i| i.raw.dropped_files.last().cloned()) {
			self.jobs.drop_in(file, self.app.document_id(), self.app.strings);
		}
	}

	/// Another document shows the workspace, its canvas focused.
	fn follow_document(&mut self) {
		if self.shell.document_changed(&self.app) {
			self.go(Route::Workspace);
			self.workspace.focus_canvas();
		}
	}

	/// The key table: each menu's entries with a key, the palette's keys, then the canvas keys.
	fn shortcuts(&self, ctx: &egui::Context) -> Vec<(String, Vec<(String, String)>)> {
		fn walk(rows: &[Row], ctx: &egui::Context, out: &mut Vec<(String, String)>) {
			for row in rows {
				match row {
					Row::Item { cmd, text, .. } => {
						if let Some(key) = key_of(*cmd) {
							out.push((strip_mnemonic(text), shortcut::label(ctx, &key)));
						}
					}
					Row::Sub { rows, .. } => walk(rows, ctx, out),
					Row::Divider => {}
				}
			}
		}
		let s = &self.app.strings;
		let mut groups: Vec<(String, Vec<(String, String)>)> =
			menus::groups(&self.app, self.workspace.columns, self.shell.has_last_export(&self.app))
				.iter()
				.map(|g| {
					let mut rows = Vec::new();
					walk(&g.rows, ctx, &mut rows);
					(strip_mnemonic(&g.text), rows)
				})
				.filter(|(_, rows)| !rows.is_empty())
				.collect();
		let palette = shortcut::label(ctx, &PALETTE);
		groups.push((s.get("PaletteSearch").to_owned(), vec![(strip_mnemonic(s.get("MenuViewPalette")), palette)]));
		let canvas = PANE_KEYS
			.iter()
			.map(|&(key, pane)| (strip_mnemonic(s.get(pane.label_key())), key.name().to_owned()))
			.collect();
		groups.push((s.get("ShortcutsCanvas").to_owned(), canvas));
		groups
	}

	/// The top bar: the menu bar (Expanded and up), else a menu button and a small app bar; a page's has Back.
	fn top_bar(&mut self, ui: &mut Mui<'_>, class: WindowClass, groups: &[Group], intents: &mut Vec<Intent>) {
		let s = self.app.strings;
		let rt = ui.rt();
		let route = *self.router.current();
		let search = format!("{} ({})", s.get("PaletteSearch"), shortcut::label(ui.ctx(), &PALETTE));
		let palette_cmd = Cmd::View(ViewCommand::Palette);
		let palette = (palette_cmd, IconAction::new(icons::ICON_SEARCH, &search).command(palette_cmd.id()));
		if route != Route::Workspace {
			let r = TopAppBar::new("page-bar", s.get(route.title_key()))
				.navigation(icons::ICON_ARROW_BACK, s.get("NavBack"))
				.action(palette.0, palette.1)
				.show(ui);
			if r.leading_clicked {
				intents.push(Intent::Back);
			}
			return;
		}
		if class >= WindowClass::Expanded {
			let bar = menus::bar(&self.app, groups).show(ui).response.rect;
			// over the bar's free end: the title, at most a share of the bar, and the palette button
			let layout = Layout::right_to_left(Align::Center);
			ui.scope_builder(UiBuilder::new().max_rect(bar.shrink2(vec2(8., 0.))).layout(layout), |ui| {
				let ui = &mut rt.ui(ui);
				let button = IconButton::new("top-bar-palette", icons::ICON_SEARCH, &search);
				if ui.mineral(button).clicked() {
					self.palette.open = true;
				}
				self.export_button(ui, groups, intents);
				let font = TypeScale::TitleMedium.font(ui.ctx());
				let ink = ui.tokens().color.on_surface;
				let title = egui::RichText::new(self.app.title()).font(font).color(ink);
				let size = vec2(ui.available_width().min(bar.width() * TITLE_SHARE), ui.available_height());
				ui.allocate_ui_with_layout(size, layout, |ui| {
					ui.add(egui::Label::new(title).truncate().selectable(false));
				});
			});
			return;
		}
		let action = |icon, key: &str, cmd: Cmd| {
			let a = IconAction::new(icon, strip_mnemonic(s.get(key)))
				.command(cmd.id())
				.disabled(!commands::runs(&self.app, cmd));
			(cmd, a)
		};
		let undo = action(icons::ICON_UNDO, "MenuEditUndo", Cmd::Act(MenuAction::Undo));
		let redo = action(icons::ICON_REDO, "MenuEditRedo", Cmd::Act(MenuAction::Redo));
		let (key, export) = self.export_main();
		let export = action(icons::ICON_IOS_SHARE, key, Cmd::Act(export));
		let export = (export.0, export.1.salt("export"));
		// Compact keeps the palette inline, the rest overflows
		let actions =
			if class == WindowClass::Medium { [undo, redo, palette, export] } else { [palette, undo, redo, export] };
		ui.horizontal(|ui| {
			let ui = &mut rt.ui(ui);
			let anchor = ui.mineral(IconButton::new("top-bar-menu", icons::ICON_MENU, s.get("NavMenu")));
			menus::app_menu(&self.app, groups).show(rt, anchor.response(), &mut self.menu_open);
			let mut bar = TopAppBar::new("top-bar", self.app.title()).actions(actions);
			let run = self.app.run_status();
			if !run.is_empty() && class == WindowClass::Medium {
				bar = bar.supporting(run);
			}
			bar.show(ui);
		});
	}

	/// The Export button's string key and action: Export last once there is a last export, else Export both.
	fn export_main(&self) -> (&'static str, MenuAction) {
		if self.shell.has_last_export(&self.app) {
			("MenuFileExportLast", MenuAction::ExportLast)
		} else {
			("MenuFileExport", MenuAction::Export(ExportKindChoice::Both))
		}
	}

	/// The top bar's Export split button: its main action ([`Self::export_main`]) and File > Export as its menu.
	fn export_button(&self, ui: &mut Mui<'_>, groups: &[Group], intents: &mut Vec<Intent>) {
		let s = &self.app.strings;
		let rt = ui.rt();
		let (key, action) = self.export_main();
		let exports = groups.iter().flat_map(|g| &g.rows).find_map(|r| match r {
			Row::Sub { rows, .. }
				if rows.iter().any(|r| matches!(r, Row::Item { cmd: Cmd::Act(MenuAction::Export(_)), .. })) =>
			{
				Some(rows.as_slice())
			}
			_ => None,
		});
		let label = strip_mnemonic(s.get(key));
		let menu = menus::command_menu(
			&self.app,
			Menu::new("top-bar-export-menu", s.get("NavMore")),
			exports.unwrap_or_default(),
		);
		// the button takes the row it is given: its label, icon, paddings and the menu half
		let font = TypeScale::LabelLarge.font(ui.ctx());
		let text = ui.painter().layout_no_wrap(label.clone(), font, Color32::WHITE);
		let w = text.size().x + EXPORT_BUTTON_EXTRA;
		let h = ui.available_height();
		let r = ui
			.allocate_ui_with_layout(vec2(w, h), Layout::left_to_right(Align::Center), |ui| {
				SplitButton::new("top-bar-export", label, menu)
					.menu_label(strip_mnemonic(s.get("MenuFileExport")))
					.icon(icons::ICON_IOS_SHARE)
					.variant(ContainerVariant::Tonal)
					.disabled(!commands::runs(&self.app, Cmd::Act(action)))
					.show(&mut rt.ui(ui))
			})
			.inner;
		if r.leading_clicked {
			intents.push(Intent::Act(action));
		}
	}

	/// The page of `route`, centred up to [`PAGE_MAX_W`].
	fn page(&mut self, ui: &mut Mui<'_>, route: Route, class: WindowClass, intents: &mut Vec<Intent>) {
		self.pages.enter(route, &self.app);
		let rt = ui.rt();
		scroll::page(ui, ("page", route.title_key()), |ui| {
			let full = ui.available_width();
			let w = (full - 2. * PAGE_MARGIN).min(PAGE_MAX_W);
			let pad = (full - w) / 2.;
			ui.allocate_ui_with_layout(vec2(full, 0.), Layout::left_to_right(Align::Min), |ui| {
				ui.add_space(pad);
				ui.allocate_ui_with_layout(vec2(w, 0.), Layout::top_down(Align::Min), |ui| {
					ui.set_width(w);
					ui.add_space(PAGE_MARGIN);
					pages::show(&mut rt.ui(ui), route, class, &self.app, &mut self.pages, intents);
				});
			});
		});
	}
}

impl Shell {
	/// Messages, while shown: a bottom panel above the status line (Expanded and up), a modal side sheet (Medium), a
	/// full-screen dialog (Compact).
	fn messages_ui(&mut self, ui: &mut Mui<'_>, class: WindowClass) {
		let s = self.app.strings;
		let rt = ui.rt();
		let (log, view) = (&self.boot.log, &mut self.messages);
		let title = strip_mnemonic(s.get("PaneMessages"));
		let mut open = view.open;
		match class {
			WindowClass::Compact => {
				Dialog::full_screen("messages-dialog", title).show_modal(rt, &mut open, |ui| view.ui(ui, &s, log));
				view.open = open;
			}
			WindowClass::Medium => {
				SideSheet::new("messages-sheet").headline(title).width(MESSAGES_SHEET_W..=MESSAGES_SHEET_W).show_modal(
					rt,
					&mut open,
					|ui| view.ui(ui, &s, log),
				);
				view.open = open;
			}
			_ if view.open => {
				let c = ui.tokens().color;
				Panel::bottom("messages")
					.frame(Frame::NONE.fill(c.surface_container_low).inner_margin(8))
					.default_size(MESSAGES_H)
					.resizable(true)
					.show(ui, |ui| view.ui(&mut rt.ui(ui), &s, log));
			}
			_ => {}
		}
	}
}

impl eframe::App for Shell {
	fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
		let rt = Arc::clone(&self.rt);
		// before the pass, which styles its root with the kit's style
		self.shown.follow(rt.core(), frame, &self.app);
		rt.pass(ui, |ui| self.pass(ui, frame));
	}
}

impl Shell {
	/// One pass inside the runtime's: its shortcuts fired, its overlays drawn.
	fn pass(&mut self, ui: &mut Mui<'_>, frame: &eframe::Frame) {
		let ctx = ui.ctx().clone();
		let rt = ui.rt();
		let mut intents = Vec::new();
		if let Some(path) = self.boot.open.take_last() {
			intents.push(Intent::Send(Event::Guarded(Guarded::Open(Source::Path(path)))));
		}
		self.close_box(&ctx, &mut intents);
		for done in self.jobs.take(self.app.document_id()) {
			if let Some(event) = self.shell.answered(&mut self.app, &self.jobs, done) {
				intents.push(Intent::Send(event));
			}
		}
		#[cfg(target_arch = "wasm32")]
		if let Some((text, on)) = waveduderust_egui_platform::pasted() {
			intents.push(Intent::Paste(text, on));
		}
		self.shell.poll(&mut self.app, &self.jobs);
		self.messages.poll(&self.boot.log);
		self.shell.tick(&mut self.app, &self.jobs, Instant::now());
		self.cache.update(&self.app);

		let groups = menus::groups(&self.app, self.workspace.columns, self.shell.has_last_export(&self.app));
		let offered: Vec<Cmd> = menus::commands(&groups).into_iter().chain(commands::keyed()).collect();
		// the commands run after the views, which commit the fields' typed text first (PROP-06)
		let fired: Vec<Intent> = commands::take_fired(&ctx, offered)
			.into_iter()
			.map(|cmd| Intent::Cmd(cmd, shortcut::take_paste(&ctx, &cmd.id())))
			.collect();
		if !fired.is_empty() {
			egui_mineral::edit::commit_pending(&ctx);
			ctx.request_repaint();
		}
		let route = *self.router.current();
		commands::bind_keys(&ctx, &self.app, route != Route::Workspace, self.workspace.takes_escape());
		let palette = Palette::entries(&self.app, &groups);

		let class = WindowClass::of(rt.core());
		let compact = class == WindowClass::Compact;
		self.run_ended(compact);
		let surface = rt.tokens().color.surface;
		Panel::top("top-bar")
			.frame(Frame::NONE.fill(surface))
			.show_separator_line(false)
			.show(ui, |ui| self.top_bar(&mut rt.ui(ui), class, &groups, &mut intents));
		if !compact {
			let (log, cache, export) = (&self.boot.log, &self.cache, self.shell.export_status());
			let clicked = Panel::bottom("status")
				.frame(Frame::NONE)
				.show_separator_line(false)
				.show(ui, |ui| {
					status::show(&mut rt.ui(ui), &self.app, log, cache, class >= WindowClass::Expanded, export)
				})
				.inner;
			if clicked {
				self.messages.show_focused();
			}
		}
		if route == Route::Workspace {
			self.messages_ui(ui, class);
		}
		let mut events = Vec::new();
		self.workspace.poll(ui, &mut self.app, &mut events);
		CentralPanel::default().frame(Frame::NONE.fill(surface)).show(ui, |ui| {
			let ui = &mut rt.ui(ui);
			if route == Route::Workspace {
				self.workspace.show(ui, class, &mut self.app, &mut events, &mut intents);
			} else {
				self.page(ui, route, class, &mut intents);
			}
		});
		intents.splice(0..0, events.into_iter().map(Intent::Send).chain(fired));

		if let Some(reply) = modal::show(&mut self.shell.modals, rt, &self.app) {
			intents.push(Intent::Reply(reply));
		}
		let placeholder = self.app.strings.get("PaletteSearch");
		let picked = palette
			.into_iter()
			.fold(
				CommandPalette::new("palette", &mut self.palette.query)
					.actions(None::<(Target, egui_mineral::palette::PaletteCommand)>),
				|p, (target, c)| match target {
					Some(target) => p.action(target, c),
					None => p.command(c),
				},
			)
			.placeholder(placeholder)
			.shortcut(PALETTE)
			.show_modal(rt, &mut self.palette.open)
			.clicked;
		match picked {
			Some(Target::Go(route)) => intents.push(Intent::Go(route)),
			Some(Target::Example(i)) => intents.push(Intent::Send(Event::Guarded(Guarded::OpenExample(i)))),
			Some(Target::Item(item)) => {
				self.app.selection.select(waveduderust_app::selection::Row::Item(item));
				self.workspace.focus_form();
			}
			None => {}
		}
		#[cfg(not(target_arch = "wasm32"))]
		if let Some(path) = self.shell.take_exported() {
			let strings = &self.app.strings;
			let toast = Snackbar::new(strings.get("ExportDone"))
				.action(Toast::Reveal, egui_mineral_material::prelude::TextAction::new(strings.get("ShowInFolder")));
			self.reveal = Some((self.toasts.push(toast), path));
		}
		#[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
		let shown = SnackbarHost::new(&mut self.toasts).show(rt);
		#[cfg(not(target_arch = "wasm32"))]
		if let Some((id, path)) = &self.reveal
			&& shown.action_clicked == Some(*id)
		{
			show_in_folder(path);
			self.reveal = None;
		}
		if !self.shell.modals.is_open() {
			self.drop_in(&ctx);
		}

		for intent in intents {
			self.apply(&ctx, intent);
		}
		self.follow_document();

		if self.shell.take_exit() {
			waveduderust_egui_platform::remember_place(frame, &mut self.app.settings);
			if let Err(e) = self.app.settings.save(&self.boot.store) {
				tracing::warn!("{e}");
			}
			ctx.send_viewport_cmd(egui::ViewportCommand::Close);
		}
		self.leave.set(self.app.document.dirty || self.app.live.export_busy());
		if let Some(tick) = self.shell.next_tick(&self.app) {
			ctx.request_repaint_after(tick);
		}
	}
}
