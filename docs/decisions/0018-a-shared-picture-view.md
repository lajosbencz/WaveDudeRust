# 0018: A shared picture view: one draw list for Skia and Canvas2D, probes read back in the browser

Status: accepted, 2026-11-12; shapes as epaint's, 2026-11-12 (below); amended by [0019](0019-egui-and-the-platform-layer.md),
2026-11-13: the Skia and Canvas2D executors and the wasm package are gone, the window and the browser draw the epaint shapes. Builds on [0016](0016-gpu-engine-in-the-browser.md)
and amends [0017](0017-a-wasm-package-and-a-web-demo.md): the web demo shows the preview's handles, labels, edge line and probe graph boxes.

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
- The shapes are a draw list (`Shape` in [`draw.rs`](../../crates/wavedude-view/src/draw.rs)). A text's `TextStyle` says its font and
  where its position lies: a label in the platform's default typeface on the alphabetic baseline over its halo,
  graph-box text in the element's (the page's) fonts from the top of its line box. Both executors are Rust and cache
  the dots' bitmap: the desktop's with Skia (`Painter` in `paint.rs`), the
  browser's with Canvas2D through web-sys (`View` in `canvas.rs`), which
  follows the Skia one shape for shape. Text is measured through `TextMetrics`: Skia's fonts on the desktop, the
  canvas's `measureText` in the browser.
- Units: the overlay is in logical (CSS) pixels; the edge line and a graph box are in device pixels of a `DeviceGrid`,
  so hairlines and text land on whole device pixels on both. The package composes groups of shapes, each with the
  device pixels per unit and its origin in device pixels of the picture; nothing crosses to the page but the canvas.
- `new View(canvas)` wraps a canvas in the document: its 2D context, its painter's bitmaps, and its computed font
  families, read once (a canvas outside the document has none and is refused).
- `Simulation.draw(view, frame, layer, width, height, dpr)` composes a picture as the preview does with nothing
  selected: the edge line, the overlay under the project's Display settings (handles and labels in the preview)
  clipped to the picture, then the boxes of the active probes measuring the layer. The viewer never edits: no
  selection, focus or pointer handling.
- The package records probes as a native run does: a `ProbeCollector` over the spec's probes, a reading queued after
  every step, drained by `engine::probes::record` (shared with the runs of `run`) with a `pending_probes()` token before
  each capture, once the token's `ready()` resolved, through the same readback wait as the capture on the GPU. Captures
  and probe readbacks are in flight together.
- Traces only grow. A frame behind the engine is run again from step 0 (0017); that rerun reads the values recorded
  already, so nothing is queued until the engine passes the recorded steps, and the collector's integrals and detector
  draws go on where they stopped. A picture's graphs are cut at the stored frame it shows (`GraphData::at`), so seeking
  either way shows the curves of that frame.
- `draw` puts the picture's pixels (`putImageData` into a bitmap of the view, scaled onto the canvas) and the groups,
  each in its own saved state, on one canvas per layer, which reaches one hairline past the picture on each side for
  the edge line; the page fits the pictures to the stage less that room, so the line always shows, and places the
  canvas. A picture is 1 to 16384 device pixels a side, half the canvas side Chrome and Firefox allow. It returns
  false, drawing nothing, while the frame is not stored. The page redraws once per frame shown and after a resize or a
  change of the device pixel ratio; a composite source's dots go through a bitmap cached until they, their place or
  the scale change, like the desktop's `Painter` does.

## Consequences

- What the preview draws changes in one place for the desktop and the browser.
- No serialisation between the package and the page: the page passes a canvas, the package draws on it.
- Canvas2D antialiases every shape; the aliased hairlines sit on whole device pixels on both, so they look alike. Text
  differs as the fonts and rasterisers do.
- With a frame divisor (the Low tier) the curves of a timeline frame between two stored frames end at the stored frame
  the picture shows; the desktop's reach the timeline frame's step.
- The traces of a long run take memory as the application's do.
- The page's drawing is checked by hand in a browser; the groups' shapes and the traces against a native run are
  tested natively.

## Alternatives rejected

- A JavaScript overlay of its own: two implementations of the same pictures.
- Burning the overlay into the picture's pixels in the package: text needs fonts there, and a hairline would scale
  with the picture instead of the screen.
- Shapes as JSON for a JavaScript executor: a second executor to keep in step with the desktop's, and a format
  between the two.
- Clearing the traces on a rerun and recording them again: the same values for the work of recording twice.

## Shapes as epaint's (added 2026-11-12)

With the window on egui, the own draw list and its executors are gone: the overlay's and a graph box's shapes are
epaint shapes in points, their text galleys laid out by epaint's fonts from the bundled Selawik (`fonts.rs`), and
epaint's tessellator makes them meshes, which the window draws with egui's wgpu renderer and an export rasterises on
the CPU into its frame (`wavedude_export::raster`): pixels are drawn where they live. `TextMetrics` stays the graph
layout's measure (epaint's fonts through `FontMetrics`, or an estimate); `DeviceGrid` puts hairlines and text on whole
device pixels and maps them back to points. epaint strokes no text, so a label's halo is its glyphs' outlines, read by
skrifa (epaint's font reader) and hinted as epaint hints the glyphs, flattened into closed paths and stroked the halo's
width in its colour under the text; where two contours' strokes overlap, the halo is darker than its colour.
