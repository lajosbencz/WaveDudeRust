# Conventions

What new code follows so that it fits the code already here. Each rule names one place that shows it. Of two fixes
with the same behaviour, the one that deletes or merges code wins over one that adds a guard.

## Toolchain and checks

The tools, their commands and their configuration are in [`CONTRIBUTING.md`](../CONTRIBUTING.md), Tools; run them before a
push. What they hold the code to:

- Edition 2024, resolver 3, MSRV `rust-version = "1.95"`, all in the root [`Cargo.toml`](../Cargo.toml) `[workspace.package]`;
  crates inherit them with `edition.workspace = true` etc. ([`crates/wavedude-core/Cargo.toml`](../crates/wavedude-core/Cargo.toml)).
- [`rustfmt.toml`](../rustfmt.toml): `hard_tabs = true`, `max_width = 120`, `use_small_heuristics = "Max"`. Indent with tabs,
  including in TOML arrays ([`crates/wavedude-app/Cargo.toml`](../crates/wavedude-app/Cargo.toml), `file-associations`).
- The lint policy is the `[workspace.lints]` table of the root [`Cargo.toml`](../Cargo.toml), inherited by every crate
  (`[lints] workspace = true`); each listed lint fails the build. The cast lints and `missing_docs` are left out on
  purpose. A needed `#[expect]` (or `#[allow]`) carries a `reason`
  ([`crates/wavedude-app/src/ui/properties/mod.rs:247`](../crates/wavedude-app/src/ui/properties/mod.rs#L247)).
- A doc comment links only to public items and wraps generic names like `<n>` in backticks, so `cargo doc` stays
  free of warnings.
- An exception in [`_typos.toml`](../_typos.toml) is a format or literature term, an identifier or a non-English file, named in
  the comment above it.

## Crates and dependency direction

- `wavedude-core`: model, `.wdproj` and scheme IO, scene painter, engines (CPU, wgpu behind feature `gpu`),
  runs and frame store (feature `run`), renderer. It builds for `wasm32-unknown-unknown` with any of its features:
  code there uses no threads, no std clocks (`web_time`'s instead, which the wasm32 lint run enforces) and no machine
  queries; on wasm32 nothing blocks, the GPU engine is built by an async call, the live run is pumped by its caller,
  the browser's run worker (`RunWorker::pump`, which yields while its engine's future is pending; decision 0024)
  and its frame stores hold their frames in
  memory up to the caller's budget and spill the rest through the caller's backend (decision 0020); the batch run,
  the spill file and the machine's memory and threads are native only
  ([decision 0015](decisions/0015-core-builds-for-wasm.md), [decision 0016](decisions/0016-gpu-engine-in-the-browser.md)).
  The run spec and the preview tiers are not in `run` ([`spec.rs`](../crates/wavedude-core/src/spec.rs),
  [`quality.rs`](../crates/wavedude-core/src/quality.rs)). Depends on no workspace crate ([`crates/wavedude-core/Cargo.toml`](../crates/wavedude-core/Cargo.toml)).
- `wavedude-model-derive`: the proc-macros `Fields` and `Word` of the model types (`syn`, `quote`); core re-exports
  them as `wavedude_core::model::{Fields, Word}`. Depends on no workspace crate.
- `wavedude-view`: what the preview and the exports draw over a picture, without a toolkit: the overlay model and its
  shapes, the probe graph box's layout and shapes, as epaint shapes in points, their text laid out by epaint's fonts;
  hairlines and text land on whole device pixels through a `DeviceGrid`
  ([`crates/wavedude-view/src/draw.rs`](../crates/wavedude-view/src/draw.rs)). The bundled Selawik fonts
  ([`crates/wavedude-view/src/fonts.rs`](../crates/wavedude-view/src/fonts.rs)) set the window's text and the text over
  pictures. Placement the preview needs (which graph boxes a picture shows, where) lives here too
  ([decision 0018](decisions/0018-a-shared-picture-view.md)).
  Depends on core without default features and on epaint alone of the egui family; builds for `wasm32-unknown-unknown`.
- `wavedude-export`: sinks, overlays, CSV, the export job. Depends on core and view (the overlay shapes); rasterises
  the overlays' epaint meshes on the CPU into the frame.
- `wavedude-app-state`: document, history, selection, commands, live runs, settings, strings, log. What differs
  between the desktop and the browser is in its `platform` module, `native` or `web` chosen at compile time with
  one API: clocks, the store of settings, schemes and crash report (files in the config dir, local storage), the
  log, file watching, the machine's facts, writing a file (a download in the browser). Files open from a path or
  their bytes (`Source`) and save to a path or a download (`Target`); in the browser the live run is in the run
  worker and export, codecs, sequences, the updater and recent files are absent (the `HAS_*` constants). Its
  `remote` module is the live run in another agent (decision 0024): the postcard messages, the worker's `Host`
  around a pumped `RunWorker` whose stores post every frame, and the page's `Remote`, a `Backend` writing them into
  a mirror store and acknowledging each once held, both over a `Port` (`Loopback` in the tests,
  [`crates/wavedude-app-state/tests/remote.rs`](../crates/wavedude-app-state/tests/remote.rs); in the browser
  `remote::web`'s `PagePort` and `WorkerPort` over `postMessage`, which wake their owners as messages arrive).
  The browser's `CoreBackend` is a `Remote` on the run worker; a worker that fails to load, crashes or stays
  silent fails the run with its reason, and nothing runs on the page. Depends on core
  (with `serde`), export (no default features, native only) and examples; no egui, no rfd
  ([`crates/wavedude-app-state/Cargo.toml`](../crates/wavedude-app-state/Cargo.toml)).
- `wavedude-app`: egui views on eframe, natively and in the browser ([`tools/build-web.sh`](../tools/build-web.sh)),
  on the renderer's wgpu device, which the engines share natively where it computes
  ([`crates/wavedude-app/src/ui/mod.rs`](../crates/wavedude-app/src/ui/mod.rs)); native only: file dialogs (`rfd`,
  [`crates/wavedude-app/src/ui/files/native.rs`](../crates/wavedude-app/src/ui/files/native.rs)), the updater, exports; in the browser uploads and
  downloads ([`crates/wavedude-app/src/ui/files/web.rs`](../crates/wavedude-app/src/ui/files/web.rs)). Its widgets are modules of
  [`ui/`](../crates/wavedude-app/src/ui/); what a view computes without a window is in the crate's top-level modules
  (`preview`, `menus`), tested without one. Depends on all of the above ([decision 0019](decisions/0019-egui-and-the-platform-layer.md)).
- `wavedude-run-worker`: binary only, the browser's run worker (decision 0024): logging to the console, its
  panics posted to the page, a WebGPU device of its own and app-state's `Host` on a `WorkerPort`, pumped while the
  run has work, the event loop turning between pumps, and asleep until a message arrives once it has none
  ([`crates/wavedude-run-worker/src/worker.rs`](../crates/wavedude-run-worker/src/worker.rs)); `run-worker.js` loads it. Natively the binary only
  says so. Depends on core and app-state, in the browser only.
- `wavedude-examples`: the example projects, embedded (the fixtures of core and the repository's `examples`), with their
  titles, descriptions and topics; the Examples dialog reads this one list. The library depends on core
  without default features alone; the binary adds `anyhow` and `serde_json`
  ([`crates/wavedude-examples/Cargo.toml`](../crates/wavedude-examples/Cargo.toml)).
- `wavedude-cli`: binary only (no `lib.rs`); core, view, export, and app-state behind feature `mp4`.

## Where new things go

- A step index is a `Step`, a timeline position a `Frame`, and the arithmetic between them lives in `Timeline`
  alone ([`crates/wavedude-core/src/model/timeline.rs`](../crates/wavedude-core/src/model/timeline.rs)): no `* visualize_every` elsewhere.
- Model property: a field on the `model` type with its derive attributes (`#[field(range = ranges::X)]` with the
  constant in [`crates/wavedude-core/src/model/ranges.rs`](../crates/wavedude-core/src/model/ranges.rs), or `dependent`, `nested`, `skip`), its clamp in
  `Normalize` ([`model/normalize.rs:17`](../crates/wavedude-core/src/model/normalize.rs#L17)), an `Attr` in the `Table` of the type in
  [`crates/wavedude-core/src/io/wdproj.rs`](../crates/wavedude-core/src/io/wdproj.rs) (`impl Table for Simulation`), a row in the form
  ([`crates/wavedude-app/src/ui/properties/forms.rs`](../crates/wavedude-app/src/ui/properties/forms.rs), by the field's accessor, `Simulation::STEPS`) and
  `Label.<Type>.<Member>` / `Hint.<Type>.<Member>` in the four string tables. Tests that fail otherwise:
  `every_field_of_a_table_type_is_an_attribute_or_skipped` (the file), `every_persisted_number_has_its_bounds_named`
  and `every_bound_is_enforced_by_normalize` (bounds, clamp),
  `every_model_field_and_word_has_its_texts_in_every_language` (strings). A new enum derives `Word` and gets
  `Enum.<Type>.<Member>` keys (the same test). The derive's `Fields::visit` walks every type and word reachable
  from `Project`, so these tests need no hand-kept list of types.
- Model edit: a variant of `Command` ([`crates/wavedude-app-state/src/commands.rs:279`](../crates/wavedude-app-state/src/commands.rs#L279)), a struct variant with
  named fields and a `///` line, handled in `commands::apply` ([`commands.rs:500`](../crates/wavedude-app-state/src/commands.rs#L500)), which returns a `Change`
  whose `ChangeKind` ([`commands.rs:343`](../crates/wavedude-app-state/src/commands.rs#L343)) tells the live controller what to redo (`LiveController::on_change`,
  [`live.rs:561`](../crates/wavedude-app-state/src/live.rs#L561)). A property edit needs no new variant: `Command::Edit` carries the model's generated typed edit
  (`Edit::Probe(0, ProbeEdit::BoxWidth(500))`).
- Views push edits as `Event::Command(...)` onto the pass's events
  ([`crates/wavedude-app/src/ui/properties/mod.rs:537`](../crates/wavedude-app/src/ui/properties/mod.rs#L537)); every other user action is an `Event` variant
  ([`crates/wavedude-app-state/src/lib.rs:70`](../crates/wavedude-app-state/src/lib.rs#L70)) handled in `AppState::handle` (`lib.rs:586`), answered with a `Response`.
- Export format: a variant of `ExportFormat` with its line in `ALL` and `extensions`
  ([`crates/wavedude-export/src/lib.rs:33`](../crates/wavedude-export/src/lib.rs#L33)), a module under [`src/sinks/`](../crates/wavedude-export/src/sinks/) implementing `FrameSink`
  ([`sinks/mod.rs:27`](../crates/wavedude-export/src/sinks/mod.rs#L27)), and its arm in `sinks::open`; the matches are exhaustive, so the compiler lists the rest.
- CLI subcommand: a variant of `Command` in [`crates/wavedude-cli/src/main.rs:35`](../crates/wavedude-cli/src/main.rs#L35) (clap derive), a module with
  `pub(crate) struct Args` and `pub(crate) fn run(args: &Args) -> Result<()>` ([`crates/wavedude-cli/src/export.rs:20`](../crates/wavedude-cli/src/export.rs#L20), [`137`](../crates/wavedude-cli/src/export.rs#L137)),
  and its arm in `main`.
- Widget: one module per widget in [`crates/wavedude-app/src/ui/`](../crates/wavedude-app/src/ui/) named after it
  ([`number_box.rs`](../crates/wavedude-app/src/ui/number_box.rs) -> `number_box`), tested in its module with egui_kittest or in the
  window's tests.
- Project format quirks (WaveDude Next's units, versions, sparse writing) stay in [`crates/wavedude-core/src/io/`](../crates/wavedude-core/src/io/)
  (`BRIGHTNESS_GAIN`, `SENSITIVITY_GAIN` in [`io/wdproj.rs`](../crates/wavedude-core/src/io/wdproj.rs)); the model holds this program's units.
- Example projects are `.wdproj` files under `examples/<topic>/` with a numeric prefix
  ([`examples/gates/07-xor.wdproj`](../examples/gates/07-xor.wdproj)), embedded with `include_bytes!` in [`crates/wavedude-examples/src/lib.rs`](../crates/wavedude-examples/src/lib.rs).

## Naming

- Widgets are immediate-mode functions over a `Ui` that return what was edited, the events going to the caller
  (`number_box(ui, rect, id, ...)`, [`crates/wavedude-app/src/ui/number_box.rs:310`](../crates/wavedude-app/src/ui/number_box.rs#L310)); a pane with state
  across passes is a struct with a `ui` method (`PropertiesPane::ui`, [`ui/properties/mod.rs:89`](../crates/wavedude-app/src/ui/properties/mod.rs#L89)).
- Test names are snake_case sentences stating the behaviour (`a_drag_on_a_number_box_scrubs_its_value`,
  [`crates/wavedude-app/tests/properties.rs:116`](../crates/wavedude-app/tests/properties.rs#L116)); mostly, as older ones are short noun phrases
  (`limits`, `probe_rules`).
- Tunables are named `const`s with a `///` line (`QUIET`, [`crates/wavedude-app-state/src/history.rs:13`](../crates/wavedude-app-state/src/history.rs#L13); `POLL`,
  [`crates/wavedude-app/src/preview/mod.rs:35`](../crates/wavedude-app/src/preview/mod.rs#L35)).

## Visibility and exports

- Each `lib.rs` opens with a `//!` summary, declares `pub mod`s and re-exports the main types with `pub use`
  ([`crates/wavedude-view/src/lib.rs`](../crates/wavedude-view/src/lib.rs), [`crates/wavedude-export/src/lib.rs`](../crates/wavedude-export/src/lib.rs)).
- Internal helpers are private modules (`mod modal; mod edit;` in the app's `ui`, `mod bits; mod yuv;` in
  export) or `pub(crate)` items (`pub(crate) fn mur_weights`, [`crates/wavedude-core/src/engine/cpu.rs:65`](../crates/wavedude-core/src/engine/cpu.rs#L65)).
  app-state has no `pub(crate)`: its modules are the public API. The cli is a binary, so its shared items are `pub(crate)`.
- Imports: `std`, then external and workspace crates, then `crate::`/`super::`, as blank-line separated groups
  ([`crates/wavedude-core/src/io/wdproj.rs`](../crates/wavedude-core/src/io/wdproj.rs)).

## Errors and panics

- Library crates define `thiserror` enums, messages lower-case but for proper nouns (`ExportError`,
  [`crates/wavedude-export/src/lib.rs:193`](../crates/wavedude-export/src/lib.rs#L193); `CommandError`, [`commands.rs:400`](../crates/wavedude-app-state/src/commands.rs#L400); `IoError`,
  [`crates/wavedude-core/src/io/mod.rs:95`](../crates/wavedude-core/src/io/mod.rs#L95)) and a `Result` alias ([`io/mod.rs:110`](../crates/wavedude-core/src/io/mod.rs#L110), [`crates/wavedude-export/src/lib.rs:230`](../crates/wavedude-export/src/lib.rs#L230)).
- `anyhow` is for binaries and setup: every `wavedude-cli` module, [`crates/wavedude-app/src/main.rs`](../crates/wavedude-app/src/main.rs),
  `platform::init_log` ([`crates/wavedude-app-state/src/platform/native.rs:183`](../crates/wavedude-app-state/src/platform/native.rs#L183)).
- A broken invariant panics through `expect` whose message states the invariant
  (`.expect("a rectangle has sides")`, [`crates/wavedude-app/src/ui/dock.rs:267`](../crates/wavedude-app/src/ui/dock.rs#L267);
  `.expect("an embedded string table is valid JSON")`, `strings.rs:93`). Bare `unwrap()` is for tests:
  `clippy::unwrap_used` is denied in `wavedude-core` and `wavedude-export` outside their tests.
- A mutex is taken through `wavedude_core::sync::lock`, which takes a poisoned lock too: the guarded values
  are whole between operations and the panic was reported where it happened.
- A missing string key is a bug and panics (`Strings::get`, [`crates/wavedude-app-state/src/strings.rs:190`](../crates/wavedude-app-state/src/strings.rs#L190)).
- No silent fallbacks: a fallback is logged and stated. The GPU engine falls back to the CPU with a warning and
  hands back why ([`crates/wavedude-core/src/engine/mod.rs:124`](../crates/wavedude-core/src/engine/mod.rs#L124)); an unreadable settings file is logged with its text and kept
  in `unreadable` ([`settings.rs:201`](../crates/wavedude-app-state/src/settings.rs#L201)); a failed command is logged and returned as `Response::Error` for a modal
  ([`crates/wavedude-app-state/src/lib.rs:528`](../crates/wavedude-app-state/src/lib.rs#L528)).
- Out-of-range values are clamped by `normalize` after load, import, every edit and before a run; it is
  idempotent ([`model/normalize.rs`](../crates/wavedude-core/src/model/normalize.rs), `Settings::normalize` at [`settings.rs:218`](../crates/wavedude-app-state/src/settings.rs#L218)).

## Logging

- `tracing` macros, always path-qualified (`tracing::warn!`), messages lower-case clauses naming the file or
  value ([`crates/wavedude-app-state/src/document.rs:276`](../crates/wavedude-app-state/src/document.rs#L276)).
- Levels: `error` for a failed user action, `warn` for a fallback or a dropped request, `info` for what the
  user did or got (`opened '...'`, [`document.rs:153`](../crates/wavedude-app-state/src/document.rs#L153)), `debug` for diagnostics, `trace` for every edit as
  `old -> new` ([`log.rs:169`](../crates/wavedude-app-state/src/log.rs#L169)).
- Channels of the app (`platform::init_log`): every line to the daily rolling file in the logs folder (Trace in debug
  builds, Debug in release, seven files kept); Info and up to the status bar and the Messages pane through the
  `LogRing` ([`log.rs:53`](../crates/wavedude-app-state/src/log.rs#L53), `RING_LINES`); Warn and up to stderr. The `log` crate's records of wgpu, winit and the
  rest are bridged into `tracing` (the `tracing-log` feature), so nothing a dependency reports is lost. Libraries
  never print; the CLI logs to stderr at the level its `-v` flags set and prints its results with `println!`.
- Failures: a library returns its error; the app shows it (a `Notice`, a dialog) and logs it. A failure before the
  window exists ends `main` with a native message box, the message also on stderr and in the log
  ([`crates/wavedude-app/src/main.rs`](../crates/wavedude-app/src/main.rs)). On Windows the app is a GUI program (`windows_subsystem`): no console
  opens, its stdout and stderr lead nowhere, and the file and the box are the record.
- Panics: the hook (`platform::install_panic_hook`) logs every panic with a backtrace; a worker's panic is caught
  by its owner (a failed run, a failed export, with a notice); the main thread's ends the program and leaves
  `crash.txt` in the logs folder, which the next start shows once.

## Comments and docs in code

- Every library module opens with `//!`: what it is, units and invariants, and a `Threads:` line where threads
  are involved ([`crates/wavedude-app-state/src/lib.rs:1`](../crates/wavedude-app-state/src/lib.rs#L1), [`crates/wavedude-app/src/ui/mod.rs`](../crates/wavedude-app/src/ui/mod.rs)); mostly, the
  `model` and [`scene/shapes`](../crates/wavedude-core/src/scene/shapes/) files and CLI modules have none.
- `///` on public items is a terse noun phrase for types (`/// What the views send.`, `lib.rs:68`) and a
  third-person clause for functions (`/// Brings the open window forward.`, [`ui/manual.rs:85`](../crates/wavedude-app/src/ui/manual.rs#L85)); present state
  only; references the spec by path (`(docs/ui/60-live-editing-and-runs.md, Edit classes)`, [`commands.rs:2`](../crates/wavedude-app-state/src/commands.rs#L2)).
- Inline `//` comments are rare and explain a non-obvious why; no TODO or FIXME is left in `src/`.
- Source and docs are ASCII (`->`, `...`, `x`, `deg`); only the translated string tables hold other characters.

## State and views

- `AppState` ([`crates/wavedude-app-state/src/lib.rs`](../crates/wavedude-app-state/src/lib.rs)) holds everything testable; the app crate is thin
  egui views over it (`App`, [`crates/wavedude-app/src/lib.rs:30`](../crates/wavedude-app/src/lib.rs#L30)).
- Views read `&App` each pass and push `Event`s; the window applies them after the pass
  ([`crates/wavedude-app/src/ui/mod.rs`](../crates/wavedude-app/src/ui/mod.rs)). What a view derives from the state is kept with its inputs and
  made again when they change (the overlay model, [`ui/preview/overlay.rs`](../crates/wavedude-app/src/ui/preview/overlay.rs)).
- Typed text applies through the one pending edit ([`crates/wavedude-app/src/ui/edit.rs:39`](../crates/wavedude-app/src/ui/edit.rs#L39)): a focused box
  offers the event its text would apply, the window applies it before any event it sends, and when the box was not
  shown in a pass.
- Runs converge on a goal without debounce or quiet timers ([`docs/ui/60-live-editing-and-runs.md`](ui/60-live-editing-and-runs.md), decision
  0012). The window repaints on `TICK` (16 ms, [`crates/wavedude-app/src/window.rs:9`](../crates/wavedude-app/src/window.rs#L9)) only while something moves,
  faster on `POLL` while a picture is due, and otherwise sleeps until a worker wakes it (`wavedude_app_state::wake`).
- Colours come from the `Palette` tokens (`Palette::of(ctx.theme())`, [`crates/wavedude-app/src/ui/theme.rs:131`](../crates/wavedude-app/src/ui/theme.rs#L131));
  fixed colours are for the preview overlays, graph boxes and colour picker.
- An interactive element states its role and label through `widget_info`, so it is in egui's AccessKit tree, where
  the tests find it ([`ui/preview/transport.rs`](../crates/wavedude-app/src/ui/preview/transport.rs), `tool_button`).

## Strings

- Every UI string is a key in all four tables `crates/wavedude-app-state/assets/strings/{en,de,fr,nl}.json`;
  [`crates/wavedude-app/tests/strings.rs`](../crates/wavedude-app/tests/strings.rs) fails on a key missing from a language or unused in the source.
- Keys are literal `"Key"` strings in the code (the test searches for them); built keys use the prefixes
  `Label.`, `Hint.`, `Enum.`, `Kind.` through `Strings::label`, `hint`, `enum_word`, `kind`. `Label.`, `Hint.` and
   `Enum.` keys are `TYPE_NAME.<field or member name>` of a model type or `Word` (`strings::model_keys`), the test
   counts those as used.
- Lookup: `s.get("Key")` on the `Strings` of `app.strings` ([`crates/wavedude-app-state/src/strings.rs:190`](../crates/wavedude-app-state/src/strings.rs#L190));
  placeholders `{0}`, `{1}` with `s.format("KeyFormat", &[&a, &b])`, keys ending in `Format`
  ([`ui/memory_warning.rs:50`](../crates/wavedude-app/src/ui/memory_warning.rs#L50)).
- Plurals are separate keys chosen in code: `ItemCountZero`, `ItemCountOne`, `ItemCountMany`
  ([`crates/wavedude-app/src/ui/properties/mod.rs:186`](../crates/wavedude-app/src/ui/properties/mod.rs#L186)). Menu texts mark the mnemonic with `_` (`"_File"`).

## Tests

- Unit tests sit in a `#[cfg(test)] mod tests` at the end of the file ([`crates/wavedude-app-state/src/strings.rs`](../crates/wavedude-app-state/src/strings.rs)).
- Integration tests per crate in `tests/`: physics against [`docs/physics.md`](physics.md) in
  [`crates/wavedude-core/tests/physics.rs`](../crates/wavedude-core/tests/physics.rs), file IO with `proptest` in [`tests/io.rs`](../crates/wavedude-core/tests/io.rs), GPU against CPU in
  [`tests/gpu.rs`](../crates/wavedude-core/tests/gpu.rs), export decoding in [`crates/wavedude-export/tests/`](../crates/wavedude-export/tests/).
- The model's fields are tested as tables: the adapter's coverage tests iterate `Field::all()` of every type
  ([`crates/wavedude-core/src/io/wdproj.rs`](../crates/wavedude-core/src/io/wdproj.rs), `covered_but`), the strings tests iterate it for labels and hints
  (`strings.rs`).
- Snapshots are text (`insta::assert_snapshot!` of hex or ASCII pictures, [`crates/wavedude-core/tests/render.rs:268`](../crates/wavedude-core/tests/render.rs#L268)),
  stored in `tests/snapshots/`, reviewed with `cargo insta review`.
- Tests that need hardware or tools return early with an `eprintln!` reason: no GPU adapter (`gpu_available`,
  [`crates/wavedude-core/tests/gpu.rs:25`](../crates/wavedude-core/tests/gpu.rs#L25)), no ffmpeg (`has_ffmpeg`, [`crates/wavedude-export/tests/common/mod.rs:89`](../crates/wavedude-export/tests/common/mod.rs#L89)).
- Timing tests are `#[ignore = "how to run it"]` ([`crates/wavedude-core/tests/render.rs:281`](../crates/wavedude-core/tests/render.rs#L281)).
- App-state tests drive `AppState<FakeBackend>` (`live::fake::FakeBackend`, `live.rs:1153`) with a fake clock
  ([`crates/wavedude-app-state/tests/state.rs:22`](../crates/wavedude-app-state/tests/state.rs#L22)).
- The window is tested headless with egui_kittest on wgpu: widgets found by role and label, clicked and typed into
  through AccessKit, with no image snapshots ([`crates/wavedude-app/tests/common/mod.rs`](../crates/wavedude-app/tests/common/mod.rs),
  [`crates/wavedude-app/tests/shell.rs`](../crates/wavedude-app/tests/shell.rs)); a widget alone in its module's tests
  ([`crates/wavedude-app/src/ui/number_box.rs`](../crates/wavedude-app/src/ui/number_box.rs)).
- A behaviour fix lands with the test that failed before it, in the same commit.

## Docs duties

- A behaviour change updates [`docs/spec`](spec/) or [`docs/ui`](ui/) in the same commit; a user-visible one also
  [`crates/wavedude-app/assets/manual.md`](../crates/wavedude-app/assets/manual.md).
- A design decision is a numbered ADR `docs/decisions/NNNN-slug.md`: `# NNNN Title`, a `Status: accepted, <date>`
  line, the decision, later changes as `## Topic (added <date>)` sections and noted on the Status line
  ([`docs/decisions/0004-file-formats.md`](decisions/0004-file-formats.md)); mostly, `0012` has `Date: ... Status: ...` and Context, Decision,
  Consequences sections.
- A deliberate difference from WaveDude Next is a row of the [`docs/divergences.md`](divergences.md) table
  (Area, WaveDude Next, WaveDudeRust, Where), pointing to the decision.
- Performance numbers go to [`docs/measurements.md`](measurements.md) with the date, the machine and the command that made them.

## Dependencies

- Versions shared by several crates live in `[workspace.dependencies]` and are used as `dep.workspace = true`;
  a dependency of one crate is declared in that crate (`eframe`, `rfd` in [`crates/wavedude-app/Cargo.toml`](../crates/wavedude-app/Cargo.toml)).
- Optional parts are features with a comment on why (`mp4`, `openh264-source` in
  [`crates/wavedude-export/Cargo.toml`](../crates/wavedude-export/Cargo.toml)); `openh264-source` is for development and tests, not distributed builds.
- GitHub Actions are pinned to a commit hash with the version as a comment
  (`actions/checkout@3d3c42e... # v7.0.1`, [`.github/workflows/ci.yml`](../.github/workflows/ci.yml)); downloaded tools are checked against a digest
  (`FFMPEG_SHA256`).
