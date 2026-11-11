//! The Properties pane (docs/ui/30): the form of the current row, built from a snapshot of the edited object
//! as JSON, every edit sent as `Command::Set` at the object's serde path.

use std::cell::Cell;
use std::rc::Rc;
use wavedude_widgets::Button;

use freya::prelude::*;
use serde_json::Value;
use wavedude_core::model::{PartKind, ranges};
use wavedude_widgets::icon::icons;
use wavedude_widgets::{Check, FocusRequest, Gradient, TestIdExt, TextBox};
use wavedude_widgets::{
	IconView, MenuEntry, MenuItem, MenuModel, NumberBox, NumberSpec, PropertyGrid, PropertyRow, use_wave_theme,
};

use wavedude_app_state::Event as AppEvent;
use wavedude_app_state::commands::{Command, Target};
use wavedude_app_state::selection::{ItemKind, Row};
use wavedude_app_state::strings::{Language, Strings};

use super::app::Cx;
use super::properties_forms as forms;
use super::shared::{App, Topics};

/// What a form shows: the row, its object as JSON and the read-outs computed from the project.
#[derive(Clone, PartialEq)]
pub struct Snapshot {
	pub row: Option<Row>,
	pub json: Rc<Value>,
	/// The part's kind, which picks its form.
	pub part: Option<PartKind>,
	pub strings: Strings,
	pub readouts: forms::Readouts,
	pub description_open: bool,
	pub count: usize,
}

/// `value` as JSON; the model's types always serialise.
pub fn to_json(value: impl serde::Serialize) -> Value {
	serde_json::to_value(value).expect("a model value serialises")
}

fn object_json(app: &App, row: Row) -> Value {
	let p = &app.document.project;
	match row {
		Row::Project => serde_json::json!({ "seed": p.seed, "description": p.description }),
		Row::Scene => to_json(p.scene.without_image()),
		Row::Simulation => to_json(&p.simulation),
		Row::Display => to_json(&p.display),
		Row::Folder(_) => Value::Null,
		Row::Item(i) => match i.kind {
			ItemKind::Source => to_json(&p.sources[i.index]),
			ItemKind::Part => to_json(&p.parts[i.index]),
			ItemKind::Probe => to_json(&p.probes[i.index]),
			ItemKind::Scheme => to_json(&p.color_schemes[i.index]),
		},
	}
}

fn snapshot(app: &App) -> Snapshot {
	let project = &app.document.project;
	let row = app.selection.current().filter(|r| match r {
		Row::Item(i) => i.exists(project),
		_ => true,
	});
	Snapshot {
		row,
		json: Rc::new(row.map_or(Value::Null, |r| object_json(app, r))),
		part: match row {
			Some(Row::Item(i)) if i.kind == ItemKind::Part => Some(project.parts[i.index].kind.clone()),
			_ => None,
		},
		strings: app.strings,
		readouts: row.map(|r| forms::readouts(app, r)).unwrap_or_default(),
		description_open: app.settings.part_description_expanded,
		count: match row {
			Some(Row::Folder(k)) => k.len(project),
			_ => 0,
		},
	}
}

/// The edited object and the means to edit it, shared by the form builders.
#[derive(Clone)]
pub struct Form {
	pub cx: Cx,
	pub target: Target,
	pub json: Rc<Value>,
	pub s: Strings,
	/// The id the form's first editor takes, so Enter on a tree row or Add can focus it; taken once.
	first_editor: Rc<Cell<Option<(AccessibilityId, FocusRequest)>>>,
	/// Number boxes show no value (a setting that does not apply).
	blank: bool,
}

/// A number's bounds from the model's range table; open for `""` and the keys the table lists as unbounded or
/// dependent.
pub fn bounds(key: &str) -> (f64, f64) {
	if let Some(r) = ranges::range(key) {
		return (r.min, r.max);
	}
	assert!(
		key.is_empty() || ranges::UNBOUNDED.contains(&key) || ranges::DEPENDENT.iter().any(|(k, _)| *k == key),
		"'{key}' is no key of the range tables"
	);
	(-1e9, 1e9)
}

/// A member name as its serde field: `CentreIndex` is `centre_index`, `BoxX` is `box_x`, `AX` is `ax`, `YMin` is
/// `y_min`.
pub fn snake(member: &str) -> String {
	let chars: Vec<char> = member.chars().collect();
	let mut out = String::with_capacity(member.len() + 4);
	for (i, &c) in chars.iter().enumerate() {
		if c.is_ascii_uppercase() && i > 0 {
			let prev = chars[i - 1];
			let next_lower = chars.get(i + 1).is_some_and(char::is_ascii_lowercase);
			if prev.is_ascii_lowercase() || prev.is_ascii_digit() || (prev.is_ascii_uppercase() && next_lower) {
				out.push('_');
			}
		}
		out.push(c.to_ascii_lowercase());
	}
	out
}

#[cfg(test)]
thread_local! {
	/// Every path a row bound; read by the forms test.
	pub static BOUND: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl Form {
	pub fn new(
		cx: Cx,
		target: Target,
		json: Rc<Value>,
		s: Strings,
		first_editor: (AccessibilityId, FocusRequest),
	) -> Self {
		Self { cx, target, json, s, first_editor: Rc::new(Cell::new(Some(first_editor))), blank: false }
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

	/// The same form over another object (a default shown greyed).
	pub fn with_json(&self, json: Value) -> Self {
		Self { json: Rc::new(json), ..self.clone() }
	}

	/// The same form with its number boxes empty.
	pub fn blanked(&self) -> Self {
		Self { blank: true, ..self.clone() }
	}

	/// The value at a dotted path, a number stepping into a list; `None` only for a field the model skips when
	/// unset (a source's `array` and `detach_after`).
	pub fn optional(&self, path: &str) -> Option<&Value> {
		path.split('.').try_fold(&*self.json, |at, k| match at {
			Value::Array(list) => list.get(k.parse::<usize>().ok()?),
			_ => at.get(k),
		})
	}

	/// The value at a dotted path. The paths are the forms' constants, so a miss is a bug.
	pub fn get(&self, path: &str) -> &Value {
		self.optional(path).unwrap_or_else(|| panic!("the form's object has no '{path}'"))
	}

	/// The value a row edits at `path`.
	pub fn bind(&self, path: &str) -> &Value {
		#[cfg(test)]
		BOUND.with_borrow_mut(|b| b.push(path.to_owned()));
		self.get(path)
	}

	/// The value a row edits at `path` as the model's type.
	pub fn bind_as<T: serde::de::DeserializeOwned>(&self, path: &str) -> T {
		serde_json::from_value(self.bind(path).clone()).unwrap_or_else(|e| panic!("'{path}' does not read back: {e}"))
	}

	pub fn num(&self, path: &str) -> f64 {
		self.get(path).as_f64().unwrap_or_else(|| panic!("'{path}' is not a number"))
	}

	pub fn flag(&self, path: &str) -> bool {
		self.get(path).as_bool().unwrap_or_else(|| panic!("'{path}' is not a flag"))
	}

	pub fn word(&self, path: &str) -> String {
		self.get(path).as_str().unwrap_or_else(|| panic!("'{path}' is not a string")).to_owned()
	}

	pub fn set(&self, path: &str, value: Value) {
		self.cx.send(AppEvent::Command(Command::Set { target: self.target, path: path.to_owned(), value }));
	}

	/// The rows of type `ty` at `prefix` (`""` or `kind.Lens`), range keys under `range`.
	pub fn scope<'a>(&'a self, ty: &'a str, prefix: &str, range: &'a str) -> Scope<'a> {
		let prefix = if prefix.is_empty() { String::new() } else { format!("{prefix}.") };
		Scope { f: self, ty, prefix, range }
	}

	fn decimal_comma(&self) -> bool {
		self.s.language() != Language::English
	}

	/// A number box row: `ty.member` names the label and hint, `range` the range table key.
	pub fn number(&self, ty: &str, member: &str, path: &str, range: &str, step: f64) -> PropertyRow {
		let (min, max) = bounds(range);
		let mut spec = NumberSpec::linear(min, max, step);
		if self.get(path).is_i64() || self.get(path).is_u64() {
			spec = spec.with_decimals(0);
		}
		self.number_with(ty, member, path, spec)
	}

	pub fn number_with(&self, ty: &str, member: &str, path: &str, spec: NumberSpec) -> PropertyRow {
		PropertyRow::switchable(self.s.label(ty, member), self.number_editor(ty, member, path, spec))
			.hint(self.s.hint(ty, member))
	}

	/// [`Self::number_with`] with `beside` right of the box.
	pub fn number_beside(&self, ty: &str, member: &str, path: &str, spec: NumberSpec, beside: Element) -> PropertyRow {
		let editor = self.number_editor(ty, member, path, spec);
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
		PropertyRow::switchable(self.s.label(ty, member), row).hint(self.s.hint(ty, member))
	}

	fn number_editor(
		&self,
		ty: &str,
		member: &str,
		path: &str,
		mut spec: NumberSpec,
	) -> impl Fn(bool) -> Element + 'static {
		if self.decimal_comma() {
			spec = spec.with_decimal_separator(',');
		}
		let bound = self.bind(path);
		let integer = bound.is_i64() || bound.is_u64();
		let value = self.num(path);
		let form = self.clone();
		let path = path.to_owned();
		let test = format!("{ty}.{member}");
		let first = self.claim_first();
		let blank = self.blank;
		move |enabled| {
			let (form, path) = (form.clone(), path.clone());
			let editor = NumberBox::new(
				WritableSetter::new(value, move |v: f64| {
					let json = if integer { Value::from(v.round() as i64) } else { Value::from(v) };
					form.set(&path, json);
				}),
				spec.clone(),
			)
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
	pub fn check(&self, ty: &str, member: &str, path: &str) -> PropertyRow {
		self.check_labelled(
			self.s.label(ty, member).to_owned(),
			self.s.hint(ty, member),
			path,
			&format!("{ty}.{member}"),
		)
	}

	pub fn check_labelled(&self, text: String, hint: &str, path: &str, test: &str) -> PropertyRow {
		self.check_in_column("", text, hint, path, test).full_width()
	}

	/// A check box in the editor column, `label` in the label column and `text` beside the box.
	pub fn check_in_column(&self, label: &str, text: String, hint: &str, path: &str, test: &str) -> PropertyRow {
		let on = self.bind_as::<bool>(path);
		let form = self.clone();
		let (path, test) = (path.to_owned(), test.to_owned());
		let editor = move |enabled| {
			let (form, path) = (form.clone(), path.clone());
			check_box_in(text.clone(), on, enabled, move |v| form.set(&path, Value::from(v)))
				.test_id(test.clone())
				.into_element()
		};
		PropertyRow::switchable(label, editor).hint(hint)
	}

	/// An enum combo: `variants` are the serde names, their words `Enum.<enum_ty>.<variant>`.
	pub fn combo(&self, ty: &str, member: &str, path: &str, enum_ty: &str, variants: &[&str]) -> PropertyRow {
		let current = self.bind_as::<String>(path);
		let items: Vec<(String, String)> =
			variants.iter().map(|v| ((*v).to_owned(), self.s.enum_word(enum_ty, v).to_owned())).collect();
		let form = self.clone();
		let path = path.to_owned();
		let test = format!("{ty}.{member}");
		let editor = move |enabled| {
			let (form, path) = (form.clone(), path.clone());
			combo_box(current.clone(), items.clone(), enabled, test.clone(), move |v| form.set(&path, Value::from(v)))
		};
		PropertyRow::switchable(self.s.label(ty, member), editor).hint(self.s.hint(ty, member))
	}

	/// A text box applied on Enter or focus loss, Esc reverting (`multiline` keeps Enter as newline).
	pub fn text(&self, ty: &str, member: &str, path: &str, multiline: bool) -> PropertyRow {
		let value = self.bind_as::<String>(path);
		let form = self.clone();
		let path = path.to_owned();
		let on_text = move |t: String| form.set(&path, Value::from(t));
		let editor = self.first(TextBox::new(value, on_text).multiline(multiline));
		PropertyRow::new(self.s.label(ty, member), tagged(editor, format!("{ty}.{member}")))
			.hint(self.s.hint(ty, member))
	}

	/// The item's name box: applied on every keystroke, so the tree follows the typing; renames through the
	/// command that keeps references.
	pub fn name(&self, ty: &str) -> PropertyRow {
		let Target::Item(target) = self.target else { return self.text(ty, "Name", "name", false) };
		let value = self.bind_as::<String>("name");
		let cx = self.cx.clone();
		let on_text = move |name: String| cx.send(AppEvent::Command(Command::Rename { target, name }));
		let editor = self.first(TextBox::new(value, on_text).live().select_all_on_focus(true));
		PropertyRow::new(self.s.label(ty, "Name"), tagged(editor, format!("{ty}.Name"))).hint(self.s.hint(ty, "Name"))
	}
}

/// The rows of one object type: labels, hints and test ids are `<ty>.<Member>`, the JSON path is the prefix
/// plus the member in snake case, the range key `<range>.<Member>`.
pub struct Scope<'a> {
	pub f: &'a Form,
	ty: &'a str,
	prefix: String,
	range: &'a str,
}

impl Scope<'_> {
	pub fn path(&self, member: &str) -> String {
		format!("{}{}", self.prefix, snake(member))
	}

	fn key(&self, member: &str) -> String {
		format!("{}.{member}", self.range)
	}

	pub fn flag(&self, member: &str) -> bool {
		self.f.flag(&self.path(member))
	}

	pub fn num(&self, member: &str) -> f64 {
		self.f.num(&self.path(member))
	}

	pub fn word(&self, member: &str) -> String {
		self.f.word(&self.path(member))
	}

	/// A number bounded by the member's own range key.
	pub fn number(&self, member: &str, step: f64) -> PropertyRow {
		self.number_in(member, &self.key(member), step)
	}

	/// A number bounded by a shared range key (`Part.X`); `""` is unbounded.
	pub fn number_in(&self, member: &str, range: &str, step: f64) -> PropertyRow {
		self.f.number(self.ty, member, &self.path(member), range, step)
	}

	pub fn number_with(&self, member: &str, spec: NumberSpec) -> PropertyRow {
		self.f.number_with(self.ty, member, &self.path(member), spec)
	}

	/// A number with `places` decimals, bounded by the member's range key, dragged by `drag`.
	pub fn decimals(&self, member: &str, drag: f64, places: u32) -> PropertyRow {
		self.decimals_in(member, &self.key(member), drag, places)
	}

	/// A number with `places` decimals, bounded by a shared range key, dragged by `drag`.
	pub fn decimals_in(&self, member: &str, range: &str, drag: f64, places: u32) -> PropertyRow {
		let (min, max) = bounds(range);
		self.number_with(member, NumberSpec::linear(min, max, 1.).with_decimals(places).with_drag_step(drag))
	}

	pub fn check(&self, member: &str) -> PropertyRow {
		self.f.check(self.ty, member, &self.path(member))
	}

	pub fn combo(&self, member: &str, enum_ty: &str, variants: &[&str]) -> PropertyRow {
		self.f.combo(self.ty, member, &self.path(member), enum_ty, variants)
	}

	pub fn text(&self, member: &str, multiline: bool) -> PropertyRow {
		self.f.text(self.ty, member, &self.path(member), multiline)
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
		.border(Border::new().fill(theme.input_border).width(1.).alignment(BorderAlignment::Inner))
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

/// A drop-down button: the current word, a menu of the choices (serde name, word) under it.
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
		let target = match row {
			Row::Project => Target::Project,
			Row::Scene => Target::Scene,
			Row::Simulation => Target::Simulation,
			Row::Display => Target::Display,
			Row::Item(i) => Target::Item(i),
			Row::Folder(kind) => return folder_view(&s, kind, snap.count),
		};
		let form = Form::new(cx, target, snap.json.clone(), s, (first_editor, request));
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

#[cfg(test)]
mod tests {
	use super::snake;

	#[test]
	fn members_become_serde_fields() {
		for (member, field) in [("CentreIndex", "centre_index"), ("BoxX", "box_x"), ("AX", "ax"), ("YMin", "y_min")] {
			assert_eq!(snake(member), field);
		}
		assert_eq!(snake("Thickness1"), "thickness1");
		assert_eq!(snake("AutoScaleX"), "auto_scale_x");
	}
}
