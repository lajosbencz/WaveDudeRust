//! Model edits as commands, applied through one [`apply`] that also leaves the selection the command makes
//! and classifies the change for the live controller (docs/ui/60-live-editing-and-runs.md, Edit classes).
//!
//! Property edits are generic: [`Command::Set`] addresses a field of a settings object or item by its serde
//! path (`frequency`, `array.size_x`, `kind.Lens.diameter`, `field.brightness`), round-tripping the object
//! through JSON, so no per-property code is needed and every change is logged at Trace as old -> new.
//! Threads: plain functions on the UI thread.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use wavedude_core::model::names::{self, Named};
use wavedude_core::model::normalize::Normalize;
use wavedude_core::model::{
	BraggReflector, Diffuser, Ellipse, FacettedGrating, Fiber, FresnelLens, GateKind, GrinLens, InterferenceFilter,
	Lens, LogicGate, MetaAxiconDesign, MetaDesign, MetaLensDesign, MetaPrismDesign, MetaSurface, Outline, PartKind,
	Point, Polygon, Probe, ProbeDetector, Project, Rectangle, SimpleGrating, Triangle, ZonePlate,
};

use wavedude_core::scene::gate_design::{self, GateDesign};

use super::clipboard::{self, Fragment, PasteTarget};
use super::log;
use super::selection::{ItemKind, ItemRef, Row, Selection, move_group};

/// The kinds of the Add part menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NewPart {
	Lens,
	FresnelLens,
	Polygon,
	Rectangle,
	Ellipse,
	Triangle,
	Outline,
	Diffuser,
	InterferenceFilter,
	SimpleGrating,
	FacettedGrating,
	MetaLens,
	MetaPrism,
	MetaAxicon,
	Fiber,
	BraggReflector,
	GrinLens,
	XorGate,
	AndGate,
	OrGate,
	ZonePlate,
}

impl NewPart {
	pub const ALL: [Self; 21] = [
		Self::Lens,
		Self::FresnelLens,
		Self::Fiber,
		Self::BraggReflector,
		Self::GrinLens,
		Self::ZonePlate,
		Self::Diffuser,
		Self::InterferenceFilter,
		Self::SimpleGrating,
		Self::FacettedGrating,
		Self::MetaLens,
		Self::MetaPrism,
		Self::MetaAxicon,
		Self::Polygon,
		Self::Rectangle,
		Self::Ellipse,
		Self::Triangle,
		Self::Outline,
		Self::XorGate,
		Self::AndGate,
		Self::OrGate,
	];

	/// A new part of this kind at its defaults.
	pub fn kind(self) -> PartKind {
		let gate = |kind| PartKind::LogicGate(LogicGate { kind, ..LogicGate::default() });
		match self {
			Self::Lens => PartKind::Lens(Lens::default()),
			Self::FresnelLens => PartKind::FresnelLens(FresnelLens::new_plate()),
			Self::Polygon => PartKind::Polygon(Polygon::default()),
			Self::Rectangle => PartKind::Rectangle(Rectangle::default()),
			Self::Ellipse => PartKind::Ellipse(Ellipse::default()),
			Self::Triangle => PartKind::Triangle(Triangle::default()),
			Self::Outline => PartKind::Outline(Outline::default()),
			Self::Diffuser => PartKind::Diffuser(Diffuser::new_part()),
			Self::InterferenceFilter => PartKind::InterferenceFilter(InterferenceFilter::new_part()),
			Self::SimpleGrating => PartKind::SimpleGrating(SimpleGrating::new_part()),
			Self::MetaLens => PartKind::MetaSurface(MetaSurface::new_part(MetaDesign::Lens(MetaLensDesign::default()))),
			Self::MetaPrism => {
				PartKind::MetaSurface(MetaSurface::new_part(MetaDesign::Prism(MetaPrismDesign::default())))
			}
			Self::MetaAxicon => {
				PartKind::MetaSurface(MetaSurface::new_part(MetaDesign::Axicon(MetaAxiconDesign::default())))
			}
			Self::FacettedGrating => PartKind::FacettedGrating(FacettedGrating::default()),
			Self::Fiber => PartKind::Fiber(Fiber::default()),
			Self::BraggReflector => PartKind::BraggReflector(BraggReflector::default()),
			Self::GrinLens => PartKind::GrinLens(GrinLens::default()),
			Self::XorGate => gate(GateKind::Xor),
			Self::AndGate => gate(GateKind::And),
			Self::OrGate => gate(GateKind::Or),
			Self::ZonePlate => PartKind::ZonePlate(ZonePlate::default()),
		}
	}

	/// The kind a part is of.
	pub fn of(kind: &PartKind) -> Self {
		match kind {
			PartKind::Lens(_) => Self::Lens,
			PartKind::FresnelLens(_) => Self::FresnelLens,
			PartKind::Polygon(_) => Self::Polygon,
			PartKind::Rectangle(_) => Self::Rectangle,
			PartKind::Ellipse(_) => Self::Ellipse,
			PartKind::Triangle(_) => Self::Triangle,
			PartKind::Outline(_) => Self::Outline,
			PartKind::Diffuser(_) => Self::Diffuser,
			PartKind::InterferenceFilter(_) => Self::InterferenceFilter,
			PartKind::SimpleGrating(_) => Self::SimpleGrating,
			PartKind::MetaSurface(m) => match m.design {
				MetaDesign::Lens(_) => Self::MetaLens,
				MetaDesign::Prism(_) => Self::MetaPrism,
				MetaDesign::Axicon(_) => Self::MetaAxicon,
			},
			PartKind::FacettedGrating(_) => Self::FacettedGrating,
			PartKind::Fiber(_) => Self::Fiber,
			PartKind::BraggReflector(_) => Self::BraggReflector,
			PartKind::GrinLens(_) => Self::GrinLens,
			PartKind::LogicGate(g) => match g.kind {
				GateKind::Xor => Self::XorGate,
				GateKind::And => Self::AndGate,
				GateKind::Or => Self::OrGate,
			},
			PartKind::ZonePlate(_) => Self::ZonePlate,
		}
	}

	/// The original's type name, the key of its `Kind.` description and its `AddPart:` test id.
	pub fn type_name(self) -> &'static str {
		match self {
			Self::Lens => "LensPart",
			Self::FresnelLens => "FresnelLensPart",
			Self::Polygon => "PolygonPart",
			Self::Rectangle => "RectanglePart",
			Self::Ellipse => "EllipsePart",
			Self::Triangle => "TrianglePart",
			Self::Outline => "OutlinePart",
			Self::Diffuser => "DiffuserPart",
			Self::InterferenceFilter => "InterferenceFilterPart",
			Self::SimpleGrating => "SimpleGratingPart",
			Self::MetaLens => "MetaLensPart",
			Self::MetaPrism => "MetaPrismPart",
			Self::MetaAxicon => "MetaAxiconPart",
			Self::FacettedGrating => "FacettedGratingPart",
			Self::Fiber => "FiberPart",
			Self::BraggReflector => "BraggReflectorPart",
			Self::GrinLens => "GrinLensPart",
			Self::XorGate => "XorGatePart",
			Self::AndGate => "AndGatePart",
			Self::OrGate => "OrGatePart",
			Self::ZonePlate => "ZonePlatePart",
		}
	}
}

/// What a [`Command::Set`] addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
	/// `seed` or `description` only.
	Project,
	Scene,
	Simulation,
	Display,
	Item(ItemRef),
}

/// A model edit.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
	/// Appends at `at` (the centre of the shown canvas when `None`).
	AddSource {
		at: Option<Point>,
	},
	AddProbe {
		at: Option<Point>,
		/// A standard probe or a probabilistic detector.
		detector: ProbeDetector,
	},
	AddPart {
		kind: NewPart,
		at: Option<Point>,
	},
	AddScheme,
	/// A copy of a built-in or user scheme as a project scheme, selected.
	ImportScheme(Box<wavedude_core::model::ColorScheme>),
	/// The target, or the whole selection when the target is one of several selected.
	Remove {
		target: ItemRef,
	},
	Duplicate {
		target: ItemRef,
	},
	/// Each item flips on its own (sources and parts: Enabled; probes: Active).
	ToggleEnabled {
		target: ItemRef,
	},
	/// A group move of the target or the selection, held inside the canvas; schemes are ignored.
	MoveBy {
		target: ItemRef,
		dx: i32,
		dy: i32,
	},
	/// Moves an item within its own list to `to` (a drag in the tree, Move up / Move down).
	Reorder {
		target: ItemRef,
		to: usize,
	},
	Rename {
		target: ItemRef,
		name: String,
	},
	/// Sets the field at the serde `path` of the target to `value`. A lens's `kind.Lens.material.mirror` renames
	/// it as the editor's Mirror box does; a linked rectangle's or ellipse's width sets its length too.
	Set {
		target: Target,
		path: String,
		value: Value,
	},
	/// Embeds image bytes as the scene: `name` is the file name, `path` where it came from.
	SetSceneImage {
		name: String,
		path: String,
		bytes: Vec<u8>,
	},
	/// Drops the image and keeps a blank canvas of its size.
	RemoveSceneImage,
	/// Pastes objects; with `place_at` (a drop on a picture) the pasted group is moved onto that pixel.
	Paste {
		fragment: Box<Fragment>,
		target: PasteTarget,
		place_at: Option<Point>,
	},
}

/// How a change affects runs and pictures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ChangeKind {
	/// Nothing changed.
	None,
	/// A project change that affects nothing shown (the description): dirty and an undo step only.
	Metadata,
	/// Redraw only.
	Display,
	/// A layer's brightness: a simulation edit with 8-bit preview samples (baked into the frames), else Display.
	Brightness,
	/// A probe's place or measurement: the frames stay, the recordings are made anew.
	ProbePresentation,
	/// Scene, solver, seed, sources, parts: the run is stale.
	Simulation,
}

impl ChangeKind {
	/// The kind covering both.
	pub fn merge(self, other: Self) -> Self {
		match (self, other) {
			(Self::Brightness, Self::ProbePresentation) | (Self::ProbePresentation, Self::Brightness) => {
				Self::Simulation
			}
			(a, b) => a.max(b),
		}
	}
}

/// The outcome of [`apply`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
	pub kind: ChangeKind,
	/// The command is an undo step by itself (paste, remove, duplicate, image) rather than part of a quiet-time
	/// step.
	pub own_step: bool,
	/// The newest edit with its value, for the log.
	pub note: Option<String>,
}

impl Change {
	fn of(kind: ChangeKind) -> Self {
		Self { kind, own_step: false, note: None }
	}

	fn step(kind: ChangeKind) -> Self {
		Self { own_step: true, ..Self::of(kind) }
	}

	fn noted(mut self, note: String) -> Self {
		self.note = Some(note);
		self
	}
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CommandError {
	#[error("no such item: {0:?}")]
	NoItem(ItemRef),
	#[error("'{0}' is not a setting of this object")]
	NoField(String),
	#[error("'{path}' cannot take {value}: {message}")]
	BadValue { path: String, value: String, message: String },
	#[error("a name cannot be empty")]
	EmptyName,
	#[error("the image cannot be decoded: {0}")]
	Image(String),
}

fn item_kind_change(kind: ItemKind) -> ChangeKind {
	match kind {
		ItemKind::Source | ItemKind::Part => ChangeKind::Simulation,
		ItemKind::Probe => ChangeKind::ProbePresentation,
		ItemKind::Scheme => ChangeKind::Display,
	}
}

/// The class of the difference between two projects (an undo, a redo, a settings paste).
pub fn classify(old: &Project, new: &Project) -> ChangeKind {
	if old.seed != new.seed
		|| old.scene != new.scene
		|| old.simulation != new.simulation
		|| old.sources != new.sources
		|| old.parts != new.parts
		|| old.display.intensity.preview != new.display.intensity.preview
	{
		return ChangeKind::Simulation;
	}
	let probes = probes_change(&old.probes, &new.probes);
	let brightness = old.display.field.brightness != new.display.field.brightness
		|| old.display.intensity.brightness != new.display.intensity.brightness;
	let display = if brightness {
		ChangeKind::Brightness
	} else if old.display != new.display || old.color_schemes != new.color_schemes {
		ChangeKind::Display
	} else if old.description != new.description {
		ChangeKind::Metadata
	} else {
		ChangeKind::None
	};
	probes.merge(display)
}

/// A probe list's change: presentation when a run records it differently, display when only boxes changed.
fn probes_change(old: &[Probe], new: &[Probe]) -> ChangeKind {
	if old == new {
		ChangeKind::None
	} else if old.len() == new.len() && old.iter().zip(new).all(|(a, b)| a.records_like(b)) {
		ChangeKind::Display
	} else {
		ChangeKind::ProbePresentation
	}
}

fn list_insert_point(project: &Project, at: Option<Point>) -> (i32, i32) {
	at.map_or_else(|| project.canvas_centre(), |p| (p.x, p.y))
}

/// Applies `command` to `project`, leaves the selection the command makes in `selection`, normalises the
/// project and classifies the change.
/// A gate's input ports: where a source must sit to feed it (part index, A and B).
fn gate_ports(project: &Project) -> Vec<(usize, [Point; 2])> {
	project
		.parts
		.iter()
		.enumerate()
		.filter_map(|(i, part)| {
			let PartKind::LogicGate(gate) = &part.kind else { return None };
			let design = GateDesign::for_gate(gate, &project.simulation)?;
			let [a, b, _, _] = gate_design::ports_of(&design, &part.base);
			Some((i, [a, b]))
		})
		.collect()
}

/// Sources sitting on a gate's input ports before a change follow the ports after it (a moved, turned or
/// retuned gate keeps its feed). Returns whether a source moved.
pub fn follow_gate_ports(before: &[(usize, [Point; 2])], project: &mut Project) -> bool {
	let after = gate_ports(project);
	let mut moved = false;
	for (index, old_ports) in before {
		let Some((_, new_ports)) = after.iter().find(|(i, _)| i == index) else { continue };
		for (old, new) in old_ports.iter().zip(new_ports) {
			if old == new {
				continue;
			}
			for source in &mut project.sources {
				if source.x.floor() as i32 == old.x && source.y.floor() as i32 == old.y {
					source.x = f64::from(new.x);
					source.y = f64::from(new.y);
					moved = true;
				}
			}
		}
	}
	moved
}

pub fn apply(project: &mut Project, selection: &mut Selection, command: Command) -> Result<Change, CommandError> {
	let ports = gate_ports(project);
	let mut change = apply_command(project, selection, command)?;
	if follow_gate_ports(&ports, project) {
		change.kind = change.kind.merge(ChangeKind::Simulation);
	}
	Ok(change)
}

fn apply_command(project: &mut Project, selection: &mut Selection, command: Command) -> Result<Change, CommandError> {
	let change = match command {
		Command::AddSource { at } => {
			let (x, y) = list_insert_point(project, at);
			let i = project.add_source_at(x, y);
			selection.set_items(vec![ItemRef::new(ItemKind::Source, i)]);
			Change::step(ChangeKind::Simulation).noted(format!("added {}", project.sources[i].name))
		}
		Command::AddProbe { at, detector } => {
			let (x, y) = list_insert_point(project, at);
			let i = project.add_probe_at(x, y);
			project.probes[i].detector = detector;
			project.probes[i].normalize();
			selection.set_items(vec![ItemRef::new(ItemKind::Probe, i)]);
			Change::step(ChangeKind::ProbePresentation).noted(format!("added {}", project.probes[i].name))
		}
		Command::AddPart { kind, at } => {
			let (x, y) = list_insert_point(project, at);
			let i = project.add_part_at(kind.kind(), x, y);
			selection.set_items(vec![ItemRef::new(ItemKind::Part, i)]);
			Change::step(ChangeKind::Simulation).noted(format!("added {}", project.parts[i].base.name))
		}
		Command::AddScheme => {
			let i = project.add_new_color_scheme();
			selection.set_items(vec![ItemRef::new(ItemKind::Scheme, i)]);
			Change::step(ChangeKind::Display)
		}
		Command::ImportScheme(scheme) => {
			let i = project.add_color_scheme(*scheme);
			selection.set_items(vec![ItemRef::new(ItemKind::Scheme, i)]);
			Change::step(ChangeKind::Display)
		}
		Command::Remove { target } => remove(project, selection, target)?,
		Command::Duplicate { target } => duplicate(project, selection, target)?,
		Command::ToggleEnabled { target } => {
			check(project, target)?;
			let mut kind = ChangeKind::None;
			for t in selection.targets(target) {
				let flipped = match t.kind {
					ItemKind::Source => project.sources.get_mut(t.index).map(|s| &mut s.enabled),
					ItemKind::Part => project.parts.get_mut(t.index).map(|p| &mut p.base.enabled),
					ItemKind::Probe => project.probes.get_mut(t.index).map(|p| &mut p.active),
					ItemKind::Scheme => None,
				};
				if let Some(flag) = flipped {
					*flag = !*flag;
					kind = kind.merge(item_kind_change(t.kind));
				}
			}
			Change::of(kind)
		}
		Command::MoveBy { target, dx, dy } => {
			check(project, target)?;
			let targets = selection.targets(target);
			let moved = move_group(project, &targets, dx, dy);
			let kind = if moved == (0, 0) {
				ChangeKind::None
			} else {
				targets.iter().fold(ChangeKind::None, |k, t| k.merge(item_kind_change(t.kind)))
			};
			Change::of(kind)
		}
		Command::Reorder { target, to } => reorder(project, selection, target, to)?,
		Command::Rename { target, name } => rename(project, target, name)?,
		Command::Set { target, path, value } => set(project, target, &path, value)?,
		Command::SetSceneImage { name, path, bytes } => {
			let size = wavedude_core::io::image_size(&bytes).map_err(CommandError::Image)?;
			let scene = &mut project.scene;
			scene.mode = wavedude_core::model::SceneMode::Image;
			scene.image = name;
			scene.image_path = path;
			scene.image_data = Some(bytes.into());
			scene.image_size = size;
			Change::step(ChangeKind::Simulation).noted(format!("scene image {}x{}", size.0, size.1))
		}
		Command::RemoveSceneImage => {
			project.scene.remove_image();
			Change::step(ChangeKind::Simulation).noted("scene image removed".into())
		}
		Command::Paste { fragment, target, place_at } => {
			let before = project.clone();
			let pasted = clipboard::paste(project, *fragment, target);
			if let Some(at) = place_at {
				clipboard::place_at(project, &pasted.items, at);
			}
			if !pasted.items.is_empty() {
				selection.set_items(pasted.items.clone());
			}
			project.normalize();
			Change::step(classify(&before, project)).noted(format!("pasted {} objects", pasted.items.len()))
		}
	};
	project.normalize();
	selection.retain_existing(project);
	Ok(change)
}

fn check(project: &Project, target: ItemRef) -> Result<(), CommandError> {
	if target.exists(project) { Ok(()) } else { Err(CommandError::NoItem(target)) }
}

fn remove_at(project: &mut Project, item: ItemRef) {
	match item.kind {
		ItemKind::Source => drop(project.sources.remove(item.index)),
		ItemKind::Part => drop(project.parts.remove(item.index)),
		ItemKind::Probe => drop(project.probes.remove(item.index)),
		ItemKind::Scheme => drop(project.color_schemes.remove(item.index)),
	}
}

/// Removes the target or the selection; the user stays at the slot of the last removed: the item that took
/// the slot, else the one above it, else the folder of the emptied list.
fn remove(project: &mut Project, selection: &mut Selection, target: ItemRef) -> Result<Change, CommandError> {
	check(project, target)?;
	let targets = selection.targets(target);
	let last = *targets.last().expect("at least the target");
	let kind = targets.iter().fold(ChangeKind::None, |k, t| k.merge(item_kind_change(t.kind)));
	for t in targets.iter().rev() {
		remove_at(project, *t);
	}
	let slot = last.index - targets.iter().filter(|t| t.kind == last.kind && t.index < last.index).count();
	match last.kind.len(project) {
		0 => selection.select(Row::Folder(last.kind)),
		len => selection.select(Row::Item(ItemRef::new(last.kind, slot.min(len - 1)))),
	}
	Ok(Change::step(kind).noted(format!("removed {} items", targets.len())))
}

/// A copy after each original, named "<name> copy"; the copies become the selection, the last current.
fn duplicate(project: &mut Project, selection: &mut Selection, target: ItemRef) -> Result<Change, CommandError> {
	check(project, target)?;
	let targets = selection.targets(target);
	let mut copies = Vec::new();
	let mut shift = 0;
	let mut list = None;
	let mut kind = ChangeKind::None;
	for t in &targets {
		if list != Some(t.kind) {
			list = Some(t.kind);
			shift = 0;
		}
		let at = t.index + shift;
		let copy = match t.kind {
			ItemKind::Source => project.duplicate_source(at),
			ItemKind::Part => project.duplicate_part(at),
			ItemKind::Probe => project.duplicate_probe(at),
			ItemKind::Scheme => project.duplicate_color_scheme(at),
		};
		shift += 1;
		copies.push(ItemRef::new(t.kind, copy));
		kind = kind.merge(item_kind_change(t.kind));
	}
	selection.set_items(copies);
	Ok(Change::step(kind).noted(format!("duplicated {} items", targets.len())))
}

fn reorder(
	project: &mut Project,
	selection: &mut Selection,
	target: ItemRef,
	to: usize,
) -> Result<Change, CommandError> {
	check(project, target)?;
	let to = to.min(target.kind.len(project) - 1);
	if to == target.index {
		return Ok(Change::of(ChangeKind::None));
	}
	fn shift<T>(v: &mut Vec<T>, from: usize, to: usize) {
		let item = v.remove(from);
		v.insert(to, item);
	}
	match target.kind {
		ItemKind::Source => shift(&mut project.sources, target.index, to),
		ItemKind::Part => shift(&mut project.parts, target.index, to),
		ItemKind::Probe => shift(&mut project.probes, target.index, to),
		ItemKind::Scheme => shift(&mut project.color_schemes, target.index, to),
	}
	let map = |i: ItemRef| -> ItemRef {
		if i.kind != target.kind {
			return i;
		}
		let index = if i.index == target.index {
			to
		} else if target.index < i.index && i.index <= to {
			i.index - 1
		} else if to <= i.index && i.index < target.index {
			i.index + 1
		} else {
			i.index
		};
		ItemRef::new(i.kind, index)
	};
	let current = selection.current();
	let items: Vec<ItemRef> = selection.items().iter().map(|&i| map(i)).collect();
	let current_item = match current {
		Some(Row::Item(i)) => Some(map(i)),
		_ => None,
	};
	*selection = Selection::default();
	for i in items.iter().filter(|&&i| Some(i) != current_item) {
		selection.toggle(*i);
	}
	if let Some(c) = current_item {
		selection.toggle(c);
	}
	// A list's order is the emitter, paint or marker order.
	Ok(Change::of(item_kind_change(target.kind)))
}

fn rename(project: &mut Project, target: ItemRef, name: String) -> Result<Change, CommandError> {
	check(project, target)?;
	let name = name.trim().to_owned();
	if name.is_empty() {
		return Err(CommandError::EmptyName);
	}
	let item: &mut dyn Named = match target.kind {
		ItemKind::Source => &mut project.sources[target.index],
		ItemKind::Part => &mut project.parts[target.index],
		ItemKind::Probe => &mut project.probes[target.index],
		ItemKind::Scheme => &mut project.color_schemes[target.index],
	};
	let old = item.name().to_owned();
	if old == name {
		return Ok(Change::of(ChangeKind::None));
	}
	item.set_name(name.clone());
	log::change(&format!("{old}.name"), &old, &name);
	// A probe's name is not a measurement: the graphs relabel, the run keeps its traces.
	let kind = match target.kind {
		ItemKind::Probe => ChangeKind::Display,
		k => item_kind_change(k),
	};
	let change = Change::of(kind).noted(format!("{old} renamed to {name}"));
	match target.kind {
		ItemKind::Scheme => {
			for layer in [&mut project.display.field, &mut project.display.intensity] {
				if layer.color_scheme.to_lowercase() == old.to_lowercase() {
					layer.color_scheme = name.clone();
				}
			}
		}
		ItemKind::Source | ItemKind::Part | ItemKind::Probe => {}
	}
	Ok(change)
}

/// Replaces the value at the dotted `path` of `root`; returns the old value.
fn put(root: &mut Value, path: &str, value: Value) -> Option<Value> {
	let mut at = root;
	let mut keys = path.split('.').peekable();
	while let Some(key) = keys.next() {
		let map = at.as_object_mut()?;
		if keys.peek().is_none() {
			return map.contains_key(key).then(|| map.insert(key.to_owned(), value)).flatten();
		}
		at = map.get_mut(key)?;
	}
	None
}

/// Sets `path` of `object` through JSON; returns (old, new) as text.
fn set_field<T: Serialize + DeserializeOwned>(
	object: &mut T,
	path: &str,
	value: Value,
) -> Result<(String, String), CommandError> {
	let mut json = serde_json::to_value(&*object).expect("model objects serialise");
	let new_text = value.to_string();
	let (old, absent) = match put(&mut json, path, value.clone()) {
		Some(old) => (old, false),
		None => {
			(insert_absent(&mut json, path, value.clone()).ok_or_else(|| CommandError::NoField(path.to_owned()))?, true)
		}
	};
	let set: T = serde_json::from_value(json).map_err(|e| CommandError::BadValue {
		path: path.to_owned(),
		value: new_text.clone(),
		message: e.to_string(),
	})?;
	if absent && !value.is_null() && get(&serde_json::to_value(&set).expect("model objects serialise"), path).is_none()
	{
		return Err(CommandError::NoField(path.to_owned()));
	}
	*object = set;
	Ok((old.to_string(), new_text))
}

/// Inserts a leaf a skipped `None` left out; the caller proves the key exists by a round trip.
fn insert_absent(root: &mut Value, path: &str, value: Value) -> Option<Value> {
	let (parent, key) = match path.rsplit_once('.') {
		Some((parent, key)) => (parent.split('.').try_fold(&mut *root, |at, k| at.get_mut(k))?, key),
		None => (root, path),
	};
	let map = parent.as_object_mut()?;
	(!map.contains_key(key)).then(|| map.insert(key.to_owned(), value));
	Some(Value::Null)
}

fn get<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
	path.split('.').try_fold(root, |at, k| at.get(k))
}

fn set(project: &mut Project, target: Target, path: &str, value: Value) -> Result<Change, CommandError> {
	let (label, (old, new), kind) = match target {
		Target::Project => {
			let field = match path {
				"seed" | "description" => path,
				_ => return Err(CommandError::NoField(path.to_owned())),
			};
			let mut both = serde_json::json!({ "seed": project.seed, "description": project.description });
			let old = put(&mut both, field, value.clone()).expect("both fields exist");
			let bad = |e: serde_json::Error| CommandError::BadValue {
				path: path.to_owned(),
				value: value.to_string(),
				message: e.to_string(),
			};
			if field == "seed" {
				project.seed = serde_json::from_value(both["seed"].take()).map_err(bad)?;
			} else {
				project.description = serde_json::from_value(both["description"].take()).map_err(bad)?;
			}
			let kind = if field == "seed" { ChangeKind::Simulation } else { ChangeKind::Metadata };
			("Project".to_owned(), (old.to_string(), value.to_string()), kind)
		}
		Target::Scene => {
			let image = project.scene.image_data.take();
			let size = project.scene.image_size;
			let result = set_field(&mut project.scene, path, value);
			project.scene.image_data = image;
			project.scene.image_size = size;
			("Scene".to_owned(), result?, ChangeKind::Simulation)
		}
		Target::Simulation => {
			("Simulation".to_owned(), set_field(&mut project.simulation, path, value)?, ChangeKind::Simulation)
		}
		Target::Display => {
			// Showing the intensity makes the run track it: other frames.
			let kind = if path.ends_with(".brightness") {
				ChangeKind::Brightness
			} else if path == "intensity.preview" {
				ChangeKind::Simulation
			} else {
				ChangeKind::Display
			};
			("Display".to_owned(), set_field(&mut project.display, path, value)?, kind)
		}
		Target::Item(item) => {
			check(project, item)?;
			let name = item.name(project).expect("checked above").to_owned();
			let mut kind = item_kind_change(item.kind);
			let result = match item.kind {
				ItemKind::Source => set_field(&mut project.sources[item.index], path, value)?,
				ItemKind::Probe => {
					let probe = &mut project.probes[item.index];
					let before = probe.clone();
					let result = set_field(probe, path, value)?;
					kind = probes_change(std::slice::from_ref(&before), std::slice::from_ref(probe));
					result
				}
				ItemKind::Scheme => set_field(&mut project.color_schemes[item.index], path, value)?,
				ItemKind::Part => set_part(project, item.index, path, value)?,
			};
			(name, result, kind)
		}
	};
	if old == new {
		return Ok(Change::of(ChangeKind::None));
	}
	log::change(&format!("{label}.{path}"), &old, &new);
	Ok(Change::of(kind).noted(format!("{label}.{path} = {new}")))
}

fn set_part(project: &mut Project, index: usize, path: &str, value: Value) -> Result<(String, String), CommandError> {
	let part = &mut project.parts[index];
	if path == "kind.Lens.material.mirror"
		&& let PartKind::Lens(lens) = &part.kind
		&& let Some(mirror) = value.as_bool()
	{
		let old = lens.material.mirror;
		names::set_lens_mirror(part, mirror);
		return Ok((old.to_string(), mirror.to_string()));
	}
	let result = set_field(part, path, value)?;
	match &mut part.kind {
		PartKind::Rectangle(Rectangle { length, width, linked: true, .. })
		| PartKind::Ellipse(Ellipse { length, width, linked: true, .. })
			if path.ends_with(".width") =>
		{
			*length = *width;
		}
		_ => {}
	}
	Ok(result)
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	fn project() -> Project {
		let mut p = Project::default();
		p.scene.width = 200;
		p.scene.height = 100;
		p
	}

	fn run(p: &mut Project, s: &mut Selection, c: Command) -> Change {
		apply(p, s, c).unwrap()
	}

	#[test]
	fn showing_the_intensity_is_a_simulation_change_and_a_probe_rename_is_display() {
		let mut p = project();
		let mut s = Selection::default();
		let on = !p.display.intensity.preview;
		let toggle = Command::Set { target: Target::Display, path: "intensity.preview".into(), value: json!(on) };
		let c = run(&mut p, &mut s, toggle);
		assert_eq!(c.kind, ChangeKind::Simulation);
		let mut other = p.clone();
		other.display.intensity.preview = !on;
		assert_eq!(classify(&other, &p), ChangeKind::Simulation);
		run(&mut p, &mut s, Command::AddProbe { at: None, detector: ProbeDetector::Standard });
		let probe = Target::Item(ItemRef::new(ItemKind::Probe, 0));
		let c = run(&mut p, &mut s, Command::Rename { target: ItemRef::new(ItemKind::Probe, 0), name: "Gauge".into() });
		assert_eq!(c.kind, ChangeKind::Display);
		let c = run(&mut p, &mut s, Command::Set { target: probe, path: "ax".into(), value: json!(7) });
		assert_eq!(c.kind, ChangeKind::ProbePresentation);
	}

	#[test]
	fn classification() {
		let (mut p, mut s) = (project(), Selection::default());
		assert_eq!(run(&mut p, &mut s, Command::AddSource { at: None }).kind, ChangeKind::Simulation);
		assert_eq!(
			run(&mut p, &mut s, Command::AddProbe { at: None, detector: ProbeDetector::Standard }).kind,
			ChangeKind::ProbePresentation
		);
		assert_eq!(run(&mut p, &mut s, Command::AddScheme).kind, ChangeKind::Display);
		let set = |target, path: &str, value| Command::Set { target, path: path.into(), value };
		let src = Target::Item(ItemRef::new(ItemKind::Source, 0));
		let c = run(&mut p, &mut s, set(src, "frequency", json!(120.0)));
		assert_eq!((c.kind, c.note.as_deref()), (ChangeKind::Simulation, Some("Source 1.frequency = 120.0")));
		assert_eq!(p.sources[0].frequency, 120.0);
		assert_eq!(run(&mut p, &mut s, set(src, "frequency", json!(120.0))).kind, ChangeKind::None);
		assert_eq!(
			run(&mut p, &mut s, set(Target::Display, "field.brightness", json!(5.0))).kind,
			ChangeKind::Brightness
		);
		assert_eq!(run(&mut p, &mut s, set(Target::Display, "field.overlay", json!(false))).kind, ChangeKind::Display);
		assert_eq!(run(&mut p, &mut s, set(Target::Project, "description", json!("x"))).kind, ChangeKind::Metadata);
		assert_eq!(run(&mut p, &mut s, set(Target::Project, "seed", json!(7))).kind, ChangeKind::Simulation);
		assert_eq!(run(&mut p, &mut s, set(Target::Scene, "edges", json!("Reflective"))).kind, ChangeKind::Simulation);
		let probe = Target::Item(ItemRef::new(ItemKind::Probe, 0));
		assert_eq!(run(&mut p, &mut s, set(probe, "ax", json!(3))).kind, ChangeKind::ProbePresentation);
		assert_eq!(
			run(&mut p, &mut s, set(probe, "box_x", json!(40))).kind,
			ChangeKind::Display,
			"a box records alike"
		);
		let mut moved = p.clone();
		moved.probes[0].box_y += 5;
		assert_eq!(classify(&p, &moved), ChangeKind::Display);
		moved.probes[0].ay += 1;
		assert_eq!(classify(&p, &moved), ChangeKind::ProbePresentation);
		assert_eq!(apply(&mut p, &mut s, set(src, "no_such", json!(1))), Err(CommandError::NoField("no_such".into())));
		assert!(matches!(
			apply(&mut p, &mut s, set(src, "frequency", json!("high"))),
			Err(CommandError::BadValue { .. })
		));
		assert_eq!(run(&mut p, &mut s, set(src, "frequency", json!(9999.0))).kind, ChangeKind::Simulation);
		assert_eq!(p.sources[0].frequency, 255.0, "normalised after the edit");
	}

	#[test]
	fn lens_mirror_renames_and_linked_width() {
		let (mut p, mut s) = (project(), Selection::default());
		run(&mut p, &mut s, Command::AddPart { kind: NewPart::Lens, at: None });
		let part = Target::Item(ItemRef::new(ItemKind::Part, 0));
		run(
			&mut p,
			&mut s,
			Command::Set { target: part, path: "kind.Lens.material.mirror".into(), value: json!(true) },
		);
		assert_eq!(p.parts[0].base.name, "Mirror 1");
		run(&mut p, &mut s, Command::AddPart { kind: NewPart::Rectangle, at: Some(Point::new(5, 5)) });
		let rect = Target::Item(ItemRef::new(ItemKind::Part, 1));
		run(&mut p, &mut s, Command::Set { target: rect, path: "kind.Rectangle.linked".into(), value: json!(true) });
		run(&mut p, &mut s, Command::Set { target: rect, path: "kind.Rectangle.width".into(), value: json!(80) });
		let PartKind::Rectangle(r) = &p.parts[1].kind else { panic!() };
		assert_eq!((r.length, r.width), (80, 80));
	}

	#[test]
	fn remove_duplicate_toggle_on_the_selection() {
		let (mut p, mut s) = (project(), Selection::default());
		for _ in 0..4 {
			run(&mut p, &mut s, Command::AddSource { at: None });
		}
		s.select(Row::Item(ItemRef::new(ItemKind::Source, 1)));
		s.toggle(ItemRef::new(ItemKind::Source, 2));
		let c = run(&mut p, &mut s, Command::Duplicate { target: ItemRef::new(ItemKind::Source, 2) });
		assert!(c.own_step);
		let names: Vec<&str> = p.sources.iter().map(|x| x.name.as_str()).collect();
		assert_eq!(names, ["Source 1", "Source 2", "Source 2 copy", "Source 3", "Source 3 copy", "Source 4"]);
		assert_eq!(s.items(), [ItemRef::new(ItemKind::Source, 2), ItemRef::new(ItemKind::Source, 4)]);
		run(&mut p, &mut s, Command::ToggleEnabled { target: ItemRef::new(ItemKind::Source, 4) });
		assert!(!p.sources[2].enabled && !p.sources[4].enabled && p.sources[3].enabled);
		run(&mut p, &mut s, Command::Remove { target: ItemRef::new(ItemKind::Source, 2) });
		assert_eq!(p.sources.len(), 4);
		assert_eq!(s.current(), Some(Row::Item(ItemRef::new(ItemKind::Source, 3))), "Source 4 took the slot");
		run(&mut p, &mut s, Command::Remove { target: ItemRef::new(ItemKind::Source, 3) });
		assert_eq!(s.current(), Some(Row::Item(ItemRef::new(ItemKind::Source, 2))), "the last row: the one above");
		for _ in 0..3 {
			run(&mut p, &mut s, Command::Remove { target: ItemRef::new(ItemKind::Source, 0) });
		}
		assert_eq!(s.current(), Some(Row::Folder(ItemKind::Source)), "an emptied list: its folder");
	}

	#[test]
	fn reorder_keeps_the_selection_on_its_items() {
		let (mut p, mut s) = (project(), Selection::default());
		for _ in 0..3 {
			run(&mut p, &mut s, Command::AddSource { at: None });
		}
		s.select(Row::Item(ItemRef::new(ItemKind::Source, 0)));
		run(&mut p, &mut s, Command::Reorder { target: ItemRef::new(ItemKind::Source, 0), to: 2 });
		assert_eq!(p.sources[2].name, "Source 1");
		assert_eq!(s.current(), Some(Row::Item(ItemRef::new(ItemKind::Source, 2))));
	}

	#[test]
	fn scheme_rename_repoints_layers() {
		let (mut p, mut s) = (project(), Selection::default());
		run(&mut p, &mut s, Command::AddScheme);
		p.display.field.color_scheme = "Scheme 1".into();
		let c = run(&mut p, &mut s, Command::Rename { target: ItemRef::new(ItemKind::Scheme, 0), name: "Mine".into() });
		assert_eq!((c.kind, p.display.field.color_scheme.as_str()), (ChangeKind::Display, "Mine"));
	}
}
