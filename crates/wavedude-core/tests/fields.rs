//! The derived typed fields of the model: listing, naming, reading, editing and hashing.

use std::collections::{BTreeSet, HashSet};
use std::hash::{DefaultHasher, Hasher};

use wavedude_core::model::ranges;
use wavedude_core::model::*;

/// A raw identifier names its member without the `r#`.
#[derive(Clone, Debug, PartialEq, Fields)]
pub struct Raw {
	pub r#type: u32,
	pub in_x: i32,
}

#[test]
fn a_raw_identifier_names_its_member_plainly() {
	assert_eq!(Raw::TYPE.field.name(), "Type");
	assert_eq!(Raw::IN_X.field.name(), "InX");
	let mut raw = Raw { r#type: 1, in_x: 2 };
	(Raw::TYPE.edit)(5).apply(&mut raw).unwrap();
	assert_eq!(raw.r#type, 5);
}

fn hash_of<T: Feed>(v: &T) -> u64 {
	let mut h = DefaultHasher::new();
	v.feed(&mut h);
	h.finish()
}

fn lens_part() -> Part {
	Part::new(PartKind::Lens(Lens::default()), 5, 6)
}

fn paths_under(prefix: &str) -> Vec<String> {
	PartField::all().into_iter().map(Field::path).filter(|p| p.starts_with(prefix)).collect()
}

#[test]
fn part_lists_every_leaf_through_its_base_and_every_kind() {
	assert_eq!(paths_under("Base.").len(), 5, "name, enabled, x, y, angle");
	// Material 4 + coating 5 + diameter, r1, r2, thickness.
	assert_eq!(paths_under("Kind.Lens.").len(), 13);
	// Material 4 + coating 5 + sides, side.
	assert_eq!(paths_under("Kind.Polygon.").len(), 11);
	// Coating 5 + length, diameter, centre index, edge index.
	assert_eq!(paths_under("Kind.GrinLens.").len(), 9);
	assert_eq!(paths_under("Kind.LogicGate.").len(), 10);
	assert_eq!(paths_under("Kind.ZonePlate.").len(), 8);
	// A Fresnel lens carries its lens's 13 and zones, division.
	assert_eq!(paths_under("Kind.FresnelLens.").len(), 15);
	assert!(paths_under("Kind.FresnelLens.Lens.Material.Mirror").len() == 1);

	let kinds: BTreeSet<String> =
		paths_under("Kind.").iter().filter_map(|p| p.split('.').nth(1).map(str::to_owned)).collect();
	assert_eq!(kinds.len(), 17, "{kinds:?}");
	let all = PartField::all();
	assert_eq!(all.iter().copied().collect::<HashSet<_>>().len(), all.len(), "no field listed twice");
	let paths: HashSet<String> = all.into_iter().map(Field::path).collect();
	assert!(paths.contains("Kind.MetaSurface.Design.Prism.Deflection"));
	assert!(paths.contains("Kind.Outline.Points"));
}

#[test]
fn paths_name_every_level_and_names_the_last() {
	let mirror = (Part::KIND.field)((PartKind::LENS.field)((Lens::MATERIAL.field)(Material::MIRROR.field)));
	assert_eq!(mirror.path(), "Kind.Lens.Material.Mirror");
	assert_eq!(mirror.name(), "Kind", "a nested field names its own level");
	assert_eq!(Material::MIRROR.field.path(), "Mirror");
	assert_eq!(Part::TYPE_NAME, "Part");
	assert_eq!(PartBase::TYPE_NAME, "Part");
	assert_eq!(Lens::TYPE_NAME, "Lens");
	assert_eq!(MetaPrismDesign::TYPE_NAME, "MetaPrism");
}

#[test]
fn names_are_the_members_in_pascal_case() {
	assert_eq!(Probe::BOX_WIDTH.field.name(), "BoxWidth");
	assert_eq!(Probe::AX.field.name(), "Ax");
	assert_eq!(Lens::R1.field.name(), "R1");
	assert_eq!(BraggReflector::THICKNESS1.field.name(), "Thickness1");
	assert_eq!(FiberNode::IN_X.field.name(), "InX");
	assert_eq!(GrinLens::CENTRE_INDEX.field.name(), "CentreIndex");
}

#[test]
fn kinds_and_bounds_follow_the_field() {
	assert_eq!(Probe::NAME.field.kind(), ValueKind::Text);
	assert_eq!(Probe::AX.field.kind(), ValueKind::Int);
	assert_eq!(Probe::BOX_WIDTH.field.kind(), ValueKind::UInt);
	assert_eq!(Probe::Y_MIN.field.kind(), ValueKind::Float);
	assert_eq!(Probe::ACTIVE.field.kind(), ValueKind::Bool);
	assert_eq!(Probe::LAYER.field.kind(), ValueKind::Word(&["Field", "Intensity"]));
	assert_eq!(Outline::POINTS.field.kind(), ValueKind::List);
	assert_eq!(Source::DETACH_AFTER.field.kind(), ValueKind::UInt, "an option has its leaf's kind");
	assert_eq!(Source::SET_ARRAY.field.kind(), ValueKind::Nested);
	assert_eq!(Probe::BOX_WIDTH.field.bounds(), Bounds::Range(ranges::PROBE_BOX_WIDTH));
	assert_eq!(Probe::Y_MIN.field.bounds(), Bounds::Dependent);
	assert_eq!(Probe::AX.field.bounds(), Bounds::Open);
	assert!(Probe::Y_MIN.field.dependent() && !Probe::AX.field.dependent());
	let both = Rectangle::WIDTH.field;
	assert!(both.dependent(), "a dependent field keeps its flag beside a range");
	assert_eq!(both.bounds(), Bounds::Range(ranges::AT_LEAST_1));
	let size_x = (Source::ARRAY.field)(SourceArray::SIZE_X.field);
	assert_eq!(size_x.bounds(), Bounds::Range(ranges::AT_LEAST_1));
}

/// Sets `field` to `new`, expecting `old` there before and the same value back through `get` after.
fn round_trip<T: Fields>(owner: &mut T, field: T::Field, old: Value, new: Value) {
	assert_eq!(field.get(owner), Some(old.clone()), "{}", field.path());
	let edit = field.edit(new.clone()).unwrap();
	assert_eq!(edit.field(), field);
	assert_eq!(edit.apply(owner).unwrap(), old);
	assert_eq!(field.get(owner), Some(new));
}

#[test]
fn get_edit_and_apply_round_trip_for_every_kind_of_value() {
	let mut p = Probe::default();
	round_trip(&mut p, Probe::NAME.field, Value::Text(String::new()), Value::Text("north".into()));
	round_trip(&mut p, Probe::AX.field, Value::Int(0), Value::Int(-40));
	round_trip(&mut p, Probe::BOX_WIDTH.field, Value::Int(400), Value::Int(520));
	round_trip(&mut p, Probe::Y_MAX.field, Value::Float(1.0), Value::Float(2.5));
	round_trip(&mut p, Probe::ACTIVE.field, Value::Bool(true), Value::Bool(false));
	round_trip(&mut p, Probe::LAYER.field, Value::Word("Field"), Value::Word("Intensity"));
	assert_eq!(p.name, "north");
	assert_eq!((p.ax, p.box_width, p.y_max, p.active, p.layer), (-40, 520, 2.5, false, Layer::Intensity));

	let mut s = Source::default();
	round_trip(&mut s, Source::DETACH_AFTER.field, Value::Absent, Value::Int(300));
	round_trip(&mut s, Source::DETACH_AFTER.field, Value::Int(300), Value::Absent);
	assert_eq!(s.detach_after, None);

	let mut o = Outline::default();
	let points = Value::Other(format!("{:?}", o.points));
	assert_eq!(Outline::POINTS.field.get(&o), Some(points));
	assert!(matches!(
		Outline::POINTS.field.edit(Value::Int(1)),
		Err(EditError::WrongKind { want: ValueKind::List, .. })
	));
	let edit = (Outline::POINTS.edit)(vec![Point::new(1, 2)]);
	assert_eq!(edit.apply(&mut o).unwrap(), Value::Other("[]".into()));
	assert_eq!(o.points, [Point::new(1, 2)]);
}

#[test]
fn nested_fields_read_and_edit_through_their_owners() {
	let mut part = lens_part();
	let mirror = (Part::KIND.field)((PartKind::LENS.field)((Lens::MATERIAL.field)(Material::MIRROR.field)));
	round_trip(&mut part, mirror, Value::Bool(false), Value::Bool(true));
	assert!(matches!(&part.kind, PartKind::Lens(l) if l.material.mirror));
	let x = (Part::BASE.field)(PartBase::X.field);
	round_trip(&mut part, x, Value::Int(5), Value::Int(9));
	assert_eq!(part.base.x, 9);
	// The typed accessors compose the same edit as the field does.
	let by_accessors = (Part::KIND.wrap)((PartKind::LENS.wrap)((Lens::MATERIAL.wrap)((Material::MIRROR.edit)(false))));
	assert_eq!(by_accessors.field(), mirror);
	assert_eq!(by_accessors.apply(&mut part).unwrap(), Value::Bool(true));
	assert!(!(Material::MIRROR.get)((Lens::MATERIAL.get)((PartKind::LENS.get)(&part.kind).unwrap())));
}

#[test]
fn a_value_of_the_wrong_kind_is_wrong_kind() {
	let wrong = |e: Result<_, EditError>| matches!(e, Err(EditError::WrongKind { .. }));
	assert!(wrong(Probe::BOX_WIDTH.field.edit(Value::Text("4".into())).map(drop)));
	assert!(wrong(Probe::BOX_WIDTH.field.edit(Value::Int(-1)).map(drop)), "a negative is no u32");
	assert!(wrong(Probe::BOX_WIDTH.field.edit(Value::Float(4.0)).map(drop)));
	assert!(wrong(Probe::LAYER.field.edit(Value::Word("Nowhere")).map(drop)));
	assert!(wrong(Probe::ACTIVE.field.edit(Value::Absent).map(drop)));
	assert!(wrong(Source::SET_ARRAY.field.edit(Value::Int(1)).map(drop)));
	let err = Probe::BOX_WIDTH.field.edit(Value::Bool(true)).unwrap_err();
	assert_eq!(err, EditError::WrongKind { field: "BoxWidth".into(), want: ValueKind::UInt });
	assert_eq!(err.to_string(), "BoxWidth takes a UInt");
}

#[test]
fn an_edit_of_another_variant_or_an_absent_option_changes_nothing() {
	let mut kind = PartKind::Polygon(Polygon::default());
	let before = kind.clone();
	let edit = PartKindEdit::Lens(LensEdit::Diameter(7));
	assert_eq!(
		edit.apply(&mut kind),
		Err(EditError::WrongKind { field: "Lens.Diameter".into(), want: ValueKind::Nested })
	);
	assert_eq!(kind, before);
	assert_eq!(edit.current(&kind), None);
	assert!((PartKind::LENS.get)(&kind).is_none());

	let mut source = Source::default();
	let edit = SourceEdit::Array(SourceArrayEdit::SizeX(3));
	assert_eq!(edit.apply(&mut source), Err(EditError::Absent("Array.SizeX".into())));
	assert_eq!(source, Source::default());
	assert_eq!(edit.current(&source), None);
	assert_eq!((Source::ARRAY.get)(&source), None);
	assert_eq!(edit.field().get(&source), None);
}

#[test]
fn current_is_the_inverse_of_apply() {
	let mut p = Probe::default();
	let original = p.clone();
	let edit = ProbeEdit::BoxWidth(500);
	let back = edit.current(&p).unwrap();
	assert_eq!(back, ProbeEdit::BoxWidth(400));
	edit.apply(&mut p).unwrap();
	assert_eq!(p.box_width, 500);
	back.apply(&mut p).unwrap();
	assert_eq!(p, original);

	let mut part = lens_part();
	let original = part.clone();
	let edit = PartEdit::Kind(PartKindEdit::Lens(LensEdit::Material(MaterialEdit::Mirror(true))));
	let back = edit.current(&part).unwrap();
	edit.apply(&mut part).unwrap();
	assert_ne!(part, original);
	back.apply(&mut part).unwrap();
	assert_eq!(part, original);
	let polygon = Part::new(PartKind::Polygon(Polygon::default()), 0, 0);
	assert_eq!(edit.current(&polygon), None);

	let mut source = Source { array: Some(SourceArray::default()), detach_after: Some(4), ..Source::default() };
	let original = source.clone();
	for edit in
		[SourceEdit::DetachAfter(None), SourceEdit::SetArray(None), SourceEdit::Array(SourceArrayEdit::Angle(30.0))]
	{
		let back = edit.current(&source).unwrap();
		edit.apply(&mut source).unwrap();
		back.apply(&mut source).unwrap();
		assert_eq!(source, original, "{edit:?}");
	}
}

#[test]
fn the_whole_prop_of_the_source_array_replaces_it() {
	let mut s = Source::default();
	assert_eq!((Source::SET_ARRAY.get)(&s), None);
	let array = SourceArray { size_x: 3, ..SourceArray::default() };
	let set = (Source::SET_ARRAY.edit)(Some(array.clone()));
	assert_eq!(set.field(), Source::SET_ARRAY.field);
	assert_eq!(set.apply(&mut s).unwrap(), Value::Absent);
	assert_eq!((Source::ARRAY.get)(&s), Some(&array));
	let off = (Source::SET_ARRAY.edit)(None);
	assert_eq!(off.apply(&mut s).unwrap(), Value::Other(format!("{array:?}")));
	assert_eq!(s.array, None);
	assert_eq!(Source::SET_ARRAY.field.name(), "Array");
	assert!(!SourceField::all().contains(&Source::SET_ARRAY.field), "a whole replacement is no leaf");
}

#[test]
fn props_are_copy_and_read_and_build_edits() {
	let ax = Probe::AX;
	let copy = ax;
	let p = Probe { ax: 12, ..Probe::default() };
	assert_eq!((ax.get)(&p), 12);
	assert_eq!((copy.get)(&p), 12);
	assert_eq!((ax.edit)(3), ProbeEdit::Ax(3));
	let points = Outline::POINTS;
	let (a, b) = (points, points);
	assert_eq!((a.get)(&Outline::default()), (b.get)(&Outline::default()));
	let material = Lens::MATERIAL;
	let again = material;
	assert_eq!((again.get)(&Lens::default()), (material.get)(&Lens::default()));
	let lens = PartKind::LENS;
	let again = lens;
	assert!((again.get)(&PartKind::Lens(Lens::default())).is_some());
	assert!((lens.get)(&PartKind::Polygon(Polygon::default())).is_none());
}

#[test]
fn feed_gives_equal_hashes_for_equal_values_and_different_ones_for_a_change() {
	let a = lens_part();
	assert_eq!(hash_of(&a), hash_of(&a.clone()));
	let mut b = a.clone();
	if let PartKind::Lens(l) = &mut b.kind {
		l.r1 += 1e-9;
	}
	assert_ne!(hash_of(&a), hash_of(&b), "a changed float");
	let mut c = a.clone();
	if let PartKind::Lens(l) = &mut c.kind {
		l.r1 = -l.r1;
	}
	assert_ne!(hash_of(&a), hash_of(&c));
	assert_ne!(hash_of(&0.0f64), hash_of(&-0.0f64), "floats go by their bits");
	assert_ne!(hash_of(&None::<u32>), hash_of(&Some(0u32)));
	assert_ne!(hash_of(&vec![1u32, 2]), hash_of(&vec![1u32, 2, 0]));
	assert_ne!(hash_of(&PartKind::Lens(Lens::default())), hash_of(&PartKind::Polygon(Polygon::default())));
	assert_ne!(hash_of(&Layer::Field), hash_of(&Layer::Intensity));
}

fn words_round_trip<W: Word + std::fmt::Debug>() {
	assert_eq!(W::MEMBERS.len(), W::NAMES.len());
	for (&member, &name) in W::MEMBERS.iter().zip(W::NAMES) {
		assert_eq!(member.name(), name);
		assert_eq!(W::parse(name), Some(member), "{}", W::TYPE_NAME);
	}
	assert_eq!(W::parse("Nowhere"), None);
	assert_eq!(W::parse(""), None);
}

#[test]
fn word_parses_every_member_back() {
	words_round_trip::<Layer>();
	words_round_trip::<Measurement>();
	words_round_trip::<ProbeKind>();
	words_round_trip::<BoxSize>();
	words_round_trip::<Stencil>();
	words_round_trip::<GateKind>();
	words_round_trip::<TriangleType>();
	words_round_trip::<FacetPattern>();
	assert_eq!(BoxSize::TYPE_NAME, "BoxSize");
	assert_eq!(BoxSize::MEMBERS[4].name(), "ExtraLarge");
	assert_eq!(Layer::parse("Intensity"), Some(Layer::Intensity));
}
