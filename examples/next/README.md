# WaveDude Next's examples

The 33 example projects of WaveDude Next, as the Examples dialog ships them. Next's originals are
[`crates/wavedude-core/tests/fixtures/examples`](../../crates/wavedude-core/tests/fixtures/examples/), kept for the tests and
[`tools/compare-csharp`](../../tools/compare-csharp/). Each file here is its original with the same name, byte for byte, except:

- `Brightness` of the `Field` and `Intensity` layers: the app's Auto brightness (`fit_brightness` of
  `wavedude_core::render`, the 99.5th percentile magnitude at the palette's top, three significant digits) of that
  layer at the run's middle timeline frame (`(frames - 1) / 2`) and at its last, the smaller of the two, so the
  brighter of the two frames sets it. The run is the project as saved (its steps, stencil and sources) on this
  program's engine at full quality (`PreviewQuality::FULL`, half samples), as an export runs it. The attribute is
  inserted after `ColorScheme` where the original omits it, and left out where neither frame has anything to fit.
- `Sensitivity` of a counting or imaging detector (Example31, Example32): the original's divided by 64 (8 squared),
  as no fit exists for it. The same source swings a cell here 2.8 to 14 times as far as in Next
  ([decision 0014](../../docs/decisions/0014-sources-as-forcing-term.md)), and detections follow the field squared.

To reproduce: load each original, run it as above, capture the middle and the last frame, set each layer's
brightness to the smaller fit and divide each `Sensitivity` by 64, editing only those attribute values in the text.
