# Conventions

What new code follows so that it fits the code already here. Each rule names one place that shows it. Of two fixes
with the same behaviour, the one that deletes or merges code wins over one that adds a guard.

## Toolchain and checks

The tools, their commands and their configuration are in [`CONTRIBUTING.md`](../CONTRIBUTING.md), Tools; run them before a
push. What they hold the code to:

- Edition 2024, resolver 3, MSRV `rust-version = "1.100"`, all in the root [`Cargo.toml`](../Cargo.toml) `[workspace.package]`;
  crates inherit them with `edition.workspace = true` etc. ([`crates/waveduderust/Cargo.toml`](../crates/waveduderust/Cargo.toml)).
- [`rustfmt.toml`](../rustfmt.toml): `hard_tabs = true`, `max_width = 120`, `use_small_heuristics = "Max"`. Indent with tabs,
  including in TOML arrays ([`frontends/waveduderust_slint/Cargo.toml`](../frontends/waveduderust_slint/Cargo.toml), `file-associations`).
- The lint policy is the `[workspace.lints]` table of the root [`Cargo.toml`](../Cargo.toml), inherited by every crate
  (`[lints] workspace = true`); each listed lint fails the build. The cast lints and `missing_docs` are left out on
  purpose. A needed `#[expect]` (or `#[allow]`) carries a `reason`
  ([`frontends/waveduderust_slint/src/export.rs`](../frontends/waveduderust_slint/src/export.rs), `formats`).
- A doc comment links only to public items and wraps generic names like `<n>` in backticks, so `cargo doc` stays
  free of warnings.
- An exception in [`_typos.toml`](../_typos.toml) is a format or literature term, an identifier or a non-English file, named in
  the comment above it.

## Crates and dependency direction

The crates are listed in [`WORKSPACE.md`](../WORKSPACE.md) ([decision 0032](decisions/0032-slint-single-frontend.md)).

- `waveduderust`: model, `.wdproj` and scheme IO, scene painter, engines (CPU, wgpu behind feature `gpu`),
  runs and frame store (feature `run`), renderer. Its stable API is the model, IO, quality, `Simulation` and the
  background `run::Run` with its `Frames`; the rest is under `waveduderust::unstable` behind `unstable-*` features,
  which first-party crates enable. It builds for `wasm32-unknown-unknown` with any of its features:
  code there uses no threads, no std clocks (`web_time`'s instead, which the wasm32 lint run enforces) and no machine
  queries; on wasm32 nothing blocks, the GPU engine is built by an async call, the live run is pumped by its caller
  (`RunWorker::pump`, which says whether work is left, waits for readiness or is idle, waking the caller's waker;
  decision 0024), and its frame stores hold their frames in memory up to the caller's budget and spill the rest
  through the caller's backend (decision 0020); the live run on a thread of its own, the spill file and the machine's
  memory and threads are native only ([decision 0015](decisions/0015-core-builds-for-wasm.md),
  [decision 0016](decisions/0016-gpu-engine-in-the-browser.md)). The run spec and the preview tiers are not in `run`
  ([`spec.rs`](../crates/waveduderust/src/spec.rs), [`quality.rs`](../crates/waveduderust/src/quality.rs)). Depends on no
  workspace crate.
- `waveduderust_macros`: the proc-macros `Fields` and `Word` of the model types (`syn`, `quote`); core re-exports
  them as `waveduderust::model::{Fields, Word}`. Depends on no workspace crate.
- `waveduderust::unstable::view`: what the preview and the exports draw over a picture, without a toolkit: the overlay
  model and its shapes, the probe graph box's layout and shapes, as epaint shapes in points, their text laid out by
  epaint's fonts; hairlines and text land on whole device pixels through a `DeviceGrid`
  ([`view/draw.rs`](../crates/waveduderust/src/view/draw.rs)). The bundled Selawik fonts
  ([`view/fonts.rs`](../crates/waveduderust/src/view/fonts.rs)) set the text over pictures. Placement the preview
  needs (which graph boxes a picture shows, where) lives here too ([decision 0018](decisions/0018-a-shared-picture-view.md)).
  Behind the `view` feature; builds for `wasm32-unknown-unknown`.
- `waveduderust::unstable::export`: sinks, overlays, CSV, the export job, the H.264 encoder seam (`h264`: the MP4
  muxer takes any `H264Encoder`; `mp4-mux` is the muxer alone, on `shiguredo_mp4`, `mp4` adds OpenH264). Behind the
  `export` feature, which enables `view` and `run`; rasterises the overlays' meshes on the CPU into the frame.
- `waveduderust_platform`: host services without a UI toolkit, shared by the CLI and the app: `settings` and their
  `store`, the program's folders (`AppDirs`), `codec` (the OpenH264 download), `user_schemes`, `export` (a project's
  full-quality run feeding the export job, native only), `machine` (memory and threads; from the navigator in the
  browser). Depends on core.
- `waveduderust_cli`: binary only (no `lib.rs`); core and platform. The global `--config-dir <DIR>` puts the app's
  folders under `DIR` (`AppDirs::under`); its tests pass an empty one, so they never read the user's.
- `waveduderust_slint` (program `waveduderust`): the window, natively and in the browser
  ([`tools/build-web.sh`](../tools/build-web.sh)). `ui/*.slint` declares the window, one file per pane, dialog or
  widget; `src/*.rs` binds callbacks, properties and models, one module per concern (`document`, `live`, `playback`,
  `tree`, `form`, `messages`, `settings`, `export`, `files`, `examples`, `gpu`; `web` in the browser). One wgpu device
  serves the engine and the window ([`src/gpu.rs`](../frontends/waveduderust_slint/src/gpu.rs)). Depends on core and
  platform.

## Where new things go

- A step index is a `Step`, a timeline position a `Frame`, and the arithmetic between them lives in `Timeline`
  alone ([`crates/waveduderust/src/model/timeline.rs`](../crates/waveduderust/src/model/timeline.rs)): no `* visualize_every` elsewhere.
- Model property: a field on the `model` type with its derive attributes (`#[field(range = ranges::X)]` with the
  constant in [`crates/waveduderust/src/model/ranges.rs`](../crates/waveduderust/src/model/ranges.rs), or `dependent`, `nested`, `skip`), its clamp in
  `Normalize` ([`model/normalize.rs`](../crates/waveduderust/src/model/normalize.rs)), an `Attr` in the `Table` of the type in
  [`crates/waveduderust/src/io/wdproj.rs`](../crates/waveduderust/src/io/wdproj.rs) (`impl Table for Simulation`). The properties form
  reads the reflected fields ([`frontends/waveduderust_slint/src/form.rs`](../frontends/waveduderust_slint/src/form.rs)). Tests that fail
  otherwise: `every_field_of_a_table_type_is_an_attribute_or_skipped` (the file), `every_persisted_number_has_its_bounds_named`
  and `every_bound_is_enforced_by_normalize` (bounds, clamp). A new enum derives `Word`. The derive's `Fields::visit`
  walks every type and word reachable from `Project`, so these tests need no hand-kept list of types.
- Model edit: the open document's `edit` with a closure over the project
  ([`frontends/waveduderust_slint/src/document.rs`](../frontends/waveduderust_slint/src/document.rs)), which records the undo step; a
  property edit goes through the model's generated typed edit (`Edit::Probe(0, ProbeEdit::BoxWidth(500))`). What an
  edit redoes in the live run follows [`docs/ui/60-live-editing-and-runs.md`](ui/60-live-editing-and-runs.md), Edit classes.
- Export format: a variant of `ExportFormat` with its line in `ALL` and `extensions`
  ([`crates/waveduderust/src/export/mod.rs:34`](../crates/waveduderust/src/export/mod.rs#L34)), a module under [`src/sinks/`](../crates/waveduderust/src/export/sinks/) implementing `FrameSink`
  ([`sinks/mod.rs:27`](../crates/waveduderust/src/export/sinks/mod.rs#L27)), and its arm in `sinks::open`; the matches are exhaustive, so the compiler lists the rest.
- CLI subcommand: a variant of `Command` in [`crates/waveduderust_cli/src/main.rs:35`](../crates/waveduderust_cli/src/main.rs#L35) (clap derive), a module with
  `pub(crate) struct Args` and `pub(crate) fn run(args: &Args) -> Result<()>` ([`crates/waveduderust_cli/src/export.rs:22`](../crates/waveduderust_cli/src/export.rs#L22), [`107`](../crates/waveduderust_cli/src/export.rs#L107)),
  and its arm in `main`.
- Widget: one `.slint` file per pane, dialog or reusable widget in
  [`frontends/waveduderust_slint/ui/`](../frontends/waveduderust_slint/ui/), its logic in the Rust module of the same concern.
- Project format quirks (9-point frequency units, versions, sparse writing) stay in [`crates/waveduderust/src/io/`](../crates/waveduderust/src/io/)
  ([`io/wdproj.rs`](../crates/waveduderust/src/io/wdproj.rs)); the model holds this program's units.
- Example projects are `.wdproj` files under `examples/<topic>/`, the folder being the dialog's tab, named with their series prefix
  ([`examples/guides/07-xor.wdproj`](../examples/guides/07-xor.wdproj)), embedded by the app's build script ([`frontends/waveduderust_slint/build.rs`](../frontends/waveduderust_slint/build.rs), listed by
  [`src/examples.rs`](../frontends/waveduderust_slint/src/examples.rs))
  ([`EXAMPLES.md`](../EXAMPLES.md)).
  [`examples`](../examples/) is all the app and the web build ship; Next's originals in
  [`crates/waveduderust/tests/fixtures/examples`](../crates/waveduderust/tests/fixtures/examples/) are for the tests and the comparison alone, shipped as the copies
  `ExampleNN_*.wdproj` ([`examples/next.md`](../examples/next.md)).

## Naming

- Slint components are UpperCamelCase, properties and callbacks kebab-case; Rust sees them snake_case
  (`preview-stacked` -> `get_preview_stacked`).
- Test names are snake_case sentences stating the behaviour (`an_edit_is_undone_and_redone_and_a_no_op_leaves_no_step`,
  [`frontends/waveduderust_slint/src/document.rs`](../frontends/waveduderust_slint/src/document.rs)); older ones are short noun phrases
  (`limits`, `probe_rules`).
- Tunables are named `const`s with a `///` line (`KEPT`, [`frontends/waveduderust_slint/src/log.rs`](../frontends/waveduderust_slint/src/log.rs)).
- New or renamed packages are snake_case with the `waveduderust_` prefix.

## Visibility and exports

- Each `lib.rs` opens with a `//!` summary, declares `pub mod`s and re-exports the main types with `pub use`
  ([`crates/waveduderust/src/view/mod.rs`](../crates/waveduderust/src/view/mod.rs), [`crates/waveduderust/src/export/mod.rs`](../crates/waveduderust/src/export/mod.rs)).
- Internal helpers are private modules (`mod bits; mod yuv;` in export) or `pub(crate)` items (`pub(crate) fn mur_weights`,
  [`crates/waveduderust/src/engine/mur.rs:32`](../crates/waveduderust/src/engine/mur.rs#L32)). The CLI and the app are binaries, so their
  shared items are `pub(crate)`.
- Imports: `std`, then external and workspace crates, then `crate::`/`super::`, as blank-line separated groups
  ([`crates/waveduderust/src/io/wdproj.rs`](../crates/waveduderust/src/io/wdproj.rs)).

## Errors and panics

- Library crates define `thiserror` enums, messages lower-case but for proper nouns (`ExportError`,
  [`crates/waveduderust/src/export/mod.rs:199`](../crates/waveduderust/src/export/mod.rs#L199); `Error`,
  [`crates/waveduderust_platform/src/export.rs`](../crates/waveduderust_platform/src/export.rs); `IoError`,
  [`crates/waveduderust/src/io/mod.rs:95`](../crates/waveduderust/src/io/mod.rs#L95)) and a `Result` alias ([`io/mod.rs:110`](../crates/waveduderust/src/io/mod.rs#L110), [`crates/waveduderust/src/export/mod.rs:243`](../crates/waveduderust/src/export/mod.rs#L243)).
- `anyhow` is for binaries and setup: every `waveduderust-cli` module.
- A broken invariant panics through `expect` whose message states the invariant (`.expect("settings serialise")`,
  [`crates/waveduderust_platform/src/settings.rs`](../crates/waveduderust_platform/src/settings.rs)). Bare `unwrap()` is for tests:
  `clippy::unwrap_used` is denied in `waveduderust` outside its tests.
- A mutex is taken through `waveduderust::unstable::sync::lock`, which takes a poisoned lock too: the guarded values
  are whole between operations and the panic was reported where it happened. A `Condvar` wait does the same with
  `.unwrap_or_else(PoisonError::into_inner)` (`RunWorker`, [`crates/waveduderust/src/run/worker.rs`](../crates/waveduderust/src/run/worker.rs)).
- No silent fallbacks: a fallback is logged and stated. The GPU engine falls back to the CPU with a warning and
  hands back why ([`crates/waveduderust/src/engine/mod.rs:124`](../crates/waveduderust/src/engine/mod.rs#L124)); a settings value that cannot be read is logged with its
  key and reads as its default ([`crates/waveduderust_platform/src/settings.rs`](../crates/waveduderust_platform/src/settings.rs)); a rejected edit is
  logged and shown in its form.
- Out-of-range values are clamped by `normalize` after load, import, every edit and before a run; it is
  idempotent ([`model/normalize.rs`](../crates/waveduderust/src/model/normalize.rs), `Settings::normalize`).

## Logging

- The UI is translated, the log is not: log lines (status bar, Messages pane, stderr) are English. A dialog may
  append an error's own text untranslated.
- `tracing` macros, always path-qualified (`tracing::warn!`), messages lower-case clauses naming the file or
  value; a line the status bar shows to the user is worded as WaveDude Next words it (`Opened <path>`).
- Levels: `error` for a failed user action, `warn` for a fallback or a dropped request, `info` for what the
  user did or got, `debug` for diagnostics, `trace` for every edit as `old -> new`.
- Channels of the app ([`frontends/waveduderust_slint/src/log.rs`](../frontends/waveduderust_slint/src/log.rs)): stderr (the console in the
  browser) and the ring of the last lines (`KEPT`) the status bar and the Messages pane show; Debug and up in debug
  builds, Info and up in release. Libraries never print; the CLI logs to stderr at the level its `-v` flags set and
  prints its results with `println!`. The log file, the crash report and the start-up failure box of
  [`docs/ui/70`](ui/70-dialogs-and-windows.md) are rows of [`parity.md`](parity.md).
- Failures: a library returns its error; the app shows it (status bar, a modal) and logs it.

## Comments and docs in code

- Every module opens with `//!`: what it is, units and invariants, the spec section it implements and a `Threads:`
  line where threads are involved ([`frontends/waveduderust_slint/src/live.rs`](../frontends/waveduderust_slint/src/live.rs)); mostly, the
  `model` and [`scene/shapes`](../crates/waveduderust/src/scene/shapes/) files and CLI modules have none.
- `///` on public items is a terse noun phrase for types and a third-person clause for functions; present state
  only; references the spec by path (`(docs/ui/60, The live run)`). A `.slint` component opens with a `//` line
  naming what it is and its spec section ([`ui/preview.slint`](../frontends/waveduderust_slint/ui/preview.slint)).
- Inline `//` comments are rare and explain a non-obvious why; no TODO or FIXME is left in `src/` or `ui/`.
- Source and docs are ASCII (`->`, `...`, `x`, `deg`); only translations hold other characters.

## State and views

- The `.slint` files hold the look, layout and focus; Rust holds the state (`State` in
  [`src/main.rs`](../frontends/waveduderust_slint/src/main.rs): document, live run, playback, settings) and pushes it to the window
  as properties and models; callbacks come back as named actions. Logic that needs no window is a Rust module tested
  without one.
- Runs converge on a goal without debounce or quiet timers ([`docs/ui/60-live-editing-and-runs.md`](ui/60-live-editing-and-runs.md),
  decision 0012). A 16 ms `slint::Timer` polls the live run and playback and draws the frame due.
- Pictures are drawn on the device the engine computes on, into textures the window shows, with no CPU readback
  ([`src/gpu.rs`](../frontends/waveduderust_slint/src/gpu.rs)); a frame the device no longer holds is read from the store.
- Colours come from the `Theme` global's tokens ([`ui/theme.slint`](../frontends/waveduderust_slint/ui/theme.slint)), the values of
  WaveDude Next's `Themes/Dark.xaml` and `Light.xaml`; fixed colours are for the preview overlays, graph boxes and
  colour picker.

## Strings

- Every UI string is wrapped in `@tr("...")` in `.slint` and translated through Slint's bundled translations; the
  English text is WaveDude Next's `Strings.EN.xaml` wording. The DE, FR and NL tables are a row of [`parity.md`](parity.md).
- Model labels, hints and enum words follow Next's `Label.<Type>.<Member>`, `Hint.<Type>.<Member>`,
  `Enum.<Type>.<Member>` entries. Menu texts carry their mnemonic as Next's do.

## Tests

- Unit tests sit in a `#[cfg(test)] mod tests` at the end of the file
  ([`frontends/waveduderust_slint/src/document.rs`](../frontends/waveduderust_slint/src/document.rs)).
- Integration tests per crate in `tests/`: physics against [`docs/physics.md`](physics.md) in
  [`crates/waveduderust/tests/physics.rs`](../crates/waveduderust/tests/physics.rs), file IO with `proptest` in [`tests/io.rs`](../crates/waveduderust/tests/io.rs), GPU against CPU in
  [`tests/gpu.rs`](../crates/waveduderust/tests/gpu.rs), export decoding in [`crates/waveduderust/tests/`](../crates/waveduderust/tests/).
- The model's fields are tested as tables: the adapter's coverage tests iterate `Field::all()` of every type
  ([`crates/waveduderust/src/io/wdproj.rs`](../crates/waveduderust/src/io/wdproj.rs), `covered_but`).
- Snapshots are text (`insta::assert_snapshot!` of hex or ASCII pictures, [`crates/waveduderust/tests/render.rs:268`](../crates/waveduderust/tests/render.rs#L268)),
  stored in `tests/snapshots/`, reviewed with `cargo insta review`.
- Tests that need hardware or tools return early with an `eprintln!` reason: no GPU adapter (`gpu_available`,
  [`crates/waveduderust/tests/gpu.rs:25`](../crates/waveduderust/tests/gpu.rs#L25)), no ffmpeg (`has_ffmpeg`, [`crates/waveduderust/tests/common/mod.rs:116`](../crates/waveduderust/tests/common/mod.rs#L116)).
- `#[ignore = "group: how to run it"]` tests live in a module named for the group, `ignored_timing`, `ignored_slow`
  or `ignored_network`, because `cargo test` filters on the module path: `-- --ignored ignored_timing::` runs
  one group ([`crates/waveduderust/tests/render.rs:338`](../crates/waveduderust/tests/render.rs#L338)).
- The window's logic is tested in its Rust modules without a window; the window itself is checked against WaveDude
  Next side by side on a nested display ([`tools/compare-csharp/README.md`](../tools/compare-csharp/README.md)) and its
  [`parity.md`](parity.md) rows ticked.
- A behaviour fix lands with the test that failed before it, in the same commit.

## Docs duties

- A behaviour change updates [`docs/spec`](spec/) or [`docs/ui`](ui/) in the same commit; a ported feature ticks its
  [`parity.md`](parity.md) row.
- A design decision is a numbered ADR `docs/decisions/NNNN-slug.md`: `# NNNN Title`, a `Status: accepted, <date>`
  line, the decision, later changes as `## Topic (added <date>)` sections and noted on the Status line
  ([`docs/decisions/0004-file-formats.md`](decisions/0004-file-formats.md)); mostly, `0012` has `Date: ... Status: ...` and Context, Decision,
  Consequences sections.
- A deliberate difference from WaveDude Next is a row of the [`docs/divergences.md`](divergences.md) table
  (Area, WaveDude Next, WaveDudeRust, Where), pointing to the decision.
- Performance numbers go to [`docs/measurements.md`](measurements.md) with the date, the machine and the command that made them.

## Dependencies

- Versions shared by several crates live in `[workspace.dependencies]` and are used as `dep.workspace = true`;
  a dependency of one crate is declared in that crate (`slint`, `rfd` in [`frontends/waveduderust_slint/Cargo.toml`](../frontends/waveduderust_slint/Cargo.toml)).
- Optional parts are features with a comment on why (`mp4`, `openh264-source` in
  [`crates/waveduderust/Cargo.toml`](../crates/waveduderust/Cargo.toml)); `openh264-source` is for development and tests, not distributed builds.
- GitHub Actions are pinned to a commit hash with the version as a comment
  (`actions/checkout@3d3c42e... # v7.0.1`, [`.github/workflows/ci.yml`](../.github/workflows/ci.yml)); downloaded tools are checked against a digest
  (`FFMPEG_SHA256`).
