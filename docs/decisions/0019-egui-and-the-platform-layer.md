# 0019: The window on egui, one UI for the desktop and the browser through a platform layer

Status: accepted, 2026-11-13. Supersedes [0002](0002-ui-freya.md) and [0017](0017-a-wasm-package-and-a-web-demo.md);
amends [0006](0006-testing.md), [0007](0007-workspace-layout.md), [0015](0015-core-builds-for-wasm.md) and
[0018](0018-a-shared-picture-view.md).

## Context

The window ran on Freya 0.5 over Skia, with the widgets in `wavedude-widgets`, natively only. The browser had a
separate engine package (`wavedude-wasm`) and a static page (`demo/`) that showed a project's pictures without the
application around them. Freya does not build for the browser, Skia adds 15 to 20 MB and a prebuilt download per
target, and two front ends (the window and the page) drift. The preview, the exports and the page each drew the
overlay with an executor of their own.

## Decision

- The window is egui on eframe with its wgpu renderer, natively and in the browser, from one crate
  ([`wavedude-app`](../../crates/wavedude-app)). The egui family is pinned to one release (eframe, egui_tiles for the
  dock, egui_commonmark for the manual, egui_material_icons, egui_extras' image loader, egui_kittest). The own widget
  crate, the headless test crate, the wasm package and the demo page are gone.
- One UI, per-target platform modules: what differs between the desktop and the browser is a module chosen at
  compile time with one API on both targets, `native` or `web`, no runtime switch and no trait object.
  [`wavedude-app-state`](../../crates/wavedude-app-state)'s `platform` holds the clocks (`web_time`), the store of
  settings, colour schemes and the crash report, the log and the panic hook, writing a file, file watching and the
  machine's facts; the app's `ui/native.rs`, `ui/web.rs` and `ui/files/{native,web}.rs` hold the OS colour scheme, the
  window's place and the file pickers. A browser feature is built on standard web APIs behind that API.
- Files in the browser: Open and every picker upload a file's bytes (`Source::Bytes`), Save As and the image save
  download them (`Target::Download`); a file a project references and the browser cannot reach (the scene image) is
  asked for as an upload. App data goes to `localStorage` through the store; the frames past the RAM cache spill to
  the origin private file system ([0020](0020-browser-frame-spill-in-opfs.md)). No File System Access API (not in
  every browser) and no IndexedDB (the origin private file system holds the large data).
- The live run builds for wasm32: the worker is pumped from the frame loop (`RunWorker::pump`, yielding while an
  engine's future is pending), natively it keeps its thread. One engine exists at a time; a run frees its engine
  before building the next.
- The engines share the renderer's wgpu device (`GpuContext::from_device`), requested with the adapter's limits where
  it runs compute shaders. A device without compute (WebGL2) runs the CPU engine.
- A run on the GPU engine colours its pictures on that device in a paint callback (`wavedude_core::render::gpu`, the
  CPU's tables, outline, dithering and palette), from the frames the engine keeps there, without a readback; a run
  on the CPU engine keeps the presenter thread and uploads RGBA8 textures written in place.
- What a picture shows besides its pixels is epaint shapes made by [`wavedude-view`](../../crates/wavedude-view) in
  the bundled Selawik (0018, Shapes as epaint's). The window draws their meshes with egui's renderer; an export
  rasterises the same meshes on the CPU into its frame (`wavedude_export::raster`), the same bytes on every machine.
- The window's tests drive it headless with egui_kittest on wgpu, found through egui's AccessKit tree by role and
  label, with no image snapshots; only the manual's figures render, on Mesa's lavapipe.

## Consequences

- One code path for both targets; a target difference is a compile error on the other side until both modules
  have it.
- The browser app is built by [`tools/build-web.sh`](../../tools/build-web.sh) and deployed to GitHub Pages by
  [`pages.yml`](../../.github/workflows/pages.yml) after a release.
- No Skia: no prebuilt download per target and no build from source where none exists.
- An export's overlays need no GPU and are the same bytes on every machine.
- The look follows WaveDude Next's box model through egui's visuals and the app's own widgets
  (`ui/theme.rs`), where egui's defaults differ.

## Alternatives rejected

- Keeping Freya natively with a separate browser front end: two UIs that drift, and the page lacks every edit.
- egui_plot for the graph boxes: the view's shapes already lay them out for the preview and the exports.
- Exports rendered offscreen through egui-wgpu: pixels differed across adapters (up to 2/255), and a machine without
  a GPU needed a software adapter; the CPU rasteriser is exact and fast enough
  ([`docs/measurements.md`](../measurements.md)).
- A runtime platform trait: both targets are known at compile time, and a trait object buys nothing.
