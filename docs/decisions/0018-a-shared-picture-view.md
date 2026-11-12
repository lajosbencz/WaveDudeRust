# 0018: A shared picture view: one draw list for Skia and Canvas2D, probes read back in the browser

Status: accepted, 2026-11-12. Builds on [0016](0016-gpu-engine-in-the-browser.md) and amends [0017](0017-a-wasm-package-and-a-web-demo.md):
the web demo shows the preview's handles, labels, edge line and probe graph boxes.

## Context

The web demo showed the bare pictures, while the desktop preview draws over them: the edge line, the handles and labels
of the sources, parts and probes, and the graph box of each active probe with its curve up to the frame shown
([`docs/ui/40-preview-pane.md`](../ui/40-preview-pane.md)). That drawing was Skia code in the widgets, its placement in the application, and the probe
readings could not be read back on wasm32 without blocking. A second implementation in JavaScript would drift from
the desktop's.

## Decision

- [`wavedude-view`](../../crates/wavedude-view) holds what a picture shows besides its pixels, without a toolkit: the
  overlay model and its shapes, the edge line (`edge_color`, `edge_line`), the graph box's layout and shapes, and which
  boxes a picture shows and where (`shows_box`, `box_size`, `box_origin`, `box_on_picture`). The application, the
  widgets and the wasm package use these; none keeps a copy.
- The shapes are a draw list (`Shape` in [`draw.rs`](../../crates/wavedude-view/src/draw.rs)). The desktop executes it with Skia
  (`draw_shapes` in [`paint.rs`](../../crates/wavedude-widgets/src/paint.rs)), the page with Canvas2D ([`demo/draw.js`](../../demo/draw.js)), which
  follows the Skia executor shape for shape. Text is measured through `TextMetrics`: Skia's fonts on the desktop, the
  page's `measureText` in the browser, passed to the package as a function.
- Units: the overlay is in logical (CSS) pixels; the edge line and a graph box are in device pixels of a `DeviceGrid`,
  so hairlines and text land on whole device pixels on both. The package returns JSON (serde, colours as `#rrggbbaa`,
  without the desktop's antialias flags): groups of shapes, each with the device pixels per unit and its origin in
  device pixels of the picture.
- `Simulation.overlay(frame, layer, width, height, dpr, measure)` composes a picture as the preview does with nothing
  selected: the edge line, the overlay under the project's Display settings (handles and labels in the preview)
  clipped to the picture, then the boxes of the active probes measuring the layer. The viewer never edits: no
  selection, focus or pointer handling.
- The package records probes as a native run does: a `ProbeCollector` over the spec's probes, a reading queued after
  every step, drained by `engine::probes::record` (shared with the runs of `run`) before each capture, once
  `pending_probes().ready()` resolved, through the same readback wait as the capture on the GPU. Captures and probe
  readbacks are in flight together.
- Traces only grow. A frame behind the engine is run again from step 0 (0017); that rerun reads the values recorded
  already, so nothing is queued until the engine passes the recorded steps, and the collector's integrals and detector
  draws go on where they stopped. A picture's graphs are cut at the stored frame it shows (`GraphData::at`), so seeking
  either way shows the curves of that frame.
- The page draws the JSON on a canvas over each picture and its caption, reaching one hairline past it for the edge
  line; the pictures are fitted to the stage less that room, so the line always shows. It redraws once per frame shown
  and after a resize or a change of the device pixel ratio; a composite source's dots go through a bitmap cached until
  the picture's size changes, like the desktop's `DotCache`.

## Consequences

- What the preview draws changes in one place for the desktop and the browser.
- The shapes cross as JSON once per frame shown and layer; a composite source's dots are in it each time.
- Canvas2D antialiases every shape; the aliased hairlines sit on whole device pixels on both, so they look alike. Text
  differs as the fonts and rasterisers do.
- With a frame divisor (the Low tier) the curves of a timeline frame between two stored frames end at the stored frame
  the picture shows; the desktop's reach the timeline frame's step.
- The traces of a long run take memory as the application's do.
- The page's drawing is checked by hand in a browser; the JSON's structure and the traces against a native run are
  tested natively.

## Alternatives rejected

- A JavaScript overlay of its own: two implementations of the same pictures.
- Burning the overlay into the picture's pixels in the package: text needs fonts there, and a hairline would scale
  with the picture instead of the screen.
- `serde-wasm-bindgen` for the shapes: a new dependency where `serde_json` and `JSON.parse` serve.
- Clearing the traces on a rerun and recording them again: the same values for the work of recording twice.
