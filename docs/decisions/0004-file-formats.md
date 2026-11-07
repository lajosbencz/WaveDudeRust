# 0004 Native project file and compatibility adapters

Status: accepted, 2026-11-05

The model is format-agnostic (serde). Native file: `.wdr`, a zip container
holding `project.json` (versioned by a `format` field), `scene.png` when the
scene has an image, and `schemes/*.json` for project colour schemes. The
README says it is a zip; renaming it opens it in any archive tool.

Compatibility adapters: read `.wdproj` (XML, versions 1 to 3), `.wdscheme`
and Wave Dude 2D `configuration.json`; write `.wdproj` so projects round-trip
to WaveDude Next. Colour schemes export as `.wdscheme` too.

## `.wdproj` is the default (added 2026-11-07, the user's decision)

Save As proposes `.wdproj` and lists it first; a third format WaveDude Next cannot open was the upstream maintainer's
objection, and `.wdproj` holds the whole model (`every_kind_round_trips_and_validates`). `.wdr` still opens and saves.
The root's `Generator="WaveDude Rust <version>"` marks a file this program wrote: its display brightness is in this
program's units (05-deviations.md) and is not fitted on opening; WaveDude Next ignores the attribute.
