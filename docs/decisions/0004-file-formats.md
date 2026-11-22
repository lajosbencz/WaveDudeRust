# 0004 Project file format and compatibility adapters

Status: accepted, 2026-11-05; revised 2026-11-10, 2026-11-14

## One project format, `.wdproj` (2026-11-10, the user's decision)

WaveDudeRust has one project file format, WaveDude Next's `.wdproj`; the zip container `.wdr` is dropped. A project
must round-trip between the two programs without loss, which two formats cannot promise and a third format WaveDude
Next cannot open defeats. The model and the engine keep their physical domain; what `.wdproj` cannot carry (whole-number positions and
coating depths) is converted in the adapter alone, and the simulation never sees the file format.

The model knows no file format (decision 0013). Files:

- `.wdproj` (XML, versions 1 to 4) read and written; the adapter ([`io/wdproj.rs`](../../crates/waveduderust/src/io/wdproj.rs), [`io/fields.rs`](../../crates/waveduderust/src/io/fields.rs)) holds every
  transformation: the 9-point frequency rescale and whole-number rounding ([spec/60](../spec/60-file-formats.md), [spec/05](../spec/05-deviations.md)).
- `.wdscheme` colour schemes read and written.
- Wave Dude 2D `configuration.json` read (import as an unsaved project).

Save and Save As write `.wdproj` (or `.xml`, the same XML). A display brightness and a detector's sensitivity are
stored as the model holds them, and nothing is fitted on opening.

## Brightness unconverted (2026-11-14, the user's decision)

`Brightness` and `Sensitivity` were converted by fixed factors of 8 and 64 between the file and the model. Our source
radiates its Amplitude exactly; Next's radiates a scene-dependent amount, 2.8 to 14 times less than ours across the 33
examples (decision 0014), so Next's numbers are hand-tuned for Next and no fixed factor maps them. The file now holds the
model's value, a missing `Brightness` is the model's layer default, and the bundled examples were divided by 8 and 64
once, so they look as before. A project from Next opens at its own number in our units.

The app and the web build ship only what [`examples`](../../examples/) holds. Next's 33 projects ship as our copies
(`ExampleNN_*.wdproj` in the topic folders): each layer's `Brightness` is our Auto fit of the brighter of the middle and the last frame, a `Sensitivity`
the original's divided by 64, every other byte Next's ([`examples/next.md`](../../examples/next.md)).
Next's originals stay in [`crates/waveduderust/tests/fixtures/examples`](../../crates/waveduderust/tests/fixtures/examples/), for the tests and the comparison
with Next alone.
