//! The Properties pane (docs/ui/30): the form of the current row, built from a snapshot of the edited object,
//! every edit sent as a typed `Command::Edit` through the model's field accessors (`Probe::BOX_WIDTH`).

use std::cell::Cell;
use std::rc::Rc;
use wavedude_widgets::Button;

use freya::prelude::*;
use wavedude_core::model::ranges::Range;
use wavedude_core::model::{
	Bounds, ColorScheme, Display, Edit as _, Field as _, Fields, Nested, Number, Part, Probe, Prop, Scene, Simulation,
	Source, Variant, Word,
};
use wavedude_widgets::icon::icons;
use wavedude_widgets::{Check, FocusRequest, Gradient, TestIdExt, TextBox};
use wavedude_widgets::{
	IconView, MenuEntry, MenuItem, MenuModel, NumberBox, NumberSpec, PropertyGrid, PropertyRow, use_wave_theme,
};

use wavedude_app_state::Event as AppEvent;
use wavedude_app_state::commands::{Command, Edit};
use wavedude_app_state::selection::{ItemKind, Row};
use wavedude_app_state::strings::{Language, Strings};

use super::app::Cx;
use super::properties_forms as forms;
use super::shared::{App, Topics};

/// The edited object, cloned from the project at selection.
#[derive(Clone, Debug, PartialEq)]
pub enum Object {
	Project {
		seed: i32,
		description: String,
	},
	/// Without the image bytes.
	Scene(Scene),
	Simulation(Simulation),
	Display(Display),
	Source(Source),
	Part(Part),
	Probe(Probe),
	Scheme(ColorScheme),
}

/// What a form shows: the row, its object and the read-outs computed from the project.
#[derive(Clone, PartialEq)]
pub struct Snapshot {
	pub row: Option<Row>,
	pub object: Option<Rc<Object>>,
	pub strings: Strings,
	pub readouts: forms::Readouts,
	pub description_open: bool,
	pub count: usize,
}

fn object_of(app: &App, row: Row) -> Option<Object> {
	let p = &app.document.project;
	Some(match row {
		Row::Project => Object::Project { seed: p.seed, description: p.description.clone() },
		Row::Scene => Object::Scene(p.scene.without_image()),
		Row::Simulation => Object::Simulation(p.simulation.clone()),
		Row::Display => Object::Display(p.display.clone()),
		Row::Folder(_) => return None,
		Row::Item(i) => match i.kind {
			ItemKind::Source => Object::Source(p.sources[i.index].clone()),
			ItemKind::Part => Object::Part(p.parts[i.index].clone()),
			ItemKind::Probe => Object::Probe(p.probes[i.index].clone()),
			ItemKind::Scheme => Object::Scheme(p.color_schemes[i.index].clone()),
		},
	})
}

fn snapshot(app: &App) -> Snapshot {
	let project = &app.document.project;
	let row = app.selection.current().filter(|r| match r {
		Row::Item(i) => i.exists(project),
		_ => true,
	});
	Snapshot {
		row,
		object: row.and_then(|r| object_of(app, r)).map(Rc::new),
		strings: app.strings,
		readouts: row.map(|r| forms::readouts(app, r)).unwrap_or_default(),
		description_open: app.settings.part_description_expanded,
		count: match row {
			Some(Row::Folder(k)) => k.len(project),
			_ => 0,
		},
	}
}

/// The means to edit the current row's object, shared by the form builders.
#[derive(Clone)]
pub struct Form {
	pub cx: Cx,
	pub row: Row,
	pub s: Strings,
	/// The id the form's first editor takes, so Enter on a tree row or Add can focus it; taken once.
	first_editor: Rc<Cell<Option<(AccessibilityId, FocusRequest)>>>,
}

/// A number's bounds from the field's declared range; open when it has none or depends on another value.
pub fn bounds_of<T: Fields, V>(p: Prop<T, V>) -> (f64, f64) {
	match p.field.bounds() {
		Bounds::Range(r) => (r.min, r.max),
		Bounds::Dependent | Bounds::Open => (-1e9, 1e9),
	}
}

impl Form {
	pub fn new(cx: Cx, row: Row, s: Strings, first_editor: (AccessibilityId, FocusRequest)) -> Self {
		Self { cx, row, s, first_editor: Rc::new(Cell::new(Some(first_editor))) }
	}

	/// The first editor's id and focus request, for the editor that claims them first.
	fn claim_first(&self) -> Option<(AccessibilityId, FocusRequest)> {
		self.first_editor.take()
	}

	/// `editor` as the form's first editor when none has claimed that place yet.
	fn first(&self, editor: TextBox) -> TextBox {
		match self.claim_first() {
			Some((id, request)) => editor.id(Some(id)).focus_request(Some(request)),
			None => editor,
		}
	}

	/// The fields of `obj`, each edit sent as `wrap` makes of it.
	pub fn scope<'a, T: Fields>(&'a self, obj: &'a T, wrap: impl Fn(T::Edit) -> Edit + 'static) -> Scope<'a, T> {
		let cx = self.cx.clone();
		Scope {
			form: self,
			obj,
			send: Rc::new(move |e| cx.send(AppEvent::Command(Command::Edit(wrap(e))))),
			blank: false,
		}
	}

	fn decimal_comma(&self) -> bool {
		self.s.language() != Language::English
	}

	/// The item's name box: applied on every keystroke, so the tree follows the typing; renames through the
	/// command that keeps references. `T` is the type whose `Name` label it takes.
	pub fn name<T: Fields>(&self, value: &str) -> PropertyRow {
		let Row::Item(target) = self.row else { unreachable!("only an item has a name") };
		let cx = self.cx.clone();
		let on_text = move |name: String| cx.send(AppEvent::Command(Command::Rename { target, name }));
		let editor = self.first(TextBox::new(value.to_owned(), on_text).live().select_all_on_focus(true));
		let ty = T::TYPE_NAME;
		PropertyRow::new(self.s.label(ty, "Name"), tagged(editor, format!("{ty}.Name"))).hint(self.s.hint(ty, "Name"))
	}
}

/// The rows of one object: labels, hints and test ids are `<TYPE_NAME>.<Member>`, the bounds the fields' own.
pub struct Scope<'a, T: Fields> {
	form: &'a Form,
	/// The object the rows show.
	pub obj: &'a T,
	send: Rc<dyn Fn(T::Edit)>,
	/// Number boxes show no value (a setting that does not apply).
	blank: bool,
}

/// A list field of an object: its items as of now and the means to replace them as a whole.
pub struct List<T: Fields, U: 'static> {
	items: Rc<Vec<U>>,
	prop: Prop<T, Vec<U>>,
	send: Rc<dyn Fn(T::Edit)>,
}

impl<T: Fields, U: 'static> Clone for List<T, U> {
	fn clone(&self) -> Self {
		Self { items: self.items.clone(), prop: self.prop, send: self.send.clone() }
	}
}

impl<T: Fields, U: PartialEq + 'static> PartialEq for List<T, U> {
	fn eq(&self, other: &Self) -> bool {
		self.prop.field == other.prop.field && self.items == other.items
	}
}

impl<T: Fields, U: Clone + 'static> List<T, U> {
	pub fn items(&self) -> &[U] {
		&self.items
	}

	/// Sets the list to the items as `change` leaves them.
	pub fn set(&self, change: impl FnOnce(&mut Vec<U>)) {
		let mut items = (*self.items).clone();
		change(&mut items);
		(self.send)((self.prop.edit)(items));
	}

	/// Sets `p` of item `index`.
	pub fn set_item<V>(&self, index: usize, p: Prop<U, V>, value: V)
	where
		U: Fields,
	{
		self.set(|items| {
			(p.edit)(value).apply(&mut items[index]).expect("an edit of a struct's field applies");
		});
	}
}

impl<'a, T: Fields> Scope<'a, T> {
	/// The scope of a nested field.
	pub fn nested<U: Fields>(&self, n: Nested<T, U>) -> Scope<'a, U> {
		let (send, wrap) = (self.send.clone(), n.wrap);
		Scope { form: self.form, obj: (n.get)(self.obj), send: Rc::new(move |e| send(wrap(e))), blank: self.blank }
	}

	/// The scope of an enum variant or an optional composite; `None` when the object is another variant or absent.
	pub fn variant<U: Fields>(&self, v: Variant<T, U>) -> Option<Scope<'a, U>> {
		let (send, wrap) = (self.send.clone(), v.wrap);
		(v.get)(self.obj).map(|obj| Scope {
			form: self.form,
			obj,
			send: Rc::new(move |e| send(wrap(e))),
			blank: self.blank,
		})
	}

	/// As [`Self::variant`], showing `default` with empty number boxes where the variant is absent.
	pub fn variant_or<'b, U: Fields>(&self, v: Variant<T, U>, default: &'b U) -> Scope<'b, U>
	where
		'a: 'b,
	{
		match self.variant(v) {
			Some(scope) => scope,
			None => self.variant_over(v, default).blanked(),
		}
	}

	/// The scope of the variant `obj` the object is, for a caller that has matched on it.
	pub fn variant_over<'b, U: Fields>(&self, v: Variant<T, U>, obj: &'b U) -> Scope<'b, U>
	where
		'a: 'b,
	{
		let (send, wrap) = (self.send.clone(), v.wrap);
		Scope { form: self.form, obj, send: Rc::new(move |e| send(wrap(e))), blank: self.blank }
	}

	/// The same edits over another value: a default shown greyed, a design's spacing.
	pub fn over<'b>(&self, obj: &'b T) -> Scope<'b, T>
	where
		'a: 'b,
	{
		Scope { form: self.form, obj, send: self.send.clone(), blank: self.blank }
	}

	/// The same rows with their number boxes empty.
	pub fn blanked(mut self) -> Self {
		self.blank = true;
		self
	}

	pub fn get<V>(&self, p: Prop<T, V>) -> V {
		(p.get)(self.obj)
	}

	pub fn set<V>(&self, p: Prop<T, V>, value: V) {
		(self.send)((p.edit)(value));
	}

	/// `p`'s setter, for a row's own handlers.
	pub fn setter<V: 'static>(&self, p: Prop<T, V>) -> impl Fn(V) + 'static {
		let send = self.send.clone();
		move |value| send((p.edit)(value))
	}

	/// `p`'s items and the means to replace them.
	pub fn list<U: Clone + 'static>(&self, p: Prop<T, Vec<U>>) -> List<T, U> {
		List { items: Rc::new(self.get(p)), prop: p, send: self.send.clone() }
	}

	/// The test id of the field's row.
	pub fn test_id(p: &Prop<T, impl Sized>) -> String {
		format!("{}.{}", T::TYPE_NAME, p.field.name())
	}

	pub fn label<V>(&self, p: &Prop<T, V>) -> &'static str {
		self.form.s.label(T::TYPE_NAME, p.field.name())
	}

	pub fn hint<V>(&self, p: &Prop<T, V>) -> &'static str {
		self.form.s.hint(T::TYPE_NAME, p.field.name())
	}

	/// A number bounded by the field's own range.
	pub fn number<V: Number + 'static>(&self, p: Prop<T, V>, step: f64) -> PropertyRow {
		let (min, max) = bounds_of(p);
		self.number_spec(p, NumberSpec::linear(min, max, step))
	}

	/// A number bounded by `range`, which the field does not declare (a position shared with the parts).
	pub fn number_in<V: Number + 'static>(&self, p: Prop<T, V>, range: Range, step: f64) -> PropertyRow {
		self.number_spec(p, NumberSpec::linear(range.min, range.max, step))
	}

	fn number_spec<V: Number + 'static>(&self, p: Prop<T, V>, mut spec: NumberSpec) -> PropertyRow {
		if V::INTEGER {
			spec = spec.with_decimals(0);
		}
		self.number_with(p, spec)
	}

	pub fn number_with<V: Number + 'static>(&self, p: Prop<T, V>, spec: NumberSpec) -> PropertyRow {
		PropertyRow::switchable(self.label(&p), self.number_editor(p, spec)).hint(self.hint(&p))
	}

	/// A number with `places` decimals, bounded by the field's range, dragged by `drag`.
	pub fn decimals<V: Number + 'static>(&self, p: Prop<T, V>, drag: f64, places: u32) -> PropertyRow {
		let (min, max) = bounds_of(p);
		self.number_with(p, NumberSpec::linear(min, max, 1.).with_decimals(places).with_drag_step(drag))
	}

	/// [`Self::number_with`] with `beside` right of the box.
	pub fn number_beside<V: Number + 'static>(&self, p: Prop<T, V>, spec: NumberSpec, beside: Element) -> PropertyRow {
		let editor = self.number_editor(p, spec);
		let row = move |enabled| {
			rect()
				.horizontal()
				.width(Size::fill())
				.content(Content::flex())
				.spacing(6.)
				.cross_align(Alignment::center())
				.child(rect().width(Size::flex(1.)).child(editor(enabled)))
				.child(beside.clone())
				.into_element()
		};
		PropertyRow::switchable(self.label(&p), row).hint(self.hint(&p))
	}

	fn number_editor<V: Number + 'static>(
		&self,
		p: Prop<T, V>,
		mut spec: NumberSpec,
	) -> impl Fn(bool) -> Element + 'static {
		if self.form.decimal_comma() {
			spec = spec.with_decimal_separator(',');
		}
		let value = self.get(p).to_f64();
		let set = self.setter(p);
		let set = Rc::new(move |v: f64| set(V::from_f64(v)));
		let test = Self::test_id(&p);
		let first = self.form.claim_first();
		let blank = self.blank;
		move |enabled| {
			let set = set.clone();
			let editor = NumberBox::new(WritableSetter::new(value, move |v: f64| set(v)), spec.clone())
				.width(Size::fill())
				.enabled(enabled)
				.blank(blank)
				.test_id(test.clone());
			match first.clone() {
				Some((id, request)) => editor.a11y_id(id).focus_request(Some(request)).into_element(),
				None => editor.into_element(),
			}
		}
	}

	/// A full-width check box carrying its label.
	pub fn check(&self, p: Prop<T, bool>) -> PropertyRow {
		self.check_as(p, |on| *on, |on| on)
	}

	/// As [`Self::check`] over a field that is not a flag: `on` says whether a value is checked, `of` the
	/// value a state makes.
	pub fn check_as<V: 'static>(&self, p: Prop<T, V>, on: fn(&V) -> bool, of: fn(bool) -> V) -> PropertyRow {
		let checked = on(&self.get(p));
		let set = self.setter(p);
		self.check_row("", self.label(&p).to_owned(), checked, move |v| set(of(v)), &p).full_width()
	}

	/// A check box in the editor column, the field's label in the label column and `text` beside the box.
	pub fn check_text(&self, p: Prop<T, bool>, text: String) -> PropertyRow {
		let set = self.setter(p);
		self.check_row(self.label(&p), text, self.get(p), set, &p)
	}

	fn check_row<V>(
		&self,
		label: &str,
		text: String,
		on: bool,
		set: impl Fn(bool) + 'static,
		p: &Prop<T, V>,
	) -> PropertyRow {
		let set = Rc::new(set);
		let test = Self::test_id(p);
		let editor = move |enabled| {
			let set = set.clone();
			check_box_in(text.clone(), on, enabled, move |v| set(v)).test_id(test.clone()).into_element()
		};
		PropertyRow::switchable(label, editor).hint(self.hint(p))
	}

	/// An enum combo over every member.
	pub fn combo<V: Word>(&self, p: Prop<T, V>) -> PropertyRow {
		self.combo_of(p, V::MEMBERS)
	}

	/// An enum combo over `members`.
	pub fn combo_of<V: Word>(&self, p: Prop<T, V>, members: &[V]) -> PropertyRow {
		let s = self.form.s;
		let current = self.get(p).name().to_owned();
		let items: Vec<(String, String)> =
			members.iter().map(|m| (m.name().to_owned(), s.enum_word(V::TYPE_NAME, m.name()).to_owned())).collect();
		let set = Rc::new(self.setter(p));
		let test = Self::test_id(&p);
		let editor = move |enabled| {
			let set = set.clone();
			combo_box(current.clone(), items.clone(), enabled, test.clone(), move |name: String| {
				if let Some(v) = V::parse(&name) {
					set(v);
				}
			})
		};
		PropertyRow::switchable(self.label(&p), editor).hint(self.hint(&p))
	}

	/// A text box applied on Enter or focus loss, Esc reverting (`multiline` keeps Enter as newline).
	pub fn text(&self, p: Prop<T, String>, multiline: bool) -> PropertyRow {
		let set = self.setter(p);
		let editor = self.form.first(TextBox::new(self.get(p), move |t: String| set(t)).multiline(multiline));
		PropertyRow::new(self.label(&p), tagged(editor, Self::test_id(&p))).hint(self.hint(&p))
	}
}

/// A plain value row.
pub fn readout(label: &str, text: impl Into<String>, test: &str) -> PropertyRow {
	PropertyRow::new(label, label_view(text.into()).test_id(test.to_owned()))
}

fn label_view(text: String) -> Rect {
	rect().width(Size::fill()).child(paragraph().width(Size::fill()).span(Span::new(text)))
}

/// A check box with its text; the whole row toggles.
pub fn check_box(text: String, on: bool, on_change: impl Fn(bool) + 'static) -> Rect {
	check_box_in(text, on, true, on_change)
}

/// As [`check_box`]; disabled, it is greyed and takes neither the pointer nor the focus.
pub fn check_box_in(text: String, on: bool, enabled: bool, on_change: impl Fn(bool) + 'static) -> Rect {
	let theme = use_wave_theme();
	let ink = if enabled { theme.text } else { theme.text_disabled };
	let mark = rect()
		.width(Size::px(14.))
		.height(Size::px(14.))
		// the disabled fill is the pane's own colour, which the input border equals in the dark theme
		.border(
			Border::new()
				.fill(if enabled { theme.input_border } else { theme.hairline })
				.width(1.)
				.alignment(BorderAlignment::Inner),
		)
		.background(if enabled { theme.input_background } else { theme.control_background })
		.center()
		.maybe_child(on.then(|| IconView::new(icons::CHECK).color(ink).size(12.)));
	let on_change = Rc::new(on_change);
	let press = on_change.clone();
	rect()
		.horizontal()
		.spacing(6.)
		.cross_align(Alignment::center())
		.a11y_focusable(enabled)
		.a11y_role(AccessibilityRole::CheckBox)
		.maybe(enabled, |r| {
			r.on_press(move |_| press(!on)).on_key_down(move |e: Event<KeyboardEventData>| {
				if e.key == Key::Character(" ".into()) {
					e.stop_propagation();
					on_change(!on);
				}
			})
		})
		.child(mark)
		.child(label().text(text).color(ink))
}

/// Wraps an element that takes no accessibility builder so it carries a test id.
pub fn tagged(child: impl IntoElement, id: impl Into<String>) -> Element {
	rect().test_id(id).child(child).into_element()
}

/// A text box applied on Enter or focus loss, Esc reverting.
pub fn text_box(value: String, multiline: bool, on_text: impl Fn(String) + 'static, test: String) -> Element {
	tagged(TextBox::new(value, on_text).multiline(multiline), test)
}

/// A drop-down button: the current word, a menu of the choices (member name, word) under it.
pub fn combo_box(
	current: String,
	items: Vec<(String, String)>,
	enabled: bool,
	test: String,
	on_pick: impl Fn(String) + 'static,
) -> Element {
	swatched_combo_box(current, items, Vec::new(), enabled, test, on_pick)
}

/// A [`combo_box`] showing the gradient `swatches` gives a value before its word, closed and in the list.
pub fn swatched_combo_box(
	current: String,
	items: Vec<(String, String)>,
	swatches: Vec<(String, Gradient)>,
	enabled: bool,
	test: String,
	on_pick: impl Fn(String) + 'static,
) -> Element {
	let on_pick = Rc::new(on_pick);
	ComboBox { current, items, swatches, enabled, test, on_pick: EventHandler::new(move |v: String| on_pick(v)) }
		.into_element()
}

/// `g` as a left-to-right fill.
fn fill_of(g: &Gradient) -> LinearGradient {
	LinearGradient::new()
		.angle(270.)
		.stops(g.stops.iter().map(|s| (s.color.to_color(), s.position as f32 * 100.).into()))
}

/// A scheme's swatch: 56 x 12 with a thin border.
const SWATCH: (f32, f32) = (56., 12.);

/// Width a drop-down button adds to its widest word: padding, gap and arrow.
const COMBO_CHROME: f32 = 56.;

#[derive(PartialEq)]
struct ComboBox {
	current: String,
	items: Vec<(String, String)>,
	swatches: Vec<(String, Gradient)>,
	enabled: bool,
	test: String,
	on_pick: EventHandler<String>,
}

impl Component for ComboBox {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let mut at = use_state(|| (0., 0.));
		let mut widest = use_state(|| 0f32);
		let ui = consume_context::<super::app::Ui>();
		let ink = if self.enabled { theme.text } else { theme.text_disabled };
		let swatch_of = |value: &str| self.swatches.iter().find(|(v, _)| v == value).map(|(_, g)| g.clone());
		let swatch = swatch_of(&self.current).map(|g| fill_of(&g));
		let word = self.items.iter().find(|(v, _)| *v == self.current).map_or(self.current.clone(), |(_, w)| w.clone());
		let entries: Vec<MenuEntry> = self
			.items
			.iter()
			.map(|(v, w)| {
				let item = MenuItem::new(v.clone(), w.clone()).check(Check::Radio(*v == self.current));
				match swatch_of(v) {
					Some(g) => item.swatch(g).into(),
					None => item.into(),
				}
			})
			.collect();
		let on_pick = self.on_pick.clone();
		let open = move |_: Event<PressEventData>| {
			let entries = entries.clone();
			let pick = on_pick.clone();
			ui.open_menu(*at.peek(), MenuModel::new(entries), move |value: String| pick.call(value));
		};
		let measure = rect()
			.position(Position::new_absolute())
			.width(Size::px(4000.))
			.opacity(0.)
			.interactive(Interactive::No)
			.child(
				rect()
					.vertical()
					.on_sized(move |e: Event<SizedEventData>| widest.set_if_modified(e.area.width()))
					.children(self.items.iter().map(|(_, w)| label().text(w.clone()).max_lines(1).into_element())),
			);
		// the column's width, never narrower than the widest word
		let natural = *widest.read();
		rect()
			.width(Size::fill())
			.on_sized(move |e: Event<SizedEventData>| {
				at.set((f64::from(e.area.min_x()), f64::from(e.area.max_y())));
			})
			.test_id(self.test.clone())
			.child(measure)
			.child(
				rect()
					.width(Size::fill())
					.maybe(natural > 0., |r| r.min_width(Size::px(natural + COMBO_CHROME)))
					.child(
						Button::new().compact().expanded().enabled(self.enabled).on_press(open).child(
							rect()
								.horizontal()
								.width(Size::fill())
								.content(Content::flex())
								.cross_align(Alignment::center())
								.maybe_child(swatch.map(|g| {
									rect()
										.width(Size::px(SWATCH.0))
										.height(Size::px(SWATCH.1))
										.margin((0., 6., 0., 0.))
										.background(g)
										.border(Border::new().fill(theme.input_border).width(0.5))
								}))
								.child(label().width(Size::flex(1.)).text(word).max_lines(1).color(ink))
								.child(IconView::new(icons::DROPDOWN).color(ink)),
						),
					),
			)
	}
}

/// The Properties pane.
#[derive(PartialEq)]
pub struct PropertiesPane;

impl Component for PropertiesPane {
	fn render(&self) -> impl IntoElement {
		let cx = Cx::get();
		let theme = use_wave_theme();
		let snap = {
			let shared = cx.shared.clone();
			use_memo(move || {
				#[cfg(test)]
				super::shared::count_run("properties");
				shared.read(Topics::DOCUMENT | Topics::SELECTION | Topics::SETTINGS, snapshot)
			})
		};
		// Enter on a tree row or an Add bumps `focus_form`: the first editor takes the focus once it is laid out.
		let first_editor = use_a11y();
		let focus_form = cx.ui.focus_form;
		let served = use_state(|| *focus_form.peek());
		let request = FocusRequest::new(focus_form, served);
		let snap = snap.read().clone();
		let s = snap.strings;
		let Some(row) = snap.row else {
			return rect()
				.expanded()
				.background(theme.tool_window_background)
				.center()
				.child(label().text(s.get("NoSelection")).color(theme.text_dim))
				.into_element();
		};
		if let Row::Folder(kind) = row {
			return folder_view(&s, kind, snap.count);
		}
		let form = Form::new(cx, row, s, (first_editor, request));
		let children = forms::build(&form, row, &snap);
		PropertyGrid::new().children(children).into_element()
	}
}

fn folder_view(s: &Strings, kind: ItemKind, count: usize) -> Element {
	let key = match kind {
		ItemKind::Source => "TreeSources",
		ItemKind::Part => "TreeParts",
		ItemKind::Probe => "TreeProbes",
		ItemKind::Scheme => "TreeColorSchemes",
	};
	let text = match count {
		0 => s.get("ItemCountZero").to_owned(),
		1 => s.get("ItemCountOne").to_owned(),
		n => s.format("ItemCountMany", &[&n]),
	};
	PropertyGrid::new()
		.child(wavedude_widgets::SectionHeader::new(s.get(key)))
		.child(readout("", text, "CollectionCount"))
		.into_element()
}
