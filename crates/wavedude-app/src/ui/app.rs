//! The root of the window: the boot hand-over from `main`, the UI-only state every view shares ([`Ui`]),
//! the command context ([`Cx`]), the notice log the views read, and the tick loop that drives the state.
//!
//! Threads: everything here runs on the UI thread; the tick loop is a Freya task.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use freya::prelude::*;
use wavedude_widgets::menu::ContextMenuRequest;
use wavedude_widgets::tree::TreeState;
use wavedude_widgets::{
	DockLayout, MenuModel, MessageLog, Progress, StatusLine, Theme, ThemeKind, use_init_wave_theme,
};

use wavedude_app_state::live::Notice;
use wavedude_app_state::log::LogRing;
use wavedude_app_state::selection::Row;
use wavedude_app_state::settings::{AppDirs, Theme as ThemeSetting};
use wavedude_app_state::strings::Strings;
use wavedude_app_state::{Event as AppEvent, Guarded};

use super::dialogs::{self, Modal};
use super::export_dialog::{self, ExportProgress};
use super::shared::{App, Shared};
use super::{downloads, menus, messages, update, window};

/// The tick period of the state loop.
pub const TICK: Duration = Duration::from_millis(16);
/// The smallest window, unzoomed logical pixels.
pub const MIN_SIZE: (f64, f64) = (800.0, 500.0);
/// Idle ticks between two state ticks when nothing time-based is pending.
const IDLE_TICKS: u32 = 15;
/// Notices kept for late readers.
const NOTICE_LOG: usize = 256;

/// Closing the window: the close box asks first (unsaved edits), then the app allows it.
#[derive(Clone, Default)]
pub struct CloseGate {
	requested: Rc<Cell<bool>>,
	allowed: Rc<Cell<bool>>,
}

impl CloseGate {
	/// The window's close hook: true when the window may close now.
	pub fn on_close_request(&self) -> bool {
		if self.allowed.get() {
			return true;
		}
		self.requested.set(true);
		false
	}

	fn take_request(&self) -> bool {
		self.requested.replace(false)
	}

	fn allow(&self) {
		self.allowed.set(true);
	}
}

/// What `main` hands to the root: the state, made before the window, and the process facts.
#[derive(Clone)]
pub struct Boot {
	app: Rc<RefCell<Option<App>>>,
	shared: Rc<RefCell<Option<Shared>>>,
	pub info: BootInfo,
}

impl Boot {
	pub fn new(app: App, info: BootInfo) -> Self {
		Self { app: Rc::new(RefCell::new(Some(app))), shared: Rc::default(), info }
	}

	/// The state handle once the root has provided it (tests drive the state through it).
	pub fn shared(&self) -> Option<Shared> {
		self.shared.borrow().clone()
	}
}

/// Facts the views need beside the state.
#[derive(Clone)]
pub struct BootInfo {
	pub ring: LogRing,
	pub dirs: AppDirs,
	pub close: CloseGate,
	/// The file named on the command line.
	pub open: Option<PathBuf>,
	/// A downloaded update, installed by `main` after a restart's exit.
	pub update: update::Pending,
	/// The window's place, seen and asked for through the event loop.
	pub placement: super::placement::Placement,
}

/// Notices of the live controller, kept with a sequence number so several views can read them.
#[derive(Clone, Default)]
pub struct NoticeLog {
	items: Rc<RefCell<VecDeque<(u64, Notice)>>>,
	next: Rc<Cell<u64>>,
}

impl NoticeLog {
	fn push(&self, notice: Notice) {
		let mut items = self.items.borrow_mut();
		items.push_back((self.next.get(), notice));
		self.next.set(self.next.get() + 1);
		while items.len() > NOTICE_LOG {
			items.pop_front();
		}
	}

	/// Notices after `seq` (all with `None`), oldest first.
	pub fn since(&self, seq: Option<u64>) -> Vec<(u64, Notice)> {
		self.items.borrow().iter().filter(|(s, _)| seq.is_none_or(|q| *s > q)).cloned().collect()
	}
}

/// A context menu entry's action bound to the handler of the menu it opened with.
#[derive(Clone)]
pub struct MenuCallback(Rc<dyn Fn()>);

impl MenuCallback {
	pub fn run(&self) {
		(self.0)();
	}
}

impl PartialEq for MenuCallback {
	fn eq(&self, other: &Self) -> bool {
		Rc::ptr_eq(&self.0, &other.0)
	}
}

impl std::fmt::Debug for MenuCallback {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("MenuCallback")
	}
}

/// UI-only state shared by the views (never persisted except the dock layout).
#[derive(Clone, Copy, PartialEq)]
pub struct Ui {
	pub modal: State<Option<Modal>>,
	/// Modals asked for while one is open, shown in turn.
	pub waiting: State<VecDeque<Modal>>,
	pub layout: State<DockLayout>,
	pub preview_shown: State<bool>,
	pub menu: State<Option<ContextMenuRequest<MenuCallback>>>,
	pub tree: State<TreeState<Row>>,
	pub messages: State<MessageLog>,
	pub status: State<StatusLine>,
	pub export: State<Option<ExportProgress>>,
	/// The oldest running download ([`downloads`]).
	pub download: State<Option<(String, Progress)>>,
	/// Bumped whenever new notices arrived.
	pub notices_revision: State<u64>,
	/// Bumped by Enter on a tree row: the properties form takes the focus.
	pub focus_form: State<u64>,
	/// Bumped by F1: the manual window opens or comes forward.
	pub manual_request: State<u64>,
	/// Cisco's OpenH264 binary is downloaded.
	pub codec_downloaded: State<bool>,
	/// The Messages pane's list, focused after a status bar double-click.
	pub messages_list: AccessibilityId,
	/// Bumped by a status bar double-click: the Messages list takes the focus once shown; the second counter is the
	/// last request it served.
	pub focus_messages: (State<u64>, State<u64>),
	/// The project tree, focused on a reveal ([`AppState::reveals`](wavedude_app_state::AppState::reveals)).
	pub tree_id: AccessibilityId,
	/// Each preview picture's place, for drops.
	pub pictures: State<[Option<PicturePlace>; 2]>,
	/// The clipboard held a pasteable fragment when a menu last opened ([`Ui::look_at_clipboard`]).
	pub pastable: State<bool>,
}

/// A preview picture's area top-left in window pixels and its canvas transform.
pub type PicturePlace = ((f32, f32), wavedude_widgets::CanvasTransform);

impl Ui {
	/// Reads the clipboard into `pastable`; an event-time read, since the OS clipboard is I/O a render never does.
	pub fn look_at_clipboard(&self) {
		let mut pastable = self.pastable;
		pastable.set_if_modified(menus::clipboard_holds_objects());
	}

	/// Opens a context menu at `at` whose activations go to `handler`.
	pub fn open_menu<A: Clone + 'static>(&self, at: (f64, f64), model: MenuModel<A>, handler: impl Fn(A) + 'static) {
		let handler = Rc::new(handler);
		let model = model.map(|action| {
			let handler = handler.clone();
			MenuCallback(Rc::new(move || handler(action.clone())))
		});
		let mut menu = self.menu;
		menu.set(Some(ContextMenuRequest { at, model }));
	}

	/// Shows `modal`, or queues it after the one open; a memory warning is asked once.
	pub fn show_modal(&self, modal: Modal) {
		let mut m = self.modal;
		if m.peek().is_none() {
			m.set(Some(modal));
			return;
		}
		let warning = |w: &Modal| matches!(w, Modal::MemoryWarning);
		let mut waiting = self.waiting;
		if warning(&modal) && (m.peek().as_ref().is_some_and(warning) || waiting.peek().iter().any(warning)) {
			return;
		}
		waiting.write().push_back(modal);
	}

	/// Drops the memory warning, open or queued: the edit it asked about is gone.
	pub fn drop_memory_warning(&self) {
		let warning = |w: &Modal| matches!(w, Modal::MemoryWarning);
		let mut waiting = self.waiting;
		waiting.write().retain(|w| !warning(w));
		if self.modal.peek().as_ref().is_some_and(warning) {
			self.close_modal();
		}
	}

	/// Closes the modal open; the next queued one shows.
	pub fn close_modal(&self) {
		let mut waiting = self.waiting;
		let next = if waiting.peek().is_empty() { None } else { waiting.write().pop_front() };
		let mut m = self.modal;
		m.set(next);
	}
}

/// The handles a command needs: the state, the UI state and the boot facts.
#[derive(Clone)]
pub struct Cx {
	pub shared: Shared,
	pub ui: Ui,
	pub info: BootInfo,
	pub notices: NoticeLog,
}

impl Cx {
	/// From context, without subscribing the caller to anything.
	pub fn get() -> Self {
		Self {
			shared: consume_context::<Shared>(),
			ui: consume_context::<Ui>(),
			info: consume_context::<BootInfo>(),
			notices: consume_context::<NoticeLog>(),
		}
	}

	/// Sends an event and acts on the state's response.
	pub fn send(&self, event: AppEvent) {
		let response = self.shared.send(event);
		dialogs::respond(self, response);
	}

	pub fn strings(&self) -> Strings {
		self.shared.peek(|a| a.strings)
	}

	/// Saves the window's size, place and the dock layout, then ends the process.
	pub fn exit(&self) {
		let layout = Some(serde_json::to_string(&*self.ui.layout.peek()).expect("a dock layout serialises"));
		let window = Platform::get().current_window();
		let size = *window.root_size.peek();
		let placement = self.info.placement.clone();
		self.shared.update(|app| {
			app.settings.layout = layout;
			if size.width > 0. && size.height > 0. && !placement.seen().maximized {
				app.settings.window_width = f64::from(size.width);
				app.settings.window_height = f64::from(size.height);
			}
			placement.save(&mut app.settings);
		});
		super::settings_views::persist(self);
		self.info.close.allow();
		Platform::get().exit();
	}
}

/// The window's root: takes the state from the boot hand-over once and provides it.
pub fn root() -> impl IntoElement {
	let boot = consume_context::<Boot>();
	let app = use_hook(|| Rc::new(RefCell::new(boot.app.borrow_mut().take())));
	let app = app.borrow_mut().take().expect("the root component renders once");
	let shared = Shared::provide(app);
	boot.shared.borrow_mut().replace(shared);
	use_provide_context(|| boot.info.clone());
	Shell
}

/// The theme a setting means on this system.
pub fn theme_kind(setting: ThemeSetting) -> ThemeKind {
	match setting {
		ThemeSetting::Dark => ThemeKind::Dark,
		ThemeSetting::Light => ThemeKind::Light,
		ThemeSetting::System => super::os_theme::current().unwrap_or_else(|| {
			match *Platform::get().current_window().preferred_theme.peek() {
				PreferredTheme::Dark => ThemeKind::Dark,
				_ => ThemeKind::Light,
			}
		}),
	}
}

/// The window's background before the first frame; System without a known OS preference counts as dark.
pub fn theme_background(setting: ThemeSetting) -> Color {
	let kind = match setting {
		ThemeSetting::Light => ThemeKind::Light,
		ThemeSetting::Dark => ThemeKind::Dark,
		ThemeSetting::System => super::os_theme::current().unwrap_or(ThemeKind::Dark),
	};
	Theme::of(kind).window_background
}

#[derive(PartialEq)]
struct Shell;

impl Component for Shell {
	fn render(&self) -> impl IntoElement {
		let shared = consume_context::<Shared>();
		let info = consume_context::<BootInfo>();
		let initial_theme = shared.peek(|a| a.settings.theme);
		let theme = use_init_wave_theme(move || Theme::of(theme_kind(initial_theme)));
		let saved_layout = shared.peek(|a| a.settings.layout.clone());
		let ready = shared.peek(|a| a.strings.get("Ready").to_owned());
		let ui = Ui {
			modal: use_state(|| None),
			waiting: use_state(VecDeque::new),
			layout: use_state(move || window::initial_layout(saved_layout.as_deref())),
			preview_shown: use_state(|| true),
			menu: use_state(|| None),
			tree: use_state(TreeState::default),
			messages: use_state(MessageLog::default),
			status: use_state(move || StatusLine { text: ready, ..StatusLine::default() }),
			export: use_state(|| None),
			download: use_state(|| None),
			notices_revision: use_state(|| 0),
			focus_form: use_state(|| 0),
			manual_request: use_state(|| 0),
			codec_downloaded: use_state({
				let shared = shared.clone();
				move || shared.peek(|a| a.codec_dir.as_deref().and_then(wavedude_export::h264::installed)).is_some()
			}),
			messages_list: use_a11y(),
			focus_messages: (use_state(|| 0), use_state(|| 0)),
			tree_id: use_a11y(),
			pictures: use_state(|| [None, None]),
			pastable: use_state(|| false),
		};
		use_provide_context(|| ui);
		let notices = use_provide_context(NoticeLog::default);
		use_hook(move || {
			let cx = Cx { shared: shared.clone(), ui, info: info.clone(), notices: notices.clone() };
			spawn(tick_loop(cx, theme));
		});
		window::MainWindow
	}
}

/// What changed since the previous tick, so the loop only touches the platform on a change.
#[derive(Default)]
struct Applied {
	title: String,
	zoom: f64,
	theme: Option<ThemeKind>,
	log_seq: Option<u64>,
	idle: u32,
}

async fn tick_loop(cx: Cx, mut theme: State<Theme>) {
	let mut applied = Applied { zoom: 1.0, ..Applied::default() };
	if let Some(path) = cx.info.open.clone() {
		cx.send(AppEvent::Guarded(Guarded::Open(path)));
	}
	if let Some(report) = wavedude_app_state::log::take_crash_report(&cx.info.dirs.logs()) {
		cx.ui.show_modal(Modal::Crash(report));
	} else if super::startup::due(&cx) {
		cx.ui.show_modal(Modal::Startup);
	} else if cx.shared.peek(|a| a.settings.check_for_updates == Some(true)) {
		super::update::check(&cx, true);
	}
	loop {
		timer(TICK).await;
		if cx.info.close.take_request() && cx.ui.modal.peek().is_none() {
			cx.send(AppEvent::Guarded(Guarded::Exit));
		}
		let busy = cx.shared.peek(|a| {
			a.live.run_is_progressing()
				|| a.live.is_working()
				|| a.live.fit_pending()
				|| a.live.playback.is_playing()
				|| a.document.history.has_pending()
		}) || cx.ui.export.peek().is_some();
		applied.idle += 1;
		if busy || applied.idle >= IDLE_TICKS {
			applied.idle = 0;
			cx.shared.tick();
			let notices = cx.shared.peek_mut(|a| a.live.take_notices());
			if !notices.is_empty() {
				for n in &notices {
					on_notice(&cx, n);
					cx.notices.push(n.clone());
				}
				let mut rev = cx.ui.notices_revision;
				*rev.write() += 1;
			}
		}
		downloads::poll(&cx);
		sync_platform(&cx, &mut applied, &mut theme);
		messages::poll_log(&cx, &mut applied.log_seq);
	}
}

/// Window title, UI zoom and theme follow the state. A zoom change scales the window with it, kept on its
/// monitor, and the smallest size the window may take.
fn sync_platform(cx: &Cx, applied: &mut Applied, theme: &mut State<Theme>) {
	let (title, zoom, setting) = cx.shared.peek(|a| (a.title(), a.settings.ui_zoom, a.settings.theme));
	if title != applied.title {
		Platform::get().set_window_title(Platform::window_id(), title.clone());
		applied.title = title;
	}
	if zoom != applied.zoom {
		let window = Platform::get().current_window();
		let size = *window.root_size.peek();
		window.set_custom_scale_factor(zoom);
		if size.width > 0. && !cx.info.placement.seen().maximized {
			cx.info.placement.resize(f64::from(size.width) * zoom, f64::from(size.height) * zoom);
		}
		cx.info.placement.set_min_size(MIN_SIZE.0 * zoom, MIN_SIZE.1 * zoom);
		applied.zoom = zoom;
	}
	let kind = theme_kind(setting);
	if applied.theme != Some(kind) {
		applied.theme = Some(kind);
		theme.set_if_modified(Theme::of(kind));
	}
}

/// The notices the shell acts on itself; the preview reads the rest from the [`NoticeLog`].
fn on_notice(cx: &Cx, notice: &Notice) {
	match notice {
		Notice::AskMemoryWarning => cx.ui.show_modal(Modal::MemoryWarning),
		Notice::WarningDropped => cx.ui.drop_memory_warning(),
		Notice::ExportReady => export_dialog::run_ready(cx),
		Notice::ExportStopped => export_dialog::run_stopped(cx),
		Notice::RunFailed { message, no_image } => {
			let s = cx.strings();
			let detail = if *no_image { s.get("RunErrorNoImage").to_owned() } else { message.clone() };
			cx.ui.show_modal(Modal::Error(format!("{}\n{detail}", s.get("RunError"))));
		}
		_ => {}
	}
}
