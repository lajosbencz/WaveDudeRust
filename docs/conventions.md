# Conventions

What new code follows so that it fits the code already here. Each rule names one place that shows it. Of two fixes
with the same behaviour, the one that deletes or merges code wins over one that adds a guard.

## Toolchain and checks

The tools, their commands and their configuration are in [`CONTRIBUTING.md`](../CONTRIBUTING.md), Tools; run them before a
push. What they hold the code to:

- Edition 2024, resolver 3, MSRV `rust-version = "1.89"`, all in the root [`Cargo.toml`](../Cargo.toml) `[workspace.package]`;
  crates inherit them with `edition.workspace = true` etc. ([`crates/wavedude-core/Cargo.toml`](../crates/wavedude-core/Cargo.toml)).
- [`rustfmt.toml`](../rustfmt.toml): `hard_tabs = true`, `max_width = 120`, `use_small_heuristics = "Max"`. Indent with tabs,
  including in TOML arrays ([`crates/wavedude-app/Cargo.toml`](../crates/wavedude-app/Cargo.toml), `file-associations`).
- The lint policy is the `[workspace.lints]` table of the root [`Cargo.toml`](../Cargo.toml), inherited by every crate
  (`[lints] workspace = true`); each listed lint fails the build. The cast lints and `missing_docs` are left out on
  purpose. A needed `#[allow]` carries a `reason`
  ([`crates/wavedude-widgets/src/picture.rs:275`](../crates/wavedude-widgets/src/picture.rs#L275)).
- A doc comment links only to public items and wraps generic names like `<n>` in backticks, so `cargo doc` stays
  free of warnings.
- An exception in [`_typos.toml`](../_typos.toml) is a format or literature term, an identifier or a non-English file, named in
  the comment above it.

## Crates and dependency direction

- `wavedude-core`: model, `.wdproj` and scheme IO, scene painter, engines (CPU, wgpu behind feature `gpu`),
  runs and frame store (feature `run`), renderer. Without `run` it builds for `wasm32-unknown-unknown`, with or
  without `gpu`: code outside `run` uses no threads, no `std::time::Instant` (`web_time::Instant` instead) and no
  machine queries; on wasm32 nothing blocks, and the GPU engine is built by an async call
  ([decision 0015](decisions/0015-core-builds-for-wasm.md), [decision 0016](decisions/0016-gpu-engine-in-the-browser.md)).
  The run spec and the preview tiers are not in `run` ([`spec.rs`](../crates/wavedude-core/src/spec.rs),
  [`quality.rs`](../crates/wavedude-core/src/quality.rs)). Depends on no workspace crate ([`crates/wavedude-core/Cargo.toml`](../crates/wavedude-core/Cargo.toml)).
- `wavedude-model-derive`: the proc-macros `Fields` and `Word` of the model types (`syn`, `quote`); core re-exports
  them as `wavedude_core::model::{Fields, Word}`. Depends on no workspace crate.
- `wavedude-view`: what the preview draws, without a toolkit: the overlay model and its shapes, the probe graph box's
  layout and shapes, and the draw list (`Shape`, its text in a `TextStyle`, serde-serialisable for the browser). Shapes
  are in the canvas's units: the overlay's in logical pixels, a graph box's and the edge line's in device pixels of a
  `DeviceGrid` ([`crates/wavedude-view/src/draw.rs`](../crates/wavedude-view/src/draw.rs)); the browser's executor is [`demo/draw.js`](../demo/draw.js). Placement both the
  desktop and the browser need (which graph boxes a picture shows, where) lives here too ([decision 0018](decisions/0018-a-shared-picture-view.md)).
  Depends on core without default features; builds for `wasm32-unknown-unknown`.
- `wavedude-export`: sinks, overlays, CSV, the export job. Depends on core and view (the graph layout).
- `wavedude-widgets`: Freya widgets. Depends on `freya`, `serde`, `regex` and view (the shapes it draws, executed by
  `Painter` in [`crates/wavedude-widgets/src/paint.rs`](../crates/wavedude-widgets/src/paint.rs)), not on core itself; no strings: labels and tips come in as data
  (`MessageLabels`, `PickerTips` in [`crates/wavedude-widgets/src/lib.rs`](../crates/wavedude-widgets/src/lib.rs)).
- `wavedude-app-state`: document, history, selection, commands, live runs, settings, strings, log. Depends on
  core, export (no default features) and examples; no Freya, no rfd ([`crates/wavedude-app-state/Cargo.toml`](../crates/wavedude-app-state/Cargo.toml)).
- `wavedude-app`: Freya views, file dialogs (`rfd`, [`crates/wavedude-app/src/ui/dialogs.rs:111`](../crates/wavedude-app/src/ui/dialogs.rs#L111)), the updater.
  Depends on all of the above.
- `wavedude-examples`: the example projects, embedded (the fixtures of core and the repository's `examples`), with their
  titles, descriptions and topics; the Examples dialog and the web demo's catalog read this one list. The library depends on core
  without default features alone; the binary adds `anyhow` and `serde_json`
  ([`crates/wavedude-examples/Cargo.toml`](../crates/wavedude-examples/Cargo.toml)).
- `wavedude-wasm`: the engine for the browser (wasm-bindgen): a project's text, a tier, stepping, probe traces, pictures and
  the shapes over them as JSON. Core with `gpu` and without `run`, and view; no example, no DOM. `session.rs`, `cache.rs` and
  `overlay.rs` are plain Rust under native tests, `lib.rs` is the JavaScript surface
  ([decision 0017](decisions/0017-a-wasm-package-and-a-web-demo.md), [decision 0018](decisions/0018-a-shared-picture-view.md)).
- `wavedude-cli`: binary only (no `lib.rs`); core, view, export, and app-state behind feature `mp4`.
- `wavedude-testing`: a dev-dependency of widgets and app only: finding and clicking in the headless Freya tests.
  A click goes through its `click`/`click_at`, which assert the target lies inside the window first; a test that
  clicks a bare point must prove the element is there some other way.

## Where new things go

- A step index is a `Step`, a timeline position a `Frame`, and the arithmetic between them lives in `Timeline`
  alone ([`crates/wavedude-core/src/model/timeline.rs`](../crates/wavedude-core/src/model/timeline.rs)): no `* visualize_every` elsewhere.
- Model property: a field on the `model` type with its derive attributes (`#[field(range = ranges::X)]` with the
  constant in [`crates/wavedude-core/src/model/ranges.rs`](../crates/wavedude-core/src/model/ranges.rs), or `dependent`, `nested`, `skip`), its clamp in
  `Normalize` ([`model/normalize.rs:17`](../crates/wavedude-core/src/model/normalize.rs#L17)), an `Attr` in the `Table` of the type in
  [`crates/wavedude-core/src/io/wdproj.rs`](../crates/wavedude-core/src/io/wdproj.rs) (`impl Table for Simulation`), a row in the form
  (`Scope::number(Simulation::STEPS, ...)`, [`crates/wavedude-app/src/ui/properties_forms.rs`](../crates/wavedude-app/src/ui/properties_forms.rs)) and
  `Label.<Type>.<Member>` / `Hint.<Type>.<Member>` in the four string tables. Tests that fail otherwise:
  `every_field_of_a_table_type_is_an_attribute_or_skipped` (the file), `every_persisted_number_has_its_bounds_named`
  and `every_bound_is_enforced_by_normalize` (bounds, clamp),
  `every_model_field_and_word_has_its_texts_in_every_language` (strings);
  the forms test `every_form_is_built_for_every_item_kind` builds every row. A new enum derives `Word` and gets
  `Enum.<Type>.<Member>` keys (the same test). The derive's `Fields::visit` walks every type and word reachable
  from `Project`, so these tests need no hand-kept list of types.
- Model edit: a variant of `Command` ([`crates/wavedude-app-state/src/commands.rs:279`](../crates/wavedude-app-state/src/commands.rs#L279)), a struct variant with
  named fields and a `///` line, handled in `commands::apply` ([`commands.rs:500`](../crates/wavedude-app-state/src/commands.rs#L500)), which returns a `Change`
  whose `ChangeKind` ([`commands.rs:343`](../crates/wavedude-app-state/src/commands.rs#L343)) tells the live controller what to redo (`LiveController::on_change`,
  [`live.rs:512`](../crates/wavedude-app-state/src/live.rs#L512)). A property edit needs no new variant: `Command::Edit` carries the model's generated typed edit
  (`Edit::Probe(0, ProbeEdit::BoxWidth(500))`).
- Views send edits as `Event::Command(...)` through `Cx::send` ([`crates/wavedude-app/src/ui/properties.rs:134`](../crates/wavedude-app/src/ui/properties.rs#L134));
  every other user action is an `Event` variant ([`crates/wavedude-app-state/src/lib.rs:67`](../crates/wavedude-app-state/src/lib.rs#L67)) handled in
  `AppState::handle` (`lib.rs:527`), answered with a `Response`.
- Export format: a variant of `ExportFormat` with its line in `ALL` and `extensions`
  ([`crates/wavedude-export/src/lib.rs:33`](../crates/wavedude-export/src/lib.rs#L33)), a module under [`src/sinks/`](../crates/wavedude-export/src/sinks/) implementing `FrameSink`
  ([`sinks/mod.rs:27`](../crates/wavedude-export/src/sinks/mod.rs#L27)), and its arm in `sinks::open`; the matches are exhaustive, so the compiler lists the rest.
- CLI subcommand: a variant of `Command` in [`crates/wavedude-cli/src/main.rs:35`](../crates/wavedude-cli/src/main.rs#L35) (clap derive), a module with
  `pub(crate) struct Args` and `pub(crate) fn run(args: &Args) -> Result<()>` ([`crates/wavedude-cli/src/export.rs:20`](../crates/wavedude-cli/src/export.rs#L20), [`137`](../crates/wavedude-cli/src/export.rs#L137)),
  and its arm in `main`.
- Widget: one module per widget in [`crates/wavedude-widgets/src/`](../crates/wavedude-widgets/src/) named after it ([`text_box.rs`](../crates/wavedude-widgets/src/text_box.rs) -> `TextBox`),
  `pub mod` plus `pub use` in `lib.rs`, shown in [`examples/gallery.rs`](../crates/wavedude-widgets/examples/gallery.rs), tested in [`tests/headless.rs`](../crates/wavedude-widgets/tests/headless.rs).
- Project format quirks (WaveDude Next's units, versions, sparse writing) stay in [`crates/wavedude-core/src/io/`](../crates/wavedude-core/src/io/)
  (`BRIGHTNESS_GAIN`, `SENSITIVITY_GAIN` in [`io/wdproj.rs`](../crates/wavedude-core/src/io/wdproj.rs)); the model holds this program's units.
- Example projects are `.wdproj` files under `examples/<topic>/` with a numeric prefix
  ([`examples/gates/07-xor.wdproj`](../examples/gates/07-xor.wdproj)), embedded with `include_bytes!` in [`crates/wavedude-examples/src/lib.rs`](../crates/wavedude-examples/src/lib.rs).

## Naming

- Hooks are `use_*` and return a handle (`use_delayed`, `use_pending_edit`, `use_wave_theme`, `use_menu_bar`).
- Widgets are structs with `new(required...)` and by-value builder methods returning `Self`
  (`TextBox::new(value, on_apply).placeholder(...)`, [`crates/wavedude-widgets/src/text_box.rs:52`](../crates/wavedude-widgets/src/text_box.rs#L52);
  `Button::new().flat()`), rendered by `impl Component`.
- Test names are snake_case sentences stating the behaviour (`the_caret_scrolls_into_view_vertically`,
  [`crates/wavedude-widgets/src/text_field.rs:545`](../crates/wavedude-widgets/src/text_field.rs#L545)); mostly, as older ones are short noun phrases (`limits`,
  `probe_rules`).
- Tunables are named `const`s with a `///` line (`QUIET`, [`crates/wavedude-app-state/src/history.rs:13`](../crates/wavedude-app-state/src/history.rs#L13); `POLL`,
  [`crates/wavedude-app/src/ui/preview/mod.rs:53`](../crates/wavedude-app/src/ui/preview/mod.rs#L53)).

## Visibility and exports

- Each `lib.rs` opens with a `//!` summary, declares `pub mod`s and re-exports the main types with `pub use`
  ([`crates/wavedude-widgets/src/lib.rs`](../crates/wavedude-widgets/src/lib.rs), [`crates/wavedude-export/src/lib.rs`](../crates/wavedude-export/src/lib.rs)).
- Internal helpers are private modules (`mod edit; mod paint; mod util;` in widgets, `mod bits; mod yuv;` in
  export) or `pub(crate)` items (`pub(crate) fn mur_weights`, [`crates/wavedude-core/src/engine/cpu.rs:64`](../crates/wavedude-core/src/engine/cpu.rs#L64)).
  app-state has no `pub(crate)`: its modules are the public API. The cli is a binary, so its shared items are `pub(crate)`.
- Imports: `std`, then external and workspace crates, then `crate::`/`super::`, as blank-line separated groups
  ([`crates/wavedude-core/src/io/wdproj.rs`](../crates/wavedude-core/src/io/wdproj.rs)).

## Errors and panics

- Library crates define `thiserror` enums, messages lower-case but for proper nouns (`ExportError`,
  [`crates/wavedude-export/src/lib.rs:193`](../crates/wavedude-export/src/lib.rs#L193); `CommandError`, [`commands.rs:400`](../crates/wavedude-app-state/src/commands.rs#L400); `IoError`,
  [`crates/wavedude-core/src/io/mod.rs:95`](../crates/wavedude-core/src/io/mod.rs#L95)) and a `Result` alias ([`io/mod.rs:110`](../crates/wavedude-core/src/io/mod.rs#L110), [`crates/wavedude-export/src/lib.rs:230`](../crates/wavedude-export/src/lib.rs#L230)).
- `anyhow` is for binaries and setup: every `wavedude-cli` module, [`crates/wavedude-app/src/main.rs`](../crates/wavedude-app/src/main.rs),
  `log::init` ([`crates/wavedude-app-state/src/log.rs:149`](../crates/wavedude-app-state/src/log.rs#L149)).
- A broken invariant panics through `expect` whose message states the invariant
  (`.expect("a dock layout serialises")`, [`crates/wavedude-app/src/ui/app.rs:266`](../crates/wavedude-app/src/ui/app.rs#L266);
  `.expect("an embedded string table is valid JSON")`, `strings.rs:93`). Bare `unwrap()` is for tests:
  `clippy::unwrap_used` is denied in `wavedude-core` and `wavedude-export` outside their tests.
- A mutex is taken through `wavedude_core::sync::lock`, which takes a poisoned lock too: the guarded values
  are whole between operations and the panic was reported where it happened.
- A missing string key is a bug and panics (`Strings::get`, [`crates/wavedude-app-state/src/strings.rs:188`](../crates/wavedude-app-state/src/strings.rs#L188)).
- No silent fallbacks: a fallback is logged and stated. The GPU engine falls back to the CPU with a warning
  ([`crates/wavedude-core/src/engine/mod.rs:92`](../crates/wavedude-core/src/engine/mod.rs#L92)); an unreadable settings file is logged with its text and kept
  in `unreadable` ([`settings.rs:203`](../crates/wavedude-app-state/src/settings.rs#L203)); a failed command is logged and returned as `Response::Error` for a modal
  ([`crates/wavedude-app-state/src/lib.rs:539`](../crates/wavedude-app-state/src/lib.rs#L539)).
- Out-of-range values are clamped by `normalize` after load, import, every edit and before a run; it is
  idempotent ([`model/normalize.rs`](../crates/wavedude-core/src/model/normalize.rs), `Settings::normalize` at [`settings.rs:220`](../crates/wavedude-app-state/src/settings.rs#L220)).

## Logging

- `tracing` macros, always path-qualified (`tracing::warn!`), messages lower-case clauses naming the file or
  value ([`crates/wavedude-app-state/src/document.rs:89`](../crates/wavedude-app-state/src/document.rs#L89)).
- Levels: `error` for a failed user action, `warn` for a fallback or a dropped request, `info` for what the
  user did or got (`opened '...'`, [`document.rs:144`](../crates/wavedude-app-state/src/document.rs#L144)), `debug` for diagnostics, `trace` for every edit as
  `old -> new` ([`log.rs:244`](../crates/wavedude-app-state/src/log.rs#L244)).
- Channels of the app (`log.rs:init`): every line to the daily rolling file in the logs folder (Trace in debug
  builds, Debug in release, seven files kept); Info and up to the status bar and the Messages pane through the
  `LogRing` ([`log.rs:55`](../crates/wavedude-app-state/src/log.rs#L55), `RING_LINES`); Warn and up to stderr. The `log` crate's records of wgpu, winit and the
  rest are bridged into `tracing` (the `tracing-log` feature), so nothing a dependency reports is lost. Libraries
  never print; the CLI logs to stderr at the level its `-v` flags set and prints its results with `println!`.
- Failures: a library returns its error; the app shows it (a `Notice`, a dialog) and logs it. A failure before the
  window exists ends `main` with a native message box, the message also on stderr and in the log
  ([`crates/wavedude-app/src/main.rs`](../crates/wavedude-app/src/main.rs)). On Windows the app is a GUI program (`windows_subsystem`): no console
  opens, its stdout and stderr lead nowhere, and the file and the box are the record.
- Panics: the hook (`log.rs:install_panic_hook`) logs every panic with a backtrace; a worker's panic is caught
  by its owner (a failed run, a failed export, with a notice); the main thread's ends the program and leaves
  `crash.txt` in the logs folder, which the next start shows once.

## Comments and docs in code

- Every library module opens with `//!`: what it is, units and invariants, and a `Threads:` line where threads
  are involved ([`crates/wavedude-app-state/src/lib.rs:1`](../crates/wavedude-app-state/src/lib.rs#L1), [`crates/wavedude-app/src/ui/shared.rs`](../crates/wavedude-app/src/ui/shared.rs)); mostly, the
  `model` and [`scene/shapes`](../crates/wavedude-core/src/scene/shapes/) files and CLI modules have none.
- `///` on public items is a terse noun phrase for types (`/// What the views send.`, `lib.rs:65`) and a
  third-person clause for functions (`/// Applies and clears the pending text.`, [`edit.rs:20`](../crates/wavedude-widgets/src/edit.rs#L20)); present state
  only; references the spec by path (`(docs/ui/60-live-editing-and-runs.md, Edit classes)`, [`commands.rs:2`](../crates/wavedude-app-state/src/commands.rs#L2)).
- Inline `//` comments are rare and explain a non-obvious why; no TODO or FIXME is left in `src/`.
- Source and docs are ASCII (`->`, `...`, `x`, `deg`); only the translated string tables hold other characters.

## State and views

- `AppState` ([`crates/wavedude-app-state/src/lib.rs`](../crates/wavedude-app-state/src/lib.rs)) holds everything testable; the app crate is thin
  `impl Component` views over it.
- Views reach the state through `Cx::get()` ([`crates/wavedude-app/src/ui/app.rs:245`](../crates/wavedude-app/src/ui/app.rs#L245)) and `Shared`
  ([`ui/shared.rs`](../crates/wavedude-app/src/ui/shared.rs)): `read(topics, f)` subscribes to `Topics`, `peek` reads without subscribing, `update`
  mutates and re-renders the subscribed parts. Derived view data is a `use_memo` over `shared.read(...)`
  ([`crates/wavedude-app/src/ui/tree.rs:447`](../crates/wavedude-app/src/ui/tree.rs#L447)). Contexts are `use_provide_context`/`consume_context` by type.
- Typed text applies through the one `PendingEdit` ([`crates/wavedude-widgets/src/edit.rs:17`](../crates/wavedude-widgets/src/edit.rs#L17)): an editor offers
  it, the window flushes it before a command, save, run or selection change, a model-writing press flushes first
  (`use_pending_edit`, [`edit.rs:55`](../crates/wavedude-widgets/src/edit.rs#L55)), an editor flushes on focus loss and unmount.
- Focus moves by a counter, served once the editor is laid out (`FocusRequest`, `use_focus_on_layout`,
  [`edit.rs:132`](../crates/wavedude-widgets/src/edit.rs#L132)).
- Text editing uses the own `TextField` on `freya-edit`, never Freya's `Input`
  ([`crates/wavedude-widgets/src/text_field.rs:1`](../crates/wavedude-widgets/src/text_field.rs#L1)); `TextBox` and `NumberBox` build on it.
- Runs converge on a goal without debounce or quiet timers ([`docs/ui/60-live-editing-and-runs.md`](ui/60-live-editing-and-runs.md), decision
  0012). Views poll the state on `TICK` (16 ms, [`app.rs:31`](../crates/wavedude-app/src/ui/app.rs#L31)) and faster on `POLL` only while a picture is due.
  A deliberate delay is a named const run through `Delayed` ([`crates/wavedude-widgets/src/delay.rs:9`](../crates/wavedude-widgets/src/delay.rs#L9),
  `HOVER_TIP_DELAY` in [`ui/preview/pictures.rs`](../crates/wavedude-app/src/ui/preview/pictures.rs)).
- Colours come from the `Theme` tokens (`use_wave_theme()`, [`crates/wavedude-widgets/src/theme.rs:33`](../crates/wavedude-widgets/src/theme.rs#L33)); fixed
  colours are for the preview overlays, graph boxes and colour picker.
- Interactive elements carry a stable test id through `TestIdExt::test_id` ([`crates/wavedude-widgets/src/util.rs:7`](../crates/wavedude-widgets/src/util.rs#L7)).

## Strings

- Every UI string is a key in all four tables `crates/wavedude-app-state/assets/strings/{en,de,fr,nl}.json`;
  [`crates/wavedude-app/tests/strings.rs`](../crates/wavedude-app/tests/strings.rs) fails on a key missing from a language or unused in the source.
- Keys are literal `"Key"` strings in the code (the test searches for them); built keys use the prefixes
  `Label.`, `Hint.`, `Enum.`, `Kind.` through `Strings::label`, `hint`, `enum_word`, `kind`. `Label.`, `Hint.` and
   `Enum.` keys are `TYPE_NAME.<field or member name>` of a model type or `Word` (`strings::model_keys`), the test
   counts those as used.
- Lookup: `s.get("Key")` on a `Strings` from `cx.strings()` ([`crates/wavedude-app/src/ui/app.rs:260`](../crates/wavedude-app/src/ui/app.rs#L260));
  placeholders `{0}`, `{1}` with `s.format("KeyFormat", &[&a, &b])`, keys ending in `Format`
  ([`ui/memory_warning.rs:73`](../crates/wavedude-app/src/ui/memory_warning.rs#L73)).
- Plurals are separate keys chosen in code: `ItemCountZero`, `ItemCountOne`, `ItemCountMany`
  ([`crates/wavedude-app/src/ui/properties.rs:638`](../crates/wavedude-app/src/ui/properties.rs#L638)). Menu texts mark the mnemonic with `_` (`"_File"`).

## Tests

- Unit tests sit in a `#[cfg(test)] mod tests` at the end of the file ([`crates/wavedude-app-state/src/strings.rs`](../crates/wavedude-app-state/src/strings.rs)).
- Integration tests per crate in `tests/`: physics against [`docs/physics.md`](physics.md) in
  [`crates/wavedude-core/tests/physics.rs`](../crates/wavedude-core/tests/physics.rs), file IO with `proptest` in [`tests/io.rs`](../crates/wavedude-core/tests/io.rs), GPU against CPU in
  [`tests/gpu.rs`](../crates/wavedude-core/tests/gpu.rs), export decoding in [`crates/wavedude-export/tests/`](../crates/wavedude-export/tests/).
- The model's fields are tested as tables: the adapter's coverage tests iterate `Field::all()` of every type
  ([`crates/wavedude-core/src/io/wdproj.rs`](../crates/wavedude-core/src/io/wdproj.rs), `covered_but`), the strings tests iterate it for labels and hints
  (`strings.rs`), the forms test builds every row of every item kind ([`crates/wavedude-app/src/ui/tests.rs`](../crates/wavedude-app/src/ui/tests.rs)).
- Snapshots are text (`insta::assert_snapshot!` of hex or ASCII pictures, [`crates/wavedude-core/tests/render.rs:268`](../crates/wavedude-core/tests/render.rs#L268)),
  stored in `tests/snapshots/`, reviewed with `cargo insta review`.
- Tests that need hardware or tools return early with an `eprintln!` reason: no GPU adapter (`gpu_available`,
  [`crates/wavedude-core/tests/gpu.rs:25`](../crates/wavedude-core/tests/gpu.rs#L25)), no ffmpeg (`has_ffmpeg`, [`crates/wavedude-export/tests/common/mod.rs:89`](../crates/wavedude-export/tests/common/mod.rs#L89)).
- Timing tests are `#[ignore = "how to run it"]` ([`crates/wavedude-core/tests/render.rs:281`](../crates/wavedude-core/tests/render.rs#L281)).
- App-state tests drive `AppState<FakeBackend>` (`live::fake::FakeBackend`, `live.rs:1002`) with a fake clock
  ([`crates/wavedude-app-state/tests/state.rs:20`](../crates/wavedude-app-state/tests/state.rs#L20)).
- Widgets and views are tested headless with `freya_testing` (`launch_test`, `sync_and_update`, `click_cursor`,
  `press_key`): widgets in [`crates/wavedude-widgets/tests/headless.rs`](../crates/wavedude-widgets/tests/headless.rs), the window in
  [`crates/wavedude-app/tests/shell.rs`](../crates/wavedude-app/tests/shell.rs) and [`crates/wavedude-app/src/ui/tests.rs`](../crates/wavedude-app/src/ui/tests.rs) on an example project.
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
  a dependency of one crate is declared in that crate (`freya`, `rfd` in [`crates/wavedude-app/Cargo.toml`](../crates/wavedude-app/Cargo.toml)).
- Optional parts are features with a comment on why (`mp4`, `openh264-source` in
  [`crates/wavedude-export/Cargo.toml`](../crates/wavedude-export/Cargo.toml)); `openh264-source` is for development and tests, not distributed builds.
- GitHub Actions are pinned to a commit hash with the version as a comment
  (`actions/checkout@3d3c42e... # v7.0.1`, [`.github/workflows/ci.yml`](../.github/workflows/ci.yml)); downloaded tools are checked against a digest
  (`FFMPEG_SHA256`).
