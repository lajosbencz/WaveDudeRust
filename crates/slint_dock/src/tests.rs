use super::*;

const AREA: Rect = Rect::new(0., 0., 1004., 604.);
const MIN: Size = Size::new(50., 50.);

fn approx(a: f32, b: f32) -> bool {
	(a - b).abs() < 1e-3
}

fn assert_rect(r: Rect, x: f32, y: f32, w: f32, h: f32) {
	assert!(
		approx(r.x, x) && approx(r.y, y) && approx(r.width, w) && approx(r.height, h),
		"{r:?} is not ({x}, {y}, {w}, {h})"
	);
}

fn registered() -> Dock {
	let mut dock =
		Dock::new(Metrics { splitter: 4., title_bar: 20., tab_strip: 16., centre_min: Size::new(100., 100.) });
	dock.register("a", Side::Left, MIN);
	dock.register("b", Side::Left, MIN);
	dock.register("m", Side::Bottom, MIN);
	dock
}

/// Left column a quarter of the width (1 : 3), "a" over "b" at 1 : 2, "m" hidden.
fn first_start() -> Arrangement {
	let left = Arrangement::split(Axis::Vertical, [(1., Arrangement::tabs(["a"])), (2., Arrangement::tabs(["b"]))]);
	Arrangement::split(Axis::Horizontal, [(1., left), (3., Arrangement::Centre)])
}

fn dock() -> Dock {
	let mut dock = registered();
	dock.arrange(&first_start()).expect("a valid arrangement");
	dock
}

fn group(dock: &Dock, id: GroupId) -> GroupLayout {
	dock.layout(AREA).groups.into_iter().find(|g| g.id == id).expect("a shown group")
}

fn pane_rect(dock: &Dock, pane: &str) -> Rect {
	group(dock, dock.group_of(pane).expect("a shown pane")).rect
}

fn target(dock: &Dock, subject: &Subject, x: f32, y: f32) -> Option<Target> {
	dock.drop_target(subject, Point::new(x, y), AREA).map(|d| d.target)
}

fn pane(id: &str) -> Subject {
	Subject::Pane(id.into())
}

#[test]
fn the_first_start_arrangement_lays_out_its_shares() {
	let dock = dock();
	let layout = dock.layout(AREA);
	assert_eq!(layout.groups.len(), 3);
	assert_rect(pane_rect(&dock, "a"), 0., 0., 250., 200.);
	assert_rect(pane_rect(&dock, "b"), 0., 204., 250., 400.);
	assert_rect(group(&dock, GroupId::CENTRE).rect, 254., 0., 750., 604.);
	let a = group(&dock, dock.group_of("a").expect("shown"));
	assert_rect(a.content, 0., 20., 250., 180.);
	assert_eq!((a.tabs, a.active), (vec!["a".to_owned()], Some(0)));
	assert_eq!(group(&dock, GroupId::CENTRE).content, group(&dock, GroupId::CENTRE).rect);
	assert!(!dock.is_shown("m"));
	let bars: Vec<(Axis, Rect)> = layout.splitters.iter().map(|s| (s.axis, s.rect)).collect();
	assert_eq!(bars.len(), 2);
	assert_eq!(bars[0].0, Axis::Vertical);
	assert_rect(bars[0].1, 0., 200., 250., 4.);
	assert_eq!(bars[1].0, Axis::Horizontal);
	assert_rect(bars[1].1, 250., 0., 4., 604.);
}

#[test]
fn minimum_sizes_hold_against_the_weights() {
	let mut dock = dock();
	dock.register("a", Side::Left, Size::new(120., 50.));
	assert_rect(pane_rect(&dock, "a"), 0., 0., 250., 200.);
	let narrow = Rect::new(0., 0., 304., 604.);
	let a = dock.layout(narrow).groups.into_iter().find(|g| g.tabs == ["a"]).expect("shown");
	assert_rect(a.rect, 0., 0., 120., 200.);
	let centre = dock.layout(narrow).groups.into_iter().find(|g| g.id == GroupId::CENTRE).expect("shown");
	assert_rect(centre.rect, 124., 0., 180., 604.);
}

#[test]
fn minimums_that_do_not_fit_shrink_in_proportion() {
	assert_eq!(distribute(100., &[(1., 100.), (1., 100.)]), vec![50., 50.]);
	assert_eq!(distribute(300., &[(1., 0.), (3., 0.)]), vec![75., 225.]);
}

#[test]
fn a_splitter_drag_puts_the_bar_under_the_pointer() {
	let mut dock = dock();
	let bar = dock.layout(AREA).splitters[1];
	assert!(dock.resize(&bar, Point::new(402., 10.), AREA));
	assert_rect(pane_rect(&dock, "a"), 0., 0., 400., 200.);
	assert_rect(group(&dock, GroupId::CENTRE).rect, 404., 0., 600., 604.);
	let column = dock.layout(AREA).splitters[0];
	assert!(dock.resize(&column, Point::new(10., 302.), AREA));
	assert_rect(pane_rect(&dock, "a"), 0., 0., 400., 300.);
	assert_rect(pane_rect(&dock, "b"), 0., 304., 400., 300.);
}

#[test]
fn a_splitter_drag_stops_at_the_minimums() {
	let mut dock = dock();
	let bar = dock.layout(AREA).splitters[1];
	dock.resize(&bar, Point::new(5., 10.), AREA);
	assert_rect(pane_rect(&dock, "a"), 0., 0., 50., 200.);
	dock.resize(&bar, Point::new(1000., 10.), AREA);
	assert_rect(group(&dock, GroupId::CENTRE).rect, 904., 0., 100., 604.);
	let column = dock.layout(AREA).splitters[0];
	dock.resize(&column, Point::new(10., 0.), AREA);
	assert_rect(pane_rect(&dock, "a"), 0., 0., 900., 70.);
	let mut stale = column;
	stale.index = 7;
	assert!(!dock.resize(&stale, Point::new(10., 0.), AREA));
}

#[test]
fn the_outer_strips_drop_at_the_edges() {
	let dock = dock();
	let a = pane("a");
	for (x, y, side) in
		[(20., 300., Side::Left), (980., 300., Side::Right), (600., 30., Side::Top), (600., 580., Side::Bottom)]
	{
		assert_eq!(target(&dock, &a, x, y), Some(Target::Edge(side)), "at {x}, {y}");
	}
	let drop = dock.drop_target(&a, Point::new(1000., 300.), AREA).expect("an edge");
	assert_rect(drop.preview, 753., 0., 251., 604.);
	let drop = dock.drop_target(&a, Point::new(600., 10.), AREA).expect("an edge");
	assert_rect(drop.preview, 0., 0., 1004., 151.);
	assert_eq!(target(&dock, &a, 1100., 300.), None);
}

#[test]
fn a_group_splits_at_its_sides_and_tabs_in_its_middle() {
	let dock = dock();
	let b = dock.group_of("b").expect("shown");
	let a = pane("a");
	assert_eq!(target(&dock, &a, 125., 450.), Some(Target::Tab(b)));
	assert_eq!(target(&dock, &a, 60., 450.), Some(Target::Beside(b, Side::Left)));
	assert_eq!(target(&dock, &a, 200., 450.), Some(Target::Beside(b, Side::Right)));
	assert_eq!(target(&dock, &a, 125., 250.), Some(Target::Beside(b, Side::Top)));
	assert_eq!(target(&dock, &a, 125., 540.), Some(Target::Beside(b, Side::Bottom)));
	let drop = dock.drop_target(&a, Point::new(125., 540.), AREA).expect("a side");
	assert_rect(drop.preview, 0., 204. + 400. * 2. / 3., 250., 400. / 3.);
	let drop = dock.drop_target(&a, Point::new(125., 450.), AREA).expect("the middle");
	assert_eq!(drop.preview, pane_rect(&dock, "b"));
}

#[test]
fn the_centre_takes_its_sides_only() {
	let dock = dock();
	let a = pane("a");
	assert_eq!(target(&dock, &a, 300., 300.), Some(Target::Beside(GroupId::CENTRE, Side::Left)));
	assert_eq!(target(&dock, &a, 629., 100.), Some(Target::Beside(GroupId::CENTRE, Side::Top)));
	assert_eq!(target(&dock, &a, 629., 300.), None);
	assert!(!dock.clone().move_to(&a, Target::Tab(GroupId::CENTRE)));
}

#[test]
fn nothing_drops_into_the_dragged_group_or_drags_the_centre() {
	let mut dock = dock();
	let b = dock.group_of("b").expect("shown");
	assert_eq!(target(&dock, &pane("b"), 125., 450.), None);
	assert_eq!(target(&dock, &pane("b"), 60., 450.), None);
	assert_eq!(target(&dock, &Subject::Group(b), 125., 450.), None);
	assert_eq!(target(&dock, &Subject::Group(b), 980., 300.), Some(Target::Edge(Side::Right)));
	assert_eq!(target(&dock, &Subject::Group(GroupId::CENTRE), 980., 300.), None);
	assert_eq!(target(&dock, &pane("m"), 980., 300.), None);
	assert_eq!(target(&dock, &pane("a"), 252., 300.), None, "over a splitter");
	assert!(!dock.move_to(&pane("b"), Target::Tab(b)));
	assert!(!dock.move_to(&Subject::Group(b), Target::Beside(b, Side::Left)));
	assert!(dock.move_to(&pane("a"), Target::Tab(b)));
	assert_eq!(target(&dock, &pane("a"), 125., 300.), None, "a tab's own group");
}

#[test]
fn an_edge_drop_takes_a_quarter_of_the_dock() {
	let mut dock = dock();
	assert!(dock.move_to(&pane("a"), Target::Edge(Side::Right)));
	assert_rect(pane_rect(&dock, "a"), 755., 0., 249., 604.);
	assert_rect(pane_rect(&dock, "b"), 0., 0., 186.75, 604.);
	assert_eq!(dock.layout(AREA).splitters.len(), 2);
	let Arrangement::Split { axis: Axis::Horizontal, children } = dock.arrangement() else { panic!("a split") };
	assert_eq!(children.len(), 3, "the column of one pane collapsed and the root merged");
}

#[test]
fn the_window_position_menu_moves_to_each_edge() {
	for side in Side::ALL {
		let mut dock = dock();
		assert!(dock.move_to(&pane("b"), Target::Edge(side)));
		let r = pane_rect(&dock, "b");
		match side {
			Side::Left => assert_rect(r, 0., 0., 249., 604.),
			Side::Right => assert_rect(r, 755., 0., 249., 604.),
			Side::Top => assert_rect(r, 0., 0., 1004., 150.),
			Side::Bottom => assert_rect(r, 0., 454., 1004., 150.),
		}
	}
}

#[test]
fn a_side_drop_takes_a_third_of_the_group() {
	let mut dock = dock();
	assert!(dock.move_to(&pane("a"), Target::Beside(GroupId::CENTRE, Side::Top)));
	let centre = group(&dock, GroupId::CENTRE).rect;
	let a = pane_rect(&dock, "a");
	assert_rect(a, 254., 0., 750., 200.);
	assert_rect(centre, 254., 204., 750., 400.);
	assert_rect(pane_rect(&dock, "b"), 0., 0., 250., 604.);
}

#[test]
fn a_tab_drop_appends_and_activates() {
	let mut dock = dock();
	let b = dock.group_of("b").expect("shown");
	assert!(dock.move_to(&pane("a"), Target::Tab(b)));
	let g = group(&dock, b);
	assert_eq!((g.tabs, g.active), (vec!["b".to_owned(), "a".to_owned()], Some(1)));
	assert_rect(g.rect, 0., 0., 250., 604.);
	assert!(dock.activate("b"));
	assert_eq!(group(&dock, b).active, Some(0));
	assert!(!dock.activate("m"));
}

#[test]
fn a_group_moves_whole() {
	let mut dock = dock();
	let b = dock.group_of("b").expect("shown");
	dock.move_to(&pane("a"), Target::Tab(b));
	assert!(dock.move_to(&Subject::Group(b), Target::Edge(Side::Bottom)));
	assert_eq!(dock.group_of("a"), Some(b));
	let g = group(&dock, b);
	assert_eq!((g.tabs.len(), g.active), (2, Some(1)));
	assert_rect(g.rect, 0., 454., 1004., 150.);
	let a = dock.group_of("a").expect("shown");
	assert_eq!(dock.layout(AREA).groups.len(), 2);
	assert!(dock.move_to(&Subject::Group(a), Target::Beside(GroupId::CENTRE, Side::Left)));
	assert_rect(pane_rect(&dock, "a"), 0., 0., 1000. / 3., 604.);
}

#[test]
fn a_closed_pane_comes_back_where_it_was() {
	let mut dock = dock();
	let before = dock.layout(AREA);
	assert!(dock.close("a"));
	assert!(!dock.close("a"));
	assert!(!dock.is_shown("a"));
	assert_rect(pane_rect(&dock, "b"), 0., 0., 250., 604.);
	assert_eq!(dock.layout(AREA).splitters.len(), 1);
	assert!(dock.show("a"));
	assert_eq!(dock.layout(AREA), before);
}

#[test]
fn closing_the_active_tab_activates_a_neighbour_and_show_brings_it_to_front() {
	let mut dock = dock();
	let b = dock.group_of("b").expect("shown");
	dock.move_to(&pane("a"), Target::Tab(b));
	dock.close("a");
	let g = group(&dock, b);
	assert_eq!((g.tabs, g.active), (vec!["b".to_owned()], Some(0)));
	dock.activate("b");
	dock.show("a");
	let g = group(&dock, b);
	assert_eq!((g.tabs, g.active), (vec!["b".to_owned(), "a".to_owned()], Some(1)));
}

#[test]
fn a_hidden_group_survives_a_move_of_its_neighbour() {
	let mut dock = dock();
	dock.close("a");
	dock.move_to(&pane("b"), Target::Edge(Side::Right));
	dock.show("a");
	assert_rect(pane_rect(&dock, "a"), 0., 0., 199.2, 604.);
}

#[test]
fn a_pane_without_a_place_shows_at_its_side() {
	let mut dock = dock();
	assert!(dock.show("m"));
	assert_rect(pane_rect(&dock, "m"), 0., 454., 1004., 150.);
	assert_rect(pane_rect(&dock, "a"), 0., 0., 250., 446. / 3.);
	assert!(!dock.show("unknown"));
}

#[test]
fn normalisation_collapses_and_merges() {
	let mut dock = registered();
	let nested = Arrangement::split(
		Axis::Horizontal,
		[
			(1., Arrangement::split(Axis::Vertical, [(1., Arrangement::tabs(["a"]))])),
			(1., Arrangement::split(Axis::Horizontal, [(1., Arrangement::Centre), (3., Arrangement::tabs(["b"]))])),
			(1., Arrangement::Tabs { tabs: Vec::new(), active: 0 }),
			(f32::NAN, Arrangement::split(Axis::Vertical, [])),
		],
	);
	dock.arrange(&nested).expect("valid");
	let expected = Arrangement::split(
		Axis::Horizontal,
		[(1., Arrangement::tabs(["a"])), (0.25, Arrangement::Centre), (0.75, Arrangement::tabs(["b"]))],
	);
	assert_eq!(dock.arrangement(), expected);
}

#[test]
fn an_arrangement_needs_one_centre() {
	let mut dock = registered();
	assert_eq!(dock.arrange(&Arrangement::tabs(["a"])), Err(ArrangeError::NoCentre));
	let two = Arrangement::split(Axis::Vertical, [(1., Arrangement::Centre), (1., Arrangement::Centre)]);
	assert_eq!(dock.arrange(&two), Err(ArrangeError::ManyCentres));
	assert_eq!(dock.arrangement(), Arrangement::Centre);
}

#[test]
fn a_restore_ignores_unknown_and_repeated_panes_and_leaves_missing_ones_hidden() {
	let mut dock = registered();
	let saved = Arrangement::split(
		Axis::Horizontal,
		[
			(
				1.,
				Arrangement::Tabs {
					tabs: vec![
						Tab { id: "gone".into(), shown: true },
						Tab { id: "a".into(), shown: false },
						Tab { id: "b".into(), shown: true },
					],
					active: 2,
				},
			),
			(1., Arrangement::tabs(["b", "gone"])),
			(3., Arrangement::Centre),
		],
	);
	dock.arrange(&saved).expect("valid");
	let expected = Arrangement::split(
		Axis::Horizontal,
		[
			(
				1.,
				Arrangement::Tabs {
					tabs: vec![Tab { id: "a".into(), shown: false }, Tab { id: "b".into(), shown: true }],
					active: 1,
				},
			),
			(3., Arrangement::Centre),
		],
	);
	assert_eq!(dock.arrangement(), expected);
	assert!(!dock.is_shown("m"));
	dock.show("m");
	assert_rect(pane_rect(&dock, "m"), 0., 454., 1004., 150.);
}

#[test]
fn an_arrangement_round_trips() {
	let mut dock = dock();
	dock.close("a");
	dock.move_to(&pane("b"), Target::Beside(GroupId::CENTRE, Side::Bottom));
	let saved = dock.arrangement();
	let before = dock.layout(AREA);
	let mut restored = registered();
	restored.arrange(&saved).expect("valid");
	assert_eq!(restored.arrangement(), saved);
	assert_eq!(
		restored.layout(AREA).groups.iter().map(|g| g.rect).collect::<Vec<_>>(),
		before.groups.iter().map(|g| g.rect).collect::<Vec<_>>()
	);
	restored.show("a");
	dock.show("a");
	assert_eq!(pane_rect(&restored, "a"), pane_rect(&dock, "a"));
}

#[cfg(feature = "serde")]
#[test]
fn an_arrangement_round_trips_through_json() {
	let mut dock = dock();
	dock.close("b");
	let json = serde_json::to_string(&dock.arrangement()).expect("serialisable");
	let mut restored = registered();
	restored.arrange(&serde_json::from_str(&json).expect("deserialisable")).expect("valid");
	assert_eq!(restored.arrangement(), dock.arrangement());
}

#[test]
fn an_arrangement_gets_positive_finite_weights() {
	let mut dock = registered();
	let weights = [(-1., Arrangement::tabs(["a"])), (f32::NAN, Arrangement::tabs(["b"])), (0., Arrangement::Centre)];
	dock.arrange(&Arrangement::split(Axis::Horizontal, weights)).expect("valid");
	let Arrangement::Split { children, .. } = dock.arrangement() else { panic!("a split") };
	let kept: Vec<f32> = children.iter().map(|&(w, _)| w).collect();
	assert_eq!(kept, [MIN_WEIGHT, 1., MIN_WEIGHT]);
}

fn bare(panes: &[&str]) -> Dock {
	let mut dock = Dock::new(Metrics { splitter: 0., title_bar: 0., tab_strip: 0., centre_min: Size::ZERO });
	for id in panes {
		dock.register(*id, Side::Left, Size::ZERO);
	}
	dock
}

#[test]
fn a_drag_beside_a_clamped_child_moves_the_pair_only() {
	let mut dock = bare(&["a", "b"]);
	dock.register("a", Side::Left, Size::new(100., 0.));
	let three = [(0.1, Arrangement::tabs(["a"])), (1., Arrangement::tabs(["b"])), (1., Arrangement::Centre)];
	dock.arrange(&Arrangement::split(Axis::Horizontal, three)).expect("valid");
	let area = Rect::new(0., 0., 600., 400.);
	let widths = |dock: &Dock| dock.layout(area).groups.iter().map(|g| g.rect.width).collect::<Vec<_>>();
	assert_eq!(widths(&dock), [100., 250., 250.]);
	let bar = dock.layout(area).splitters[0];
	assert!(dock.resize(&bar, Point::new(150., 10.), area));
	let w = widths(&dock);
	assert!(approx(w[0], 150.) && approx(w[1], 200.) && approx(w[2], 250.), "{w:?}");
}

#[test]
fn a_child_dragged_to_nothing_keeps_a_share() {
	let mut dock = bare(&["a", "b", "m"]);
	let column = Arrangement::split(Axis::Vertical, [(1., Arrangement::tabs(["a"])), (1., Arrangement::tabs(["b"]))]);
	dock.arrange(&Arrangement::split(Axis::Horizontal, [(1., column), (1., Arrangement::Centre)])).expect("valid");
	let area = Rect::new(0., 0., 600., 400.);
	let bar = dock.layout(area).splitters[0];
	dock.resize(&bar, Point::new(10., -50.), area);
	let rect =
		|dock: &Dock, id: &str| dock.layout(area).groups.into_iter().find(|g| g.tabs == [id]).expect("shown").rect;
	assert!(rect(&dock, "a").height < 0.01);
	dock.show("m");
	assert!(rect(&dock, "a").height < 0.01, "normalising keeps the weight");
	dock.close("b");
	let a = rect(&dock, "a");
	assert!(approx(a.height, 400.), "{a:?}");
}

#[test]
fn weights_of_nothing_share_equally() {
	assert_eq!(distribute(100., &[(0., 0.), (0., 0.)]), vec![50., 50.]);
}

#[test]
fn a_tab_moved_out_before_the_active_one_keeps_it_active() {
	let mut dock = registered();
	let tabs = ["a", "b", "m"].map(|id| Tab { id: id.into(), shown: true }).to_vec();
	let saved =
		Arrangement::split(Axis::Horizontal, [(1., Arrangement::Tabs { tabs, active: 1 }), (3., Arrangement::Centre)]);
	dock.arrange(&saved).expect("valid");
	assert!(dock.move_to(&pane("a"), Target::Edge(Side::Right)));
	let g = group(&dock, dock.group_of("b").expect("shown"));
	assert_eq!((g.tabs, g.active), (vec!["b".to_owned(), "m".to_owned()], Some(0)));
}

#[test]
fn a_resize_ignores_a_pointer_off_the_numbers() {
	let mut dock = dock();
	let before = dock.arrangement();
	let bar = dock.layout(AREA).splitters[1];
	assert!(!dock.resize(&bar, Point::new(f32::NAN, 0.), AREA));
	assert!(!dock.resize(&bar, Point::new(f32::INFINITY, 0.), AREA));
	assert_eq!(dock.arrangement(), before);
}

#[test]
fn a_pane_shown_at_an_occupied_edge_tabs_into_its_group() {
	let mut dock = dock();
	dock.register("n", Side::Bottom, MIN);
	dock.show("m");
	let m = dock.group_of("m").expect("shown");
	dock.show("n");
	assert_eq!(dock.group_of("n"), Some(m));
	let g = group(&dock, m);
	assert_eq!((g.tabs, g.active), (vec!["m".to_owned(), "n".to_owned()], Some(1)));
	assert_rect(g.rect, 0., 454., 1004., 150.);
	dock.close("m");
	dock.close("n");
	dock.register("o", Side::Bottom, MIN);
	dock.show("o");
	assert_eq!(dock.group_of("o"), Some(m));
}

#[test]
fn an_edge_across_the_root_split_takes_a_group_of_its_own() {
	let mut dock = dock();
	dock.register("n", Side::Bottom, MIN);
	dock.show("m");
	dock.register("l", Side::Left, MIN);
	dock.show("l");
	assert_ne!(dock.group_of("l"), dock.group_of("a"));
	assert_rect(pane_rect(&dock, "l"), 0., 0., 250., 604.);
}

#[test]
fn the_tab_strip_takes_room_from_a_group_of_several_panes() {
	let mut dock = dock();
	let b = dock.group_of("b").expect("shown");
	assert_rect(group(&dock, b).content, 0., 224., 250., 380.);
	dock.move_to(&pane("a"), Target::Tab(b));
	let g = group(&dock, b);
	assert_rect(g.content, g.rect.x, g.rect.y + 20., g.rect.width, g.rect.height - 36.);

	let mut dock = Dock::new(Metrics { splitter: 0., title_bar: 20., tab_strip: 16., centre_min: Size::ZERO });
	dock.register("a", Side::Left, MIN);
	dock.register("b", Side::Left, MIN);
	let tabs = Arrangement::tabs(["a", "b"]);
	dock.arrange(&Arrangement::split(Axis::Vertical, [(1., tabs), (100., Arrangement::Centre)])).expect("valid");
	assert_rect(pane_rect(&dock, "a"), 0., 0., 1004., MIN.height + 36.);
}
