# 0013: Typed model fields; the model without serde

Status: accepted, 2026-11-11.

## Context

A model field was addressed by a string in five systems that drifted apart independently: the properties forms
(serde path strings and `word("Kind") == "Line"` conditions), `Command::Set { path, value }` (applied by
round-tripping the object through JSON), the range table keyed `"Type.Property"` (asserted at run time), the
`.wdproj` attribute tables, and the string tables (`Label.<Type>.<Member>`). History snapshotted the project as
JSON text and `SceneKey` hashed parts through a serde serializer. A misspelt path compiled and failed at run time
or not at all; the probe-form and Fresnel-writer bugs came from a field one system knew and another did not.

## Decision

- `wavedude-model-derive` provides `#[derive(Fields)]` and `#[derive(Word)]`. For each struct or enum it generates
  a `<Type>Field` enum (one member per field, the PascalCase of the name; a nested field wraps the nested type's),
  a `<Type>Edit` enum (the same members carrying the new value), the `Fields`, `Field`, `Edit` and `Feed` impls
  (`crates/wavedude-core/src/model/fields.rs`) and an associated const per field (`Probe::BOX_WIDTH`: a `Prop`, a
  `Nested` or a `Variant` holding the field, a getter and an edit constructor).
- `Field::all()` is the model's table; `Fields::visit` hands a `Visitor` every type and `Word` reachable from a type, so
  the tests that check each type need no list of types. A field's bound is an attribute on it (`range`, `dependent`); a
  field that
  is no scalar (a list, the image bytes) is `skip`, handled by hand in the adapter and the forms.
- The `.wdproj` adapter owns everything XML: typed `Attr` tables in Next's order with the names, presence and
  codecs that differ from the model (`crates/wavedude-core/src/io/fields.rs`, `io/wdproj.rs`). Reading builds a
  typed edit per attribute and applies it to a base; writing compares against the base.
- `Command::Edit(Edit)` carries a typed edit of the project, a source, a part, a probe or a scheme; history steps
  hold project clones. Forms are built over a `Scope<T: Fields>` whose rows take a `Prop`; labels, hints and test
  ids are `<TYPE_NAME>.<Member>`.
- The model has no serde derives or attributes. serde stays in core for the Wave Dude 2D and scheme JSON files.

## Consequences

- A form row of the wrong value type, a misspelt field, an edit of a field the object does not have and a combo
  over a non-enum fail to compile. A field without a label, a hint, an attribute in the file tables or a bound
  fails a test that iterates `Field::all()`.
- A new field costs its derive attributes, an `Attr`, a form row, four-language `Label.`/`Hint.` keys and the
  `Normalize` clamp (docs/conventions.md, Where new things go).
- A proc-macro crate joins the workspace (`syn`, `quote`, already in the lock file); its output is exercised by
  the model's tests.
- The string keys follow the model's type and member names, not serde names.

## Alternatives rejected

- Keep the serde paths and add tests that every path exists: finds the drift after the fact, keeps the JSON
  round trip per edit, and leaves the model shaped by a file format.
- `macro_rules!` struct definitions: the definition of every model type moves into macro syntax, which hides the
  types from readers and tools.
- A marker type per field with trait impls: the same checks as the const accessors with a type per field, which
  needs the derive anyway and reads worse at the call site.
