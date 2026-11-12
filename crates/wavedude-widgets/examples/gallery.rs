//! Every widget of the crate in one window: `cargo run -p wavedude-widgets --example gallery`.

use std::rc::Rc;
use std::time::Duration;

use bytes::Bytes;
use freya::elements::image::ImageHandle;
use freya::engine::prelude::{AlphaType, Paint, PaintStyle};
use freya::prelude::*;
use wavedude_view::graph::GraphLayout;
use wavedude_widgets::graph_box::GraphBoxEvent;
use wavedude_widgets::icon::icons;
use wavedude_widgets::menu::ContextMenuRequest;
use wavedude_widgets::status_bar::StatusLine;
use wavedude_widgets::tree::RowKind;
use wavedude_widgets::*;
use wavedude_widgets::{Button, GradientStop, MenuItem, Side, Theme};

const CANVAS: (u32, u32) = (320, 180);

fn main() {
	launch(LaunchConfig::new().with_window(WindowConfig::new(app).with_size(1400., 900.).with_title("Widget gallery")));
}

fn wave_frame(gradient: &Gradient, phase: f64) -> Option<ImageHandle> {
	let (w, h) = CANVAS;
	let mut bytes = Vec::with_capacity((w * h * 4) as usize);
	for y in 0..h {
		for x in 0..w {
			let r = (f64::from(x) - 100.).hypot(f64::from(y) - 90.);
			let v = 0.5 + 0.5 * (r / 6. - phase).sin() * (-r / 200.).exp();
			let c = gradient.color_at(v);
			bytes.extend_from_slice(&[c.r, c.g, c.b, 255]);
		}
	}
	ImageHandle::from_rgba(w, h, Bytes::from(bytes), AlphaType::Unpremul)
}

fn log_line(level: LogLevel, text: &str) -> LogLine {
	LogLine::new("12:00:00.000", level, "WaveDude.Gallery", text)
}

fn app() -> impl IntoElement {
	let mut theme_state = use_init_wave_theme(Theme::dark);
	let theme = use_wave_theme();
	let mut layout = use_state(|| {
		let mut l = DockLayout::default();
		l.show("project", Side::Left);
		l.show("properties", Side::Right);
		l.show("messages", Side::Bottom);
		l
	});
	let mut log = use_state(|| {
		let mut log = MessageLog::default();
		log.extend([
			log_line(LogLevel::Debug, "Engine grid 320 x 180, rim 32, open edges"),
			log_line(LogLevel::Info, "Opened gallery.wdproj"),
			log_line(LogLevel::Warn, "The run does not fit in RAM").detail("budget 6.0 GB, need 9.7 GB"),
		]);
		log
	});
	let mut status = use_state(StatusLine::default);
	let mut context_menu = use_state(|| None::<ContextMenuRequest>);
	let tree_state = use_state(TreeState::<u32>::default);
	let gradient = use_state(|| {
		Gradient::new(vec![
			GradientStop { position: 0., color: Rgb::new(0, 0, 40) },
			GradientStop { position: 0.5, color: Rgb::new(0, 160, 255) },
			GradientStop { position: 1., color: Rgb::new(255, 255, 255) },
		])
	});
	let mut timeline = use_state(|| TimelineModel {
		frames: 250,
		position: 0,
		in_point: Some(20),
		out_point: Some(200),
		available: Rc::from([(0, 180)]),
		bounded: false,
	});
	let mut playing = use_state(|| false);
	let mut busy = use_state(|| false);
	let field_brightness = use_state(|| 1.0);
	let intensity_brightness = use_state(|| 2.5);
	let amplitude = use_state(|| 1.0);
	let frequency = use_state(|| 25.0);
	let swatch = use_state(|| Rgb::new(255, 0, 0));
	let collapsed = use_state(|| false);
	let described = use_state(|| true);
	let mut graph_size = use_state(|| (220f32, 120f32));
	let mut graph_selected = use_state(|| false);

	use_hook(move || {
		spawn(async move {
			loop {
				timer(Duration::from_millis(40)).await;
				if *playing.peek() {
					let mut t = timeline.write();
					t.position = if t.position + 1 >= t.frames { 0 } else { t.position + 1 };
				}
			}
		});
	});

	let mut say = move |level: LogLevel, text: String| {
		status.write().apply(level, &text);
		log.write().push(log_line(level, &text));
	};

	let phase = f64::from(timeline.read().position) * 0.3;
	let frame = wave_frame(&gradient.read(), phase);

	let model = MenuModel::new(vec![
		Submenu::new(
			"file",
			"_File",
			vec![
				MenuItem::new("file:new", "_New").shortcut("Ctrl+N").icon(icons::PLUS).into(),
				MenuItem::new("file:open", "_Open...").shortcut("Ctrl+O").icon(icons::FOLDER).into(),
				MenuEntry::Separator,
				Submenu::new(
					"file:export",
					"_Export",
					vec![
						MenuItem::new("file:export:field", "_Field...").into(),
						MenuItem::new("file:export:intensity", "_Intensity...").into(),
					],
				)
				.into(),
				MenuItem::new("file:close", "_Close").enabled(false).into(),
			],
		)
		.into(),
		Submenu::new(
			"view",
			"_View",
			vec![
				MenuItem::new("view:dark", "_Dark theme").check(Check::Radio(theme.kind == ThemeKind::Dark)).into(),
				MenuItem::new("view:light", "_Light theme").check(Check::Radio(theme.kind == ThemeKind::Light)).into(),
				MenuEntry::Separator,
				Submenu::new(
					"view:panes",
					"_Panes",
					["project", "properties", "messages"]
						.iter()
						.map(|id| {
							MenuItem::new(format!("pane:{id}"), format!("_{id}"))
								.check(Check::Checked(layout.read().is_shown(id)))
								.into()
						})
						.collect(),
				)
				.into(),
				MenuItem::new("view:busy", "_Busy").check(Check::Checked(*busy.read())).into(),
			],
		)
		.into(),
		Submenu::new("help", "_Help", vec![MenuItem::new("help:about", "_About").shortcut("F1").into()]).into(),
	]);
	let on_menu = move |id: String| {
		match id.as_str() {
			"view:dark" => theme_state.set(Theme::dark()),
			"view:light" => theme_state.set(Theme::light()),
			"view:busy" => {
				let b = !*busy.peek();
				busy.set(b);
			}
			_ => {}
		}
		if let Some(pane) = id.strip_prefix("pane:") {
			let side = if pane == "messages" { Side::Bottom } else { Side::Left };
			layout.write().toggle(pane, side);
		}
		say(LogLevel::Info, format!("Menu: {id}"));
	};

	let rows = vec![
		TreeRow::new(0, 0, RowKind::Root, "Gallery").icon(icons::FOLDER),
		TreeRow::new(1, 1, RowKind::Settings, "Scene").icon(icons::GEAR),
		TreeRow::new(2, 1, RowKind::Folder, "Sources").icon(icons::FOLDER).group(0).can_add(),
		TreeRow::new(3, 2, RowKind::Item, "Source 1").icon(icons::SOURCE).group(0).enabled(true),
		TreeRow::new(4, 2, RowKind::Item, "Source 2").icon(icons::SOURCE).group(0).enabled(false),
		TreeRow::new(5, 1, RowKind::Folder, "Probes").icon(icons::FOLDER).group(1).can_add(),
		TreeRow::new(6, 2, RowKind::Item, "Probe 1").icon(icons::PROBE).group(1).enabled(true),
	];
	let tree = TreeView::new(rows, tree_state, move |action: TreeAction<u32>| {
		if let TreeAction::ContextMenu { at, .. } = action {
			context_menu.set(Some(ContextMenuRequest {
				at,
				model: MenuModel::new(vec![
					MenuItem::new("tree:rename", "_Rename").shortcut("F2").into(),
					MenuItem::new("tree:delete", "_Delete").shortcut("Del").into(),
				]),
			}));
		} else {
			say(LogLevel::Info, format!("Tree: {action:?}"));
		}
	})
	.test_id("Tree");

	let properties = PropertyGrid::new()
		.child(DescriptionHeader::new("A point source emitting a sine wave.", described).title("Point source"))
		.child(
			SectionHeader::new("Emission")
				.collapsible(collapsed)
				.child(PropertyRow::new(
					"Amplitude",
					NumberBox::new(amplitude, NumberSpec::linear(0., 10., 0.1)).test_id("Amplitude"),
				))
				.child(
					PropertyRow::new(
						"Frequency level",
						NumberBox::new(frequency, NumberSpec::linear(0., 255., 1.).with_decimals(2)),
					)
					.hint("Up to 255; above it the grid cannot sample the wave"),
				)
				.child(PropertyRow::new("Colour", ColorButton::new(swatch)))
				.child(
					PropertyRow::new(
						"Disabled row",
						NumberBox::new(WritableSetter::new(1.0, |_| {}), NumberSpec::integer(0., 9.)),
					)
					.enabled(false),
				),
		)
		.child(
			SectionHeader::new("Colour scheme")
				.child(PropertyRow::new("Gradient", GradientEditor::new(gradient)).full_width()),
		);

	let overlay = move |canvas: &freya::engine::prelude::Canvas, t: &CanvasTransform, pointer: Option<(f32, f32)>| {
		let (x, y) = t.to_screen(100., 90.);
		let mut paint = Paint::default();
		paint.set_anti_alias(true);
		paint.set_style(PaintStyle::Stroke);
		paint.set_stroke_width(1.5);
		paint.set_color(freya::engine::prelude::SkColor::from_rgb(255, 230, 0));
		canvas.draw_circle((x, y), 6., &paint);
		if let Some((px, py)) = pointer {
			paint.set_color(freya::engine::prelude::SkColor::from_rgb(220, 220, 220));
			canvas.draw_circle((px, py), 3., &paint);
		}
	};
	let picture = |title: &'static str, brightness: State<f64>, edge: Color| {
		rect()
			.width(Size::flex(1.))
			.height(Size::fill())
			.child(
				HeaderStrip::new(title, brightness, move |()| {
					let mut log = log;
					log.write().push(log_line(LogLevel::Info, &format!("Auto brightness: {title}")));
				})
				.test_id(format!("Strip:{title}")),
			)
			.child(
				CanvasImage::new(frame.clone(), CANVAS)
					.edge(Some(edge))
					.busy(*busy.read())
					.overlay(overlay)
					.hit_test(|t: &CanvasTransform, p: (f32, f32)| {
						let (x, y) = t.to_canvas(p.0, p.1);
						((x - 100.).hypot(y - 90.) < 8.).then_some(1)
					})
					.track_pointer(true)
					.on_event(move |e: PictureEvent| {
						if e.phase == picture::PointerPhase::Secondary {
							context_menu.set(Some(ContextMenuRequest {
								at: e.global,
								model: MenuModel::new(vec![
									MenuItem::new("pic:brightness", "_Brightness...").into(),
									MenuItem::new("pic:auto", "_Auto brightness").into(),
								]),
							}));
						}
					})
					.test_id(format!("Picture:{title}")),
			)
			.content(Content::flex())
	};
	let values: Rc<[f32]> = (0..400).map(|i| ((f64::from(i) / 12. - phase).sin() * 0.8) as f32).collect();
	let (gw, gh) = *graph_size.read();
	let graph = GraphBox::new(
		GraphData::new("Probe 1", move |_, w, h| {
			let (pw, ph) = (w - 40., h - 30.);
			let n = values.len().max(2) as f32 - 1.;
			GraphLayout {
				width: w,
				height: h,
				visible: true,
				has_plot: true,
				plot_x: 32.,
				plot_y: 20.,
				plot_width: pw,
				plot_height: ph,
				line_width: 1.2,
				curve: values
					.iter()
					.enumerate()
					.map(|(i, v)| (32. + pw * i as f32 / n, 20. + ph * (0.5 - v / 2.)))
					.collect(),
				title: "Probe 1".into(),
				readout: "= 0.42".into(),
				..GraphLayout::default()
			}
		}),
		gw,
		gh,
		move |e: GraphBoxEvent| match e {
			GraphBoxEvent::Resize { width, height, .. } => graph_size.set((width.max(80.), height.max(50.))),
			GraphBoxEvent::Focused => graph_selected.set(true),
			_ => {}
		},
	)
	.selected(*graph_selected.read());

	let controls = rect()
		.horizontal()
		.cross_align(Alignment::center())
		.spacing(10.)
		.padding(6.)
		.child(label().text("Linear"))
		.child(NumberBox::new(amplitude, NumberSpec::linear(0., 10., 0.1)).width(Size::px(80.)))
		.child(label().text("Logarithmic"))
		.child(NumberBox::new(intensity_brightness, NumberSpec::logarithmic(0.0001, 100_000.)).width(Size::px(80.)))
		.child(ColorButton::new(swatch))
		.child(Spinner::new().size(20.).color(theme.accent).stroke_width(2.))
		.child(IconView::new(icons::PLAY).color(theme.text))
		.child(
			Button::new()
				.compact()
				.on_press(move |_| {
					let p = !*playing.peek();
					playing.set(p);
				})
				.child(label().text(if *playing.read() { "Pause" } else { "Play" })),
		);

	let centre = rect()
		.width(Size::fill())
		.height(Size::fill())
		.background(theme.window_background)
		.color(theme.text)
		.content(Content::flex())
		.child(
			rect()
				.horizontal()
				.width(Size::fill())
				.height(Size::flex(1.))
				.content(Content::flex())
				.spacing(4.)
				.child(picture("Field", field_brightness, Color::from_rgb(0, 0, 255)))
				.child(picture("Intensity", intensity_brightness, Color::from_rgb(255, 0, 0)))
				.child(rect().padding(6.).child(graph)),
		)
		.child(
			Timeline::new(timeline.read().clone(), move |e: TimelineEvent| {
				let mut t = timeline.write();
				match e {
					TimelineEvent::Seek(f) => t.position = f,
					TimelineEvent::SetIn(f) => t.in_point = Some(f),
					TimelineEvent::SetOut(f) => t.out_point = Some(f),
					TimelineEvent::PlayPause => {
						drop(t);
						let p = !*playing.peek();
						playing.set(p);
					}
				}
			})
			.tooltip("Frame; drag the brackets to set the in and out points"),
		)
		.child(controls);

	let panes = vec![
		DockPane::new("project", "Project", tree),
		DockPane::new("properties", "Properties", properties),
		DockPane::new("messages", "Messages", MessageList::new(log)),
	];
	let busy_state = (*busy.read()).then(|| ("Computing a frame".to_string(), Progress::Indeterminate));
	let position = timeline.read().position;
	rect()
		.width(Size::fill())
		.height(Size::fill())
		.background(theme.window_background)
		.color(theme.text)
		.font_size(theme.font_size)
		.content(Content::flex())
		.child(MenuBar::new(model, on_menu))
		.child(rect().width(Size::fill()).height(Size::flex(1.)).child(DockHost::new(layout, panes, centre)))
		.child(
			StatusBar::new(status.read().clone())
				.right(format!("Frame {position} of 250"))
				.busy(busy_state)
				.tooltip("Double-click to open the Messages pane")
				.on_double_click(move |()| layout.write().show("messages", Side::Bottom)),
		)
		.child(ContextMenuHost::new(context_menu, move |id: String| {
			let mut log = log;
			log.write().push(log_line(LogLevel::Info, &format!("Context menu: {id}")));
		}))
}
