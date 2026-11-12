# 0017: A wasm package and a web demo

Status: accepted, 2026-11-12. Builds on [0015](0015-core-builds-for-wasm.md) and [0016](0016-gpu-engine-in-the-browser.md).

## Context

With the core and its GPU engine building for wasm32, a page can run a project in the visitor's browser, which is the
quickest way to try the program and a check that the engine runs on a second platform. The Examples dialog's list lived
in `wavedude-app-state`, with the fixtures embedded in the application; a demo needs the same list and files without the
application's state, and an engine package must not carry fifty-three projects nobody asked for.

## Decision

- [`wavedude-wasm`](../../crates/wavedude-wasm) is the engine and nothing else: `Simulation.open(text, tier)` and `openOn(gpu, text, tier)`,
  `stepToward`, `render`, `requestGpu`. It opens one `.wdproj` text, builds the run spec through `RunRequest` and `PreviewDefaults`
  as the application does (a tier is the same quality and dither depth in both), and keeps the stored frames of the
  timeline. No example, no file dialog, no DOM: a page or another tool owns those. `session.rs` is plain Rust and tested
  natively on the CPU engine; `lib.rs` is the JavaScript surface.
- A project whose scene image is a file beside the project, not embedded, is refused with a message naming the file:
  a browser has no folder to read it from.
- Frames are stepped to on demand. The cache is keyed by stored frame, holds 512 MiB by default (`budgetBytes`), and
  evicts the frame farthest from the one stored last. A frame behind the engine's step is run again from step 0, as
  the application's worker does; forward seeks step on from where the engine is.
- The GPU engine on wasm waits for no readback (0016): the package never reads probes or calls
  `prepare_frame`, and awaits `PendingCapture::ready()` before `finish_capture`. `requestGpu()` returns one device a page
  passes to every `openOn`, which only borrows it, so one device serves every project (wasm-bindgen consumes an optional object
  argument, hence a function apart from the CPU's `open`); without WebGPU, or when the
  engine cannot be built on it, the CPU engine runs, and the engine in use and whether the GPU fell back are getters.
- Rayon has no threads in the browser here: its loops run on the calling thread. Threads would need shared memory and
  cross-origin isolation, which GitHub Pages cannot set.
- [`wavedude-examples`](../../crates/wavedude-examples) owns the example list (titles, topics, descriptions, the embedded
  files), moved from `wavedude-app-state`; the application and the demo build both use it. Its binary writes the files
  and `catalog.json` (topics, examples, tiers with the application's English titles from the string table), so the
  package never contains an example and the page fetches them over HTTP.
- [`demo/`](../../demo) is a static page of ES modules, no bundler and no npm: [`demo/build.sh`](../../demo/build.sh) builds the package, runs
  `wasm-bindgen --target web`, writes the examples and copies the page. It refuses a wasm-bindgen CLI whose version
  differs from the crate's in `Cargo.lock`.
- [`pages.yml`](../../.github/workflows/pages.yml) builds the site and deploys it to GitHub Pages. `release.yml` calls it after `publish`, so the demo
  shows the latest release; it can also be run by hand. The build has no cache, like the packages: its output runs in
  visitors' browsers. A pull request's CI builds neither the page nor the site; the wasm32 job lints the package.

## Consequences

- The demo is the engine and the picture only: no edits, no handles, no export, no save.
- A bump of the wasm-bindgen crate needs the same CLI version; the workflow reads it from `Cargo.lock`, so only a local
  install has to follow.
- No npm package is published. One would be a build step over the same `pkg` folder.
- The page's code is checked by hand in a browser; nothing in CI runs it.

## Alternatives rejected

- Embedding the examples in the package: a reusable engine would carry them, and the page would save a fetch only
  for a demo's convenience, at the cost of every other user of the package.
- Two example lists, one for the application and one for the page: they would drift.
- Wasm threads: no cross-origin isolation on GitHub Pages.
- A bundler or npm packaging: a page of three files and a generated folder needs neither.
