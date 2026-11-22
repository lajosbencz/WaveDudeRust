//! The property forms of docs/ui/30 as data: [`build`] lays out the form of a row (its sections and their rows, each
//! with what its editor shows and the events an edit makes, through the model's field accessors such as
//! `Probe::BOX_WIDTH`); a window draws the rows with its own widgets and applies what they return.

mod build;
mod unit;

use std::borrow::Cow;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;

use crate::Event;
use crate::commands::{Command, Edit};
use crate::strings::Strings;
use waveduderust::model::ranges::Range;
use waveduderust::model::{
	Edit as _, Field as _, Fields, ItemRef, Nested, Part, PartBase, Probe, Prop, Rgb, Source, Stop, Variant, Word,
};

pub use self::build::build;
use crate::App;
use crate::gradient;
use crate::number::NumberSpec;
use crate::properties::{Readouts, bounds_of};

/// An editor's identity within the form, stable across passes.
pub type Key = u64;

/// The key of `salt`.
pub fn key(salt: impl Hash) -> Key {
	let mut h = DefaultHasher::new();
	salt.hash(&mut h);
	h.finish()
}

/// The events that set a field to a value.
pub type Setter<'a, V> = Rc<dyn Fn(V) -> Event + 'a>;

/// What an editor asks for: an event for the state, or a request for the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
	Send(Event),
	Ask(Request),
}

/// What a form asks the window for, beyond the state's events.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Request {
	Dialog(Dialog),
	/// The parts' description shows (the setting `part_description_expanded`).
	DescriptionOpen(bool),
}

/// A platform file dialog a form asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialog {
	/// The file picker for the scene image.
	PickSceneImage,
	/// The save dialog for the embedded scene image.
	SaveSceneImage,
	/// The file picker for a colour scheme to add.
	PickScheme,
}

/// A button's drawing beside or instead of its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
	Save,
	Close,
	Minus,
	Up,
	Down,
	/// The link toggle, on.
	Linked,
	/// The link toggle, off.
	Parted,
}

/// A number box: `label` names it, `hint` is its tip where it has no row of its own.
pub struct Number<'a> {
	pub key: Key,
	pub label: &'static str,
	pub hint: &'static str,
	pub value: f64,
	pub spec: NumberSpec,
	/// The unit's text, none for a dimensionless number.
	pub unit: Option<&'static str>,
	/// Shows no value (a setting that does not apply).
	pub blank: bool,
	/// May take the focus asked for the form's first editor.
	pub first: bool,
	pub apply: Box<dyn Fn(f64) -> Event + 'a>,
}

/// A text box; `apply` makes the event of a text, none when the text means nothing.
pub struct Text<'a> {
	pub key: Key,
	pub label: &'static str,
	pub value: Cow<'a, str>,
	/// Enter is a newline; the text wraps.
	pub multiline: bool,
	/// Every keystroke applies; else Enter or the focus leaving, Esc reverting.
	pub live: bool,
	pub first: bool,
	pub apply: Box<dyn Fn(String) -> Option<Event> + 'a>,
}

/// A check box with `text` beside it.
pub struct Check<'a> {
	pub key: Key,
	pub text: &'static str,
	pub on: bool,
	pub apply: Box<dyn Fn(bool) -> Vec<Event> + 'a>,
}

/// A drop-down: the items' words, the word shown closed, and action rows after the items; `pick` takes an item's
/// index, or the item count plus an action's.
pub struct Choice<'a> {
	pub key: Key,
	pub label: &'static str,
	pub words: Vec<Cow<'a, str>>,
	pub current: Option<usize>,
	pub shown: Cow<'a, str>,
	/// Each item's gradient swatch; empty when the items have none.
	pub swatches: Vec<Option<&'a [Stop]>>,
	pub actions: Vec<&'static str>,
	pub pick: Box<dyn Fn(usize) -> Vec<Out> + 'a>,
}

/// A button: its text (the tip names a glyph button), and what a press asks for.
pub struct Button<'a> {
	pub key: Key,
	pub text: &'static str,
	pub tip: &'static str,
	pub glyph: Option<Glyph>,
	pub enabled: bool,
	pub press: Box<dyn Fn() -> Vec<Out> + 'a>,
}

/// A colour swatch with its picker.
pub struct Color<'a> {
	pub key: Key,
	pub value: Rgb,
	pub tip: &'static str,
	pub apply: Box<dyn Fn(Rgb) -> Event + 'a>,
}

/// An outline's point.
pub struct PointRow<'a> {
	pub x: Number<'a>,
	pub y: Number<'a>,
	pub remove: Button<'a>,
}

/// An outline's points: column headings, a row per point, Add point.
pub struct Points<'a> {
	pub heading: [&'static str; 2],
	pub items: Vec<PointRow<'a>>,
	pub add: Button<'a>,
}

/// An interference filter's layer.
pub struct LayerRow<'a> {
	pub thickness: Number<'a>,
	pub index: Number<'a>,
	pub up: Button<'a>,
	pub down: Button<'a>,
	pub remove: Button<'a>,
}

/// An interference filter's layers, the outermost first; `reorder` moves one from an index to another.
pub struct Layers<'a> {
	pub key: Key,
	pub heading: [&'static str; 2],
	pub items: Vec<LayerRow<'a>>,
	pub grip_tip: &'static str,
	pub reorder: Box<dyn Fn(usize, usize) -> Event + 'a>,
	pub add: Button<'a>,
	pub clear: Button<'a>,
}

/// A fibre's node: three lines (the node, its In and Out handles), each a caption and its X and Y.
pub struct NodeRow<'a> {
	pub lines: [(&'static str, Number<'a>, Number<'a>); 3],
	pub smooth: Check<'a>,
	pub remove: Button<'a>,
}

/// A fibre's nodes and Add node.
pub struct Nodes<'a> {
	pub heading: [&'static str; 2],
	pub items: Vec<NodeRow<'a>>,
	pub add: Button<'a>,
}

/// A colour scheme's gradient bar.
pub struct GradientBar<'a> {
	pub key: Key,
	pub gradient: gradient::Gradient<'a>,
	pub tip: &'static str,
	pub apply: Box<dyn Fn(Vec<Stop>) -> Event + 'a>,
}

/// A colour stop: its ordinal, position, swatch, hex text and remove button.
pub struct StopRow<'a> {
	pub ordinal: usize,
	pub position: Number<'a>,
	pub color: Color<'a>,
	pub hex: Text<'a>,
	pub remove: Button<'a>,
}

/// What a row edits.
pub enum RowKind<'a> {
	Number(Number<'a>),
	/// A number with a button after it.
	NumberButton(Number<'a>, Button<'a>),
	/// A check box carrying its text; with a row label, the box sits in the editor column.
	Check(Check<'a>),
	Choice(Choice<'a>),
	Text(Text<'a>),
	/// The scene image's name, its browse, save and remove buttons.
	Image {
		name: Text<'a>,
		browse: Button<'a>,
		save: Button<'a>,
		remove: Button<'a>,
	},
	Readout(Cow<'a, str>),
	/// A secondary value: a read-out in a quieter look.
	Value(Cow<'a, str>),
	/// A full-width paragraph.
	Paragraph(&'static str),
	Buttons(Vec<Button<'a>>),
	/// Rows of one object (X, Y, Angle; a width and a height) the frontend lays out on one line.
	Group(Vec<Row<'a>>),
	/// Two number rows with the link toggle beside both.
	Linked(Box<[Row<'a>; 2]>, Button<'a>),
	Points(Points<'a>),
	Layers(Layers<'a>),
	Nodes(Nodes<'a>),
	Gradient(GradientBar<'a>),
	Stop(StopRow<'a>),
}

/// A row: `label` in the label column (none spans both), `hint` its tooltip, greyed and inert unless `enabled`.
pub struct Row<'a> {
	pub label: Option<&'static str>,
	pub hint: &'static str,
	pub enabled: bool,
	pub kind: RowKind<'a>,
}

impl<'a> Row<'a> {
	fn collect<'r>(&'r self, out: &mut Vec<&'r Row<'a>>) {
		out.push(self);
		if let RowKind::Group(rows) = &self.kind {
			rows.iter().for_each(|r| r.collect(out));
		}
	}
}

/// The text of the unit of `member` of `ty`.
fn unit_text(s: &Strings, ty: &str, member: &str) -> Option<&'static str> {
	unit::of(ty, member).map(|u| s.get(u.key()))
}

/// A titled group of rows.
pub struct Section<'a> {
	pub title: &'static str,
	pub rows: Vec<Row<'a>>,
}

/// The parts' description header: the kind's text shows while `open`.
pub struct Description {
	pub title: &'static str,
	pub tip: &'static str,
	pub text: &'static str,
	pub open: bool,
}

/// A row's form.
#[derive(Default)]
pub struct Form<'a> {
	pub description: Option<Description>,
	pub sections: Vec<Section<'a>>,
}

impl Form<'_> {
	/// Every row, sections in order; a group's rows follow the group.
	pub fn rows(&self) -> impl Iterator<Item = &Row<'_>> {
		let mut all = Vec::new();
		self.sections.iter().flat_map(|s| &s.rows).for_each(|r| r.collect(&mut all));
		all.into_iter()
	}
}

/// What the forms read beside the edited object.
#[derive(Clone, Copy)]
pub struct Cx<'a> {
	pub s: Strings,
	pub app: &'a App,
	pub r: &'a Readouts,
}

/// The form being laid out.
pub struct Builder<'a> {
	pub cx: Cx<'a>,
	form: Form<'a>,
	rows: Vec<Row<'a>>,
	/// The next row is enabled.
	enabled: bool,
}

impl<'a> Builder<'a> {
	fn new(cx: Cx<'a>) -> Self {
		Self { cx, form: Form::default(), rows: Vec::new(), enabled: true }
	}

	fn finish(self) -> Form<'a> {
		self.form
	}

	/// The next row is greyed and its editor disabled unless `enabled`.
	pub fn on(&mut self, enabled: bool) -> &mut Self {
		self.enabled = enabled;
		self
	}

	/// A section: `title`, then the rows `rows` adds.
	pub fn section(&mut self, title: &'static str, rows: impl FnOnce(&mut Self)) {
		let outer = std::mem::take(&mut self.rows);
		rows(self);
		let rows = std::mem::replace(&mut self.rows, outer);
		self.form.sections.push(Section { title, rows });
	}

	pub fn push(&mut self, label: Option<&'static str>, hint: &'static str, kind: RowKind<'a>) {
		let enabled = std::mem::replace(&mut self.enabled, true);
		self.rows.push(Row { label, hint, enabled, kind });
	}

	/// The rows `rows` adds as one row labelled `label`; `parts` relabels them in order (none keeps their labels).
	/// A group that is not enabled greys them all.
	pub fn group(&mut self, label: &'static str, parts: &[&'static str], rows: impl FnOnce(&mut Self)) {
		let enabled = std::mem::replace(&mut self.enabled, true);
		let outer = std::mem::take(&mut self.rows);
		rows(self);
		let mut rows = std::mem::replace(&mut self.rows, outer);
		for (i, row) in rows.iter_mut().enumerate() {
			row.enabled &= enabled;
			if let Some(&part) = parts.get(i) {
				row.label = Some(part);
				if let RowKind::Number(n) = &mut row.kind {
					n.label = part;
				}
			}
		}
		self.push(Some(label), "", RowKind::Group(rows));
	}

	/// A plain value row.
	pub fn readout(&mut self, label: &'static str, text: impl Into<Cow<'a, str>>, hint: &'static str) {
		self.push(Some(label), hint, RowKind::Readout(text.into()));
	}

	/// A secondary value row.
	pub fn value(&mut self, label: &'static str, text: impl Into<Cow<'a, str>>, hint: &'static str) {
		self.push(Some(label), hint, RowKind::Value(text.into()));
	}

	pub fn paragraph(&mut self, text: &'static str) {
		self.push(None, "", RowKind::Paragraph(text));
	}

	/// The parts' description header.
	pub fn description(&mut self, title: &'static str, tip: &'static str, text: &'static str, open: bool) {
		self.form.description = Some(Description { title, tip, text, open });
	}

	/// The item's name box: applied on every keystroke, so the tree follows the typing; renames through the
	/// command that keeps references. `T` is the type whose `Name` label it takes.
	pub fn name<T: Fields>(&mut self, target: ItemRef, value: &'a str) {
		let (s, ty) = (self.cx.s, T::TYPE_NAME);
		let label = s.label(ty, "Name");
		let text = Text {
			key: name_key(target),
			label,
			value: value.into(),
			multiline: false,
			live: true,
			first: true,
			apply: Box::new(move |name| Some(Event::Command(Command::Rename { target, name }))),
		};
		self.push(Some(label), s.hint(ty, "Name"), RowKind::Text(text));
	}
}

/// The key of `item`'s name box.
pub fn name_key(item: ItemRef) -> Key {
	key(("Name", item))
}

/// The key of `item`'s Enabled (or Active) box; none for a scheme.
pub fn enabled_key(item: ItemRef) -> Option<Key> {
	let edit = match item {
		ItemRef::Source(i) => Edit::Source(i, (Source::ENABLED.edit)(false)),
		ItemRef::Part(i) => Edit::Part(i, (Part::BASE.wrap)((PartBase::ENABLED.edit)(false))),
		ItemRef::Probe(i) => Edit::Probe(i, (Probe::ACTIVE.edit)(false)),
		_ => return None,
	};
	Some(key(edit.field_ref()))
}

/// A button that asks for `out` when pressed.
pub fn button<'a>(
	key: Key,
	text: &'static str,
	tip: &'static str,
	enabled: bool,
	press: impl Fn() -> Vec<Out> + 'a,
) -> Button<'a> {
	Button { key, text, tip, glyph: None, enabled, press: Box::new(press) }
}

/// A glyph button named by `tip`.
pub fn glyph<'a>(
	key: Key,
	glyph: Glyph,
	tip: &'static str,
	enabled: bool,
	press: impl Fn() -> Vec<Out> + 'a,
) -> Button<'a> {
	Button { key, text: "", tip, glyph: Some(glyph), enabled, press: Box::new(press) }
}

/// The rows of one object: labels and hints are `<TYPE_NAME>.<Member>`'s, the bounds the fields' own; `wrap` makes
/// an edit of the object an edit of the project.
pub struct Scope<'a, T: Fields, W> {
	pub obj: &'a T,
	wrap: W,
	/// Number boxes show no value (a setting that does not apply).
	blank: bool,
}

/// A list field of an object: its items and the means to replace them as a whole.
pub struct List<'a, T: Fields, U, W> {
	pub items: &'a [U],
	prop: Prop<T, Vec<U>>,
	wrap: W,
	s: Strings,
}

impl<T: Fields, U, W: Clone> Clone for List<'_, T, U, W> {
	fn clone(&self) -> Self {
		Self { items: self.items, prop: self.prop, wrap: self.wrap.clone(), s: self.s }
	}
}

impl<'a, T: Fields, U: Clone + 'a, W: Fn(T::Edit) -> Edit + Clone + 'a> List<'a, T, U, W> {
	/// The event that sets the list to the items as `change` leaves them.
	pub fn set(&self, change: impl FnOnce(&mut Vec<U>)) -> Event {
		let mut items = self.items.to_vec();
		change(&mut items);
		Event::Command(Command::Edit((self.wrap)((self.prop.edit)(items))))
	}

	/// The event that sets `p` of item `index` to `value`.
	pub fn set_item<V>(&self, index: usize, p: Prop<U, V>, value: V) -> Event
	where
		U: Fields,
	{
		self.set(|items| {
			(p.edit)(value).apply(&mut items[index]).expect("an edit of a struct's field applies");
		})
	}

	/// The key of `member` of item `index`.
	pub fn key(&self, index: usize, member: &str) -> Key {
		key(((self.wrap)((self.prop.edit)(Vec::new())).field_ref(), index, member))
	}

	/// The box for `p` of item `index`, with `p`'s label and hint.
	pub fn number<V: waveduderust::model::Number + 'a>(
		&self,
		index: usize,
		p: Prop<U, V>,
		spec: NumberSpec,
	) -> Number<'a>
	where
		U: Fields,
	{
		let (s, ty, name) = (self.s, U::TYPE_NAME, p.field.name());
		let list = self.clone();
		Number {
			key: self.key(index, name),
			label: s.label(ty, name),
			hint: s.hint(ty, name),
			value: (p.get)(&self.items[index]).to_f64(),
			spec,
			unit: unit_text(&s, ty, name),
			blank: false,
			first: false,
			apply: Box::new(move |v| list.set_item(index, p, V::from_f64(v))),
		}
	}

	/// A glyph button that sets the list to the items as `change` leaves them.
	pub fn button(
		&self,
		index: usize,
		member: &str,
		glyph_of: Glyph,
		tip: &'static str,
		change: impl Fn(&mut Vec<U>) + 'a,
	) -> Button<'a> {
		let list = self.clone();
		glyph(self.key(index, member), glyph_of, tip, true, move || vec![Out::Send(list.set(&change))])
	}

	/// The button that removes item `index`.
	pub fn remove(&self, index: usize, tip: &'static str) -> Button<'a> {
		self.button(index, "Remove", Glyph::Minus, tip, move |items| {
			items.remove(index);
		})
	}

	/// The button that moves item `index` by `by` places.
	pub fn shift(&self, index: usize, by: isize, tip: &'static str) -> Button<'a> {
		let (member, g) = if by < 0 { ("Up", Glyph::Up) } else { ("Down", Glyph::Down) };
		self.button(index, member, g, tip, move |items| {
			let to = index as isize + by;
			if (0..items.len() as isize).contains(&to) {
				items.swap(index, to as usize);
			}
		})
	}

	/// The event that moves item `from` to index `to`.
	pub fn reorder(&self, from: usize, to: usize) -> Event {
		self.set(|items| {
			let item = items.remove(from);
			items.insert(to, item);
		})
	}
}

impl<'a, T: Fields, W: Fn(T::Edit) -> Edit + Clone + 'a> Scope<'a, T, W> {
	pub fn new(obj: &'a T, wrap: W) -> Self {
		Self { obj, wrap, blank: false }
	}

	pub fn nested<U: Fields>(
		&self,
		n: Nested<T, U>,
	) -> Scope<'a, U, impl Fn(U::Edit) -> Edit + Clone + 'a + use<'a, T, U, W>> {
		let wrap = self.wrap.clone();
		Scope { obj: (n.get)(self.obj), wrap: move |e| wrap((n.wrap)(e)), blank: self.blank }
	}

	/// The scope of the variant `obj` the object is, for a caller that has matched on it.
	pub fn variant_over<U: Fields>(
		&self,
		v: Variant<T, U>,
		obj: &'a U,
	) -> Scope<'a, U, impl Fn(U::Edit) -> Edit + Clone + 'a + use<'a, T, U, W>> {
		let wrap = self.wrap.clone();
		Scope { obj, wrap: move |e| wrap((v.wrap)(e)), blank: self.blank }
	}

	/// The scope of an enum variant or an optional composite; where the object is another variant or absent,
	/// `default` with empty number boxes.
	pub fn variant_or<U: Fields>(
		&self,
		v: Variant<T, U>,
		default: &'a U,
	) -> Scope<'a, U, impl Fn(U::Edit) -> Edit + Clone + 'a + use<'a, T, U, W>> {
		let (obj, blank) = match (v.get)(self.obj) {
			Some(obj) => (obj, self.blank),
			None => (default, true),
		};
		let wrap = self.wrap.clone();
		Scope { obj, wrap: move |e| wrap((v.wrap)(e)), blank }
	}

	/// The same edits over another value: a design's spacing.
	pub fn over(&self, obj: &'a T) -> Self {
		Self { obj, wrap: self.wrap.clone(), blank: self.blank }
	}

	pub fn get<V>(&self, p: Prop<T, V>) -> V {
		(p.get)(self.obj)
	}

	/// The event that sets `p` to `value`.
	pub fn event<V>(&self, p: Prop<T, V>, value: V) -> Event {
		Event::Command(Command::Edit((self.wrap)((p.edit)(value))))
	}

	/// The events that set `p` to a value.
	pub fn setter<V: 'a>(&self, p: Prop<T, V>) -> Setter<'a, V> {
		let wrap = self.wrap.clone();
		Rc::new(move |value| Event::Command(Command::Edit(wrap((p.edit)(value)))))
	}

	/// The editor key of `p`, `sample` any value of it.
	pub fn key<V>(&self, p: Prop<T, V>, sample: V) -> Key {
		key((self.wrap)((p.edit)(sample)).field_ref())
	}

	/// The list `p` whose items are `items`.
	pub fn list<U: Clone>(&self, s: Strings, p: Prop<T, Vec<U>>, items: &'a [U]) -> List<'a, T, U, W> {
		List { items, prop: p, wrap: self.wrap.clone(), s }
	}

	pub fn label<V>(&self, s: &Strings, p: &Prop<T, V>) -> &'static str {
		s.label(T::TYPE_NAME, p.field.name())
	}

	pub fn hint<V>(&self, s: &Strings, p: &Prop<T, V>) -> &'static str {
		s.hint(T::TYPE_NAME, p.field.name())
	}

	/// A number bounded by the field's own range.
	pub fn number<V: waveduderust::model::Number + 'a>(&self, f: &mut Builder<'a>, p: Prop<T, V>, step: f64) {
		let (min, max) = bounds_of(p);
		self.number_spec(f, p, NumberSpec::linear(min, max, step));
	}

	/// A number bounded by `range`, which the field does not declare (a position shared with the parts).
	pub fn number_in<V: waveduderust::model::Number + 'a>(
		&self,
		f: &mut Builder<'a>,
		p: Prop<T, V>,
		range: Range,
		step: f64,
	) {
		self.number_spec(f, p, NumberSpec::linear(range.min, range.max, step));
	}

	fn number_spec<V: waveduderust::model::Number + 'a>(&self, f: &mut Builder<'a>, p: Prop<T, V>, spec: NumberSpec) {
		let spec = if V::INTEGER { spec.with_decimals(0) } else { spec };
		self.number_with(f, p, spec);
	}

	/// A number with `places` decimals, bounded by the field's range, dragged by `drag` a pixel.
	pub fn decimals<V: waveduderust::model::Number + 'a>(
		&self,
		f: &mut Builder<'a>,
		p: Prop<T, V>,
		drag: f64,
		places: u32,
	) {
		let (min, max) = bounds_of(p);
		self.number_with(f, p, NumberSpec::linear(min, max, 1.).with_decimals(places).with_drag_step(drag));
	}

	pub fn number_with<V: waveduderust::model::Number + 'a>(
		&self,
		f: &mut Builder<'a>,
		p: Prop<T, V>,
		spec: NumberSpec,
	) {
		let s = f.cx.s;
		let n = self.number_box(&s, p, spec);
		f.push(Some(n.label), n.hint, RowKind::Number(n));
	}

	/// The box of `p`.
	pub fn number_box<V: waveduderust::model::Number + 'a>(
		&self,
		s: &Strings,
		p: Prop<T, V>,
		spec: NumberSpec,
	) -> Number<'a> {
		let value = self.get(p);
		let set = self.setter(p);
		Number {
			key: self.key(p, value),
			label: self.label(s, &p),
			hint: self.hint(s, &p),
			value: value.to_f64(),
			spec,
			unit: unit_text(s, T::TYPE_NAME, p.field.name()),
			blank: self.blank,
			first: true,
			apply: Box::new(move |v| set(V::from_f64(v))),
		}
	}

	/// A full-width check box carrying its label.
	pub fn check(&self, f: &mut Builder<'a>, p: Prop<T, bool>) {
		self.check_as(f, p, |on| *on, |on| on);
	}

	/// As [`Self::check`] over a field that is not a flag: `on` says whether a value is checked, `of` the value a
	/// state makes.
	pub fn check_as<V: 'a>(&self, f: &mut Builder<'a>, p: Prop<T, V>, on: fn(&V) -> bool, of: fn(bool) -> V) {
		let s = f.cx.s;
		let value = self.get(p);
		let checked = on(&value);
		let set = self.setter(p);
		let check = Check {
			key: self.key(p, value),
			text: self.label(&s, &p),
			on: checked,
			apply: Box::new(move |v| vec![set(of(v))]),
		};
		f.push(None, self.hint(&s, &p), RowKind::Check(check));
	}

	/// A check box in the editor column, the field's label in the label column and `text` beside the box.
	pub fn check_text(&self, f: &mut Builder<'a>, p: Prop<T, bool>, text: &'static str) {
		let s = f.cx.s;
		let on = self.get(p);
		let set = self.setter(p);
		let check = Check { key: self.key(p, on), text, on, apply: Box::new(move |v| vec![set(v)]) };
		f.push(Some(self.label(&s, &p)), self.hint(&s, &p), RowKind::Check(check));
	}

	/// An enum combo over every member.
	pub fn combo<V: Word>(&self, f: &mut Builder<'a>, p: Prop<T, V>) {
		self.combo_of(f, p, V::MEMBERS);
	}

	/// An enum combo over `members`.
	pub fn combo_of<V: Word>(&self, f: &mut Builder<'a>, p: Prop<T, V>, members: &'static [V]) {
		let s = f.cx.s;
		let current = self.get(p);
		let set = self.setter(p);
		let label = self.label(&s, &p);
		let choice = Choice {
			key: self.key(p, current),
			label,
			words: members.iter().map(|m| s.enum_word(V::TYPE_NAME, m.name()).into()).collect(),
			current: members.iter().position(|m| *m == current),
			shown: s.enum_word(V::TYPE_NAME, current.name()).into(),
			swatches: Vec::new(),
			actions: Vec::new(),
			pick: Box::new(move |i| {
				members.get(i).filter(|m| **m != current).map(|m| Out::Send(set(*m))).into_iter().collect()
			}),
		};
		f.push(Some(label), self.hint(&s, &p), RowKind::Choice(choice));
	}

	/// A text box of `p`, whose text is `value`, applied on Enter or focus loss, Esc reverting (`multiline` keeps
	/// Enter as newline).
	pub fn text(&self, f: &mut Builder<'a>, p: Prop<T, String>, value: &'a str, multiline: bool) {
		let s = f.cx.s;
		let label = self.label(&s, &p);
		let set = self.setter(p);
		let text = Text {
			key: self.key(p, String::new()),
			label,
			value: value.into(),
			multiline,
			live: false,
			first: true,
			apply: Box::new(move |t| Some(set(t))),
		};
		f.push(Some(label), self.hint(&s, &p), RowKind::Text(text));
	}
}

#[cfg(test)]
mod tests {
	use crate::AppState;
	use crate::commands::NewPart;
	use crate::selection::Row as TreeRow;
	use crate::settings::AppDirs;
	use waveduderust::model::{ItemKind, ItemRef};

	use super::*;
	use crate::properties::readouts;

	fn app() -> (tempfile::TempDir, App) {
		let dir = tempfile::tempdir().unwrap();
		let app = AppState::start(&AppDirs::under(dir.path()), []).expect("state starts");
		(dir, app)
	}

	#[test]
	fn every_part_kind_has_a_form_whose_keys_are_unique() {
		let (_dir, mut app) = app();
		let now = std::time::Instant::now();
		for (kind, _) in crate::menus::PART_KINDS {
			app.apply(Command::AddPart { kind, at: None }, now).expect("the part is added");
		}
		app.apply(Command::AddSource { at: None }, now).expect("the source is added");
		app.apply(Command::AddScheme, now).expect("the scheme is added");
		let p = &app.document.project;
		let mut rows = vec![TreeRow::Project, TreeRow::Scene, TreeRow::Simulation, TreeRow::Display];
		rows.extend((0..p.parts.len()).map(|i| TreeRow::Item(ItemRef::at(ItemKind::Part, i, p).unwrap())));
		rows.push(TreeRow::Item(ItemRef::at(ItemKind::Source, 0, p).unwrap()));
		rows.push(TreeRow::Item(ItemRef::at(ItemKind::Scheme, 0, p).unwrap()));
		for row in rows {
			let r = readouts(&app, row);
			let form = build(Cx { s: app.strings, app: &app, r: &r }, row).expect("a form");
			assert!(!form.sections.is_empty(), "{row:?} has sections");
			let mut keys: Vec<Key> = form
				.rows()
				.filter_map(|r| match &r.kind {
					RowKind::Number(n) => Some(n.key),
					RowKind::Text(t) => Some(t.key),
					RowKind::Check(c) => Some(c.key),
					RowKind::Choice(c) => Some(c.key),
					_ => None,
				})
				.collect();
			let len = keys.len();
			keys.sort_unstable();
			keys.dedup();
			assert_eq!(keys.len(), len, "{row:?} repeats a key");
		}
	}

	#[test]
	fn a_number_edit_is_the_fields_edit_and_a_disabled_row_says_so() {
		let (_dir, mut app) = app();
		let now = std::time::Instant::now();
		app.apply(Command::AddSource { at: None }, now).expect("the source is added");
		let p = &app.document.project;
		let row = TreeRow::Item(ItemRef::at(ItemKind::Source, 0, p).unwrap());
		let r = readouts(&app, row);
		let form = build(Cx { s: app.strings, app: &app, r: &r }, row).expect("a form");
		let x = app.strings.label("Source", "X");
		let RowKind::Number(n) = &form.rows().find(|r| r.label == Some(x)).expect("an X row").kind else {
			panic!("X is a number")
		};
		let Event::Command(Command::Edit(edit)) = (n.apply)(42.) else { panic!("an edit") };
		assert_eq!(edit, Edit::Source(p.sources[0].id(), waveduderust::model::SourceEdit::X(42.)));
		let width = app.strings.label("Source", "PulseWidth");
		let pulse = form.rows().find(|r| r.label == Some(width)).expect("a pulse width row");
		assert!(!pulse.enabled, "a continuous source greys the pulse rows");
	}

	#[test]
	fn a_list_button_edits_the_list() {
		let (_dir, mut app) = app();
		let now = std::time::Instant::now();
		app.apply(Command::AddPart { kind: NewPart::Outline, at: None }, now).expect("the outline is added");
		let row = TreeRow::Item(ItemRef::at(ItemKind::Part, 0, &app.document.project).unwrap());
		let r = readouts(&app, row);
		let form = build(Cx { s: app.strings, app: &app, r: &r }, row).expect("a form");
		let points = form
			.rows()
			.find_map(|r| match &r.kind {
				RowKind::Points(p) => Some(p),
				_ => None,
			})
			.expect("the outline's points");
		let before = points.items.len();
		let [Out::Send(Event::Command(Command::Edit(edit)))] = &(points.add.press)()[..] else { panic!("one edit") };
		let (mut project, mut view) = (app.document.project.clone(), app.document.view.clone());
		let mut selection = app.selection.clone();
		crate::commands::apply(&mut project, &mut view, &mut selection, Command::Edit(edit.clone()))
			.expect("the edit applies");
		let waveduderust::model::PartKind::Outline(o) = &project.parts[0].kind else { panic!("an outline") };
		assert_eq!(o.points.len(), before + 1);
	}
}
