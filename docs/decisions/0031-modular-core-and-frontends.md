# 0031: A simulation library, a headless app and thin frontends

Status: accepted, 2026-11-21 (the user's decisions). Supersedes [0007](0007-workspace-layout.md); amends
[0019](0019-egui-and-the-platform-layer.md) (the classic window moves onto egui-mineral, the crates below replace its
list).

## Context

- The crates grew around two windows. `wavedude-core` exposes its implementation: every module is `pub`, about 560
  `pub fn`, no `#[non_exhaustive]`, so any refactor is a breaking change.
- The classic window draws its own widgets and look (`ui/theme.rs`); the Material window uses egui-mineral.
- Dioxus `subsecond` (dx 0.7.10) patches only the crate that holds `main.rs`: UI in a library, or in the `lib.rs`
  of the binary's own package, is not patched. egui-mineral's `unstable-hotpatch` wraps `Runtime::pass` in
  `subsecond::call`, which is enough for a bin-only crate.
- The app ships to the browser, Windows, macOS and Linux (AppImage; Flathub reaches GNOME Software and KDE Discover).

## Decision

### Crates

| Crate | Holds | From |
|---|---|---|
| `waveduderust` | the simulation library: model, file formats, scene, engines, runner, renderer, overlays, exports | `wavedude-core`, `wavedude-view`, `wavedude-export` |
| `waveduderust_macros` | the model derives | `wavedude-model-derive` |
| `waveduderust_examples` | the example projects, embedded | `wavedude-examples` |
| `waveduderust_app` | the headless application: state, commands, forms and dialogs as data, the canvas's view model (`preview`), platform capabilities, the OpenH264 download | `wavedude-app-state`, `wavedude-frontend` |
| `waveduderust_workers` | the browser's run and spill workers, one crate, a binary each; the spill protocol is `waveduderust_app::spill` (the run worker depends on the app, so the app cannot depend on this crate) | `wavedude-run-worker`, `wavedude-spill-worker` |
| `waveduderust_cli` | headless checks, benches, scripted runs and exports | `wavedude-cli` |
| `waveduderust_egui_preview` | what draws the simulation in both egui frontends: picture paint callbacks, overlays, graph boxes, timeline, gradient editor, drawn icons | `wavedude-egui` |
| `waveduderust_egui_platform` | egui start-up (native, web) and the OS integration: input, keys, window placement, OS theme, icon fonts, file dialogs, clipboard | `wavedude-egui` |
| `waveduderust_icon_subset` | the build step that subsets the icon fonts to the icons a frontend uses; a build-dependency, so egui stays off the host build | `wavedude-icon-subset` |
| `frontends/classic` (`waveduderust_classic`, bin `waveduderust`) | the WaveDude Next replica on egui-mineral's core: its own `Stylekit` and widgets (`src/kit/`), the dock and panes | `wavedude-app` |
| `frontends/material` (`waveduderust_material`, bin `waveduderust-material`) | the Material window on `egui_mineral_material` as it is | `wavedude-mineral` |
| `frontends/gnome`, `frontends/kde` (bins `waveduderust-gnome`, `waveduderust-kde`) | native frontends, gtk4 + libadwaita and cxx-qt + Kirigami, in the desktop's look | new |

- Two egui frontends, each its own program: classic (the default) and Material; no looks as features, no switch
  at runtime. What both need and does not paint lives in `waveduderust_app` (shell logic as data) or
  `waveduderust_egui_preview` and `waveduderust_egui_platform` (egui); what paints stays in the frontend, even if similar in both.
- Frontends are bin-only crates (no `lib.rs`), so `dx serve --hotpatch --features egui_mineral/unstable-hotpatch`
  patches every UI function; `main.rs` is touched before `dx serve` (dx 0.7.10 relink bug). Their tests are
  `#[cfg(test)]` modules.
- Rules: nothing in `waveduderust`, `waveduderust_app` or `waveduderust_workers` depends on egui, gtk, qt or
  egui-mineral; frontends depend on `waveduderust_app` and their toolkit, never on each other.

### The library's API

- A facade at the root: `Project`, `Simulation` (builder: backend Auto / Cpu / Gpu with the reported fallback,
  intensity tracking; `build_async` where the GPU needs it), `FieldView`, probe readings, and the background run:
  the builder's `spawn(RunOptions)` gives a `Run` (progress, wait, cancel, traces) whose frames read through a
  read-only `Frames`. The background run is stable API; the live worker the editor drives (`RunWorker`, `Goal`,
  `Pump`, the store's write side, spill) is `unstable-run`, as it follows the UI.
- Everything else is crate-private or behind an opt-in `unstable-<area>` feature (`unstable-engine`: engine,
  render, rng, sync; `unstable-scene`; `unstable-run`: spec and the live worker; `unstable-export`, `unstable-view`
  until step 5 hardens them; `unstable-reflect`: the model's field reflection), reached through
  `unstable::<area>`, the live worker's items in `run` and the reflection in `model` (`model::fields`, each
  type's `Field` and `Edit` enums and accessor consts). Unstable items may break in any release; API diffs skip
  them. Default features never enable them. Features, not a `--cfg` flag: any dependency can turn a feature on,
  but only first-party crates use them and a flag would have to reach every dx, wasm and CI build.
- Model types, options, errors and responses are `#[non_exhaustive]`, built through `Default`, `new` or builders;
  one `thiserror` enum per area; no panics, no `anyhow` in the API.
- Typed ids (`SourceId`, `ProbeId`, `PartId`) instead of indices: stable for a session, given on add and load,
  never saved. The seed stays Next's `i32`.
- The display settings (brightness, handles, labels) leave `Project` for `model::View`, which the file still
  carries as `Display`; io loads and saves the pair, the core takes brightness through `RunOptions`, a layer's
  settings or `&View`. Done.
- No network in the library: MP4 takes an encoder provider; downloading OpenH264 is `waveduderust_app`'s.
- Public dependencies: `half::f16`, and `wgpu` behind `gpu` to share a device (`Simulation::with_device`).
- Same project, seed and backend give the same readings.

### Versions and tooling

- Every crate shares MAJOR.MINOR with the app; patches are per crate. `waveduderust_macros` and `waveduderust` publish (names held by 0.0.0 placeholders); the rest are `publish = false`.
- MSRV is the pinned toolchain (1.100).
- Checks come from a `justfile` that CI calls (egui-mineral's pattern): cargo-hack per feature on native and
  wasm32, nextest, a public API diff (`cargo public-api`) that fails on a breaking change without a MINOR bump
  (`unstable-*` skipped), a version alignment check, ast-grep rules for what clippy cannot say.

### Names and packaging

- Executables: `waveduderust`, `waveduderust-gnome`, `waveduderust-kde`, `waveduderust-cli`.
- App id `io.github.lajosbencz.WaveDudeRust` (Flathub verifies `io.github.<user>.<repo>` against the repository's
  name), switched with the restructure. The settings folder is `APP_FOLDER` and does not move.
- Platforms are packages, not crates: the browser (Pages), Windows (installer), macOS (universal app), Linux AppImage
  carry the egui app; Flathub carries the egui app and later the GNOME and KDE apps. Packaging files live in
  `packaging/<target>/`. Store builds turn off the self-updater and the OpenH264 download through features.

### Order

1. Docs (this record), then the tooling.
2. Hot-patch check on the Material window as a bin-only crate.
3. Merges and renames into the crates above; `frontends/egui` hosts the Material look. Executables keep their
   names until step 7: installers, file types and the updater name them.
4. The library's facade and the `unstable-*` gates. The facade first: `Simulation` with `Backend`, `FieldView`,
   probe traces as `(probe, trace)` pairs, and `run::engines(Backend)` for a run worker; native only until
   `build_async`. Then `Run`, then `Frames`, then the gates and the consumers' paths; the API baseline of the
   default features closes the step: model, io, quality, the facade and `run`'s background run, nothing that
   names `unstable`. The model's field reflection is most of it, for step 5.
5. Model hardening (non-exhaustive types, ids, encoder provider).
6. The two egui frontends: the shared shell logic moves down into `waveduderust_app`, and `waveduderust_egui` splits
   into `waveduderust_egui_preview` and `waveduderust_egui_platform`; `frontends/egui` becomes
   `frontends/material`; `crates/wavedude-app` moves to `frontends/classic` and takes the shared shell, then its
   hand-rolled widgets give way to its kit on egui-mineral's core, piece by piece, each checked against WaveDude
   Next. Then the display split (done).
7. Packaging; then GNOME, KDE.

## Consequences

- A refactor inside the library is not a breaking change; the API diff shows what is.
- One egui program per platform carries both looks; its size grows by a kit.
- Library edits rebuild and restart under dx; frontend edits patch live.
- The classic look is rebuilt on egui-mineral widgets; until step 6 the old classic window stays.

## Alternatives rejected

- One crate per look: switching looks means another program, and every package ships two.
- The classic look as a Material theme: colours and corners only, the components keep their MD3 shapes.
- Per-view hot-patch boundaries (`HotFn` per pane): not needed once the UI is in the bin crate.
- Publishing now: the API is not ready; the guardrails run without publishing.
