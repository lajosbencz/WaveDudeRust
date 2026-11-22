//! Model edits as commands, applied through one [`apply`] that also leaves the selection the command makes and
//! classifies the change for the live controller (docs/ui/60-live-editing-and-runs.md, Edit classes).
//!
//! Property edits are typed: [`Command::Edit`] carries an [`Edit`] of the model's generated edit enums; every change
//! is logged at Trace as old -> new.
//!
//! Threads: plain functions on the UI thread.

use waveduderust::model::Edit as ModelEdit;
use waveduderust::model::names::{self, Named};
use waveduderust::model::normalize::Normalize;
use waveduderust::model::{
	BraggReflector, Diffuser, Ellipse, FacettedGrating, Fiber, FresnelLens, GateKind, GrinLens, InterferenceFilter,
	ItemKind, ItemRef, Lens, LogicGate, MetaAxiconDesign, MetaDesign, MetaLensDesign, MetaPrismDesign, MetaSurface,
	Outline, PartKind, Point, Polygon, Probe, ProbeDetector, Project, Rectangle, SimpleGrating, Triangle, View,
	ZonePlate,
};
use waveduderust::model::{
	ColorSchemeEdit, ColorSchemeField, DisplayLayerEdit, EditError, EllipseEdit, Field as _, Fields as _, LensEdit,
	MaterialEdit, Part, PartBaseEdit, PartEdit, PartField, PartKindEdit, ProbeEdit, ProbeField, ProjectEdit,
	ProjectField, RectangleEdit, SourceEdit, SourceField, Value as FieldValue, ViewEdit, ViewField,
};
use waveduderust::model::{PartId, ProbeId, SchemeId, SourceId};

use waveduderust::unstable::scene::gate_design::{self, GateDesign};

use super::clipboard::{self, Fragment, PasteTarget};
use super::log;
use super::selection::{Row, Selection, move_group};

/// The kinds of the Add part menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
		let gate = |kind| PartKind::LogicGate(LogicGate::default().with_kind(kind));
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

	/// The kind a part is of; `None` for a kind the menu lacks.
	pub fn of(kind: &PartKind) -> Option<Self> {
		match kind {
			PartKind::Lens(_) => Some(Self::Lens),
			PartKind::FresnelLens(_) => Some(Self::FresnelLens),
			PartKind::Polygon(_) => Some(Self::Polygon),
			PartKind::Rectangle(_) => Some(Self::Rectangle),
			PartKind::Ellipse(_) => Some(Self::Ellipse),
			PartKind::Triangle(_) => Some(Self::Triangle),
			PartKind::Outline(_) => Some(Self::Outline),
			PartKind::Diffuser(_) => Some(Self::Diffuser),
			PartKind::InterferenceFilter(_) => Some(Self::InterferenceFilter),
			PartKind::SimpleGrating(_) => Some(Self::SimpleGrating),
			PartKind::MetaSurface(m) => match m.design {
				MetaDesign::Lens(_) => Some(Self::MetaLens),
				MetaDesign::Prism(_) => Some(Self::MetaPrism),
				MetaDesign::Axicon(_) => Some(Self::MetaAxicon),
				_ => None,
			},
			PartKind::FacettedGrating(_) => Some(Self::FacettedGrating),
			PartKind::Fiber(_) => Some(Self::Fiber),
			PartKind::BraggReflector(_) => Some(Self::BraggReflector),
			PartKind::GrinLens(_) => Some(Self::GrinLens),
			PartKind::LogicGate(g) => match g.kind {
				GateKind::Xor => Some(Self::XorGate),
				GateKind::And => Some(Self::AndGate),
				GateKind::Or => Some(Self::OrGate),
				_ => None,
			},
			PartKind::ZonePlate(_) => Some(Self::ZonePlate),
			_ => None,
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
	View(ViewField),
	Source(SourceId, SourceField),
	Part(PartId, PartField),
	Probe(ProbeId, ProbeField),
	Scheme(SchemeId, ColorSchemeField),
}

/// A typed edit of the project's own fields, of its view's or of one item's, by the item's id.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
	Project(ProjectEdit),
	View(ViewEdit),
	Source(SourceId, SourceEdit),
	Part(PartId, PartEdit),
	Probe(ProbeId, ProbeEdit),
	Scheme(SchemeId, ColorSchemeEdit),
}

impl Edit {
	/// The item the edit addresses; `None` for the project's and the view's own fields.
	fn item(&self) -> Option<ItemRef> {
		Some(match self {
			Self::Project(_) | Self::View(_) => return None,
			Self::Source(i, _) => ItemRef::Source(*i),
			Self::Part(i, _) => ItemRef::Part(*i),
			Self::Probe(i, _) => ItemRef::Probe(*i),
			Self::Scheme(i, _) => ItemRef::Scheme(*i),
		})
	}

	/// The field the edit sets.
	pub fn field_ref(&self) -> FieldRef {
		match self {
			Self::Project(e) => FieldRef::Project(e.field()),
			Self::View(e) => FieldRef::View(e.field()),
			Self::Source(i, e) => FieldRef::Source(*i, e.field()),
			Self::Part(i, e) => FieldRef::Part(*i, e.field()),
			Self::Probe(i, e) => FieldRef::Probe(*i, e.field()),
			Self::Scheme(i, e) => FieldRef::Scheme(*i, e.field()),
		}
	}

	/// The edit that restores what `p` and `v` hold now; `None` when the item or the field is absent. Side effects
	/// of applying (the mirror rename, normalisation) are not undone; a linked width is exact, as linked parts are
	/// normalised equal.
	pub fn current(&self, p: &Project, v: &View) -> Option<Self> {
		Some(match self {
			Self::Project(e) => Self::Project(e.current(p)?),
			Self::View(e) => Self::View(e.current(v)?),
			Self::Source(i, e) => Self::Source(*i, e.current(p.source(*i)?)?),
			Self::Part(i, e) => Self::Part(*i, e.current(p.part(*i)?)?),
			Self::Probe(i, e) => Self::Probe(*i, e.current(p.probe(*i)?)?),
			Self::Scheme(i, e) => Self::Scheme(*i, e.current(p.color_scheme(*i)?)?),
		})
	}

	/// The edited field's value in `p` and `v`.
	fn value(&self, p: &Project, v: &View) -> FieldValue {
		match self {
			Self::Project(e) => e.field().get(p),
			Self::View(e) => e.field().get(v),
			Self::Source(i, e) => p.source(*i).and_then(|o| e.field().get(o)),
			Self::Part(i, e) => p.part(*i).and_then(|o| e.field().get(o)),
			Self::Probe(i, e) => p.probe(*i).and_then(|o| e.field().get(o)),
			Self::Scheme(i, e) => p.color_scheme(*i).and_then(|o| e.field().get(o)),
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
			},
			Self::View(e) => (View::TYPE_NAME, e.field().path()),
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
	ImportScheme(Box<waveduderust::model::ColorScheme>),
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
	/// A probe's place or measurement, or the intensity hidden: the request is made anew, the frames stay while
	/// the run in force serves it.
	Respec,
	/// Brightness and `Respec` together: a simulation edit with 8-bit preview samples, else
	/// `Respec`.
	ProbesAndBrightness,
	/// Scene, solver, seed, sources, parts: the run is stale.
	Simulation,
}

impl ChangeKind {
	/// The least destructive kind covering both.
	pub fn merge(self, other: Self) -> Self {
		match (self, other) {
			(Self::Brightness, Self::Respec) | (Self::Respec, Self::Brightness) => Self::ProbesAndBrightness,
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
		ItemKind::Probe => ChangeKind::Respec,
		ItemKind::Scheme => ChangeKind::Display,
		_ => ChangeKind::Simulation,
	}
}

/// The class of the difference between two projects and their views (an undo, a redo, a settings paste).
pub fn classify(old: &Project, old_view: &View, new: &Project, new_view: &View) -> ChangeKind {
	if old.seed != new.seed
		|| old.scene != new.scene
		|| old.simulation != new.simulation
		|| old.sources != new.sources
		|| old.parts != new.parts
		|| (!old_view.intensity.preview && new_view.intensity.preview)
	{
		return ChangeKind::Simulation;
	}
	let hidden = old_view.intensity.preview && !new_view.intensity.preview;
	let probes = if hidden { ChangeKind::Respec } else { probes_change(&old.probes, &new.probes) };
	let brightness = old_view.field.brightness != new_view.field.brightness
		|| old_view.intensity.brightness != new_view.intensity.brightness;
	let display = if brightness {
		ChangeKind::Brightness
	} else if old_view != new_view || old.color_schemes != new.color_schemes {
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
		ChangeKind::Respec
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

/// Applies `command` to `project` and its `view`, leaves the selection the command makes in `selection`,
/// normalises both and classifies the change.
pub fn apply(
	project: &mut Project,
	view: &mut View,
	selection: &mut Selection,
	command: Command,
) -> Result<Change, CommandError> {
	let ports = gate_ports(project);
	let mut change = apply_command(project, view, selection, command)?;
	if follow_gate_ports(&ports, project) {
		change.kind = change.kind.merge(ChangeKind::Simulation);
	}
	Ok(change)
}

fn apply_command(
	project: &mut Project,
	view: &mut View,
	selection: &mut Selection,
	command: Command,
) -> Result<Change, CommandError> {
	let change = match command {
		Command::AddSource { at } => {
			let (x, y) = list_insert_point(project, at);
			let i = project.add_source_at(x, y);
			selection.set_items(vec![ItemRef::Source(i)]);
			Change::step(ChangeKind::Simulation).noted(format!("added {}", added(project, i)))
		}
		Command::AddProbe { at, detector } => {
			let (x, y) = list_insert_point(project, at);
			let i = project.add_probe_at(x, y);
			if let Some(probe) = project.probe_mut(i) {
				probe.detector = detector;
				probe.normalize();
			}
			selection.set_items(vec![ItemRef::Probe(i)]);
			Change::step(ChangeKind::Respec).noted(format!("added {}", added(project, i)))
		}
		Command::AddPart { kind, at } => {
			let (x, y) = list_insert_point(project, at);
			let i = project.add_part_at(kind.kind(), x, y);
			selection.set_items(vec![ItemRef::Part(i)]);
			Change::step(ChangeKind::Simulation).noted(format!("added {}", added(project, i)))
		}
		Command::AddScheme => {
			let i = project.add_new_color_scheme();
			selection.set_items(vec![ItemRef::Scheme(i)]);
			Change::step(ChangeKind::Display)
		}
		Command::ImportScheme(scheme) => {
			let i = project.add_color_scheme(*scheme);
			selection.set_items(vec![ItemRef::Scheme(i)]);
			Change::step(ChangeKind::Display)
		}
		Command::Remove { target } => remove(project, selection, target)?,
		Command::Duplicate { target } => duplicate(project, selection, target)?,
		Command::ToggleEnabled { target } => {
			check(project, target)?;
			let mut kind = ChangeKind::None;
			for t in selection.targets(target, project) {
				let flipped = match t {
					ItemRef::Source(id) => project.source_mut(id).map(|s| &mut s.enabled),
					ItemRef::Part(id) => project.part_mut(id).map(|p| &mut p.base.enabled),
					ItemRef::Probe(id) => project.probe_mut(id).map(|p| &mut p.active),
					_ => None,
				};
				if let Some(flag) = flipped {
					*flag = !*flag;
					kind = kind.merge(item_kind_change(t.kind()));
				}
			}
			Change::of(kind)
		}
		Command::MoveBy { target, dx, dy } => {
			check(project, target)?;
			let targets = selection.targets(target, project);
			let moved = move_group(project, &targets, dx, dy);
			let kind = if moved == (0, 0) {
				ChangeKind::None
			} else {
				targets.iter().fold(ChangeKind::None, |k, t| k.merge(item_kind_change(t.kind())))
			};
			Change::of(kind)
		}
		Command::Reorder { target, to } => reorder(project, selection, target, to)?,
		Command::Rename { target, name } => rename(project, view, target, name)?,
		Command::Edit(edit) => apply_edit(project, view, &edit)?,
		Command::SetSceneImage { name, path, bytes } => {
			let size = waveduderust::io::image_size(&bytes).map_err(CommandError::Image)?;
			let scene = &mut project.scene;
			scene.mode = waveduderust::model::SceneMode::Image;
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
			let before = (project.clone(), view.clone());
			let pasted = clipboard::paste(project, view, *fragment, target);
			if let Some(at) = place_at {
				clipboard::place_at(project, &pasted.items, at);
			}
			if !pasted.items.is_empty() {
				selection.set_items(pasted.items.clone());
			}
			project.normalize();
			view.normalize();
			let kind = classify(&before.0, &before.1, project, view);
			Change::step(kind).noted(format!("pasted {} objects", pasted.items.len()))
		}
	};
	project.normalize();
	view.normalize();
	selection.retain_existing(project);
	Ok(change)
}

fn check(project: &Project, target: ItemRef) -> Result<(), CommandError> {
	if target.exists(project) { Ok(()) } else { Err(CommandError::NoItem(target)) }
}

/// The name of an item just added.
fn added(project: &Project, item: impl Into<ItemRef>) -> &str {
	item.into().name(project).unwrap_or_default()
}

/// Removes the target or the selection; the user stays at the slot of the last removed: the item that took
/// the slot, else the one above it, else the folder of the emptied list.
fn remove(project: &mut Project, selection: &mut Selection, target: ItemRef) -> Result<Change, CommandError> {
	check(project, target)?;
	let targets = selection.targets(target, project);
	let last = *targets.last().expect("at least the target");
	let kind = targets.iter().fold(ChangeKind::None, |k, t| k.merge(item_kind_change(t.kind())));
	let last_at = project.index_of(last).expect("checked");
	let above = targets
		.iter()
		.filter(|&&t| project.index_of(t).is_some_and(|i| t.kind() == last.kind() && i < last_at))
		.count();
	for &t in &targets {
		project.remove(t);
	}
	let slot = last_at - above;
	match last.kind().len(project) {
		0 => selection.select(Row::Folder(last.kind())),
		len => selection.select(Row::Item(ItemRef::at(last.kind(), slot.min(len - 1), project).expect("in range"))),
	}
	Ok(Change::step(kind).noted(format!("removed {} items", targets.len())))
}

/// A copy after each original, named "<name> copy"; the copies become the selection, the last current.
fn duplicate(project: &mut Project, selection: &mut Selection, target: ItemRef) -> Result<Change, CommandError> {
	check(project, target)?;
	let targets = selection.targets(target, project);
	let mut copies = Vec::new();
	let mut kind = ChangeKind::None;
	for &t in &targets {
		let copy = match t {
			ItemRef::Source(id) => project.duplicate_source(id).map(ItemRef::from),
			ItemRef::Part(id) => project.duplicate_part(id).map(ItemRef::from),
			ItemRef::Probe(id) => project.duplicate_probe(id).map(ItemRef::from),
			ItemRef::Scheme(id) => project.duplicate_color_scheme(id).map(ItemRef::from),
			_ => None,
		};
		let Some(copy) = copy else {
			continue;
		};
		copies.push(copy);
		kind = kind.merge(item_kind_change(t.kind()));
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
	let to = to.min(target.kind().len(project) - 1);
	if Some(to) == project.index_of(target) {
		return Ok(Change::of(ChangeKind::None));
	}
	project.move_item(target, to);
	let current_item = selection.current_item();
	let items: Vec<ItemRef> = selection.items().to_vec();
	*selection = Selection::default();
	for i in items.iter().filter(|&&i| Some(i) != current_item) {
		selection.toggle(*i);
	}
	if let Some(c) = current_item {
		selection.toggle(c);
	}
	// A list's order is the emitter, paint or marker order.
	Ok(Change::of(item_kind_change(target.kind())))
}

/// Renames `target`; the layers of `view` showing a renamed scheme follow it.
fn rename(project: &mut Project, view: &mut View, target: ItemRef, name: String) -> Result<Change, CommandError> {
	check(project, target)?;
	let name = name.trim().to_owned();
	if name.is_empty() {
		return Err(CommandError::EmptyName);
	}
	let recording = |p: &Project| match target {
		ItemRef::Probe(id) => p.probe(id).map(Probe::recording_key),
		_ => None,
	};
	let before = recording(project);
	let item: &mut dyn Named = match target {
		ItemRef::Source(id) => project.source_mut(id).map(|i| i as &mut dyn Named),
		ItemRef::Part(id) => project.part_mut(id).map(|i| i as &mut dyn Named),
		ItemRef::Probe(id) => project.probe_mut(id).map(|i| i as &mut dyn Named),
		ItemRef::Scheme(id) => project.color_scheme_mut(id).map(|i| i as &mut dyn Named),
		_ => None,
	}
	.ok_or(CommandError::NoItem(target))?;
	let old = item.name().to_owned();
	if old == name {
		return Ok(Change::of(ChangeKind::None));
	}
	item.set_name(name.clone());
	log::change(&format!("{old}.name"), &old, &name);
	// A probe's name is not a measurement, so the graphs relabel and the run keeps its traces; a detector's
	// name seeds its stream.
	let kind = match target.kind() {
		ItemKind::Probe if before == recording(project) => ChangeKind::Display,
		ItemKind::Probe => ChangeKind::Respec,
		k => item_kind_change(k),
	};
	let change = Change::of(kind).noted(format!("{old} renamed to {name}"));
	match target.kind() {
		ItemKind::Scheme => {
			for layer in [&mut view.field, &mut view.intensity] {
				if layer.color_scheme.to_lowercase() == old.to_lowercase() {
					layer.color_scheme = name.clone();
				}
			}
		}
		ItemKind::Source | ItemKind::Part | ItemKind::Probe => {}
		_ => {}
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
	}
}

fn view_change(edit: &ViewEdit) -> ChangeKind {
	match edit {
		ViewEdit::Field(DisplayLayerEdit::Brightness(_)) | ViewEdit::Intensity(DisplayLayerEdit::Brightness(_)) => {
			ChangeKind::Brightness
		}
		// Showing the intensity makes the run track it: other frames. A run tracking it serves the hidden.
		ViewEdit::Intensity(DisplayLayerEdit::Preview(true)) => ChangeKind::Simulation,
		ViewEdit::Intensity(DisplayLayerEdit::Preview(false)) => ChangeKind::Respec,
		_ => ChangeKind::Display,
	}
}

fn apply_edit(project: &mut Project, view: &mut View, edit: &Edit) -> Result<Change, CommandError> {
	if let Some((target, name)) = edit.rename() {
		return rename(project, view, target, name.to_owned());
	}
	if let Some(item) = edit.item() {
		check(project, item)?;
	}
	let label = edit.describe(project);
	let missing = || CommandError::NoItem(edit.item().expect("an item edit"));
	let (old, kind) = match edit {
		Edit::Project(e) => (e.apply(project)?, project_change(e)),
		Edit::View(e) => (e.apply(view)?, view_change(e)),
		Edit::Source(i, e) => (e.apply(project.source_mut(*i).ok_or_else(missing)?)?, ChangeKind::Simulation),
		Edit::Part(i, e) => (run_part(project.part_mut(*i).ok_or_else(missing)?, e)?, ChangeKind::Simulation),
		Edit::Probe(i, e) => {
			let probe = project.probe_mut(*i).ok_or_else(missing)?;
			let before = probe.clone();
			let old = e.apply(probe)?;
			(old, probes_change(std::slice::from_ref(&before), std::slice::from_ref(probe)))
		}
		Edit::Scheme(i, e) => (e.apply(project.color_scheme_mut(*i).ok_or_else(missing)?)?, ChangeKind::Display),
	};
	// The value the model keeps: normalising may clamp what was asked for.
	project.normalize();
	view.normalize();
	let new = edit.value(project, view);
	if old == new {
		return Ok(Change::of(ChangeKind::None));
	}
	log::change(&label, &old.to_string(), &new.to_string());
	Ok(Change::of(kind).noted(format!("{label} = {new}")))
}

#[cfg(test)]
mod tests {
	use super::*;
	use waveduderust::model::{SceneEdit, SimulationEdit, VideoEdit, ranges};

	fn project() -> Project {
		let mut p = Project::default();
		p.scene.width = 200;
		p.scene.height = 100;
		p
	}

	fn run(p: &mut Project, v: &mut View, s: &mut Selection, c: Command) -> Change {
		apply(p, v, s, c).unwrap()
	}

	fn at(p: &Project, kind: ItemKind, i: usize) -> ItemRef {
		ItemRef::at(kind, i, p).unwrap()
	}

	/// The follower pairs a gate's ports before and after a command by the gate's name: a removal before the
	/// gate shifts its index, and the sources feeding it must not jump to the gate that took the index.
	#[test]
	fn a_removed_part_before_a_gate_leaves_its_feed_where_it_is() {
		use waveduderust::model::{GateKind, LogicGate, Part, PartKind, Rectangle, Source};
		let mut p = project();
		p.scene.width = 3000;
		p.scene.height = 1500;
		let (mut v, mut s) = (View::default(), Selection::default());
		p.parts.push(Part::new(PartKind::Rectangle(Rectangle::default()), 50, 50));
		for (name, x, y) in [("A", 800, 400), ("B", 800, 1000)] {
			let mut gate = Part::new(PartKind::LogicGate(LogicGate::default().with_kind(GateKind::Xor)), x, y);
			gate.base.name = name.into();
			p.parts.push(gate);
		}
		p.normalize();
		let ports = gate_ports(&p);
		assert_eq!(ports.len(), 2, "both gates have ports");
		let feed = ports[0].1[0];
		assert_ne!(feed, ports[1].1[0], "the gates' ports differ");
		p.sources.push(Source::default().with_name("S".into()).with_x(f64::from(feed.x)).with_y(f64::from(feed.y)));
		let target = at(&p, ItemKind::Part, 0);
		run(&mut p, &mut v, &mut s, Command::Remove { target });
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
		let (mut v, mut s) = (View::default(), Selection::default());
		let mut other = v.clone();
		other.intensity.preview = !v.intensity.preview;
		assert_eq!(classify(&p, &other, &p, &v), ChangeKind::Simulation);
		run(&mut p, &mut v, &mut s, Command::AddProbe { at: None, detector: ProbeDetector::Standard });
		let target = at(&p, ItemKind::Probe, 0);
		let c = run(&mut p, &mut v, &mut s, Command::Rename { target, name: "Gauge".into() });
		assert_eq!(c.kind, ChangeKind::Display);
		// a detector's name is part of what it records
		run(&mut p, &mut v, &mut s, Command::AddProbe { at: None, detector: ProbeDetector::Counting });
		let target = at(&p, ItemKind::Probe, 1);
		let c = run(&mut p, &mut v, &mut s, Command::Rename { target, name: "PM 2".into() });
		assert_eq!(c.kind, ChangeKind::Respec);
	}

	fn edit(p: &mut Project, v: &mut View, s: &mut Selection, e: Edit) -> Change {
		run(p, v, s, Command::Edit(e))
	}

	/// One edit of every arm of `project_change` and of every item kind, in order; the last repeats the first.
	#[test]
	fn edits_classify_by_what_they_change() {
		let (mut p, mut v, mut s) = (project(), View::default(), Selection::default());
		let additions = [
			(Command::AddSource { at: None }, Some(ChangeKind::Simulation)),
			(Command::AddProbe { at: None, detector: ProbeDetector::Standard }, Some(ChangeKind::Respec)),
			(Command::AddProbe { at: None, detector: ProbeDetector::Counting }, None),
			(Command::AddPart { kind: NewPart::Rectangle, at: None }, None),
			(Command::AddScheme, Some(ChangeKind::Display)),
		];
		for (command, kind) in additions {
			let change = run(&mut p, &mut v, &mut s, command.clone());
			if let Some(kind) = kind {
				assert_eq!(change.kind, kind, "{command:?}");
			}
		}
		assert!(v.intensity.preview, "the table hides the intensity first");
		let (source, part, scheme) = (p.sources[0].id(), p.parts[0].id(), p.color_schemes[0].id());
		let probe = |i: usize| p.probes[i].id();
		let table = [
			(Edit::Project(ProjectEdit::Seed(7)), ChangeKind::Simulation),
			(Edit::Project(ProjectEdit::Description("x".into())), ChangeKind::Metadata),
			(Edit::Project(ProjectEdit::Scene(SceneEdit::Width(300))), ChangeKind::Simulation),
			(Edit::Project(ProjectEdit::Simulation(SimulationEdit::Steps(777))), ChangeKind::Simulation),
			(Edit::View(ViewEdit::Field(DisplayLayerEdit::Brightness(5.5))), ChangeKind::Brightness),
			(Edit::View(ViewEdit::Intensity(DisplayLayerEdit::Brightness(5.5))), ChangeKind::Brightness),
			(Edit::View(ViewEdit::Intensity(DisplayLayerEdit::Preview(false))), ChangeKind::Respec),
			(Edit::View(ViewEdit::Intensity(DisplayLayerEdit::Preview(true))), ChangeKind::Simulation),
			(Edit::View(ViewEdit::Field(DisplayLayerEdit::Overlay(false))), ChangeKind::Display),
			(Edit::View(ViewEdit::Video(VideoEdit::Fps(11))), ChangeKind::Display),
			(Edit::Source(source, SourceEdit::Frequency(120.0)), ChangeKind::Simulation),
			(
				Edit::Part(part, PartEdit::Kind(PartKindEdit::Rectangle(RectangleEdit::Length(33)))),
				ChangeKind::Simulation,
			),
			(Edit::Probe(probe(0), ProbeEdit::Ax(7)), ChangeKind::Respec),
			(Edit::Probe(probe(0), ProbeEdit::BoxX(40)), ChangeKind::Display),
			(Edit::Probe(probe(1), ProbeEdit::Sensitivity(40.0)), ChangeKind::Respec),
			(Edit::Probe(probe(1), ProbeEdit::BoxX(30)), ChangeKind::Display),
			(Edit::Scheme(scheme, ColorSchemeEdit::Description("d".into())), ChangeKind::Display),
			(Edit::Project(ProjectEdit::Seed(7)), ChangeKind::None),
		];
		for (e, want) in table {
			assert_eq!(edit(&mut p, &mut v, &mut s, e.clone()).kind, want, "{e:?}");
		}
		// cumulative: each row's change stays in the next
		let mut moved = (p.clone(), v.clone());
		type Change = fn(&mut (Project, View));
		let diffs: [(_, Change, _); 3] = [
			("a probe's box moved", |(p, _)| p.probes[0].box_y += 5, ChangeKind::Display),
			("and its anchor", |(p, _)| p.probes[0].ay += 1, ChangeKind::Respec),
			(
				"and the brightness: the frames stay with 16-bit samples",
				|(_, v)| v.field.brightness += 1.0,
				ChangeKind::ProbesAndBrightness,
			),
		];
		for (case, change, want) in diffs {
			change(&mut moved);
			assert_eq!(classify(&p, &v, &moved.0, &moved.1), want, "{case}");
		}
	}

	#[test]
	fn typed_lens_mirror_renames_and_linked_width() {
		let (mut p, mut v, mut s) = (project(), View::default(), Selection::default());
		run(&mut p, &mut v, &mut s, Command::AddPart { kind: NewPart::Lens, at: None });
		let mirror = PartEdit::Kind(PartKindEdit::Lens(LensEdit::Material(MaterialEdit::Mirror(true))));
		let lens = p.parts[0].id();
		edit(&mut p, &mut v, &mut s, Edit::Part(lens, mirror));
		assert_eq!(p.parts[0].base.name, "Mirror 1");
		run(&mut p, &mut v, &mut s, Command::AddPart { kind: NewPart::Rectangle, at: Some(Point::new(5, 5)) });
		let id = p.parts[1].id();
		let rect = |e| Edit::Part(id, PartEdit::Kind(PartKindEdit::Rectangle(e)));
		edit(&mut p, &mut v, &mut s, rect(RectangleEdit::Linked(true)));
		edit(&mut p, &mut v, &mut s, rect(RectangleEdit::Width(80)));
		let PartKind::Rectangle(r) = &p.parts[1].kind else { panic!() };
		assert_eq!((r.length, r.width), (80, 80));
	}

	#[test]
	fn current_is_the_inverse_of_an_edit() {
		let (mut p, mut v, mut s) = (project(), View::default(), Selection::default());
		run(&mut p, &mut v, &mut s, Command::AddProbe { at: None, detector: ProbeDetector::Standard });
		run(&mut p, &mut v, &mut s, Command::AddPart { kind: NewPart::Rectangle, at: None });
		run(&mut p, &mut v, &mut s, Command::AddPart { kind: NewPart::Rectangle, at: None });
		let linked = PartEdit::Kind(PartKindEdit::Rectangle(RectangleEdit::Linked(true)));
		let (a, b) = (p.parts[0].id(), p.parts[1].id());
		edit(&mut p, &mut v, &mut s, Edit::Part(b, linked));
		let edits = [
			Edit::Project(ProjectEdit::Seed(9)),
			Edit::View(ViewEdit::Field(DisplayLayerEdit::Brightness(3.0))),
			Edit::Probe(p.probes[0].id(), ProbeEdit::Ax(11)),
			Edit::Part(a, PartEdit::Kind(PartKindEdit::Rectangle(RectangleEdit::Length(33)))),
			Edit::Part(b, PartEdit::Kind(PartKindEdit::Rectangle(RectangleEdit::Width(44)))),
		];
		for e in edits {
			let before = (p.clone(), v.clone());
			let undo = e.current(&p, &v).expect("the field exists");
			edit(&mut p, &mut v, &mut s, e);
			assert_ne!((&p, &v), (&before.0, &before.1));
			edit(&mut p, &mut v, &mut s, undo);
			assert_eq!((&p, &v), (&before.0, &before.1));
		}
		let elsewhere = Project::default().add_probe();
		assert_eq!(Edit::Probe(elsewhere, ProbeEdit::Ax(1)).current(&p, &v), None, "no such item");
	}

	#[test]
	fn an_edit_is_described_by_its_subject_and_path_noted_with_its_value_and_normalised() {
		let (mut p, mut v, mut s) = (project(), View::default(), Selection::default());
		run(&mut p, &mut v, &mut s, Command::AddProbe { at: None, detector: ProbeDetector::Standard });
		let e = Edit::Probe(p.probes[0].id(), ProbeEdit::Ax(7));
		assert_eq!(e.describe(&p), "Probe 'Probe 1'.Ax");
		assert_eq!(e.item(), Some(at(&p, ItemKind::Probe, 0)));
		let c = edit(&mut p, &mut v, &mut s, e);
		assert_eq!((c.note.as_deref(), p.probes[0].ax), (Some("Probe 'Probe 1'.Ax = 7"), 7));
		let e = Edit::View(ViewEdit::Intensity(DisplayLayerEdit::Preview(true)));
		assert_eq!(e.describe(&p), "Display.Intensity.Preview");
		assert_eq!(e.item(), None);
		let c = edit(&mut p, &mut v, &mut s, Edit::Project(ProjectEdit::Seed(7)));
		assert_eq!(c.note.as_deref(), Some("Project.Seed = 7"));
		run(&mut p, &mut v, &mut s, Command::AddSource { at: None });
		let source = p.sources[0].id();
		let c = edit(&mut p, &mut v, &mut s, Edit::Source(source, SourceEdit::Frequency(ranges::LEVEL.max * 2.)));
		assert_eq!((c.kind, p.sources[0].frequency), (ChangeKind::Simulation, ranges::LEVEL.max));
	}

	#[test]
	fn a_scheme_renamed_by_the_command_or_a_name_edit_repoints_the_layers() {
		let renames: [fn(SchemeId, &str) -> Command; 2] = [
			|id, n| Command::Rename { target: ItemRef::Scheme(id), name: n.into() },
			|id, n| Command::Edit(Edit::Scheme(id, ColorSchemeEdit::Name(n.into()))),
		];
		for rename in renames {
			let (mut p, mut v, mut s) = (project(), View::default(), Selection::default());
			run(&mut p, &mut v, &mut s, Command::AddScheme);
			let id = p.color_schemes[0].id();
			let rename = |n| rename(id, n);
			v.field.color_scheme = "Scheme 1".into();
			let c = run(&mut p, &mut v, &mut s, rename("  Mine "));
			assert_eq!(
				(c.kind, p.color_schemes[0].name.as_str(), v.field.color_scheme.as_str()),
				(ChangeKind::Display, "Mine", "Mine"),
				"{:?}",
				rename("Mine")
			);
			assert_eq!(apply(&mut p, &mut v, &mut s, rename("  ")), Err(CommandError::EmptyName));
			assert_eq!(p.color_schemes[0].name, "Mine");
		}
	}

	#[test]
	fn an_edit_of_the_wrong_part_kind_changes_nothing() {
		let (mut p, mut v, mut s) = (project(), View::default(), Selection::default());
		run(&mut p, &mut v, &mut s, Command::AddPart { kind: NewPart::Rectangle, at: None });
		let before = p.clone();
		let wrong = Edit::Part(p.parts[0].id(), PartEdit::Kind(PartKindEdit::Lens(LensEdit::Diameter(10))));
		let err = apply(&mut p, &mut v, &mut s, Command::Edit(wrong)).unwrap_err();
		assert!(matches!(err, CommandError::Edit(EditError::Absent(_))), "{err:?}");
		assert_eq!(p, before);
		let elsewhere = Project::default().add_source();
		let missing = Command::Edit(Edit::Source(elsewhere, SourceEdit::Frequency(1.0)));
		assert_eq!(apply(&mut p, &mut v, &mut s, missing), Err(CommandError::NoItem(ItemRef::Source(elsewhere))));
	}

	#[test]
	fn remove_duplicate_toggle_on_the_selection() {
		let (mut p, mut v, mut s) = (project(), View::default(), Selection::default());
		for _ in 0..4 {
			run(&mut p, &mut v, &mut s, Command::AddSource { at: None });
		}
		let src = |p: &Project, i| at(p, ItemKind::Source, i);
		s.select(Row::Item(src(&p, 1)));
		s.toggle(src(&p, 2));
		let target = src(&p, 2);
		let c = run(&mut p, &mut v, &mut s, Command::Duplicate { target });
		assert!(c.own_step);
		let names: Vec<&str> = p.sources.iter().map(|x| x.name.as_str()).collect();
		assert_eq!(names, ["Source 1", "Source 2", "Source 2 copy", "Source 3", "Source 3 copy", "Source 4"]);
		assert_eq!(s.items(), [src(&p, 2), src(&p, 4)]);
		assert_eq!(target, src(&p, 3), "the original keeps its id, the copy has a fresh one");
		let target = src(&p, 4);
		run(&mut p, &mut v, &mut s, Command::ToggleEnabled { target });
		assert!(!p.sources[2].enabled && !p.sources[4].enabled && p.sources[3].enabled);
		let target = src(&p, 2);
		run(&mut p, &mut v, &mut s, Command::Remove { target });
		assert_eq!(p.sources.len(), 4);
		assert_eq!(s.current(), Some(Row::Item(src(&p, 3))), "Source 4 took the slot");
		let target = src(&p, 3);
		run(&mut p, &mut v, &mut s, Command::Remove { target });
		assert_eq!(s.current(), Some(Row::Item(src(&p, 2))), "the last row: the one above");
		for _ in 0..3 {
			let target = src(&p, 0);
			run(&mut p, &mut v, &mut s, Command::Remove { target });
		}
		assert_eq!(s.current(), Some(Row::Folder(ItemKind::Source)), "an emptied list: its folder");
	}

	#[test]
	fn reorder_keeps_the_selection_on_its_items() {
		let (mut p, mut v, mut s) = (project(), View::default(), Selection::default());
		for _ in 0..3 {
			run(&mut p, &mut v, &mut s, Command::AddSource { at: None });
		}
		let target = at(&p, ItemKind::Source, 0);
		s.select(Row::Item(target));
		run(&mut p, &mut v, &mut s, Command::Reorder { target, to: 2 });
		assert_eq!(p.sources[2].name, "Source 1");
		assert_eq!(s.current(), Some(Row::Item(target)));
		assert_eq!(at(&p, ItemKind::Source, 2), target, "the id moved with the item");
	}

	#[test]
	fn an_edit_normalised_back_to_the_kept_value_changes_nothing() {
		let (mut p, mut v) = (project(), View::default());
		let max = ranges::SIM_STEPS.max as u32;
		p.simulation.steps = max;
		let edit = Edit::Project(ProjectEdit::Simulation(SimulationEdit::Steps(2 * max)));
		let change = run(&mut p, &mut v, &mut Selection::default(), Command::Edit(edit));
		assert_eq!(change.kind, ChangeKind::None, "the steps are clamped back to their maximum");
		assert_eq!(p.simulation.steps, max);
	}
}
