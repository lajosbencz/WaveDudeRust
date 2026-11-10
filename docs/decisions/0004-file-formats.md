# 0004 Project file format and compatibility adapters

Status: accepted, 2026-11-05; revised 2026-11-10

## One project format, `.wdproj` (2026-11-10, the user's decision)

WaveDudeRust has one project file format, WaveDude Next's `.wdproj`; the zip container `.wdr` is dropped. A project
must round-trip between the two programs without loss, which two formats cannot promise and a third format WaveDude
Next cannot open defeats. The model and the engine keep their physical domain; what `.wdproj` cannot carry (whole-number positions and
coating depths, brightness units) is converted in the adapter alone, and the simulation never sees the file format.

The model is format-agnostic (serde). Files:

- `.wdproj` (XML, versions 1 to 4) read and written; the adapter (`io/wdproj.rs`, `io/fields.rs`) holds every
  transformation: the 9-point frequency rescale and the brightness units (spec/60, spec/05).
- `.wdscheme` colour schemes read and written.
- Wave Dude 2D `configuration.json` read (import as an unsaved project).

Save and Save As write `.wdproj` (or `.xml`, the same XML). A display brightness is stored in WaveDude Next's units,
the model's value times 8, so nothing is fitted on opening.
