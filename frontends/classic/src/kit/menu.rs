//! Menus in WaveDude Next's look on egui-mineral's core: the bar ([`BarState`], its headers' Alt mnemonics, F10 and
//! a lone Alt), drop-downs, context menus and submenus, painted from [`MENU`] and the [`Palette`].
//!
//! Keys (docs/ui/10, Conventions): Up/Down walk the enabled rows, Home/End reach the ends, Right or Enter opens a
//! submenu, Left or Esc closes one level, Left/Right in a menu of the bar open its neighbour, a letter picks its
//! row; rows sharing a letter take the focus in turn. A menu closed after keys worked it gives the focus back.

use std::sync::Arc;
use std::time::Duration;

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
	self, Align2, Color32, Context, EventFilter, FontFamily, FontId, Galley, Id, Key, LayerId, Pos2, Rect, Response,
	Role, Sense, Shape, Stroke, Ui, Vec2, WidgetInfo, accesskit, layers::ShapeIdx, pos2, vec2,
};
use egui_material_icons::MaterialIcon;
use egui_mineral::anchor::{Align, Anchor, SafeTriangle, Side, Surroundings};
use egui_mineral::focus::{self, Modality, RovingFocus};
use egui_mineral::i18n::Direction;
use egui_mineral::kit::menu::{FocusTarget, MenuNav, Walk};
use egui_mineral::kit::menubar::{self, BarState};
use egui_mineral::kit::state::TempState;
use egui_mineral::kit::{interact, layer};
use egui_mineral::layout::{Axis, Edge};
use egui_mineral::overlay::outside_press;
use egui_mineral::runtime::Core;
use egui_mineral::shortcut::{self, Mnemonic, Scope, Trigger};
use egui_mineral::widget::Availability;
use waveduderust_app::strings::split_mnemonic;
use waveduderust_egui_preview::drawn as icons;

use super::ClassicUi;
use super::tokens::{BORDER, FONT_SIZE, MENU, Palette};

/// The wait before the pointer resting on a row switches the open submenu.
const SUBMENU_DELAY: Duration = Duration::from_millis(200);

/// What a row shows in its icon column.
#[derive(Clone, Copy, Default)]
pub(crate) enum Lead {
	#[default]
	None,
	Icon(MaterialIcon),
	Drawn(icons::Icon),
}

impl From<MaterialIcon> for Lead {
	fn from(icon: MaterialIcon) -> Self {
		Self::Icon(icon)
	}
}

impl From<Option<MaterialIcon>> for Lead {
	fn from(icon: Option<MaterialIcon>) -> Self {
		icon.map_or(Self::None, Self::Icon)
	}
}

impl From<icons::Icon> for Lead {
	fn from(icon: icons::Icon) -> Self {
		Self::Drawn(icon)
	}
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
	Check,
	Radio,
}

/// A swatch before a row's text: its size and what paints it.
pub(crate) type Swatch<'a> = (Vec2, &'a dyn Fn(&Ui, Rect));

/// A row of a menu.
#[must_use]
pub(crate) struct Entry<'a> {
	text: &'a str,
	marked: bool,
	lead: Lead,
	mark: Option<(Mark, bool)>,
	shortcut: Option<String>,
	enabled: bool,
	italic: bool,
	tip: Option<&'a str>,
	swatch: Option<Swatch<'a>>,
}

impl<'a> Entry<'a> {
	/// `marked` has `_` before its mnemonic and `__` for an underscore.
	pub(crate) fn new(marked: &'a str) -> Self {
		Self {
			text: marked,
			marked: true,
			lead: Lead::None,
			mark: None,
			shortcut: None,
			enabled: true,
			italic: false,
			tip: None,
			swatch: None,
		}
	}

	/// `text` as it is, without a mnemonic.
	pub(crate) fn plain(text: &'a str) -> Self {
		Self { marked: false, ..Self::new(text) }
	}

	pub(crate) fn icon(mut self, lead: impl Into<Lead>) -> Self {
		self.lead = lead.into();
		self
	}

	/// The shortcut's text in the shortcut column.
	pub(crate) fn shortcut(mut self, shortcut: Option<String>) -> Self {
		self.shortcut = shortcut;
		self
	}

	/// A check box row, its mark in the icon column while `on`.
	pub(crate) fn check(mut self, on: bool) -> Self {
		self.mark = Some((Mark::Check, on));
		self
	}

	/// A radio row, its dot in the icon column while `on`.
	pub(crate) fn radio(mut self, on: bool) -> Self {
		self.mark = Some((Mark::Radio, on));
		self
	}

	pub(crate) fn enabled(mut self, enabled: bool) -> Self {
		self.enabled = enabled;
		self
	}

	pub(crate) fn italic(mut self) -> Self {
		self.italic = true;
		self
	}

	/// The tooltip, also shown while disabled.
	pub(crate) fn tip(mut self, tip: &'a str) -> Self {
		self.tip = Some(tip);
		self
	}

	pub(crate) fn swatch(mut self, swatch: Option<Swatch<'a>>) -> Self {
		self.swatch = swatch;
		self
	}
}

/// A row shown: its response, and whether it was picked this pass (click, Enter or its letter).
pub(crate) struct Picked {
	#[cfg_attr(target_arch = "wasm32", expect(dead_code, reason = "the browser reveals no recent file"))]
	pub response: Response,
	pub picked: bool,
}

/// A row as keys see it.
#[derive(Clone, Copy)]
struct RowKey {
	id: Id,
	enabled: bool,
	submenu: bool,
	mnemonic: Option<Mnemonic>,
}

/// The widest text and shortcut of a level.
#[derive(Clone, Copy, Default, PartialEq)]
struct Columns {
	text: f32,
	shortcut: f32,
}

impl Columns {
	fn row_width(self, min: f32) -> f32 {
		(MENU.text + self.text + MENU.shortcut_gap + self.shortcut + MENU.trail).max(min - 2. * (BORDER + MENU.padding))
	}
}

/// An open menu between passes.
#[derive(Clone)]
struct MenuState {
	open: bool,
	/// Placed below it: a header, a trigger, or the pointer.
	anchor: Rect,
	nav: MenuNav,
	/// The first level takes the focus itself once shown: keys come to the menu, no row highlighted.
	panel: bool,
	/// Each level's rows and widths last pass.
	rows: Vec<Vec<RowKey>>,
	columns: Vec<Columns>,
	/// The focus before keys worked the menu.
	return_to: Option<Id>,
	/// The pass it opened in, whose press opened it.
	opened: u64,
}

impl Default for MenuState {
	fn default() -> Self {
		Self {
			open: false,
			anchor: Rect::NOTHING,
			nav: MenuNav::default(),
			panel: false,
			rows: Vec::new(),
			columns: Vec::new(),
			return_to: None,
			opened: 0,
		}
	}
}

impl TempState for MenuState {}

/// How a menu ended this pass.
#[derive(Default)]
struct Outcome {
	/// A row was picked.
	picked: bool,
	/// Esc closed its first level.
	escaped: bool,
	/// Left or Right on its first level: the neighbouring menu of the bar.
	nav: Option<Edge>,
	/// A row held the focus.
	focused: bool,
}

/// A level shown this pass.
struct Level {
	panel: Rect,
	rows: Vec<RowKey>,
	columns: Columns,
}

/// One pass of an open menu.
struct Run<'r> {
	core: &'r Core,
	id: Id,
	st: MenuState,
	levels: Vec<Level>,
	/// The row its letter picked.
	activate: Option<Id>,
	out: Outcome,
	min_width: f32,
	parent: LayerId,
	/// The pointer moved this pass.
	moved: bool,
}

/// Which level keys go to last pass: the one holding the focus, else the deepest.
fn key_level(ctx: &Context, rows: &[Vec<RowKey>]) -> Option<(usize, Option<usize>)> {
	let focused = ctx.memory(egui::Memory::focused);
	let held = focused.and_then(|f| {
		rows.iter().enumerate().find_map(|(l, r)| r.iter().position(|k| k.id == f).map(|i| (l, Some(i))))
	});
	held.or_else(|| rows.len().checked_sub(1).map(|l| (l, None)))
}

fn level_id(menu: Id, level: usize) -> Id {
	menu.with(("level", level))
}

/// Whether a menu (shown last pass or this one) or one of egui's popups is open.
pub(crate) fn any_open(ctx: &Context) -> bool {
	let shown = ctx.data(|d| d.get_temp::<u64>(Id::unique("classic::menu_shown")));
	shown.is_some_and(|p| p + 1 >= ctx.cumulative_pass_nr()) || egui::Popup::is_any_open(ctx)
}

fn mark_shown(ctx: &Context) {
	let pass = ctx.cumulative_pass_nr();
	ctx.data_mut(|d| d.insert_temp(Id::unique("classic::menu_shown"), pass));
}

impl<'r> Run<'r> {
	fn new(core: &'r Core, id: Id, st: MenuState, min_width: f32, parent: LayerId) -> Self {
		let ctx = core.ctx();
		let moved = ctx.input(|i| i.pointer.delta() != Vec2::ZERO);
		let mut run = Self {
			core,
			id,
			st,
			levels: Vec::new(),
			activate: None,
			out: Outcome::default(),
			min_width,
			parent,
			moved,
		};
		run.letters();
		run
	}

	/// The letter typed for a row of the level keys go to: a row alone on it is picked; the core moves the focus
	/// through rows sharing it.
	fn letters(&mut self) {
		let ctx = self.core.ctx().clone();
		let Some((level, _)) = key_level(&ctx, &self.st.rows) else { return };
		let rows = self.st.rows[level].clone();
		let Some((i, row)) = rows.iter().enumerate().find(|(_, r)| shortcut::take_item(&ctx, r.id)) else { return };
		self.keys_worked();
		if row.submenu {
			self.st.nav.open(&ctx, level, i, true);
		} else {
			self.activate = Some(row.id);
		}
	}

	fn keys_worked(&mut self) {
		shortcut::set_mnemonics_revealed(self.core.ctx(), true);
		if self.st.return_to.is_none() {
			let focused = self.core.ctx().memory(egui::Memory::focused);
			let own = focused.is_some_and(|f| self.st.rows.iter().flatten().any(|r| r.id == f));
			if !own {
				self.st.return_to = focused;
			}
		}
	}

	/// Closes `level`; the focus goes to the row that opened it.
	fn close_level(&mut self, level: usize) {
		if let Some(parent) = self.st.nav.close_level(level) {
			self.focus_parent(level, parent);
		}
	}

	fn focus_parent(&self, level: usize, parent: usize) {
		let id = level_id(self.id, level - 1).with(parent);
		self.core.ctx().memory_mut(|m| m.request_focus(id));
	}

	/// Shows level 0 at `place` and the open submenus inside it.
	fn show<R>(&mut self, add: impl FnOnce(&mut MenuUi<'_, '_>) -> R) -> R {
		let ctx = self.core.ctx().clone();
		let columns = self.st.columns.first().copied().unwrap_or_default();
		let size = panel_size(&ctx, level_id(self.id, 0), columns, self.min_width);
		let anchor = Anchor::new(Side::Bottom, Align::Start);
		let rect = anchor.place(self.st.anchor, size, surroundings(&ctx)).rect;
		self.level(0, rect.min, add)
	}

	fn level<R>(&mut self, level: usize, at: Pos2, add: impl FnOnce(&mut MenuUi<'_, '_>) -> R) -> R {
		let ctx = self.core.ctx().clone();
		let id = level_id(self.id, level);
		let area = layer::floating(self.core, id, at).role(Role::Menu).show(&ctx, |ui| {
			let mut menu = MenuUi::new(ui, self, level);
			let r = add(&mut menu);
			menu.finish();
			r
		});
		layer::keep_above(&ctx, area.response.layer_id, self.parent);
		self.parent = area.response.layer_id;
		area.inner
	}

	/// The keys of this pass on the levels shown, then the letters bound for the next.
	fn keys(&mut self) {
		let ctx = self.core.ctx().clone();
		let rows: Vec<Vec<RowKey>> = self.levels.iter().map(|l| l.rows.clone()).collect();
		let focused = ctx.memory(egui::Memory::focused);
		let held = focused
			.and_then(|f| rows.iter().enumerate().find_map(|(l, r)| r.iter().position(|k| k.id == f).map(|i| (l, i))));
		let panel = focused.and_then(|f| (0..rows.len()).find(|&l| level_id(self.id, l) == f));
		self.out.focused = held.is_some() || panel.is_some();
		let deepest = rows.len().saturating_sub(1);
		let level = held.map(|(l, _)| l).or(panel).unwrap_or(deepest);
		if let Some(l) = panel {
			lock(&ctx, level_id(self.id, l));
		}
		let presses = interact::plain_key_presses(&ctx);
		if !presses.is_empty() {
			self.keys_worked();
			// the next pass locks the row the keys focused
			ctx.request_repaint();
		}
		if layer::consume_escape(&ctx) {
			if level > 0 {
				self.close_level(level);
			} else {
				self.out.escaped = true;
			}
			return;
		}
		ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab));
		let enabled: Vec<Id> =
			rows.get(level).map_or_else(Vec::new, |r| r.iter().filter(|k| k.enabled).map(|k| k.id).collect());
		match held {
			Some((l, i)) => {
				let id = rows[l][i].id;
				// a letter shared by rows moved the focus
				if focus::modality(&ctx) == Modality::Keyboard {
					self.keys_worked();
				}
				RovingFocus::new(Axis::Vertical).update(self.core, &enabled, |_| true);
				lock(&ctx, id);
				match self.st.nav.walk(&ctx, Direction::Ltr, l, i, rows[l][i].submenu) {
					Some(Walk::Closed { parent }) => self.focus_parent(l, parent),
					Some(Walk::Bar(edge)) => self.out.nav = Some(edge),
					Some(Walk::Opened) | None => {}
				}
			}
			None => {
				for key in &presses {
					match key {
						Key::ArrowDown | Key::Home => self.st.nav.request_focus(level, FocusTarget::SelectedOrFirst),
						Key::ArrowUp | Key::End => self.st.nav.request_focus(level, FocusTarget::Last),
						Key::ArrowLeft if level > 0 => self.close_level(level),
						Key::ArrowLeft => self.out.nav = Some(Edge::Start),
						Key::ArrowRight => self.out.nav = Some(Edge::End),
						_ => {}
					}
				}
			}
		}
		self.st.nav.settle_hover(&ctx, SUBMENU_DELAY);
		self.bind(&rows);
	}

	/// Binds the letters of the level keys will go to next pass.
	fn bind(&self, rows: &[Vec<RowKey>]) {
		let ctx = self.core.ctx();
		let Some((level, _)) = key_level(ctx, rows) else { return };
		for row in rows[level].iter().filter(|r| r.enabled) {
			if let Some(m) = row.mnemonic {
				shortcut::bind_mnemonic(ctx, Scope::Global, m, Trigger::Either, row.id);
			}
		}
	}

	/// Closes on a press outside the panels and `inside`, or once a row was picked.
	fn settle(&mut self, inside: &[Rect]) {
		let mut rects: Vec<Rect> = self.levels.iter().map(|l| l.panel).collect();
		rects.extend_from_slice(inside);
		let fresh = self.st.opened == self.core.ctx().cumulative_pass_nr();
		if (!fresh && outside_press(self.core, &rects)) || self.out.picked || self.out.escaped {
			self.st.open = false;
		}
		self.st.rows = self.levels.iter().map(|l| l.rows.clone()).collect();
		let columns: Vec<Columns> = self.levels.iter().map(|l| l.columns).collect();
		if columns != self.st.columns {
			self.st.columns = columns;
			self.core.ctx().request_repaint();
		}
		if !self.st.open {
			self.st.nav.reset(None);
			self.st.panel = false;
			self.st.rows.clear();
		}
		mark_shown(self.core.ctx());
	}
}

/// Keeps egui's focus walk, Tab and Esc off the focused row: the menu takes them.
fn lock(ctx: &Context, id: Id) {
	ctx.memory_mut(|m| {
		m.set_focus_lock_filter(
			id,
			EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
		);
	});
}

fn surroundings(ctx: &Context) -> Surroundings {
	Surroundings::new(ctx.content_rect(), Direction::Ltr)
}

/// A level's last size, else the one its last widths give.
fn panel_size(ctx: &Context, id: Id, columns: Columns, min_width: f32) -> Vec2 {
	let width = columns.row_width(min_width) + 2. * (BORDER + MENU.padding);
	ctx.memory(|m| m.area_rect(id)).map_or(vec2(width, 0.), |r| vec2(width, r.height()))
}

/// What a level paints once its widths are known.
enum Deferred {
	Highlight(ShapeIdx, f32),
	Separator(f32),
	Shortcut(Arc<Galley>, f32, Color32),
	Arrow(f32, Color32),
}

/// A level of a menu being shown: rows are added top down, painted against the widths of the last pass.
pub(crate) struct MenuUi<'u, 'r> {
	ui: &'u mut Ui,
	run: &'u mut Run<'r>,
	level: usize,
	palette: &'static Palette,
	/// The rows' left edge and the next row's top.
	left: f32,
	y: f32,
	width: f32,
	columns: Columns,
	rows: Vec<RowKey>,
	deferred: Vec<Deferred>,
	/// A separator asked for after a row, drawn before the next one.
	separator: bool,
	background: ShapeIdx,
	top: f32,
	/// The row ordinal, for ids.
	index: usize,
}

impl<'u, 'r> MenuUi<'u, 'r> {
	fn new(ui: &'u mut Ui, run: &'u mut Run<'r>, level: usize) -> Self {
		let palette = Palette::of(ui.ctx().theme());
		ui.spacing_mut().item_spacing = Vec2::ZERO;
		let origin = ui.max_rect().min;
		let background = ui.painter().add(Shape::Noop);
		let width = run.st.columns.get(level).copied().unwrap_or_default().row_width(run.min_width);
		let inset = BORDER + MENU.padding;
		Self {
			ui,
			run,
			level,
			palette,
			left: origin.x + inset,
			y: origin.y + inset,
			top: origin.y,
			width,
			columns: Columns::default(),
			rows: Vec::new(),
			deferred: Vec::new(),
			separator: false,
			background,
			index: 0,
		}
	}

	fn level_id(&self) -> Id {
		level_id(self.run.id, self.level)
	}

	pub(crate) fn ctx(&self) -> &Context {
		self.ui.ctx()
	}

	/// Closes the menu once this pass ends.
	#[cfg_attr(target_arch = "wasm32", expect(dead_code, reason = "the browser reveals no recent file"))]
	pub(crate) fn close(&mut self) {
		self.run.out.picked = true;
	}

	/// A separator between the rows around it; none at either end or after another.
	pub(crate) fn separator(&mut self) {
		self.separator = !self.rows.is_empty();
	}

	fn flush_separator(&mut self) {
		if std::mem::take(&mut self.separator) {
			self.deferred.push(Deferred::Separator(self.y + MENU.separator_line));
			self.y += MENU.separator;
		}
	}

	/// A row; picking it closes the menu.
	pub(crate) fn item(&mut self, entry: Entry<'_>) -> Picked {
		let (response, _) = self.row(&entry, false);
		let picked = entry.enabled && (response.clicked() || self.run.activate == Some(response.id));
		if picked {
			self.run.out.picked = true;
		}
		Picked { response, picked }
	}

	/// A row that opens a submenu, which `add` fills.
	pub(crate) fn submenu<R>(&mut self, entry: Entry<'_>, add: impl FnOnce(&mut MenuUi<'_, '_>) -> R) -> Option<R> {
		let index = self.index;
		let (response, rect) = self.row(&entry, true);
		let level = self.level;
		let open = self.run.st.nav.open_child(level) == Some(index);
		if entry.enabled && response.clicked() && !open {
			let keys = focus::modality(self.ui.ctx()) == Modality::Keyboard;
			self.run.st.nav.open(self.ui.ctx(), level, index, keys);
		}
		if self.run.st.nav.open_child(level) != Some(index) {
			return None;
		}
		let ctx = self.ui.ctx().clone();
		let child = level_id(self.run.id, level + 1);
		let columns = self.run.st.columns.get(level + 1).copied().unwrap_or_default();
		let size = panel_size(&ctx, child, columns, 0.);
		let inset = BORDER + MENU.padding;
		let panel_left = self.left - inset;
		let panel_right = self.left + self.width + inset;
		let beside = Rect::from_min_max(
			pos2(panel_left + BORDER, rect.top() - inset),
			pos2(panel_right - BORDER, rect.bottom()),
		);
		let at = Anchor::new(Side::End, Align::Start).place(beside, size, surroundings(&ctx)).rect.min;
		Some(self.run.level(level + 1, at, add))
	}

	/// Lays out and interacts one row; paints what does not depend on the widths.
	fn row(&mut self, entry: &Entry<'_>, submenu: bool) -> (Response, Rect) {
		self.flush_separator();
		let p = self.palette;
		let index = self.index;
		let id = self.level_id().with(index);
		self.index += 1;
		let rect = Rect::from_min_size(pos2(self.left, self.y), vec2(self.width, MENU.row));
		self.y += MENU.row;
		let (text, mnemonic) = if entry.marked { parse(entry.text) } else { (entry.text.to_owned(), None) };
		let sense = if entry.enabled { Sense::click() } else { Sense::hover() };
		let response = self.ui.interact(rect, id, sense);
		let response = match entry.tip {
			Some(tip) if entry.enabled => response.on_hover_text(tip),
			Some(tip) => response.on_disabled_hover_text(tip),
			None => response,
		};
		describe(&response, &text, entry, submenu, mnemonic);
		self.rows.push(RowKey { id, enabled: entry.enabled, submenu, mnemonic });
		let open_child = submenu && self.run.st.nav.open_child(self.level) == Some(index);
		self.hover(&response, index, submenu);
		let focused = response.has_focus();
		// keys moved the focus off the hovered row: only the focused one shows
		let keys_hold = !focused && self.run.st.rows.iter().flatten().any(|r| self.ui.memory(|m| m.has_focus(r.id)));
		let highlight = entry.enabled && ((response.hovered() && !keys_hold) || focused || open_child);
		if highlight {
			let idx = self.ui.painter().add(Shape::Noop);
			self.deferred.push(Deferred::Highlight(idx, rect.top()));
		}
		let ink = if entry.enabled { p.text } else { p.text_disabled };
		let painter = self.ui.painter();
		let icon =
			Rect::from_center_size(pos2(rect.left() + MENU.icon_center, rect.center().y), Vec2::splat(MENU.icon));
		match entry.mark {
			Some((Mark::Check, true)) => paint_check(painter, icon.center(), ink),
			Some((Mark::Radio, true)) => {
				painter.circle_filled(icon.center(), MENU.radio, ink);
			}
			_ => match entry.lead {
				Lead::Icon(glyph) => {
					let font = FontId::new(MENU.icon, glyph.font_family());
					painter.text(icon.center(), Align2::CENTER_CENTER, glyph.codepoint, font, ink);
				}
				Lead::Drawn(drawn) => icons::paint(painter, icon, drawn, ink),
				Lead::None => {}
			},
		}
		let mut x = rect.left() + MENU.text;
		if let Some((size, paint)) = entry.swatch {
			paint(self.ui, Rect::from_min_size(pos2(x, rect.center().y - size.y / 2.), size));
			x += size.x + MENU.swatch_gap;
		}
		let job = label_job(self.run.core, &text, mnemonic, ink, entry.italic);
		let galley = self.ui.painter().layout_job(job);
		let text_width = x - rect.left() - MENU.text + galley.size().x;
		self.ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.), galley, ink);
		self.columns.text = self.columns.text.max(text_width);
		if let Some(keys) = &entry.shortcut {
			let shortcut_ink = if entry.enabled { p.menu_shortcut } else { p.text_disabled };
			let galley = self.ui.painter().layout_no_wrap(keys.clone(), font(), shortcut_ink);
			self.columns.shortcut = self.columns.shortcut.max(galley.size().x);
			self.deferred.push(Deferred::Shortcut(galley, rect.top(), shortcut_ink));
		}
		if submenu {
			self.deferred.push(Deferred::Arrow(rect.top(), if entry.enabled { p.text_dim } else { p.text_disabled }));
		}
		(response, rect)
	}

	/// The pointer over a row: the focus follows it while keys work the menu, a submenu opens or closes.
	fn hover(&mut self, response: &Response, index: usize, submenu: bool) {
		if !self.run.moved || !response.hovered() {
			return;
		}
		let ctx = self.ui.ctx().clone();
		let id = response.id;
		let held =
			ctx.memory(egui::Memory::focused).is_some_and(|f| self.run.st.rows.iter().flatten().any(|r| r.id == f));
		if held && response.sense.is_focusable() {
			ctx.memory_mut(|m| m.request_focus(id));
		}
		let level = self.level;
		let want = submenu && response.sense.senses_click();
		// with none open, a submenu opens at once
		if want && self.run.st.nav.open_child(level).is_none() {
			self.run.st.nav.open(&ctx, level, index, false);
			return;
		}
		let child = ctx.memory(|m| m.area_rect(level_id(self.run.id, level + 1)));
		let pointer = ctx.input(|i| i.pointer.latest_pos().map(|p| (p - i.pointer.delta(), p)));
		let toward = child.zip(pointer).is_some_and(|(r, (from, to))| SafeTriangle::new(from, r).contains(to));
		self.run.st.nav.hover(&ctx, level, index, want, toward);
	}

	/// Paints the panel and what waited for the widths, sizes the area, records the level.
	fn finish(self) {
		let Self { ui, run, level, palette: p, left, y, columns, rows, deferred, background, top, .. } = self;
		let width = columns.row_width(run.min_width);
		let inset = BORDER + MENU.padding;
		let panel = Rect::from_min_max(pos2(left - inset, top), pos2(left + width + inset, y + inset));
		let painter = ui.painter();
		painter.set(background, Shape::rect_filled(panel, 0., p.menu_background));
		painter.rect_stroke(panel, 0., Stroke::new(BORDER, p.menu_border), egui::StrokeKind::Inside);
		let font_height = |g: &Galley| g.size().y;
		for d in deferred {
			match d {
				Deferred::Highlight(idx, top) => {
					let rect = Rect::from_min_size(pos2(left, top), vec2(width, MENU.row));
					painter.set(idx, Shape::rect_filled(rect, 0., p.menu_hover));
				}
				Deferred::Separator(y) => {
					let x = left + MENU.separator_start..=left + width - MENU.separator_end;
					painter.hline(x, y + 0.5, Stroke::new(BORDER, p.menu_border));
				}
				Deferred::Shortcut(galley, top, ink) => {
					let x = left + MENU.text + columns.text + MENU.shortcut_gap;
					let h = font_height(&galley);
					painter.galley(pos2(x, top + (MENU.row - h) / 2.), galley, ink);
				}
				Deferred::Arrow(top, ink) => {
					let right = left + width - MENU.arrow_end;
					let cy = top + MENU.row / 2.;
					let (w, h) = (MENU.arrow.x, MENU.arrow.y / 2.);
					let points = vec![pos2(right - w, cy - h), pos2(right, cy), pos2(right - w, cy + h)];
					painter.add(Shape::convex_polygon(points, ink, Stroke::NONE));
				}
			}
		}
		ui.allocate_rect(panel, Sense::hover());
		let lid = level_id(run.id, level);
		ui.interact(panel, lid, Sense::FOCUSABLE);
		// the opening level focuses its first or last enabled row, or itself
		let mut enabled = rows.iter().filter(|r| r.enabled);
		let to = match run.st.nav.take_focus(level) {
			Some(FocusTarget::Last) => enabled.next_back().map(|r| r.id),
			Some(_) => enabled.next().map(|r| r.id),
			None if level == 0 && std::mem::take(&mut run.st.panel) => Some(lid),
			None => None,
		};
		if let Some(to) = to {
			ui.ctx().memory_mut(|m| m.request_focus(to));
		}
		if run.levels.len() <= level {
			run.levels.resize_with(level + 1, || Level {
				panel: Rect::NOTHING,
				rows: Vec::new(),
				columns: Columns::default(),
			});
		}
		run.levels[level] = Level { panel, rows, columns };
	}
}

fn font() -> FontId {
	FontId::new(FONT_SIZE, FontFamily::Proportional)
}

/// The text of a label marked as Next's (`_` before the mnemonic, `__` an underscore) and its mnemonic.
fn parse(marked: &str) -> (String, Option<Mnemonic>) {
	let (text, at) = split_mnemonic(marked);
	let Some(at) = at else { return (text, None) };
	let (head, tail) = text.split_at(at);
	Mnemonic::parse(&format!("{}&{}", head.replace('&', "&&"), tail.replace('&', "&&")))
}

/// `text` with its mnemonic underlined while mnemonics show.
fn label_job(core: &Core, text: &str, mnemonic: Option<Mnemonic>, ink: Color32, italic: bool) -> LayoutJob {
	let plain = TextFormat { font_id: font(), color: ink, italics: italic, ..Default::default() };
	shortcut::mnemonic_job(core, text, &plain, mnemonic, Stroke::new(1., ink))
}

/// The access key AccessKit reports for `mnemonic`.
fn access_key(mnemonic: Mnemonic) -> String {
	mnemonic.char().to_lowercase().collect()
}

/// Next's check mark around `center`.
fn paint_check(painter: &egui::Painter, center: Pos2, ink: Color32) {
	let points = MENU.check.map(|p| center + p);
	painter.add(Shape::line(points.to_vec(), Stroke::new(MENU.check_width, ink)));
}

/// Names a row by its text for assistive technology, with its state, access key and submenu.
fn describe(response: &Response, text: &str, entry: &Entry<'_>, submenu: bool, mnemonic: Option<Mnemonic>) {
	let enabled = entry.enabled;
	response.widget_info(|| match entry.mark {
		Some((Mark::Check, on)) => WidgetInfo::selected(Role::MenuItemCheckBox, enabled, on, text),
		Some((Mark::Radio, on)) => WidgetInfo::selected(Role::MenuItemRadio, enabled, on, text),
		None => WidgetInfo::labeled(Role::MenuItem, enabled, text),
	});
	response.ctx.accesskit_node_builder(response.id, |node| {
		if let Some(m) = mnemonic {
			node.set_access_key(access_key(m));
		}
		if submenu {
			node.set_has_popup(accesskit::HasPopup::Menu);
		}
	});
}

/// Shows the menu `id` of `st` while it is open; `inside` are rects a press on keeps it open. The content's result
/// and whether the menu stays open.
fn show_open<R>(
	ctx: &Context,
	id: Id,
	st: MenuState,
	owner: &Response,
	min_width: f32,
	add: impl FnOnce(&mut MenuUi<'_, '_>) -> R,
) -> Option<R> {
	ctx.accesskit_node_builder(owner.id, |b| {
		b.set_has_popup(accesskit::HasPopup::Menu);
		b.set_expanded(st.open);
	});
	if !st.open {
		st.store(ctx, id);
		return None;
	}
	let core = Core::of(ctx)?;
	let mut run = Run::new(&core, id, st, min_width, owner.layer_id);
	let r = run.show(add);
	run.keys();
	run.settle(&[owner.rect]);
	let Run { st, out, .. } = run;
	if !st.open {
		// keys worked it: the focus goes back to where they took it from
		if out.focused || out.escaped {
			let to = st.return_to.unwrap_or(owner.id);
			ctx.memory_mut(|m| m.request_focus(to));
		}
		shortcut::set_mnemonics_revealed(ctx, false);
	}
	st.store(ctx, id);
	Some(r)
}

/// Opens `st` at `anchor`, `focus` its row to focus, none the level itself.
fn open(ctx: &Context, st: &mut MenuState, anchor: Rect, focus: Option<FocusTarget>) {
	let columns = std::mem::take(&mut st.columns);
	let opened = ctx.cumulative_pass_nr();
	let return_to = ctx.memory(egui::Memory::focused);
	*st = MenuState { open: true, anchor, panel: focus.is_none(), columns, opened, return_to, ..Default::default() };
	st.nav.reset(focus);
}

/// The first row for a menu keys opened.
fn keyboard_end(ctx: &Context) -> Option<FocusTarget> {
	(focus::modality(ctx) == Modality::Keyboard).then_some(FocusTarget::SelectedOrFirst)
}

/// A drop-down below `trigger`, toggled by its click, at least `min_width` wide; the content's result while shown.
pub(crate) fn below<R>(trigger: &Response, min_width: f32, add: impl FnOnce(&mut MenuUi<'_, '_>) -> R) -> Option<R> {
	let ctx = trigger.ctx.clone();
	let id = trigger.id.with("classic::menu");
	let mut st = MenuState::load(&ctx, id);
	if trigger.clicked() {
		if st.open {
			st.open = false;
		} else {
			open(&ctx, &mut st, trigger.rect, keyboard_end(&ctx));
		}
	}
	st.anchor = trigger.rect;
	show_open(&ctx, id, st, trigger, min_width, add)
}

/// A context menu of `target`: at the pointer on a secondary click, at its bottom left on Shift+F10 or the Apps key
/// while it has the focus.
pub(crate) fn context<R>(target: &Response, add: impl FnOnce(&mut MenuUi<'_, '_>) -> R) -> Option<R> {
	let ctx = target.ctx.clone();
	let id = target.id.with("classic::menu");
	let mut st = MenuState::load(&ctx, id);
	let at = if target.secondary_clicked() {
		target.interact_pointer_pos()
	} else {
		let keys = target.has_focus()
			&& ctx.input_mut(|i| {
				i.consume_key(egui::Modifiers::SHIFT, Key::F10) || i.consume_key(egui::Modifiers::NONE, Key::Menu)
			});
		keys.then(|| target.rect.left_bottom())
	};
	if let Some(at) = at {
		open(&ctx, &mut st, Rect::from_min_size(at, Vec2::ZERO), keyboard_end(&ctx));
	}
	show_open(&ctx, id, st, target, 0., add)
}

/// The menu `id` of `owner`, shown at `at` while the caller keeps it `open`.
pub(crate) fn popup<R>(
	owner: &Response,
	id: Id,
	at: Pos2,
	open_now: &mut bool,
	add: impl FnOnce(&mut MenuUi<'_, '_>) -> R,
) -> Option<R> {
	let ctx = owner.ctx.clone();
	let mut st = MenuState::load(&ctx, id);
	if *open_now && (!st.open || st.anchor.min != at) {
		open(&ctx, &mut st, Rect::from_min_size(at, Vec2::ZERO), keyboard_end(&ctx));
	}
	st.open &= *open_now;
	let r = show_open(&ctx, id, st, owner, 0., add);
	*open_now = MenuState::load(&ctx, id).open;
	r
}

/// The menu bar: `headers` are the menus' marked titles, `add` fills the open one; `enabled` false greys it and
/// keeps its keys off. The content's result while a menu shows.
pub(crate) fn bar<R>(
	ui: &mut ClassicUi<'_>,
	headers: &[&str],
	enabled: bool,
	mut add: impl FnMut(usize, &mut MenuUi<'_, '_>) -> R,
) -> Option<R> {
	let ctx = ui.ctx().clone();
	let core = ui.core();
	let p = Palette::of(ctx.theme());
	let id = ui.make_persistent_id("classic::menu_bar");
	let mut bar = BarState::load(&ctx, id);
	let width = ui.available_width();
	let (rect, _) = ui.allocate_exact_size(vec2(width, MENU.bar_height), Sense::hover());
	ui.painter().rect_filled(rect, 0., p.window_background);
	if !enabled {
		bar.open = None;
	}
	let availability = Availability::disabled_if(!enabled);
	// the bar takes the focus only while keys work it, which keeps it out of the Tab cycle
	let sense = if bar.active { Sense::click() } else { Sense::CLICK };
	let mut x = rect.left() + MENU.bar_inset;
	let mut responses = Vec::with_capacity(headers.len());
	let mut mnemonics = Vec::with_capacity(headers.len());
	for (i, marked) in headers.iter().enumerate() {
		let (text, mnemonic) = parse(marked);
		let ink = if enabled { p.text } else { p.text_disabled };
		let job = label_job(core, &text, mnemonic.filter(|_| enabled), ink, false);
		let galley = ui.painter().layout_job(job);
		let r = Rect::from_min_size(
			pos2(x, rect.top() + (MENU.bar_height - MENU.header_height) / 2.),
			vec2(galley.size().x + 2. * MENU.header_padding, MENU.header_height),
		);
		x = r.right();
		let response = interact::interact(ui, r, id.with(("header", i)), sense, availability);
		if enabled && (response.hovered() || response.has_focus() || bar.open == Some(i)) {
			ui.painter().rect_filled(r, 0., p.menu_hover);
		}
		ui.painter().galley(pos2(r.left() + MENU.header_padding, r.center().y - galley.size().y / 2.), galley, ink);
		response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, &text));
		let expanded = bar.open == Some(i);
		ctx.accesskit_node_builder(response.id, |node| {
			if let Some(m) = mnemonic {
				node.set_access_key(access_key(m));
			}
			node.set_has_popup(accesskit::HasPopup::Menu);
			node.set_expanded(expanded);
		});
		mnemonics.push(mnemonic);
		responses.push(response);
	}
	let ids: Vec<Id> = responses.iter().map(|r| r.id).collect();
	if enabled {
		react(&ctx, id, &mut bar, &responses, &mnemonics);
	}
	bar.toggle_activation(core, availability, &ids, 0);
	if enabled
		&& bar.open.is_none()
		&& let Some(f) = ids.iter().position(|&h| ctx.memory(|m| m.has_focus(h)))
	{
		if layer::consume_escape_from(&ctx, ids[f]) {
			leave(&ctx, &mut bar);
		} else {
			RovingFocus::new(Axis::Horizontal).update_group(core, id, &ids, |_| true);
			lock(&ctx, ids[f]);
			ctx.request_repaint();
		}
	}
	let result = bar.open.map(|i| {
		let menu = id.with(("menu", i));
		let mut st = MenuState::load(&ctx, menu);
		if !st.open {
			open(&ctx, &mut st, responses[i].rect, None);
		}
		st.anchor = responses[i].rect;
		let mut run = Run::new(core, menu, st, 0., ui.layer_id());
		let r = run.show(|m| add(i, m));
		run.keys();
		run.settle(&[responses[i].rect]);
		let Run { mut st, out, .. } = run;
		bar.revealed |= shortcut::mnemonics_revealed(&ctx);
		if let Some(edge) = out.nav {
			let next = menubar::adjacent(i, edge, headers.len());
			st.open = false;
			bar.open = Some(next);
			reopen(&ctx, id, next, responses[next].rect, Some(FocusTarget::SelectedOrFirst), st.return_to);
		} else if out.escaped {
			// back to the header, the bar keeps the keys
			bar.open = None;
			bar.active = true;
			bar.revealed = true;
			shortcut::set_mnemonics_revealed(&ctx, true);
			bar.return_to = bar.return_to.or(st.return_to);
			menubar::refocus_title(&ctx, id, i, ids[i]);
		} else if !st.open {
			bar.open = None;
			let back = bar.return_to.take().or(st.return_to);
			let worked = out.focused || ids.iter().any(|&h| ctx.memory(|m| m.has_focus(h)));
			bar.close(&ctx);
			if worked {
				ctx.memory_mut(|m| match back {
					Some(to) => m.request_focus(to),
					None => ids.iter().for_each(|&h| m.surrender_focus(h)),
				});
			}
		}
		st.store(&ctx, menu);
		r
	});
	bar.settle(&ctx, &ids);
	bar.store(&ctx, id);
	result
}

/// Leaves the bar: the focus goes back to where the keys took it from.
fn leave(ctx: &Context, bar: &mut BarState) {
	let back = bar.return_to.take();
	let focused = ctx.memory(egui::Memory::focused);
	bar.close(ctx);
	ctx.memory_mut(|m| match back {
		Some(to) => m.request_focus(to),
		None => focused.into_iter().for_each(|f| m.surrender_focus(f)),
	});
}

/// The headers' letters, clicks, hover and keys.
fn react(ctx: &Context, id: Id, bar: &mut BarState, headers: &[Response], mnemonics: &[Option<Mnemonic>]) {
	let menu_open = bar.open.is_some();
	for (i, h) in headers.iter().enumerate() {
		// an open menu takes the letters for its rows
		if !menu_open && let Some(m) = mnemonics[i] {
			let trigger = if bar.active { Trigger::Either } else { Trigger::Alt };
			shortcut::bind_mnemonic(ctx, Scope::Global, m, trigger, h.id);
		}
		if !menu_open && shortcut::take_item(ctx, h.id) {
			if !headers.iter().any(Response::has_focus) {
				bar.return_to = bar.return_to.or(ctx.memory(egui::Memory::focused));
			}
			bar.open = Some(i);
			bar.revealed = true;
			shortcut::set_mnemonics_revealed(ctx, true);
			reopen(ctx, id, i, h.rect, Some(FocusTarget::SelectedOrFirst), None);
		} else if h.clicked() {
			bar.open = if bar.open == Some(i) { None } else { Some(i) };
			if bar.open.is_some() {
				reopen(ctx, id, i, h.rect, keyboard_end(ctx).filter(|_| h.has_focus()), None);
			}
		} else if bar.open.is_some_and(|o| o != i) && h.hovered() && ctx.input(|i| i.pointer.delta() != Vec2::ZERO) {
			bar.open = Some(i);
			reopen(ctx, id, i, h.rect, None, None);
		} else if h.has_focus() && bar.open.is_none() {
			for key in interact::plain_key_presses(ctx) {
				let end = match key {
					Key::ArrowDown => FocusTarget::SelectedOrFirst,
					Key::ArrowUp => FocusTarget::Last,
					_ => continue,
				};
				ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key));
				bar.open = Some(i);
				reopen(ctx, id, i, h.rect, Some(end), None);
				break;
			}
		}
	}
}

/// Opens menu `i` of the bar fresh at `anchor`; `back` is where its keys' focus returns to.
fn reopen(ctx: &Context, bar: Id, i: usize, anchor: Rect, end: Option<FocusTarget>, back: Option<Id>) {
	let menu = bar.with(("menu", i));
	let mut st = MenuState::load(ctx, menu);
	open(ctx, &mut st, anchor, end);
	st.return_to = back.or(st.return_to);
	st.store(ctx, menu);
}
