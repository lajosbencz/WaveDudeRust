//! Model edits as commands, applied through one [`apply`] that also leaves the selection the command makes
//! and classifies the change for the live controller (docs/ui/60-live-editing-and-runs.md, Edit classes).
//!
//! Property edits are typed: [`Command::Edit`] carries an [`Edit`] of the model's generated edit enums, and
//! every change is logged at Trace as old -> new.
//! Threads: plain functions on the UI thread.

use wavedude_core::model::Edit as ModelEdit;
use wavedude_core::model::names::{self, Named};
use wavedude_core::model::normalize::Normalize;
use wavedude_core::model::{
	BraggReflector, Diffuser, Ellipse, FacettedGrating, Fiber, FresnelLens, GateKind, GrinLens, InterferenceFilter,
	ItemKind, ItemRef, Lens, LogicGate, MetaAxiconDesign, MetaDesign, MetaLensDesign, MetaPrismDesign, MetaSurface,
	Outline, PartKind, Point, Polygon, Probe, ProbeDetector, Project, Rectangle, SimpleGrating, Triangle, ZonePlate,
};
use wavedude_core::model::{
	ColorSchemeEdit, ColorSchemeField, DisplayEdit, DisplayLayerEdit, EditError, EllipseEdit, Field as _, LensEdit,
	MaterialEdit, Part, PartBaseEdit, PartEdit, PartField, PartKindEdit, ProbeEdit, ProbeField, ProjectEdit,
	ProjectField, RectangleEdit, SourceEdit, SourceField, Value as FieldValue,
};

use wavedude_core::scene::gate_design::{self, GateDesign};

use super::clipboard::{self, Fragment, PasteTarget};
use super::log;
use super::selection::{Row, Selection, move_group};

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

/// Which field of which object an [`Edit`] sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldRef {
	Project(ProjectField),
	Source(usize, SourceField),
	Part(usize, PartField),
	Probe(usize, ProbeField),
	Scheme(usize, ColorSchemeField),
}

/// A typed edit of the project's own fields or of one item's, by the item's index.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
	Project(ProjectEdit),
	Source(usize, SourceEdit),
	Part(usize, PartEdit),
	Probe(usize, ProbeEdit),
	Scheme(usize, ColorSchemeEdit),
}

impl Edit {
	/// The item the edit addresses; `None` for the project's own fields.
	fn item(&self) -> Option<ItemRef> {
		let (kind, index) = match self {
			Self::Project(_) => return None,
			Self::Source(i, _) => (ItemKind::Source, *i),
			Self::Part(i, _) => (ItemKind::Part, *i),
			Self::Probe(i, _) => (ItemKind::Probe, *i),
			Self::Scheme(i, _) => (ItemKind::Scheme, *i),
		};
		Some(ItemRef::new(kind, index))
	}

	/// The field the edit sets.
	pub fn field_ref(&self) -> FieldRef {
		match self {
			Self::Project(e) => FieldRef::Project(e.field()),
			Self::Source(i, e) => FieldRef::Source(*i, e.field()),
			Self::Part(i, e) => FieldRef::Part(*i, e.field()),
			Self::Probe(i, e) => FieldRef::Probe(*i, e.field()),
			Self::Scheme(i, e) => FieldRef::Scheme(*i, e.field()),
		}
	}

	/// The edit that restores what `p` holds now; `None` when the item or the field is absent. Side effects of
	/// applying (the mirror rename, normalisation) are not undone; a linked width is exact, as linked parts are
	/// normalised equal.
	pub fn current(&self, p: &Project) -> Option<Self> {
		Some(match self {
			Self::Project(e) => Self::Project(e.current(p)?),
			Self::Source(i, e) => Self::Source(*i, e.current(p.sources.get(*i)?)?),
			Self::Part(i, e) => Self::Part(*i, e.current(p.parts.get(*i)?)?),
			Self::Probe(i, e) => Self::Probe(*i, e.current(p.probes.get(*i)?)?),
			Self::Scheme(i, e) => Self::Scheme(*i, e.current(p.color_schemes.get(*i)?)?),
		})
	}

	/// The edited field's value in `p`.
	fn value(&self, p: &Project) -> FieldValue {
		match self {
			Self::Project(e) => e.field().get(p),
			Self::Source(i, e) => p.sources.get(*i).and_then(|o| e.field().get(o)),
			Self::Part(i, e) => p.parts.get(*i).and_then(|o| e.field().get(o)),
			Self::Probe(i, e) => p.probes.get(*i).and_then(|o| e.field().get(o)),
			Self::Scheme(i, e) => p.color_schemes.get(*i).and_then(|o| e.field().get(o)),
		}
		.unwrap_or(FieldValue::Absent)
	}

	/// The label and the path of the innermost object's field.
	fn subject(&self) -> (&'static str, String) {
		match self {
			Self::Project(e) => match e {
				ProjectEdit::Seed(_) | ProjectEdit::Description(_) => ("Project", e.field().path()),
				ProjectEdit::Scene(e) => ("Scene", e.field().path()),
				ProjectEdit::Simulation(e) => ("Simulation", e.field().path()),
				ProjectEdit::Display(e) => ("Display", e.field().path()),
			},
			Self::Source(_, e) => ("Source", e.field().path()),
			Self::Part(_, e) => ("Part", e.field().path()),
			Self::Probe(_, e) => ("Probe", e.field().path()),
			Self::Scheme(_, e) => ("Scheme", e.field().path()),
		}
	}

	/// The subject and the path, with the item's name when it has one: `Probe 'Probe 1'.BoxWidth`.
	fn describe(&self, project: &Project) -> String {
		let (label, path) = self.subject();
		match self.item().and_then(|item| item.name(project)) {
			Some(name) => format!("{label} '{name}'.{path}"),
			None => format!("{label}.{path}"),
		}
	}

	/// The rename an edit of an item's `Name` stands for.
	fn rename(&self) -> Option<(ItemRef, &str)> {
		let (item, name) = match self {
			Self::Source(_, SourceEdit::Name(n))
			| Self::Part(_, PartEdit::Base(PartBaseEdit::Name(n)))
			| Self::Probe(_, ProbeEdit::Name(n))
			| Self::Scheme(_, ColorSchemeEdit::Name(n)) => (self.item()?, n),
			_ => return None,
		};
		Some((item, name))
	}
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
	/// A typed field edit; a lens's mirror renames it as the editor's Mirror box does, a linked rectangle's or
	/// ellipse's width sets its length too.
	Edit(Edit),
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
	/// Brightness and `ProbePresentation` together: a simulation edit with 8-bit preview samples, else
	/// `ProbePresentation`.
	ProbesAndBrightness,
	/// Scene, solver, seed, sources, parts: the run is stale.
	Simulation,
}

impl ChangeKind {
	/// The least destructive kind covering both.
	pub fn merge(self, other: Self) -> Self {
		match (self, other) {
			(Self::Brightness, Self::ProbePresentation) | (Self::ProbePresentation, Self::Brightness) => {
				Self::ProbesAndBrightness
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
	#[error("{0}")]
	Edit(#[from] EditError),
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
	} else if old.iter().map(Probe::recording_key).eq(new.iter().map(Probe::recording_key)) {
		ChangeKind::Display
	} else {
		ChangeKind::ProbePresentation
	}
}

fn list_insert_point(project: &Project, at: Option<Point>) -> (i32, i32) {
	at.map_or_else(|| project.canvas_centre(), |p| (p.x, p.y))
}

/// A gate's input ports: where a source must sit to feed it, by the gate's name (unique within the parts, and
/// what identifies an item across a command that shifts indices, as `Selection::follow`).
fn gate_ports(project: &Project) -> Vec<(String, [Point; 2])> {
	project
		.parts
		.iter()
		.filter_map(|part| {
			let PartKind::LogicGate(gate) = &part.kind else { return None };
			let design = GateDesign::for_gate(gate, &project.simulation)?;
			let [a, b, _, _] = gate_design::ports_of(&design, &part.base);
			Some((part.base.name.clone(), [a, b]))
		})
		.collect()
}

/// Sources sitting on a gate's input ports before a change follow the ports after it (a moved, turned or
/// retuned gate keeps its feed). Returns whether a source moved.
pub fn follow_gate_ports(before: &[(String, [Point; 2])], project: &mut Project) -> bool {
	let after = gate_ports(project);
	let mut moved = false;
	for (name, old_ports) in before {
		let Some((_, new_ports)) = after.iter().find(|(n, _)| n == name) else { continue };
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

/// Applies `command` to `project`, leaves the selection the command makes in `selection`, normalises the
/// project and classifies the change.
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
		Command::Edit(edit) => apply_edit(project, &edit)?,
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
	let recording = (target.kind == ItemKind::Probe).then(|| project.probes[target.index].recording_key());
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
	// A probe's name is not a measurement, so the graphs relabel and the run keeps its traces; a detector's
	// name seeds its stream.
	let kind = match target.kind {
		ItemKind::Probe if recording == Some(project.probes[target.index].recording_key()) => ChangeKind::Display,
		ItemKind::Probe => ChangeKind::ProbePresentation,
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

/// Applies `edit` to `part`; the old value.
fn run_part(part: &mut Part, edit: &PartEdit) -> Result<FieldValue, EditError> {
	if let PartEdit::Kind(PartKindEdit::Lens(LensEdit::Material(MaterialEdit::Mirror(on)))) = edit
		&& let PartKind::Lens(lens) = &part.kind
	{
		let old = FieldValue::Bool(lens.material.mirror);
		names::set_lens_mirror(part, *on);
		return Ok(old);
	}
	let old = edit.apply(part)?;
	match (edit, &mut part.kind) {
		(
			PartEdit::Kind(PartKindEdit::Rectangle(RectangleEdit::Width(_))),
			PartKind::Rectangle(Rectangle { length, width, linked: true, .. }),
		)
		| (
			PartEdit::Kind(PartKindEdit::Ellipse(EllipseEdit::Width(_))),
			PartKind::Ellipse(Ellipse { length, width, linked: true, .. }),
		) => *length = *width,
		_ => {}
	}
	Ok(old)
}

fn project_change(edit: &ProjectEdit) -> ChangeKind {
	match edit {
		ProjectEdit::Seed(_) | ProjectEdit::Scene(_) | ProjectEdit::Simulation(_) => ChangeKind::Simulation,
		ProjectEdit::Description(_) => ChangeKind::Metadata,
		ProjectEdit::Display(
			DisplayEdit::Field(DisplayLayerEdit::Brightness(_))
			| DisplayEdit::Intensity(DisplayLayerEdit::Brightness(_)),
		) => ChangeKind::Brightness,
		// Showing the intensity makes the run track it: other frames.
		ProjectEdit::Display(DisplayEdit::Intensity(DisplayLayerEdit::Preview(_))) => ChangeKind::Simulation,
		ProjectEdit::Display(_) => ChangeKind::Display,
	}
}

fn apply_edit(project: &mut Project, edit: &Edit) -> Result<Change, CommandError> {
	if let Some((target, name)) = edit.rename() {
		return rename(project, target, name.to_owned());
	}
	if let Some(item) = edit.item() {
		check(project, item)?;
	}
	let label = edit.describe(project);
	let (old, kind) = match edit {
		Edit::Project(e) => (e.apply(project)?, project_change(e)),
		Edit::Source(i, e) => (e.apply(&mut project.sources[*i])?, ChangeKind::Simulation),
		Edit::Part(i, e) => (run_part(&mut project.parts[*i], e)?, ChangeKind::Simulation),
		Edit::Probe(i, e) => {
			let probe = &mut project.probes[*i];
			let before = probe.clone();
			let old = e.apply(probe)?;
			(old, probes_change(std::slice::from_ref(&before), std::slice::from_ref(probe)))
		}
		Edit::Scheme(i, e) => (e.apply(&mut project.color_schemes[*i])?, ChangeKind::Display),
	};
	// The value the model keeps: normalising may clamp what was asked for.
	project.normalize();
	let new = edit.value(project);
	if old == new {
		return Ok(Change::of(ChangeKind::None));
	}
	log::change(&label, &old.to_string(), &new.to_string());
	Ok(Change::of(kind).noted(format!("{label} = {new}")))
}

#[cfg(test)]
mod tests {
	use super::*;
	use wavedude_core::model::{SceneEdit, SimulationEdit, VideoEdit};

	fn project() -> Project {
		let mut p = Project::default();
		p.scene.width = 200;
		p.scene.height = 100;
		p
	}

	fn run(p: &mut Project, s: &mut Selection, c: Command) -> Change {
		apply(p, s, c).unwrap()
	}

	/// The follower pairs a gate's ports before and after a command by the gate's name: a removal before the
	/// gate shifts its index, and the sources feeding it must not jump to the gate that took the index.
	#[test]
	fn a_removed_part_before_a_gate_leaves_its_feed_where_it_is() {
		use wavedude_core::model::{GateKind, LogicGate, Part, PartKind, Rectangle, Source};
		let mut p = project();
		p.scene.width = 3000;
		p.scene.height = 1500;
		let mut s = Selection::default();
		p.parts.push(Part::new(PartKind::Rectangle(Rectangle::default()), 50, 50));
		for (name, x, y) in [("A", 800, 400), ("B", 800, 1000)] {
			let mut gate =
				Part::new(PartKind::LogicGate(LogicGate { kind: GateKind::Xor, ..LogicGate::default() }), x, y);
			gate.base.name = name.into();
			p.parts.push(gate);
		}
		p.normalize();
		let ports = gate_ports(&p);
		assert_eq!(ports.len(), 2, "both gates have ports");
		let feed = ports[0].1[0];
		assert_ne!(feed, ports[1].1[0], "the gates' ports differ");
		p.sources.push(Source { name: "S".into(), x: f64::from(feed.x), y: f64::from(feed.y), ..Source::default() });
		run(&mut p, &mut s, Command::Remove { target: ItemRef::new(ItemKind::Part, 0) });
		assert_eq!(p.parts.len(), 2);
		assert_eq!(
			(p.sources[0].x, p.sources[0].y),
			(f64::from(feed.x), f64::from(feed.y)),
			"the source stays on A's port"
		);
	}

	#[test]
	fn a_probe_rename_is_display_unless_the_detector_records_by_name() {
		let mut p = project();
		let mut s = Selection::default();
		let mut other = p.clone();
		other.display.intensity.preview = !p.display.intensity.preview;
		assert_eq!(classify(&other, &p), ChangeKind::Simulation);
		run(&mut p, &mut s, Command::AddProbe { at: None, detector: ProbeDetector::Standard });
		let c = run(&mut p, &mut s, Command::Rename { target: ItemRef::new(ItemKind::Probe, 0), name: "Gauge".into() });
		assert_eq!(c.kind, ChangeKind::Display);
		// a detector's name is part of what it records
		run(&mut p, &mut s, Command::AddProbe { at: None, detector: ProbeDetector::Counting });
		let c = run(&mut p, &mut s, Command::Rename { target: ItemRef::new(ItemKind::Probe, 1), name: "PM 2".into() });
		assert_eq!(c.kind, ChangeKind::ProbePresentation);
	}

	fn edit(p: &mut Project, s: &mut Selection, e: Edit) -> Change {
		run(p, s, Command::Edit(e))
	}

	/// One edit of every arm of `project_change` and of every item kind, in order; the last repeats the first.
	#[test]
	fn edits_classify_by_what_they_change() {
		let (mut p, mut s) = (project(), Selection::default());
		for command in [
			Command::AddSource { at: None },
			Command::AddProbe { at: None, detector: ProbeDetector::Standard },
			Command::AddProbe { at: None, detector: ProbeDetector::Counting },
			Command::AddPart { kind: NewPart::Rectangle, at: None },
			Command::AddScheme,
		] {
			run(&mut p, &mut s, command);
		}
		let preview = !p.display.intensity.preview;
		let display = |e| Edit::Project(ProjectEdit::Display(e));
		let table = [
			(Edit::Project(ProjectEdit::Seed(7)), ChangeKind::Simulation),
			(Edit::Project(ProjectEdit::Description("x".into())), ChangeKind::Metadata),
			(Edit::Project(ProjectEdit::Scene(SceneEdit::Width(300))), ChangeKind::Simulation),
			(Edit::Project(ProjectEdit::Simulation(SimulationEdit::Steps(777))), ChangeKind::Simulation),
			(display(DisplayEdit::Field(DisplayLayerEdit::Brightness(5.5))), ChangeKind::Brightness),
			(display(DisplayEdit::Intensity(DisplayLayerEdit::Brightness(5.5))), ChangeKind::Brightness),
			(display(DisplayEdit::Intensity(DisplayLayerEdit::Preview(preview))), ChangeKind::Simulation),
			(display(DisplayEdit::Field(DisplayLayerEdit::Overlay(false))), ChangeKind::Display),
			(display(DisplayEdit::Video(VideoEdit::Fps(11))), ChangeKind::Display),
			(Edit::Source(0, SourceEdit::Frequency(120.0)), ChangeKind::Simulation),
			(Edit::Part(0, PartEdit::Kind(PartKindEdit::Rectangle(RectangleEdit::Length(33)))), ChangeKind::Simulation),
			(Edit::Probe(0, ProbeEdit::Ax(7)), ChangeKind::ProbePresentation),
			(Edit::Probe(0, ProbeEdit::BoxX(40)), ChangeKind::Display),
			(Edit::Probe(1, ProbeEdit::Sensitivity(40.0)), ChangeKind::ProbePresentation),
			(Edit::Probe(1, ProbeEdit::BoxX(30)), ChangeKind::Display),
			(Edit::Scheme(0, ColorSchemeEdit::Description("d".into())), ChangeKind::Display),
			(Edit::Project(ProjectEdit::Seed(7)), ChangeKind::None),
		];
		for (e, want) in table {
			assert_eq!(edit(&mut p, &mut s, e.clone()).kind, want, "{e:?}");
		}
	}

	#[test]
	fn a_change_is_noted_with_its_subject_and_the_edit_is_normalised() {
		let (mut p, mut s) = (project(), Selection::default());
		let c = edit(&mut p, &mut s, Edit::Project(ProjectEdit::Seed(7)));
		assert_eq!(c.note.as_deref(), Some("Project.Seed = 7"));
		run(&mut p, &mut s, Command::AddProbe { at: None, detector: ProbeDetector::Standard });
		let c = edit(&mut p, &mut s, Edit::Probe(0, ProbeEdit::Ax(7)));
		assert_eq!((c.note.as_deref(), p.probes[0].ax), (Some("Probe 'Probe 1'.Ax = 7"), 7));
		run(&mut p, &mut s, Command::AddSource { at: None });
		let c = edit(&mut p, &mut s, Edit::Source(0, SourceEdit::Frequency(9999.0)));
		assert_eq!((c.kind, p.sources[0].frequency), (ChangeKind::Simulation, 255.0));
	}

	#[test]
	fn typed_lens_mirror_renames_and_linked_width() {
		let (mut p, mut s) = (project(), Selection::default());
		run(&mut p, &mut s, Command::AddPart { kind: NewPart::Lens, at: None });
		let mirror = PartEdit::Kind(PartKindEdit::Lens(LensEdit::Material(MaterialEdit::Mirror(true))));
		edit(&mut p, &mut s, Edit::Part(0, mirror));
		assert_eq!(p.parts[0].base.name, "Mirror 1");
		run(&mut p, &mut s, Command::AddPart { kind: NewPart::Rectangle, at: Some(Point::new(5, 5)) });
		let rect = |e| Edit::Part(1, PartEdit::Kind(PartKindEdit::Rectangle(e)));
		edit(&mut p, &mut s, rect(RectangleEdit::Linked(true)));
		edit(&mut p, &mut s, rect(RectangleEdit::Width(80)));
		let PartKind::Rectangle(r) = &p.parts[1].kind else { panic!() };
		assert_eq!((r.length, r.width), (80, 80));
	}

	#[test]
	fn current_is_the_inverse_of_an_edit() {
		let (mut p, mut s) = (project(), Selection::default());
		run(&mut p, &mut s, Command::AddProbe { at: None, detector: ProbeDetector::Standard });
		run(&mut p, &mut s, Command::AddPart { kind: NewPart::Rectangle, at: None });
		run(&mut p, &mut s, Command::AddPart { kind: NewPart::Rectangle, at: None });
		let linked = PartEdit::Kind(PartKindEdit::Rectangle(RectangleEdit::Linked(true)));
		edit(&mut p, &mut s, Edit::Part(1, linked));
		let edits = [
			Edit::Project(ProjectEdit::Seed(9)),
			Edit::Project(ProjectEdit::Display(DisplayEdit::Field(DisplayLayerEdit::Brightness(3.0)))),
			Edit::Probe(0, ProbeEdit::Ax(11)),
			Edit::Part(0, PartEdit::Kind(PartKindEdit::Rectangle(RectangleEdit::Length(33)))),
			Edit::Part(1, PartEdit::Kind(PartKindEdit::Rectangle(RectangleEdit::Width(44)))),
		];
		for e in edits {
			let before = p.clone();
			let undo = e.current(&p).expect("the field exists");
			edit(&mut p, &mut s, e);
			assert_ne!(p, before);
			edit(&mut p, &mut s, undo);
			assert_eq!(p, before);
		}
		assert_eq!(Edit::Probe(5, ProbeEdit::Ax(1)).current(&p), None, "no such item");
	}

	#[test]
	fn an_edit_describes_its_subject_and_path() {
		let (mut p, mut s) = (project(), Selection::default());
		run(&mut p, &mut s, Command::AddProbe { at: None, detector: ProbeDetector::Standard });
		let e = Edit::Probe(0, ProbeEdit::BoxWidth(500));
		assert_eq!(e.describe(&p), "Probe 'Probe 1'.BoxWidth");
		assert_eq!(e.item(), Some(ItemRef::new(ItemKind::Probe, 0)));
		let e = Edit::Project(ProjectEdit::Display(DisplayEdit::Intensity(DisplayLayerEdit::Preview(true))));
		assert_eq!(e.describe(&p), "Display.Intensity.Preview");
		assert_eq!(e.item(), None);
	}

	#[test]
	fn a_name_edit_renames_like_the_command() {
		let (mut p, mut s) = (project(), Selection::default());
		run(&mut p, &mut s, Command::AddScheme);
		p.display.field.color_scheme = "Scheme 1".into();
		let name = |n: &str| Edit::Scheme(0, ColorSchemeEdit::Name(n.into()));
		let c = edit(&mut p, &mut s, name("  Mine "));
		assert_eq!(
			(c.kind, p.color_schemes[0].name.as_str(), p.display.field.color_scheme.as_str()),
			(ChangeKind::Display, "Mine", "Mine")
		);
		let blank = apply(&mut p, &mut s, Command::Edit(name("  ")));
		assert_eq!(blank, Err(CommandError::EmptyName));
		assert_eq!(p.color_schemes[0].name, "Mine");
	}

	#[test]
	fn an_edit_of_the_wrong_part_kind_changes_nothing() {
		let (mut p, mut s) = (project(), Selection::default());
		run(&mut p, &mut s, Command::AddPart { kind: NewPart::Rectangle, at: None });
		let before = p.clone();
		let wrong = Edit::Part(0, PartEdit::Kind(PartKindEdit::Lens(LensEdit::Diameter(10))));
		let err = apply(&mut p, &mut s, Command::Edit(wrong)).unwrap_err();
		assert!(matches!(err, CommandError::Edit(EditError::Absent(_))), "{err:?}");
		assert_eq!(p, before);
		let missing = Command::Edit(Edit::Source(3, SourceEdit::Frequency(1.0)));
		assert_eq!(apply(&mut p, &mut s, missing), Err(CommandError::NoItem(ItemRef::new(ItemKind::Source, 3))));
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
		let mut moved = p.clone();
		moved.probes[0].box_y += 5;
		assert_eq!(classify(&p, &moved), ChangeKind::Display);
		moved.probes[0].ay += 1;
		assert_eq!(classify(&p, &moved), ChangeKind::ProbePresentation);
		moved.display.field.brightness += 1.0;
		assert_eq!(classify(&p, &moved), ChangeKind::ProbesAndBrightness, "the frames stay with 16-bit samples");
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

	#[test]
	fn an_edit_normalised_back_to_the_kept_value_changes_nothing() {
		let mut p = project();
		p.simulation.steps = 10_000_000;
		let edit = Edit::Project(ProjectEdit::Simulation(SimulationEdit::Steps(20_000_000)));
		let change = run(&mut p, &mut Selection::default(), Command::Edit(edit));
		assert_eq!(change.kind, ChangeKind::None, "the steps are clamped back to their maximum");
		assert_eq!(p.simulation.steps, 10_000_000);
	}
}
