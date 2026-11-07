//! The handle overlay of a picture: drawing ([`draw`]) and the pointer and key handling that turns presses,
//! drags and keys on handles into selection changes and model commands (docs/ui/40-preview-pane.md, Interaction).
//!
//! Drawing happens in the picture's render callback in the cell's logical pixels, clipped to the picture. The
//! emitter dots of composite sources are rasterised once per layout into a device-pixel bitmap ([`DotCache`]),
//! so thousands of dots cost one image draw a frame.
//! Threads: the UI thread.

use std::cell::RefCell;
use std::hash::{DefaultHasher, Hash, Hasher};

use freya::engine::prelude::{
	Canvas, Font, FontMgr, FontStyle, ISize, Paint, PaintStyle, PathBuilder, PathEffect, SkImage, SkRect,
	raster_n32_premul,
};
use freya::prelude::*;
use serde_json::{Value, json};
use wave_core::model::{Layer, PartKind, ProbeKind, Project};
use wave_widgets::picture::{CanvasTransform, PictureEvent, PointerPhase};

use super::handles::{
	self, DRAG_THRESHOLD, Handle, HandleKind, MarqueeMode, OverlayModel, drag_delta, grip_edits, lever_angle,
	marquee_hits, marquee_select, moved_node, moved_point, to_part_frame, turn,
};
use super::{PaneStates, open_picture_menu};
use crate::ui::properties::to_json;
use crate::ui::shared::{App, Shared};
use wave_app_state::Event as AppEvent;
use wave_app_state::commands::{Command, Target};
use wave_app_state::selection::{ItemKind, ItemRef, Row};

/// The handles' fixed colours: light grey over a dark halo, the selection bright yellow.
pub const HANDLE: Color = Color::from_rgb(0xD8, 0xD8, 0xD8);
pub const SELECTED: Color = Color::from_rgb(0xFF, 0xE6, 0x00);
pub const HALO: Color = Color::from_argb(0xB0, 0x00, 0x00, 0x00);

fn pen(color: Color, width: f32) -> Paint {
	let mut p = Paint::default();
	p.set_anti_alias(true);
	p.set_style(PaintStyle::Stroke);
	p.set_stroke_width(width);
	p.set_color(color);
	p
}

fn brush(color: Color) -> Paint {
	let mut p = Paint::default();
	p.set_anti_alias(true);
	p.set_style(PaintStyle::Fill);
	p.set_color(color);
	p
}

thread_local! {
	static FONT: Font = {
		let typeface = FontMgr::default().legacy_make_typeface(None, FontStyle::normal());
		let mut font = Font::default();
		if let Some(t) = typeface {
			font.set_typeface(t);
		}
		font.set_size(11.);
		font
	};
}

/// The emitter dots as a bitmap at device pixels, rebuilt when the dots, the fit or the device scale change.
#[derive(Default)]
pub struct DotCache(RefCell<Option<(u64, SkImage)>>);

impl DotCache {
	fn draw(&self, canvas: &Canvas, t: &CanvasTransform, dots: &[(f32, f32)]) {
		if dots.is_empty() {
			return;
		}
		let device = canvas.local_to_device_as_3x3().scale_x().abs().max(1e-3);
		let (px, py, pw, ph) = t.picture();
		let mut h = DefaultHasher::new();
		for d in dots {
			(d.0.to_bits(), d.1.to_bits()).hash(&mut h);
		}
		(t.scale.to_bits(), px.to_bits(), py.to_bits(), device.to_bits()).hash(&mut h);
		let key = h.finish();
		let mut slot = self.0.borrow_mut();
		if slot.as_ref().is_none_or(|(k, _)| *k != key) {
			let size = ISize::new((pw * device).ceil().max(1.) as i32, (ph * device).ceil().max(1.) as i32);
			let Some(mut surface) = raster_n32_premul(size) else { return };
			let c = surface.canvas();
			c.scale((device, device));
			let halo = brush(HALO);
			let dot = brush(HANDLE);
			for &(x, y) in dots {
				let (sx, sy) = (x * t.scale, y * t.scale);
				c.draw_circle((sx, sy), 2.5, &halo);
			}
			for &(x, y) in dots {
				let (sx, sy) = (x * t.scale, y * t.scale);
				c.draw_circle((sx, sy), 1.5, &dot);
			}
			*slot = Some((key, surface.image_snapshot()));
		}
		if let Some((_, image)) = slot.as_ref() {
			canvas.save();
			canvas.translate((px, py));
			canvas.scale((1. / device, 1. / device));
			canvas.draw_image(image, (0., 0.), None);
			canvas.restore();
		}
	}
}

/// What one picture's overlay draws besides the model.
pub struct OverlayFrame<'a> {
	pub model: &'a OverlayModel,
	pub focused: Option<usize>,
	pub marquee: Option<((f32, f32), (f32, f32))>,
	pub accent: Color,
	pub dots: &'a DotCache,
}

fn handle_color(h: &Handle, focused: bool, accent: Color) -> Color {
	if focused {
		accent
	} else if h.selected {
		SELECTED
	} else {
		HANDLE
	}
}

fn glyph(canvas: &Canvas, kind: HandleKind, (x, y): (f32, f32), color: Color) {
	let halo = pen(HALO, 3.5);
	let ink = pen(color, 1.5);
	let both = |f: &dyn Fn(&Paint)| {
		f(&halo);
		f(&ink);
	};
	let ticks = |inner: f32, outer: f32, p: &Paint| {
		canvas.draw_line((x - outer, y), (x - inner, y), p);
		canvas.draw_line((x + inner, y), (x + outer, y), p);
		canvas.draw_line((x, y - outer), (x, y - inner), p);
		canvas.draw_line((x, y + inner), (x, y + outer), p);
	};
	match kind {
		HandleKind::Source => both(&|p| {
			canvas.draw_circle((x, y), 6., p);
			ticks(7., 10., p);
		}),
		HandleKind::PartCentre => both(&|p| {
			canvas.draw_circle((x, y), 7., p);
			canvas.draw_circle((x, y), 3., p);
			ticks(8., 10., p);
		}),
		HandleKind::ProbeA | HandleKind::ProbeB => {
			let r = if kind == HandleKind::ProbeA { 5. } else { 4. };
			both(&|p| {
				canvas.draw_rect(SkRect::new(x - r, y - r, x + r, y + r), p);
				ticks(r + 1., 7., p);
			})
		}
		HandleKind::SourceLever | HandleKind::PartLever => {
			canvas.draw_circle((x, y), 6., &brush(HALO));
			canvas.draw_circle((x, y), 4.5, &brush(color));
		}
		HandleKind::Grip => {
			canvas.draw_rect(SkRect::new(x - 5.5, y - 5.5, x + 5.5, y + 5.5), &brush(HALO));
			canvas.draw_rect(SkRect::new(x - 4., y - 4., x + 4., y + 4.), &brush(color));
		}
		HandleKind::Point(_) | HandleKind::Node(_) => both(&|p| {
			canvas.draw_circle((x, y), 4.5, p);
		}),
		HandleKind::InHandle(_) | HandleKind::OutHandle(_) => {
			canvas.draw_circle((x, y), 4.5, &brush(HALO));
			canvas.draw_circle((x, y), 3., &brush(color));
		}
	}
}

/// Draws the overlay of one picture in the cell's logical pixels.
pub fn draw(canvas: &Canvas, t: &CanvasTransform, f: &OverlayFrame<'_>) {
	let s = |p: (f32, f32)| t.to_screen(p.0, p.1);
	let m = f.model;
	for line in &m.lines {
		let (a, b) = (s(line.a), s(line.b));
		canvas.draw_line(a, b, &pen(HALO, 3.));
		let mut dash = pen(if line.selected { SELECTED } else { HANDLE }, 1.5);
		dash.set_path_effect(PathEffect::dash(&[5., 4.], 0.));
		canvas.draw_line(a, b, &dash);
	}
	f.dots.draw(canvas, t, &m.dots);
	for outline in &m.outlines {
		let mut path = PathBuilder::new();
		for (i, p) in outline.points.iter().enumerate() {
			let q = s(*p);
			if i == 0 {
				path.move_to(q);
			} else {
				path.line_to(q);
			}
		}
		if outline.closed {
			path.close();
		}
		let path = path.detach();
		canvas.draw_path(&path, &pen(HALO, 3.));
		canvas.draw_path(&path, &pen(SELECTED, 1.));
	}
	for (i, h) in m.handles.iter().enumerate() {
		let at = s(h.at);
		if let Some(from) = h.from {
			let from = s(from);
			canvas.draw_line(from, at, &pen(HALO, 3.));
			let mut line = pen(handle_color(h, false, f.accent), 1.);
			if !h.kind.is_lever() {
				line.set_path_effect(PathEffect::dash(&[2., 2.], 0.));
			}
			canvas.draw_line(from, at, &line);
		}
		glyph(canvas, h.kind, at, handle_color(h, f.focused == Some(i), f.accent));
	}
	let (px, py, pw, ph) = t.picture();
	FONT.with(|font| {
		for label in &m.labels {
			let (x, y) = s(label.at);
			let (width, _) = font.measure_str(&label.text, None);
			let (mut lx, mut ly) = if label.below { (x - width / 2., y + 24.) } else { (x + 13., y + 4.) };
			lx = lx.clamp(px + 2., (px + pw - width - 2.).max(px + 2.));
			ly = ly.clamp(py + 12., (py + ph - 3.).max(py + 12.));
			let mut halo = pen(HALO, 3.);
			halo.set_stroke_join(freya::engine::prelude::PaintJoin::Round);
			canvas.draw_str(&label.text, (lx, ly), font, &halo);
			canvas.draw_str(&label.text, (lx, ly), font, &brush(if label.selected { SELECTED } else { HANDLE }));
		}
	});
	if let Some((a, b)) = f.marquee {
		let (a, b) = (s(a), s(b));
		let r = SkRect::new(a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1));
		let mut fill = brush(f.accent);
		fill.set_alpha_f(0.15);
		canvas.draw_rect(r, &fill);
		let mut edge = pen(f.accent, 1.);
		edge.set_path_effect(PathEffect::dash(&[4., 3.], 0.));
		canvas.draw_rect(r, &edge);
	}
}

/// A press in progress on a picture.
#[derive(Clone, Debug, PartialEq)]
pub enum Grab {
	/// A whole-item drag through group moves; `origin` is the item's first anchor at the press.
	Items {
		item: ItemRef,
		press: (f32, f32),
		origin: (i32, i32),
		group: bool,
		moved: bool,
	},
	/// A drag that writes properties of one item; `before` is the item as it was, for Esc.
	Edit {
		item: ItemRef,
		kind: HandleKind,
		before: Value,
		paths: Vec<String>,
	},
	Marquee {
		layer: Layer,
		a: (f32, f32),
		b: (f32, f32),
		mode: MarqueeMode,
	},
	/// A middle-button drag moving the zoomed view; `last` is the pointer in window pixels.
	Pan {
		last: (f64, f64),
	},
}

fn anchor_of(project: &Project, item: ItemRef) -> Option<(i32, i32)> {
	match item.kind {
		ItemKind::Source => project.sources.get(item.index).map(|s| (s.x.floor() as i32, s.y.floor() as i32)),
		ItemKind::Part => project.parts.get(item.index).map(|p| (p.base.x, p.base.y)),
		ItemKind::Probe => project.probes.get(item.index).map(|p| (p.ax, p.ay)),
		ItemKind::Scheme => None,
	}
}

fn item_json(project: &Project, item: ItemRef) -> Value {
	match item.kind {
		ItemKind::Source => to_json(&project.sources[item.index]),
		ItemKind::Part => to_json(&project.parts[item.index]),
		ItemKind::Probe => to_json(&project.probes[item.index]),
		ItemKind::Scheme => unreachable!("a scheme has no handle"),
	}
}

fn json_at<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
	path.split('.').try_fold(value, |v, key| v.get(key))
}

/// The property writes that put handle `kind` of `item` at the canvas point `p` (`snap`: Shift on a lever).
pub fn handle_edits(
	project: &Project,
	item: ItemRef,
	kind: HandleKind,
	p: (f32, f32),
	snap: bool,
) -> Vec<(String, Value)> {
	let pixel = (p.0.floor() as i32, p.1.floor() as i32);
	match (item.kind, kind) {
		(ItemKind::Source, HandleKind::SourceLever) => project.sources.get(item.index).map_or_else(Vec::new, |s| {
			vec![("array.angle".into(), json!(lever_angle((s.x as f32 + 0.5, s.y as f32 + 0.5), p, snap)))]
		}),
		(ItemKind::Probe, HandleKind::ProbeA) => project.probes.get(item.index).map_or_else(Vec::new, |probe| {
			let mut edits = vec![("ax".into(), json!(pixel.0)), ("ay".into(), json!(pixel.1))];
			if (probe.box_x, probe.box_y) == (probe.ax, probe.ay) {
				edits.extend([("box_x".into(), json!(pixel.0)), ("box_y".into(), json!(pixel.1))]);
			}
			edits
		}),
		(ItemKind::Probe, HandleKind::ProbeB) => vec![("bx".into(), json!(pixel.0)), ("by".into(), json!(pixel.1))],
		(ItemKind::Part, _) => {
			let Some(part) = project.parts.get(item.index) else { return Vec::new() };
			match (kind, &part.kind) {
				(HandleKind::PartLever, _) => {
					vec![("angle".into(), json!(lever_angle((part.base.x as f32, part.base.y as f32), p, snap)))]
				}
				(HandleKind::Grip, k) => grip_edits(k, to_part_frame(part, p)),
				(HandleKind::Point(i), PartKind::Outline(o)) => {
					let points = moved_point(&o.points, i, to_part_frame(part, p));
					vec![("kind.Outline.points".into(), to_json(points))]
				}
				(HandleKind::Node(_) | HandleKind::InHandle(_) | HandleKind::OutHandle(_), PartKind::Fiber(f)) => {
					let nodes = moved_node(f, kind, to_part_frame(part, p));
					vec![("kind.Fiber.nodes".into(), to_json(nodes))]
				}
				(kind, _) => unreachable!("{kind:?} is no handle of part {}", part.base.name),
			}
		}
		(item_kind, kind) => unreachable!("{kind:?} is no edit handle of a {item_kind:?}"),
	}
}

fn send_edits(shared: &Shared, item: ItemRef, edits: Vec<(String, Value)>) -> Vec<String> {
	edits
		.into_iter()
		.map(|(path, value)| {
			shared.send(AppEvent::Command(Command::Set { target: Target::Item(item), path: path.clone(), value }));
			path
		})
		.collect()
}

/// A whole-item handle that a property edit moves rather than a group move: a line probe's A alone, and B.
fn moves_by_edit(project: &Project, item: ItemRef, kind: HandleKind) -> bool {
	match kind {
		HandleKind::ProbeB => true,
		HandleKind::ProbeA => project.probes.get(item.index).is_some_and(|p| p.kind == ProbeKind::Line),
		_ => !kind.moves_item(),
	}
}

fn ctrl(m: Modifiers) -> bool {
	m.contains(Modifiers::CONTROL) || m.contains(Modifiers::META)
}

/// Handles a pointer event of the picture of `layer` with the overlay `model` it was built with.
pub fn on_pointer(
	shared: &Shared,
	states: PaneStates,
	layer: Layer,
	model: &OverlayModel,
	scale: f32,
	e: PictureEvent,
) {
	let mut grab = states.grab;
	let mut focus = states.focus;
	let index = e.handle.map(|h| h as usize);
	match e.phase {
		PointerPhase::Secondary => open_picture_menu(shared, states, layer, e.global),
		PointerPhase::Down if e.button == Some(MouseButton::Middle) => {
			if states.view.peek().zoom > 1. {
				grab.set(Some(Grab::Pan { last: e.global }));
			}
		}
		PointerPhase::Down => {
			focus.write()[layer as usize] = None;
			let Some(h) = index.and_then(|i| model.handles.get(i)) else {
				let mode = if ctrl(e.modifiers) {
					MarqueeMode::Toggle
				} else if e.modifiers.contains(Modifiers::SHIFT) {
					MarqueeMode::Add
				} else {
					MarqueeMode::Replace
				};
				grab.set(Some(Grab::Marquee { layer, a: e.canvas, b: e.canvas, mode }));
				return;
			};
			let item = h.item;
			if ctrl(e.modifiers) {
				shared.update(|app| app.selection.toggle(item));
				return;
			}
			if e.modifiers.contains(Modifiers::SHIFT) {
				shared.update(|app| {
					if !app.selection.contains(item) {
						let mut items = app.selection.items().to_vec();
						items.push(item);
						app.selection.set_items(items);
					}
				});
				return;
			}
			let (group, by_edit, project_view) = shared.update(|app| {
				let group = h.kind.moves_item() && app.selection.contains(item) && app.selection.items().len() > 1;
				if !group && app.selection.current_item() != Some(item) {
					app.selection.select(Row::Item(item));
				}
				let p = &app.document.project;
				(group, !group && moves_by_edit(p, item, h.kind), (anchor_of(p, item), item_json(p, item)))
			});
			shared.send(AppEvent::MouseDown);
			grab.set(Some(if by_edit {
				Grab::Edit { item, kind: h.kind, before: project_view.1, paths: Vec::new() }
			} else {
				let origin = project_view.0.expect("a handle's item exists");
				Grab::Items { item, press: e.canvas, origin, group, moved: false }
			}));
		}
		PointerPhase::Move => {
			let current = grab.peek().clone();
			match current {
				Some(Grab::Items { item, press, origin, group, moved }) => {
					let anchor = shared.peek(|app| anchor_of(&app.document.project, item)).unwrap_or(origin);
					let applied = (anchor.0 - origin.0, anchor.1 - origin.1);
					let (dx, dy) = drag_delta(press, e.canvas, applied);
					let far = (e.canvas.0 - press.0).hypot(e.canvas.1 - press.1) * scale >= DRAG_THRESHOLD;
					if (dx, dy) != (0, 0) && (moved || far) {
						shared.send(AppEvent::Command(Command::MoveBy { target: item, dx, dy }));
						grab.set(Some(Grab::Items { item, press, origin, group, moved: true }));
					}
				}
				Some(Grab::Edit { item, kind, before, mut paths }) => {
					let edits = shared.peek(|app| {
						handle_edits(
							&app.document.project,
							item,
							kind,
							e.canvas,
							e.modifiers.contains(Modifiers::SHIFT),
						)
					});
					for path in send_edits(shared, item, edits) {
						if !paths.contains(&path) {
							paths.push(path);
						}
					}
					grab.set(Some(Grab::Edit { item, kind, before, paths }));
				}
				Some(Grab::Marquee { layer, a, mode, .. }) => {
					grab.set(Some(Grab::Marquee { layer, a, b: e.canvas, mode }));
				}
				Some(Grab::Pan { last }) => {
					let (dx, dy) = ((e.global.0 - last.0) as f32, (e.global.1 - last.1) as f32);
					let mut view = states.view;
					let panned = view.peek().panned(dx, dy, scale);
					view.set(panned);
					grab.set(Some(Grab::Pan { last: e.global }));
				}
				None => {}
			}
		}
		PointerPhase::Up => {
			let current = grab.peek().clone();
			grab.set(None);
			match current {
				Some(Grab::Items { item, group, moved, .. }) => {
					if group && !moved {
						shared.update(|app| app.selection.select(Row::Item(item)));
					}
					shared.send(AppEvent::MouseUp);
				}
				Some(Grab::Edit { .. }) => {
					shared.send(AppEvent::MouseUp);
				}
				Some(Grab::Pan { .. }) => {}
				Some(Grab::Marquee { a, b, mode, .. }) => {
					let far = (b.0 - a.0).hypot(b.1 - a.1) * scale >= DRAG_THRESHOLD;
					if !far {
						if mode == MarqueeMode::Replace {
							shared.update(|app| app.deselect());
						}
						return;
					}
					let hits = marquee_hits(model, a, b);
					shared.update(|app| {
						let items = marquee_select(app.selection.items(), &hits, mode);
						app.selection.set_items(items);
					});
				}
				None => {}
			}
		}
	}
}

/// Esc during a drag: puts the item or the group back. Returns whether a drag was cancelled.
pub fn cancel_drag(shared: &Shared, states: PaneStates) -> bool {
	let mut grab = states.grab;
	let current = grab.peek().clone();
	grab.set(None);
	match current {
		Some(Grab::Items { item, origin, .. }) => {
			if let Some(anchor) = shared.peek(|app| anchor_of(&app.document.project, item))
				&& anchor != origin
			{
				shared.send(AppEvent::Command(Command::MoveBy {
					target: item,
					dx: origin.0 - anchor.0,
					dy: origin.1 - anchor.1,
				}));
			}
			shared.send(AppEvent::MouseUp);
			true
		}
		Some(Grab::Edit { item, before, paths, .. }) => {
			for path in paths {
				if let Some(value) = json_at(&before, &path) {
					shared.send(AppEvent::Command(Command::Set {
						target: Target::Item(item),
						path,
						value: value.clone(),
					}));
				}
			}
			shared.send(AppEvent::MouseUp);
			true
		}
		Some(Grab::Marquee { .. } | Grab::Pan { .. }) => true,
		None => false,
	}
}

/// Keys on a focused picture: Tab through the handles, arrows nudge, Del removes, Esc cancels a drag.
pub fn on_key(shared: &Shared, states: PaneStates, layer: Layer, model: &OverlayModel, e: &Event<KeyboardEventData>) {
	let mut focus = states.focus;
	let shift = e.modifiers.contains(Modifiers::SHIFT);
	let focused = focus.peek()[layer as usize]
		.and_then(|(item, kind)| model.handles.iter().position(|h| h.item == item && h.kind == kind));
	match &e.key {
		Key::Named(NamedKey::Escape) => {
			if cancel_drag(shared, states) {
				e.prevent_default();
				e.stop_propagation();
			}
		}
		Key::Named(NamedKey::Tab) if !ctrl(e.modifiers) => {
			let n = model.handles.len();
			let next = match (focused, shift) {
				(None, false) if n > 0 => Some(0),
				(None, true) if n > 0 => Some(n - 1),
				(Some(i), false) if i + 1 < n => Some(i + 1),
				(Some(i), true) if i > 0 => Some(i - 1),
				_ => None,
			};
			focus.write()[layer as usize] = next.map(|i| (model.handles[i].item, model.handles[i].kind));
			if let Some(i) = next {
				let item = model.handles[i].item;
				shared.update(|app| {
					if !app.selection.contains(item) {
						app.selection.select(Row::Item(item));
					}
				});
				e.prevent_default();
				e.stop_propagation();
			}
		}
		Key::Named(NamedKey::Delete) => {
			if let Some(h) = focused.and_then(|i| model.handles.get(i)) {
				shared.send(AppEvent::Command(Command::Remove { target: h.item }));
				focus.write()[layer as usize] = None;
				e.prevent_default();
				e.stop_propagation();
			}
		}
		Key::Named(arrow @ (NamedKey::ArrowLeft | NamedKey::ArrowRight | NamedKey::ArrowUp | NamedKey::ArrowDown)) => {
			let Some(h) = focused.and_then(|i| model.handles.get(i)) else { return };
			let n = if shift { 10 } else { 1 };
			let (dx, dy) = match arrow {
				NamedKey::ArrowLeft => (-n, 0),
				NamedKey::ArrowRight => (n, 0),
				NamedKey::ArrowUp => (0, -n),
				_ => (0, n),
			};
			nudge(shared, h, dx, dy);
			e.prevent_default();
			e.stop_propagation();
		}
		_ => {}
	}
}

/// Arrow keys on handle `h`: a whole-item handle moves its item or the selection it belongs to by whole pixels;
/// a lever turns by a degree a pixel (Right and Up counter-clockwise); other handles move like a drag.
pub fn nudge(shared: &Shared, h: &Handle, dx: i32, dy: i32) {
	let item = h.item;
	let edits = shared.peek(|app| {
		let p = &app.document.project;
		let grouped = app.selection.contains(item) && app.selection.items().len() > 1;
		if h.kind.is_lever() {
			let delta = f64::from(dx - dy);
			let angle = match item.kind {
				ItemKind::Source => {
					p.sources.get(item.index).and_then(|s| s.array.as_ref()).map(|a| ("array.angle", a.angle))
				}
				ItemKind::Part => p.parts.get(item.index).map(|part| ("angle", part.base.angle)),
				ItemKind::Probe | ItemKind::Scheme => unreachable!("a {:?} has no lever", item.kind),
			};
			return Some(angle.map_or_else(Vec::new, |(path, a)| vec![(path.to_owned(), json!(turn(a, delta)))]));
		}
		if h.kind.moves_item() && (grouped || !moves_by_edit(p, item, h.kind)) {
			return None;
		}
		Some(handle_edits(p, item, h.kind, (h.at.0 + dx as f32, h.at.1 + dy as f32), false))
	});
	match edits {
		None => {
			shared.send(AppEvent::Command(Command::MoveBy { target: item, dx, dy }));
		}
		Some(edits) => {
			send_edits(shared, item, edits);
		}
	}
}

/// The overlay model of the picture of `layer` at `scale`.
pub fn model_of(app: &App, layer: Layer, scale: f32) -> OverlayModel {
	let d = &app.document.project.display;
	handles::build(
		&app.document.project,
		app.selection.items(),
		app.selection.current_item(),
		layer,
		scale,
		d.handles_in_preview,
		d.labels_in_preview,
	)
}

#[cfg(test)]
mod tests {
	use wave_core::model::{Part, Probe, Rectangle, Source, SourceArray};

	use super::*;

	fn project() -> Project {
		let mut p = Project::default();
		p.scene.width = 200;
		p.scene.height = 100;
		p.sources.push(Source { x: 10.0, y: 10.0, array: Some(SourceArray::default()), ..Source::default() });
		p.probes.push(Probe { ax: 5, ay: 5, box_x: 5, box_y: 5, kind: ProbeKind::Line, ..Probe::default() });
		p.parts.push(Part::new(PartKind::Rectangle(Rectangle::default()), 50, 50));
		p
	}

	#[test]
	fn edits_put_handles_at_the_pointer() {
		let p = project();
		let probe = ItemRef::new(ItemKind::Probe, 0);
		let a = handle_edits(&p, probe, HandleKind::ProbeA, (7.9, 8.2), false);
		assert_eq!(a.len(), 4, "an attached box follows A");
		assert_eq!(a[0], ("ax".into(), json!(7)));
		let lever = handle_edits(&p, ItemRef::new(ItemKind::Source, 0), HandleKind::SourceLever, (10.5, 0.), false);
		assert_eq!(lever, vec![("array.angle".into(), json!(90.0))]);
		let grip = handle_edits(&p, ItemRef::new(ItemKind::Part, 0), HandleKind::Grip, (80., 60.), false);
		assert_eq!(grip[0], ("kind.Rectangle.length".into(), json!(60)));
		assert_eq!(grip[1], ("kind.Rectangle.width".into(), json!(20)));
	}

	#[test]
	fn a_line_probes_ends_move_by_edits() {
		let p = project();
		let probe = ItemRef::new(ItemKind::Probe, 0);
		assert!(moves_by_edit(&p, probe, HandleKind::ProbeA));
		assert!(moves_by_edit(&p, probe, HandleKind::ProbeB));
		assert!(!moves_by_edit(&p, ItemRef::new(ItemKind::Source, 0), HandleKind::Source));
		assert!(moves_by_edit(&p, ItemRef::new(ItemKind::Part, 0), HandleKind::Grip));
	}

	#[test]
	fn json_paths_read_nested_values() {
		let v = json!({ "kind": { "Fiber": { "nodes": [1, 2] } }, "x": 3 });
		assert_eq!(json_at(&v, "kind.Fiber.nodes"), Some(&json!([1, 2])));
		assert_eq!(json_at(&v, "x"), Some(&json!(3)));
		assert_eq!(json_at(&v, "y"), None);
	}
}
