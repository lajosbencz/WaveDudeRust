//! The panes round the preview: a [`Dock`] mapped into ui/app.slint's DockArea models and the panes' places
//! (docs/ui/00, Panes and default arrangement). The arrangement is the settings' `Layout`, its JSON written once
//! a change is let go of.

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, VecModel};
use slint_dock::{Arrangement, Axis, Dock, GroupId, Layout, Metrics, Point, Rect, Side, Size, Subject, Target};

use crate::models;
use crate::ui::{self, AppWindow};

/// The panes round the preview, each with its `*-shown` and `*-place` properties of ui/app.slint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pane {
	Project,
	Properties,
	Messages,
	Performance,
}

impl Pane {
	/// View > Panes order.
	const ALL: [Self; 4] = [Self::Project, Self::Properties, Self::Messages, Self::Performance];

	/// The id the dock and the window know the pane by.
	const fn id(self) -> &'static str {
		match self {
			Self::Project => "project",
			Self::Properties => "properties",
			Self::Messages => "messages",
			Self::Performance => "performance",
		}
	}

	/// Next's MainWindow.xaml.
	const fn side(self) -> Side {
		match self {
			Self::Project => Side::Left,
			Self::Properties => Side::Right,
			Self::Messages | Self::Performance => Side::Bottom,
		}
	}

	fn set(self, app: &AppWindow, shown: bool, at: ui::DockPlace) {
		match self {
			Self::Project => {
				app.set_project_shown(shown);
				app.set_project_place(at);
			}
			Self::Properties => {
				app.set_properties_shown(shown);
				app.set_properties_place(at);
			}
			Self::Messages => {
				app.set_messages_shown(shown);
				app.set_messages_place(at);
			}
			Self::Performance => {
				app.set_performance_shown(shown);
				app.set_performance_place(at);
			}
		}
	}
}

/// Next's DockHost minimums: 60 px a pane, 100 px the preview.
const PANE_MIN: Size = Size::new(60., 60.);
/// The title bar and the tab strip are a row high (Theme.row-height).
const METRICS: Metrics = Metrics { splitter: 4., title_bar: 23., tab_strip: 23., centre_min: Size::new(100., 100.) };

struct Docking {
	dock: Dock,
	area: Rect,
	layout: Layout,
	/// The group last pressed or shown, the centre included; its title bar is lit, the first group's while it
	/// is none or gone.
	focused: Option<GroupId>,
	/// A refresh waits for the drag's end.
	stale: bool,
	groups: Rc<VecModel<ui::DockGroup>>,
	splitters: Rc<VecModel<ui::DockSplitter>>,
	saver: Saver,
}

/// Writes the arrangement through `sink` when it differs from the last one written or read.
struct Saver {
	last: String,
	sink: Box<dyn Fn(String)>,
}

impl Saver {
	fn new(dock: &Dock, sink: impl Fn(String) + 'static) -> Self {
		Self { last: text(dock), sink: Box::new(sink) }
	}

	fn commit(&mut self, dock: &Dock) {
		let text = text(dock);
		if text != self.last {
			self.last.clone_from(&text);
			(self.sink)(text);
		}
	}
}

/// Next's first-start layout: a left column of a quarter, Project over Properties 1 : 2; the rest hidden.
fn default_arrangement() -> Arrangement {
	let left = Arrangement::split(
		Axis::Vertical,
		[(1., Arrangement::tabs([Pane::Project.id()])), (2., Arrangement::tabs([Pane::Properties.id()]))],
	);
	Arrangement::split(Axis::Horizontal, [(1., left), (3., Arrangement::Centre)])
}

/// The dock of the registered panes in the `saved` arrangement's JSON, else in the default one; an unreadable one
/// is logged.
fn restore(saved: Option<&str>) -> Dock {
	let mut dock = Dock::new(METRICS);
	for pane in Pane::ALL {
		dock.register(pane.id(), pane.side(), PANE_MIN);
	}
	let read = saved.map(|text| {
		serde_json::from_str::<Arrangement>(text)
			.map_err(|e| e.to_string())
			.and_then(|a| dock.arrange(&a).map_err(|e| e.to_string()))
	});
	if let Some(Err(e)) = &read {
		tracing::warn!("the saved pane layout cannot be read, the default is used: {e}");
	}
	if !matches!(read, Some(Ok(()))) {
		dock.arrange(&default_arrangement()).expect("the default arrangement has one centre");
	}
	dock
}

/// Sets up the dock in the `saved` arrangement and wires the window's dock callbacks; `save` takes the
/// arrangement after each change.
pub(crate) fn wire(app: &AppWindow, saved: Option<&str>, save: impl Fn(String) + 'static) {
	let dock = restore(saved);
	let saver = Saver::new(&dock, save);
	let (groups, splitters) = (Rc::new(VecModel::default()), Rc::new(VecModel::default()));
	app.set_dock_metrics(ui::DockMetrics {
		title: METRICS.title_bar,
		tab: METRICS.tab_strip,
		splitter: METRICS.splitter,
	});
	app.set_dock_groups(ModelRc::from(groups.clone()));
	app.set_dock_splitters(ModelRc::from(splitters.clone()));
	let docking = Rc::new(RefCell::new(Docking {
		dock,
		area: Rect::new(0., 0., app.get_dock_width(), app.get_dock_height()),
		layout: Layout::default(),
		focused: None,
		stale: false,
		groups,
		splitters,
		saver,
	}));
	docking.borrow_mut().refresh(app);

	let (dk, weak) = (docking.clone(), app.as_weak());
	app.on_dock_resized(move || {
		let Some(app) = weak.upgrade() else { return };
		let mut d = dk.borrow_mut();
		d.area = Rect::new(0., 0., app.get_dock_width(), app.get_dock_height());
		d.refresh(&app);
	});
	let (dk, weak) = (docking.clone(), app.as_weak());
	app.on_dock_splitter_moved(move |index, x, y| {
		let Some(app) = weak.upgrade() else { return };
		let mut d = dk.borrow_mut();
		let Some(bar) = usize::try_from(index).ok().and_then(|i| d.layout.splitters.get(i).copied()) else { return };
		let area = d.area;
		if d.dock.resize(&bar, Point::new(x, y), area) {
			d.refresh(&app);
		}
	});
	let dk = docking.clone();
	app.on_dock_splitter_released(move || {
		let d = &mut *dk.borrow_mut();
		d.saver.commit(&d.dock);
	});
	let (dk, weak) = (docking.clone(), app.as_weak());
	app.on_dock_activate(move |pane| {
		let Some(app) = weak.upgrade() else { return };
		let mut d = dk.borrow_mut();
		d.dock.activate(&pane);
		d.focused = d.dock.group_of(&pane);
		d.changed(&app);
	});
	let (dk, weak) = (docking.clone(), app.as_weak());
	app.on_dock_move_to(move |pane, side| {
		let Some(app) = weak.upgrade() else { return };
		let mut d = dk.borrow_mut();
		if d.dock.move_to(&Subject::Pane(pane.to_string()), Target::Edge(edge(side))) {
			d.focused = d.dock.group_of(&pane);
			d.changed(&app);
		}
	});
	let dk = docking.clone();
	app.on_dock_drag_over(move |pane, group, x, y| {
		let d = dk.borrow();
		let drop = subject(&d.dock, &pane, group).and_then(|s| d.dock.drop_target(&s, Point::new(x, y), d.area));
		drop.map_or_else(Default::default, |t| {
			let r = t.preview;
			ui::DockPreview { shown: true, x: r.x, y: r.y, width: r.width, height: r.height }
		})
	});
	let (dk, weak) = (docking.clone(), app.as_weak());
	app.on_dock_dropped(move |pane, group, x, y| {
		let Some(app) = weak.upgrade() else { return };
		let mut d = dk.borrow_mut();
		let area = d.area;
		if drop_at(&mut d.dock, &pane, group, Point::new(x, y), area) {
			d.focused = d.dock.group_of(&pane);
			d.changed(&app);
		} else if d.stale {
			d.refresh(&app);
		}
	});
	let (dk, weak) = (docking.clone(), app.as_weak());
	app.on_dock_cancelled(move || {
		let Some(app) = weak.upgrade() else { return };
		let mut d = dk.borrow_mut();
		if d.stale {
			d.refresh(&app);
		}
	});
	// a press anywhere in a group lights it, as Next's focus does; the window's events report it unconsumed
	let (dk, weak) = (docking.clone(), app.as_weak());
	app.on_dock_pressed(move |x, y| {
		let Some(app) = weak.upgrade() else { return };
		let mut d = dk.borrow_mut();
		let at = Point::new(x - app.get_dock_x(), y - app.get_dock_y());
		let hit = d.layout.groups.iter().find(|g| g.rect.contains(at)).map(|g| g.id);
		if hit.is_some() && hit != d.focused {
			d.focused = hit;
			d.refresh(&app);
		}
	});
	let weak = app.as_weak();
	app.on_pane_shown(move |pane, shown| {
		let Some(app) = weak.upgrade() else { return };
		let mut d = docking.borrow_mut();
		if shown {
			d.dock.show(&pane);
			d.focused = d.dock.group_of(&pane);
		} else {
			d.dock.close(&pane);
		}
		d.changed(&app);
	});
}

impl Docking {
	/// Shows and saves a change of the arrangement.
	fn changed(&mut self, app: &AppWindow) {
		self.refresh(app);
		self.saver.commit(&self.dock);
	}

	/// Lays the dock out anew into the window's models and the panes' places, once no drag is on: new models would
	/// drop the instance of the pressed title bar or tab, and with it the pointer grab the drag lives on.
	fn refresh(&mut self, app: &AppWindow) {
		self.stale = app.get_dock_dragging();
		if self.stale {
			return;
		}
		self.layout = self.dock.layout(self.area);
		let chrome: Vec<_> = self.layout.groups.iter().filter(|g| g.id != GroupId::CENTRE).collect();
		let focused = self
			.focused
			.filter(|&f| f == GroupId::CENTRE || chrome.iter().any(|g| g.id == f))
			.or(chrome.first().map(|g| g.id));
		let mut groups = Vec::with_capacity(chrome.len());
		let mut places: Vec<(&str, ui::DockPlace)> = Vec::new();
		for g in &self.layout.groups {
			if g.id == GroupId::CENTRE {
				app.set_preview_place(place(g.content, true));
				continue;
			}
			places.extend(g.tabs.iter().enumerate().map(|(i, t)| (t.as_str(), place(g.content, g.active == Some(i)))));
			let tabs: Vec<ui::DockTab> = g
				.tabs
				.iter()
				.enumerate()
				.map(|(i, t)| ui::DockTab { pane: t.as_str().into(), active: g.active == Some(i) })
				.collect();
			groups.push(ui::DockGroup {
				x: g.rect.x,
				y: g.rect.y,
				width: g.rect.width,
				height: g.rect.height,
				pane: g.active.and_then(|i| g.tabs.get(i)).map_or_else(Default::default, |t| t.as_str().into()),
				active: focused == Some(g.id),
				tabs: ModelRc::new(VecModel::from(tabs)),
			});
		}
		let splitters = self.layout.splitters.iter().map(|s| ui::DockSplitter {
			x: s.rect.x,
			y: s.rect.y,
			width: s.rect.width,
			height: s.rect.height,
			rows: s.axis == Axis::Vertical,
		});
		models::update(&self.groups, groups);
		models::update(&self.splitters, splitters);
		for pane in Pane::ALL {
			let at = places.iter().find(|(p, _)| *p == pane.id()).map(|(_, p)| p.clone());
			pane.set(app, at.is_some(), at.unwrap_or_default());
		}
	}
}

/// The arrangement's JSON.
fn text(dock: &Dock) -> String {
	serde_json::to_string(&dock.arrangement()).expect("an arrangement serialises")
}

fn edge(side: ui::DockSide) -> Side {
	match side {
		ui::DockSide::Left => Side::Left,
		ui::DockSide::Right => Side::Right,
		ui::DockSide::Top => Side::Top,
		ui::DockSide::Bottom => Side::Bottom,
	}
}

/// What a drag of `pane` moves: its group by the title bar (`group`), else the pane by its tab.
fn subject(dock: &Dock, pane: &str, group: bool) -> Option<Subject> {
	if group { dock.group_of(pane).map(Subject::Group) } else { Some(Subject::Pane(pane.to_owned())) }
}

/// Drops the drag of `pane` at `pointer` in `area`. Whether the layout changed.
fn drop_at(dock: &mut Dock, pane: &str, group: bool, pointer: Point, area: Rect) -> bool {
	let Some(subject) = subject(dock, pane, group) else { return false };
	dock.drop_target(&subject, pointer, area).is_some_and(|t| dock.move_to(&subject, t.target))
}

fn place(r: Rect, visible: bool) -> ui::DockPlace {
	ui::DockPlace { x: r.x, y: r.y, width: r.width, height: r.height, visible }
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn default_layout_is_a_left_quarter_of_project_over_properties() {
		let dock = first_start();
		let layout = dock.layout(Rect::new(0., 0., 1004., 604.));
		let ids: Vec<_> = layout.groups.iter().map(|g| g.tabs.clone()).collect();
		assert_eq!(ids, [vec!["project".to_owned()], vec!["properties".to_owned()], vec![]]);
		assert_eq!(layout.groups[0].rect.width, 250.);
		assert_eq!(layout.groups[0].rect.height, 200.);
		assert!(!dock.is_shown("messages") && !dock.is_shown("performance"));
	}

	fn first_start() -> Dock {
		restore(None)
	}

	const AREA: Rect = Rect::new(0., 0., 1004., 604.);

	#[test]
	fn a_title_bar_drags_its_group_and_a_tab_its_pane() {
		let mut dock = first_start();
		let project = dock.group_of("project").expect("shown");
		assert_eq!(subject(&dock, "project", true), Some(Subject::Group(project)));
		assert_eq!(subject(&dock, "project", false), Some(Subject::Pane("project".into())));
		assert_eq!(subject(&dock, "messages", true), None);
		// tab Project into Properties, then drag the pair by its title bar to the right edge
		assert!(drop_at(&mut dock, "project", false, Point::new(125., 400.), AREA));
		assert_eq!(dock.group_of("project"), dock.group_of("properties"));
		assert!(drop_at(&mut dock, "properties", true, Point::new(1000., 300.), AREA));
		let right = dock.layout(AREA).groups.into_iter().find(|g| g.tabs.len() == 2).expect("the pair");
		assert_eq!(right.rect.x + right.rect.width, AREA.width);
	}

	#[test]
	fn a_release_off_a_target_changes_nothing() {
		let mut dock = first_start();
		let before = dock.arrangement();
		// over its own group, and outside the dock
		assert!(!drop_at(&mut dock, "project", true, Point::new(125., 100.), AREA));
		assert!(!drop_at(&mut dock, "project", false, Point::new(-5., 100.), AREA));
		assert_eq!(dock.arrangement(), before);
	}

	#[test]
	fn a_saved_arrangement_restores_with_its_hidden_panes_and_unknown_ids_dropped() {
		let mut dock = first_start();
		assert!(dock.show("messages"));
		assert!(dock.move_to(&Subject::Pane("project".into()), Target::Edge(Side::Right)));
		assert!(dock.close("messages"));
		let saved = text(&dock);
		let mut restored = restore(Some(&saved));
		assert_eq!(restored.arrangement(), dock.arrangement());
		assert_eq!(places(&restored), places(&dock));
		assert!(!restored.is_shown("messages"));
		// Messages comes back where it was
		assert!(restored.show("messages") && dock.show("messages"));
		assert_eq!(places(&restored), places(&dock));

		let with_unknown = saved.replacen("\"project\"", "\"gone\"", 1);
		let restored = restore(Some(&with_unknown));
		assert!(!restored.is_shown("project") && restored.is_shown("properties"));
	}

	/// The groups' panes and rectangles; a restore gives fresh group ids.
	fn places(dock: &Dock) -> Vec<(Vec<String>, Rect)> {
		dock.layout(AREA).groups.into_iter().map(|g| (g.tabs, g.rect)).collect()
	}

	#[test]
	fn an_unreadable_arrangement_falls_back_to_the_default() {
		let default = first_start().arrangement();
		for text in ["{", "\"Tabs\"", r#"{"Tabs":{"tabs":[{"id":"project","shown":true}],"active":0}}"#] {
			assert_eq!(restore(Some(text)).arrangement(), default, "{text}");
		}
	}

	#[test]
	fn a_change_saves_once_and_an_unchanged_arrangement_not_at_all() {
		let saved = Rc::new(RefCell::new(Vec::new()));
		let mut dock = first_start();
		let sink = saved.clone();
		let mut saver = Saver::new(&dock, move |t| sink.borrow_mut().push(t));
		saver.commit(&dock);
		assert!(saved.borrow().is_empty());
		assert!(dock.move_to(&Subject::Pane("properties".into()), Target::Edge(Side::Right)));
		saver.commit(&dock);
		saver.commit(&dock);
		assert_eq!(*saved.borrow(), [text(&dock)]);
		assert_eq!(restore(saved.borrow().last().map(String::as_str)).arrangement(), dock.arrangement());
	}
}
