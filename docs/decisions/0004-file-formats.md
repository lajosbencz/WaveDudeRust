# 0004 Native project file and compatibility adapters

Status: accepted, 2026-11-05

The model is format-agnostic (serde). Native file: `.wdr`, a zip container
holding `project.json` (versioned by a `format` field) and `scene.<ext>` when
the scene has an image; the project's colour schemes are part of the JSON.
The README says it is a zip; renaming it opens it in any archive tool.

## Sparse JSON (added 2026-11-08, the user's decision)

`project.json` leaves out every value equal to its type's default (`io/sparse.rs`), object by object; the
reader's serde defaults put them back. A default that changes in a later version therefore changes what an
old file means, as it does for `.wdproj`; that is accepted. Measured on the 24 examples (docs/measurements.md),
the container is a fraction of the `.wdproj` text, which is the point of the format.

Compatibility adapters: read `.wdproj` (XML, versions 1 to 3), `.wdscheme`
and Wave Dude 2D `configuration.json`; write `.wdproj` so projects round-trip
to WaveDude Next. Colour schemes export as `.wdscheme` too.

## `.wdproj` is the default (added 2026-11-07, the user's decision)

Save As proposes `.wdproj` and lists it first; a third format WaveDude Next cannot open was the upstream maintainer's
objection, and `.wdproj` holds the whole model (`every_kind_round_trips_and_validates`). `.wdr` still opens and saves.
The root's `Generator="WaveDude Rust <version>"` marks a file this program wrote: its display brightness is in this
program's units (05-deviations.md) and is not fitted on opening; WaveDude Next ignores the attribute.
