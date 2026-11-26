//! The panes round the preview: a [`Dock`] mapped into ui/app.slint's DockArea models and the panes' places
//! (docs/ui/00, Panes and default arrangement).

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ComponentHandle, Model, ModelRc, VecModel};
use slint_dock::{Arrangement, Axis, Dock, GroupId, Layout, Metrics, Point, Rect, Side, Size, Subject};

use crate::ui::{self, AppWindow};

/// The panes in View > Panes order with their default sides (Next's MainWindow.xaml).
const PANES: [(&str, Side); 4] =
	[("project", Side::Left), ("properties", Side::Right), ("messages", Side::Bottom), ("performance", Side::Bottom)];
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
}

/// Next's first-start layout: a left column of a quarter, Project over Properties 1 : 2; the rest hidden.
fn default_arrangement() -> Arrangement {
	let left = Arrangement::split(
		Axis::Vertical,
		[(1., Arrangement::tabs(["project"])), (2., Arrangement::tabs(["properties"]))],
	);
	Arrangement::split(Axis::Horizontal, [(1., left), (3., Arrangement::Centre)])
}

/// Sets up the dock in its default layout and wires the window's dock callbacks.
pub(crate) fn wire(app: &AppWindow) {
	let mut dock = Dock::new(METRICS);
	for (id, side) in PANES {
		dock.register(id, side, PANE_MIN);
	}
	dock.arrange(&default_arrangement()).expect("the default arrangement has one centre");
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
	let (dk, weak) = (docking.clone(), app.as_weak());
	app.on_dock_activate(move |pane| {
		let Some(app) = weak.upgrade() else { return };
		let mut d = dk.borrow_mut();
		d.dock.activate(&pane);
		d.focused = d.dock.group_of(&pane);
		d.refresh(&app);
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
			d.refresh(&app);
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
		d.refresh(&app);
	});
}

impl Docking {
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
		update(&self.groups, groups);
		update(&self.splitters, splitters.collect());
		for (id, _) in PANES {
			let at = places.iter().find(|(p, _)| *p == id).map(|(_, p)| p.clone());
			set_pane(app, id, at.is_some(), at.unwrap_or_default());
		}
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

/// Replaces `model`'s rows in place when their count holds, so a dragged splitter keeps its pointer grab.
fn update<T: Clone + 'static>(model: &VecModel<T>, rows: Vec<T>) {
	if model.row_count() == rows.len() {
		for (i, row) in rows.into_iter().enumerate() {
			model.set_row_data(i, row);
		}
	} else {
		model.set_vec(rows);
	}
}

fn set_pane(app: &AppWindow, id: &str, shown: bool, at: ui::DockPlace) {
	match id {
		"project" => {
			app.set_project_shown(shown);
			app.set_project_place(at);
		}
		"properties" => {
			app.set_properties_shown(shown);
			app.set_properties_place(at);
		}
		"messages" => {
			app.set_messages_shown(shown);
			app.set_messages_place(at);
		}
		"performance" => {
			app.set_performance_shown(shown);
			app.set_performance_place(at);
		}
		_ => unreachable!("a pane of PANES"),
	}
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
		let mut dock = Dock::new(METRICS);
		for (id, side) in PANES {
			dock.register(id, side, PANE_MIN);
		}
		dock.arrange(&default_arrangement()).expect("one centre");
		dock
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
}
