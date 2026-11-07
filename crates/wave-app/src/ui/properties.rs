//! The Properties pane (docs/ui/30): the form of the current row, built from a snapshot of the edited object
//! as JSON, every edit sent as `Command::Set` at the object's serde path.

use std::rc::Rc;

use freya::prelude::*;
use serde_json::Value;
use wave_core::model::{PartKind, ranges};
use wave_widgets::icon::icons;
use wave_widgets::{Check, TestIdExt};
use wave_widgets::{
	IconView, MenuEntry, MenuItem, MenuModel, NumberBox, NumberSpec, PropertyGrid, PropertyRow, use_wave_theme,
};

use wave_app_state::Event as AppEvent;
use wave_app_state::commands::{Command, Target};
use wave_app_state::history::scene_without_image;
use wave_app_state::selection::{ItemKind, Row};
use wave_app_state::strings::{Language, Strings};

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
		Row::Scene => to_json(scene_without_image(&p.scene)),
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
		json: Rc::new(row.map(|r| object_json(app, r)).unwrap_or(Value::Null)),
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

	pub fn number_with(&self, ty: &str, member: &str, path: &str, mut spec: NumberSpec) -> PropertyRow {
		if self.decimal_comma() {
			spec = spec.with_decimal_separator(',');
		}
		let bound = self.bind(path);
		let integer = bound.is_i64() || bound.is_u64();
		let value = self.num(path);
		let form = self.clone();
		let path_owned = path.to_owned();
		let editor = NumberBox::new(
			WritableSetter::new(value, move |v: f64| {
				let json = if integer { Value::from(v.round() as i64) } else { Value::from(v) };
				form.set(&path_owned, json);
			}),
			spec,
		)
		.width(Size::fill())
		.test_id(format!("{ty}.{member}"));
		PropertyRow::new(self.s.label(ty, member), editor).hint(self.s.hint(ty, member))
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
		let on = self.bind_as::<bool>(path);
		let form = self.clone();
		let path = path.to_owned();
		PropertyRow::new("", check_box(text, on, move |v| form.set(&path, Value::from(v))).test_id(test.to_owned()))
			.hint(hint)
			.full_width()
	}

	/// An enum combo: `variants` are the serde names, their words `Enum.<enum_ty>.<variant>`.
	pub fn combo(&self, ty: &str, member: &str, path: &str, enum_ty: &str, variants: &[&str]) -> PropertyRow {
		let current = self.bind_as::<String>(path);
		let items: Vec<(String, String)> =
			variants.iter().map(|v| ((*v).to_owned(), self.s.enum_word(enum_ty, v).to_owned())).collect();
		let form = self.clone();
		let path = path.to_owned();
		let editor = combo_box(&self.cx, current, items, format!("{ty}.{member}"), move |v| {
			form.set(&path, Value::from(v));
		});
		PropertyRow::new(self.s.label(ty, member), editor).hint(self.s.hint(ty, member))
	}

	/// A text box applied on every keystroke (`multiline` keeps Enter as newline).
	pub fn text(&self, ty: &str, member: &str, path: &str, multiline: bool) -> PropertyRow {
		let value = self.bind_as::<String>(path);
		let form = self.clone();
		let path = path.to_owned();
		let on_text = move |t: String| form.set(&path, Value::from(t));
		PropertyRow::new(self.s.label(ty, member), text_box(value, multiline, on_text, format!("{ty}.{member}")))
			.hint(self.s.hint(ty, member))
	}

	/// The item's name box: renames through the command that keeps references.
	pub fn name(&self, ty: &str) -> PropertyRow {
		let Target::Item(target) = self.target else { return self.text(ty, "Name", "name", false) };
		let value = self.bind_as::<String>("name");
		let cx = self.cx.clone();
		let on_text = move |name: String| cx.send(AppEvent::Command(Command::Rename { target, name }));
		PropertyRow::new(self.s.label(ty, "Name"), text_box(value, false, on_text, format!("{ty}.Name")))
			.hint(self.s.hint(ty, "Name"))
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
	let theme = use_wave_theme();
	let mark = rect()
		.width(Size::px(14.))
		.height(Size::px(14.))
		.border(Border::new().fill(theme.input_border).width(1.).alignment(BorderAlignment::Inner))
		.background(theme.input_background)
		.center()
		.maybe_child(on.then(|| IconView::new(icons::CHECK).color(theme.text).size(12.)));
	let on_change = Rc::new(on_change);
	let press = on_change.clone();
	rect()
		.horizontal()
		.spacing(6.)
		.cross_align(Alignment::center())
		.a11y_focusable(true)
		.a11y_role(AccessibilityRole::CheckBox)
		.on_press(move |_| press(!on))
		.on_key_down(move |e: Event<KeyboardEventData>| {
			if e.key == Key::Character(" ".into()) {
				e.stop_propagation();
				on_change(!on);
			}
		})
		.child(mark)
		.child(label().text(text))
}

/// Wraps an element that takes no accessibility builder so it carries a test id.
pub fn tagged(child: impl IntoElement, id: impl Into<String>) -> Element {
	rect().test_id(id).child(child).into_element()
}

/// A text box; `on_text` gets every edit.
pub fn text_box(value: String, multiline: bool, on_text: impl Fn(String) + 'static, test: String) -> Element {
	tagged(
		Input::new(WritableSetter::new(value, move |t: String| on_text(t)))
			.multiline(multiline)
			.width(Size::fill())
			.compact(),
		test,
	)
}

/// A drop-down button: the current word, a menu of the choices (serde name, word) under it.
pub fn combo_box(
	_cx: &Cx,
	current: String,
	items: Vec<(String, String)>,
	test: String,
	on_pick: impl Fn(String) + 'static,
) -> Element {
	let on_pick = Rc::new(on_pick);
	ComboBox { current, items, test, on_pick: EventHandler::new(move |v: String| on_pick(v)) }.into_element()
}

/// Width a drop-down button adds to its widest word: padding, gap and arrow.
const COMBO_CHROME: f32 = 56.;
const COMBO_MIN: f32 = 80.;

#[derive(PartialEq)]
struct ComboBox {
	current: String,
	items: Vec<(String, String)>,
	test: String,
	on_pick: EventHandler<String>,
}

impl Component for ComboBox {
	fn render(&self) -> impl IntoElement {
		let theme = use_wave_theme();
		let mut at = use_state(|| (0., 0.));
		let mut cell = use_state(|| 0f32);
		let mut widest = use_state(|| 0f32);
		let ui = consume_context::<super::app::Ui>();
		let word = self.items.iter().find(|(v, _)| *v == self.current).map_or(self.current.clone(), |(_, w)| w.clone());
		let (items, current, on_pick) = (self.items.clone(), self.current.clone(), self.on_pick.clone());
		let open = move |_: Event<PressEventData>| {
			let entries: Vec<MenuEntry> = items
				.iter()
				.map(|(v, w)| MenuItem::new(v.clone(), w.clone()).check(Check::Radio(*v == current)).into())
				.collect();
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
		let natural = *widest.read();
		let width = if natural > 0. {
			Size::px((natural + COMBO_CHROME).max(COMBO_MIN).min(cell.read().max(COMBO_MIN)))
		} else {
			Size::fill()
		};
		rect()
			.width(Size::fill())
			.on_sized(move |e: Event<SizedEventData>| {
				cell.set_if_modified(e.area.width());
				at.set((f64::from(e.area.min_x()), f64::from(e.area.max_y())));
			})
			.test_id(self.test.clone())
			.child(measure)
			.child(
				rect().width(width).child(
					Button::new().compact().expanded().on_press(open).child(
						rect()
							.horizontal()
							.width(Size::fill())
							.content(Content::flex())
							.cross_align(Alignment::center())
							.child(label().width(Size::flex(1.)).text(word).max_lines(1))
							.child(IconView::new(icons::DROPDOWN).color(theme.text)),
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
		let form = Form { cx, target, json: snap.json.clone(), s };
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
		.child(wave_widgets::SectionHeader::new(s.get(key)))
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
