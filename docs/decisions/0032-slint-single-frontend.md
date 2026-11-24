# 0032: One frontend on Slint, rebuilt from the core

Status: accepted, 2026-11-24 (the user's decision).

## Context

- WaveDude Next's maintainer found the egui UI unmaintainable by humans and suggested Slint
  ([wavedudenext/WaveDudeNext#7](https://github.com/wavedudenext/WaveDudeNext/issues/7)).
- A spike on Slint 1.19 (`unstable-wgpu-30`) shared one wgpu device between the engine and the renderer and showed
  the field texture without CPU readback, natively and in the browser on WebGPU, live during a run. Release binary
  24.5 MB against the Material frontend's 39.1 MB. `.slint` files reload at runtime with `slint/live-preview`.

## Decision

- One frontend, `frontends/waveduderust_slint` (binary `waveduderust`), native and browser. No parallel frontends.
- Kept: `waveduderust`, `waveduderust_macros`, `waveduderust_cli`. The CLI's host services (settings and their
  store, folders, the OpenH264 download, user colour schemes) move to `waveduderust_platform`, shared with the app.
- Removed: `waveduderust_app`, `waveduderust_egui_platform`, `waveduderust_egui_preview`, `waveduderust_examples`,
  `waveduderust_icon_subset`, `waveduderust_workers`, the classic and Material frontends. Their code is reference at
  tag `v0.4.0`; [docs/rebuild.md](../rebuild.md) maps it.
- The UI is declared in `.slint` files; Rust binds callbacks, properties and models. Look, layout and shortcuts follow
  `docs/ui/`; documented divergences stand.
- `just hot` runs the app with Slint's live preview.

## Crates

| Crate | Holds |
|---|---|
| `waveduderust` | the simulation library: model, file formats, scene, engines, runner, renderer, overlays, exports |
| `waveduderust_macros` | the model derives |
| `waveduderust_cli` | headless checks, benches, scripted runs and exports |
| `waveduderust_platform` | host services shared by the CLI and the app |
| `frontends/waveduderust_slint` | the app |

- Package names are snake_case with the `waveduderust_` prefix.
- Nothing in `waveduderust` or `waveduderust_platform` depends on a UI toolkit.
- Every crate shares MAJOR.MINOR with the app; patches are per crate. `waveduderust_macros` and `waveduderust`
  publish; the rest are `publish = false`.
- Executables: `waveduderust`, `waveduderust-cli`. App id `io.github.lajosbencz.WaveDudeRust`; the settings folder is
  `APP_FOLDER`.
- Platforms are packages, not crates: the browser (Pages), Windows (installer), macOS (universal app), Linux
  (AppImage, Flathub). Packaging files live in `packaging/<target>/`. Store builds turn off the self-updater and the
  OpenH264 download through features.

## Core API

- A facade at the root: `Project`, `Simulation` (builder: backend Auto / Cpu / Gpu with the reported fallback,
  intensity tracking; `build_async` where the GPU needs it), `FieldView`, probe readings, and the background run:
  the builder's `spawn(RunOptions)` gives a `Run` (progress, wait, cancel, traces) whose frames read through a
  read-only `Frames`.
- Everything else is crate-private or behind an opt-in `unstable-<area>` feature, reached through
  `waveduderust::unstable::<area>`. Unstable items may break in any release; API diffs skip them. Default features
  never enable them. Features, not a `--cfg` flag: any dependency can turn a feature on, and a flag would have to
  reach every wasm and CI build.
- Model types, options, errors and responses are `#[non_exhaustive]`, built through `Default`, `new` or builders;
  one `thiserror` enum per area; no panics, no `anyhow` in the API.
- Typed ids (`SourceId`, `ProbeId`, `PartId`) instead of indices: stable for a session, never saved.
- No network in the library: MP4 takes an encoder provider; downloading OpenH264 is `waveduderust_platform`'s.
- Public dependencies: `half::f16`, and `wgpu` behind `gpu` to share a device (`Simulation::with_device`).
- Same project, seed and backend give the same readings.
- MSRV is the pinned toolchain. Checks come from a `justfile` that CI calls: cargo-hack per feature on native and
  wasm32, nextest, a public API diff (`cargo public-api`) that fails on a breaking change without a MINOR bump
  (`unstable-*` skipped), a version alignment check, ast-grep rules.
