# Conventions

What new code follows so that it fits the code already here. Each rule names one place that shows it. Of two fixes
with the same behaviour, the one that deletes or merges code wins over one that adds a guard.

## Toolchain and checks

The tools, their commands and their configuration are in [`CONTRIBUTING.md`](../CONTRIBUTING.md), Tools; run them before a
push. What they hold the code to:

- Edition 2024, resolver 3, MSRV `rust-version = "1.100"`, all in the root [`Cargo.toml`](../Cargo.toml) `[workspace.package]`;
  crates inherit them with `edition.workspace = true` etc. ([`crates/waveduderust/Cargo.toml`](../crates/waveduderust/Cargo.toml)).
- [`rustfmt.toml`](../rustfmt.toml): `hard_tabs = true`, `max_width = 120`, `use_small_heuristics = "Max"`. Indent with tabs,
  including in TOML arrays ([`frontends/classic/Cargo.toml`](../frontends/classic/Cargo.toml), `file-associations`).
- The lint policy is the `[workspace.lints]` table of the root [`Cargo.toml`](../Cargo.toml), inherited by every crate
  (`[lints] workspace = true`); each listed lint fails the build. The cast lints and `missing_docs` are left out on
  purpose. A needed `#[expect]` (or `#[allow]`) carries a `reason`
  ([`frontends/classic/src/ui/properties/mod.rs:247`](../frontends/classic/src/ui/properties/mod.rs#L247)).
- A doc comment links only to public items and wraps generic names like `<n>` in backticks, so `cargo doc` stays
  free of warnings.
- An exception in [`_typos.toml`](../_typos.toml) is a format or literature term, an identifier or a non-English file, named in
  the comment above it.

## Crates and dependency direction

- `waveduderust`: model, `.wdproj` and scheme IO, scene painter, engines (CPU, wgpu behind feature `gpu`),
  runs and frame store (feature `run`), renderer. Its stable API is the model, IO, quality, `Simulation` and the
  background `run::Run` with its `Frames`; the rest is under `waveduderust::unstable` behind `unstable-*` features
  ([decision 0031](decisions/0031-modular-core-and-frontends.md)), which first-party crates enable. It builds for `wasm32-unknown-unknown` with any of its features:
  code there uses no threads, no std clocks (`web_time`'s instead, which the wasm32 lint run enforces) and no machine
  queries; on wasm32 nothing blocks, the GPU engine is built by an async call, the live run is pumped by its caller,
  the browser's run worker (`RunWorker::pump`, which says whether work is left, waits for readiness (an engine, a
  readback, the device, the spill's writes) or is idle, waking the caller's waker; decision 0024)
  and its frame stores hold their frames in
  memory up to the caller's budget and spill the rest through the caller's backend (decision 0020); the live run on a thread
  of its own, the spill file and the machine's memory and threads are native only
  ([decision 0015](decisions/0015-core-builds-for-wasm.md), [decision 0016](decisions/0016-gpu-engine-in-the-browser.md)).
  The run spec and the preview tiers are not in `run` ([`spec.rs`](../crates/waveduderust/src/spec.rs),
  [`quality.rs`](../crates/waveduderust/src/quality.rs)). Depends on no workspace crate ([`crates/waveduderust/Cargo.toml`](../crates/waveduderust/Cargo.toml)).
- `waveduderust_macros`: the proc-macros `Fields` and `Word` of the model types (`syn`, `quote`); core re-exports
  them as `waveduderust::model::{Fields, Word}`. Depends on no workspace crate.
- `waveduderust::unstable::view`: what the preview and the exports draw over a picture, without a toolkit: the overlay model and its
  shapes, the probe graph box's layout and shapes, as epaint shapes in points, their text laid out by epaint's fonts;
  hairlines and text land on whole device pixels through a `DeviceGrid`
  ([`crates/waveduderust/src/view/draw.rs`](../crates/waveduderust/src/view/draw.rs)). The bundled Selawik fonts
  ([`crates/waveduderust/src/view/fonts.rs`](../crates/waveduderust/src/view/fonts.rs)) set the window's text and the text over
  pictures. Placement the preview needs (which graph boxes a picture shows, where) lives here too
  ([decision 0018](decisions/0018-a-shared-picture-view.md)).
  Behind the `view` feature; epaint alone of the egui family; builds for `wasm32-unknown-unknown`.
- `waveduderust::unstable::export`: sinks, overlays, CSV, the export job, the H.264 encoder seam (`h264`: the MP4 muxer takes any
  `H264Encoder`; `mp4-mux` is the muxer alone, on `shiguredo_mp4`, `mp4` adds OpenH264). Behind the `export` feature, which enables `view` and `run`; rasterises
  the overlays' epaint meshes on the CPU into the frame.
- `waveduderust_app`: document, history, selection, commands, live runs, settings, strings, log. What differs
  between the desktop and the browser is in its `platform` module, `native` or `web` chosen at compile time with
  one API: clocks, the store of settings, schemes and crash report (files in the config dir, local storage), the
  log, file watching, the machine's facts, writing a file (a download in the browser). Files open from a path or
  their bytes (`Source`) and save to a path or a download (`Target`); in the browser the live run is in the run
  worker; exports run on the page into the origin private file system (`platform::Spool`; the module `opfs` splits into `session`, `spill`, `queue` and `spool`: the session folder and
  its lock, the frames past memory posted to the spill worker, the queue of writes in flight, the exports' files) and leave as downloads, MP4 through the browser's WebCodecs encoder (`platform::WebCodecsFactory`, module `webcodecs`); OpenH264 and the updater are compiled out and recent files are absent (`HAS_PATHS`). Its
  `remote` module is the live run in another agent (decision 0024): the postcard messages, the worker's `Host`
  around a pumped `RunWorker` whose stores post every frame, and the page's `Remote`, a `Backend` writing them into
  a mirror store and acknowledging each once held, both over a `Port` (`Loopback` in the tests,
  [`crates/waveduderust_app/tests/remote.rs`](../crates/waveduderust_app/tests/remote.rs); in the browser
  `remote::web`'s `PagePort` and `WorkerPort` over `postMessage`, which wake their owners as messages arrive; the
  page's `PagePort` speaks a `Wire`, the run worker's or the spill worker's).
  The browser's `CoreBackend` is a `Remote` on the run worker; a worker that fails to load, crashes, stays
  silent, speaks another protocol version (`protocol::VERSION`) or is of another build (`build_id`) fails every
  run with its reason (`WorkerGone`) until the page is reloaded, and nothing runs on the page. Depends on core
  (with `serde`), export (no default features, with `serde`; with MP4 native only), examples and, in the browser, the
  spill worker's protocol; no egui, no rfd
  ([`crates/waveduderust_app/Cargo.toml`](../crates/waveduderust_app/Cargo.toml)).
- `waveduderust_app`: what the windows compute without a UI toolkit, tested without one: the menus' actions and when
  each is enabled ([`menus.rs`](../crates/waveduderust_app/src/menus.rs); an entry's icon is a neutral `Icon` each
  window draws from its own set), the forms, the preview's models (`preview`: view, handles, overlay, graph boxes,
  timeline, the presenter), the export session ([`export/web.rs`](../crates/waveduderust_app/src/export/web.rs) steps
  it on the page; [`export/delivery.rs`](../crates/waveduderust_app/src/export/delivery.rs) names the downloads),
  dialogs' filters, the settings table, messages and the updater (native only). Depends on core,
  view, export and app-state; no egui, eframe or winit
  ([`crates/waveduderust_app/Cargo.toml`](../crates/waveduderust_app/Cargo.toml)).
- `waveduderust_egui_preview`: what draws the simulation in every egui window, whatever its widgets and look: the
  pictures on the renderer's wgpu device, the handle overlay, the graph boxes, the timeline, the gradient editor and the
  drawn icons. Colours come in as small structs the window fills from its theme (`TimelineColors`, `GradientColors`,
  an accent). Depends on `waveduderust_egui_platform`, eframe, app-state, core and view.
- `waveduderust_egui_platform`: egui start-up and the OS integration: the renderer's wgpu device, which the engines share
  natively where it computes ([`crates/waveduderust_egui_platform/src/lib.rs`](../crates/waveduderust_egui_platform/src/lib.rs)),
  the keys, input helpers, the clipboard, the theme setting resolved against the OS's scheme, the window's place and
  the menu icons. Depends on eframe, app-state and core; never on `waveduderust_egui_preview`.
- `waveduderust_classic`: the classic window on eframe, natively and in the browser ([`tools/build-web.sh`](../tools/build-web.sh))
  ([`frontends/classic/src/ui/mod.rs`](../frontends/classic/src/ui/mod.rs)): it paints the course of
  `waveduderust_app::shell` and runs its tasks through `waveduderust_egui_platform::jobs`. Its widgets and look are modules of
  [`ui/`](../frontends/classic/src/ui/). Depends on frontend, egui and the crates below them
  ([decision 0019](decisions/0019-egui-and-the-platform-layer.md)).
- `waveduderust_material` (program `wavedude-mineral`, in `frontends/material`): a second window on egui-mineral's Material stylekit over `waveduderust_app`, `waveduderust_egui_preview` and `waveduderust_egui_platform`
  ([`frontends/material/DESIGN.md`](../frontends/material/DESIGN.md)); it depends on no other window.
- `wavedude-run-worker`: binary only, the browser's run worker (decision 0024): logging to the console, its
  panics posted to the page, a WebGPU device of its own (requested again after a loss) and app-state's `Host` on a
  `WorkerPort`, pumped in 8 ms
  slices, the event loop turning between pumps: again at once while the run has work, else asleep until what it waits
  for is ready or a message arrives, kicking the GPU queue with empty submits while the device owes completions
  (decision 0026) ([`crates/waveduderust_workers/src/bin/wavedude-run-worker/worker.rs`](../crates/waveduderust_workers/src/bin/wavedude-run-worker/worker.rs)); `run-worker.js` loads it. Natively the binary only
  says so. Depends on core and app-state, in the browser only.
- `wavedude-spill-worker`: the browser's spill worker (decision 0020): its library is the protocol with the page
  (`ToSpill`, `FromSpill`, `VERSION`), its binary a dedicated worker logging to the console, its panics posted to the
  page, that keeps each store generation's spilled frames in one file of the tab's session folder through a
  synchronous access handle, handling the page's messages in order
  ([`crates/waveduderust_workers/src/bin/wavedude-spill-worker/worker.rs`](../crates/waveduderust_workers/src/bin/wavedude-spill-worker/worker.rs)); `spill-worker.js` loads it.
  Natively the binary only says so. Depends on serde, and in the browser on web-sys and postcard; no workspace crate.
- `waveduderust_examples`: the example projects, embedded from the repository's [`examples`](../examples/), with their
  titles, descriptions and topics; the Examples dialog reads this one list. Depends on core without default
  features alone ([`crates/waveduderust_examples/Cargo.toml`](../crates/waveduderust_examples/Cargo.toml)).
- `wavedude-cli`: binary only (no `lib.rs`); core, view, export and app-state (the app's folders: settings, the user's
  colour schemes, OpenH264); MP4 behind feature `mp4`. The global `--config-dir <DIR>` puts the app's folders under
  `DIR` (`AppDirs::under`); its tests pass an empty one, so they never read the user's.

## Where new things go

- A step index is a `Step`, a timeline position a `Frame`, and the arithmetic between them lives in `Timeline`
  alone ([`crates/waveduderust/src/model/timeline.rs`](../crates/waveduderust/src/model/timeline.rs)): no `* visualize_every` elsewhere.
- Model property: a field on the `model` type with its derive attributes (`#[field(range = ranges::X)]` with the
  constant in [`crates/waveduderust/src/model/ranges.rs`](../crates/waveduderust/src/model/ranges.rs), or `dependent`, `nested`, `skip`), its clamp in
  `Normalize` ([`model/normalize.rs:17`](../crates/waveduderust/src/model/normalize.rs#L17)), an `Attr` in the `Table` of the type in
  [`crates/waveduderust/src/io/wdproj.rs`](../crates/waveduderust/src/io/wdproj.rs) (`impl Table for Simulation`), a row in the form
  ([`frontends/classic/src/ui/properties/forms.rs`](../frontends/classic/src/ui/properties/forms.rs), by the field's accessor, `Simulation::STEPS`) and
  `Label.<Type>.<Member>` / `Hint.<Type>.<Member>` in the four string tables. Tests that fail otherwise:
  `every_field_of_a_table_type_is_an_attribute_or_skipped` (the file), `every_persisted_number_has_its_bounds_named`
  and `every_bound_is_enforced_by_normalize` (bounds, clamp),
  `every_model_field_and_word_has_its_texts_in_every_language` (strings). A new enum derives `Word` and gets
  `Enum.<Type>.<Member>` keys (the same test). The derive's `Fields::visit` walks every type and word reachable
  from `Project`, so these tests need no hand-kept list of types.
- Model edit: a variant of `Command` ([`crates/waveduderust_app/src/commands.rs:279`](../crates/waveduderust_app/src/commands.rs#L279)), a struct variant with
  named fields and a `///` line, handled in `commands::apply` ([`commands.rs:500`](../crates/waveduderust_app/src/commands.rs#L500)), which returns a `Change`
  whose `ChangeKind` ([`commands.rs:343`](../crates/waveduderust_app/src/commands.rs#L343)) tells the live controller what to redo (`LiveController::on_change`,
  [`live.rs:561`](../crates/waveduderust_app/src/live.rs#L561)). A property edit needs no new variant: `Command::Edit` carries the model's generated typed edit
  (`Edit::Probe(0, ProbeEdit::BoxWidth(500))`).
- Views push edits as `Event::Command(...)` onto the pass's events
  ([`frontends/classic/src/ui/properties/mod.rs:537`](../frontends/classic/src/ui/properties/mod.rs#L537)); every other user action is an `Event` variant
  ([`crates/waveduderust_app/src/lib.rs:70`](../crates/waveduderust_app/src/lib.rs#L70)) handled in `AppState::handle` (`lib.rs:586`), answered with a `Response`.
- Export format: a variant of `ExportFormat` with its line in `ALL` and `extensions`
  ([`crates/waveduderust/src/export/mod.rs:34`](../crates/waveduderust/src/export/mod.rs#L34)), a module under [`src/sinks/`](../crates/waveduderust/src/export/sinks/) implementing `FrameSink`
  ([`sinks/mod.rs:27`](../crates/waveduderust/src/export/sinks/mod.rs#L27)), and its arm in `sinks::open`; the matches are exhaustive, so the compiler lists the rest.
- CLI subcommand: a variant of `Command` in [`crates/waveduderust_cli/src/main.rs:35`](../crates/waveduderust_cli/src/main.rs#L35) (clap derive), a module with
  `pub(crate) struct Args` and `pub(crate) fn run(args: &Args) -> Result<()>` ([`crates/waveduderust_cli/src/export.rs:22`](../crates/waveduderust_cli/src/export.rs#L22), [`107`](../crates/waveduderust_cli/src/export.rs#L107)),
  and its arm in `main`.
- Widget: one module per widget in [`frontends/classic/src/ui/`](../frontends/classic/src/ui/) named after it
  ([`number_box.rs`](../frontends/classic/src/ui/number_box.rs) -> `number_box`), tested in its module with egui_kittest or in the
  window's tests.
- Project format quirks (9-point frequency units, versions, sparse writing) stay in [`crates/waveduderust/src/io/`](../crates/waveduderust/src/io/)
  ([`io/wdproj.rs`](../crates/waveduderust/src/io/wdproj.rs)); the model holds this program's units.
- Example projects are `.wdproj` files under `examples/<topic>/`, the folder being the dialog's tab, named with their series prefix
  ([`examples/guides/07-xor.wdproj`](../examples/guides/07-xor.wdproj)), embedded with `include_bytes!` in [`crates/waveduderust_examples/src/lib.rs`](../crates/waveduderust_examples/src/lib.rs)
  ([`EXAMPLES.md`](../EXAMPLES.md)).
  [`examples`](../examples/) is all the app and the web build ship; Next's originals in
  [`crates/waveduderust/tests/fixtures/examples`](../crates/waveduderust/tests/fixtures/examples/) are for the tests and the comparison alone, shipped as the copies
  `ExampleNN_*.wdproj` ([`examples/next.md`](../examples/next.md)).

## Naming

- Widgets are immediate-mode functions over a `Ui` that return what was edited, the events going to the caller
  (`number_box(ui, rect, id, ...)`, [`frontends/classic/src/ui/number_box.rs:39`](../frontends/classic/src/ui/number_box.rs#L39)); a pane with state
  across passes is a struct with a `ui` method (`PropertiesPane::ui`, [`ui/properties/mod.rs:89`](../frontends/classic/src/ui/properties/mod.rs#L89)).
- Test names are snake_case sentences stating the behaviour (`typed_text_applies_before_a_shortcut_and_when_its_form_goes`,
  [`frontends/classic/src/ui/tests/properties.rs:110`](../frontends/classic/src/ui/tests/properties.rs#L110)); mostly, as older ones are short noun phrases
  (`limits`, `probe_rules`).
- Tunables are named `const`s with a `///` line (`QUIET`, [`crates/waveduderust_app/src/history.rs:13`](../crates/waveduderust_app/src/history.rs#L13); `POLL`,
  [`crates/waveduderust_app/src/preview/mod.rs:36`](../crates/waveduderust_app/src/preview/mod.rs#L36)).

## Visibility and exports

- Each `lib.rs` opens with a `//!` summary, declares `pub mod`s and re-exports the main types with `pub use`
  ([`crates/waveduderust/src/view/mod.rs`](../crates/waveduderust/src/view/mod.rs), [`crates/waveduderust/src/export/mod.rs`](../crates/waveduderust/src/export/mod.rs)).
- Internal helpers are private modules (`mod modal; mod edit;` in the app's `ui`, `mod bits; mod yuv;` in
  export) or `pub(crate)` items (`pub(crate) fn mur_weights`, [`crates/waveduderust/src/engine/mur.rs:32`](../crates/waveduderust/src/engine/mur.rs#L32)).
  app-state has no `pub(crate)`: its modules are the public API. The cli is a binary, so its shared items are `pub(crate)`.
- Imports: `std`, then external and workspace crates, then `crate::`/`super::`, as blank-line separated groups
  ([`crates/waveduderust/src/io/wdproj.rs`](../crates/waveduderust/src/io/wdproj.rs)).

## Errors and panics

- Library crates define `thiserror` enums, messages lower-case but for proper nouns (`ExportError`,
  [`crates/waveduderust/src/export/mod.rs:199`](../crates/waveduderust/src/export/mod.rs#L199); `CommandError`, [`commands.rs:400`](../crates/waveduderust_app/src/commands.rs#L400); `IoError`,
  [`crates/waveduderust/src/io/mod.rs:95`](../crates/waveduderust/src/io/mod.rs#L95)) and a `Result` alias ([`io/mod.rs:110`](../crates/waveduderust/src/io/mod.rs#L110), [`crates/waveduderust/src/export/mod.rs:243`](../crates/waveduderust/src/export/mod.rs#L243)).
- `anyhow` is for binaries and setup: every `wavedude-cli` module, [`frontends/classic/src/main.rs`](../frontends/classic/src/main.rs),
  `platform::init_log` ([`crates/waveduderust_app/src/platform/native.rs:183`](../crates/waveduderust_app/src/platform/native.rs#L183)).
- A broken invariant panics through `expect` whose message states the invariant
  (`.expect("a rectangle has sides")`, [`frontends/classic/src/ui/dock.rs:267`](../frontends/classic/src/ui/dock.rs#L267);
  `.expect("an embedded string table is valid JSON")`, `strings.rs:93`). Bare `unwrap()` is for tests:
  `clippy::unwrap_used` is denied in `waveduderust` outside its tests.
- A mutex is taken through `waveduderust::unstable::sync::lock`, which takes a poisoned lock too: the guarded values
  are whole between operations and the panic was reported where it happened. A `Condvar` wait does the same with
  `.unwrap_or_else(PoisonError::into_inner)` (`RunWorker`, [`crates/waveduderust/src/run/worker.rs`](../crates/waveduderust/src/run/worker.rs)).
- A missing string key is a bug and panics (`Strings::get`, [`crates/waveduderust_app/src/strings.rs:190`](../crates/waveduderust_app/src/strings.rs#L190)).
- No silent fallbacks: a fallback is logged and stated. The GPU engine falls back to the CPU with a warning and
  hands back why ([`crates/waveduderust/src/engine/mod.rs:124`](../crates/waveduderust/src/engine/mod.rs#L124)); a settings value that cannot be read is logged with its
  key and reads as its default ([`settings.rs:284`](../crates/waveduderust_app/src/settings.rs#L284)); a failed command is logged and returned as `Response::Error` for a modal
  ([`crates/waveduderust_app/src/lib.rs:528`](../crates/waveduderust_app/src/lib.rs#L528)).
- Out-of-range values are clamped by `normalize` after load, import, every edit and before a run; it is
  idempotent ([`model/normalize.rs`](../crates/waveduderust/src/model/normalize.rs), `Settings::normalize` at [`settings.rs:240`](../crates/waveduderust_app/src/settings.rs#L240)).

## Logging

- The UI is translated, the log is not: window, menu and dialog text comes from the string tables in every language;
  log lines (file, status bar, Messages pane) are English. A dialog may append an error's own text untranslated.
- `tracing` macros, always path-qualified (`tracing::warn!`), messages lower-case clauses naming the file or
  value ([`crates/waveduderust_app/src/document.rs:276`](../crates/waveduderust_app/src/document.rs#L276)).
- Levels: `error` for a failed user action, `warn` for a fallback or a dropped request, `info` for what the
  user did or got (`opened '...'`, [`document.rs:153`](../crates/waveduderust_app/src/document.rs#L153)), `debug` for diagnostics, `trace` for every edit as
  `old -> new` ([`log.rs:169`](../crates/waveduderust_app/src/log.rs#L169)).
- Channels of the app (`platform::init_log`): every line to the daily rolling file in the logs folder (Trace in debug
  builds, Debug in release, seven files kept); Info and up to the status bar and the Messages pane through the
  `LogRing` ([`log.rs:53`](../crates/waveduderust_app/src/log.rs#L53), `RING_LINES`); Warn and up to stderr. The `log` crate's records of wgpu, winit and the
  rest are bridged into `tracing` (the `tracing-log` feature), so nothing a dependency reports is lost. Libraries
  never print; the CLI logs to stderr at the level its `-v` flags set and prints its results with `println!`.
- Failures: a library returns its error; the app shows it (a `Notice`, a dialog) and logs it. A failure before the
  window exists ends `main` with a native message box, the message also on stderr and in the log
  ([`frontends/classic/src/main.rs`](../frontends/classic/src/main.rs)). On Windows the app is a GUI program (`windows_subsystem`): no console
  opens, its stdout and stderr lead nowhere, and the file and the box are the record.
- Panics: the hook (`platform::install_panic_hook`) logs every panic with a backtrace; a worker's panic is caught
  by its owner (a failed run, a failed export, with a notice); a window task's is raised again on the UI thread
  ([`crates/waveduderust_egui_platform/src/jobs/native.rs`](../crates/waveduderust_egui_platform/src/jobs/native.rs)); the main thread's ends the program and leaves
  `crash.txt` in the logs folder, which the next start shows once.

## Comments and docs in code

- Every library module opens with `//!`: what it is, units and invariants, and a `Threads:` line where threads
  are involved ([`crates/waveduderust_app/src/lib.rs:1`](../crates/waveduderust_app/src/lib.rs#L1), [`frontends/classic/src/ui/mod.rs`](../frontends/classic/src/ui/mod.rs)); mostly, the
  `model` and [`scene/shapes`](../crates/waveduderust/src/scene/shapes/) files and CLI modules have none.
- `///` on public items is a terse noun phrase for types (`/// What the views send.`, `lib.rs:68`) and a
  third-person clause for functions (`/// Draws \`pieces\`.`, [`ui/markdown.rs`](../frontends/classic/src/ui/markdown.rs)); present state
  only; references the spec by path (`(docs/ui/60-live-editing-and-runs.md, Edit classes)`, [`commands.rs:2`](../crates/waveduderust_app/src/commands.rs#L2)).
- Inline `//` comments are rare and explain a non-obvious why; no TODO or FIXME is left in `src/`.
- Source and docs are ASCII (`->`, `...`, `x`, `deg`); only the translated string tables hold other characters.

## State and views

- `AppState` ([`crates/waveduderust_app/src/lib.rs`](../crates/waveduderust_app/src/lib.rs)) holds everything testable; the window crates are thin
  egui views over it (`App`, [`crates/waveduderust_app/src/lib.rs:33`](../crates/waveduderust_app/src/lib.rs#L33)).
- Views read `&App` each pass and push `Event`s; the window applies them after the pass
  ([`frontends/classic/src/ui/mod.rs`](../frontends/classic/src/ui/mod.rs)). What a view derives from the state is kept with its inputs and
  made again when they change (the overlay model, [`overlay.rs`](../crates/waveduderust_egui_preview/src/overlay.rs)).
- Typed text applies through the one pending edit ([`frontends/classic/src/ui/edit.rs:39`](../frontends/classic/src/ui/edit.rs#L39)): a focused box
  offers the event its text would apply, the window applies it before any event it sends, and when the box was not
  shown in a pass.
- Runs converge on a goal without debounce or quiet timers ([`docs/ui/60-live-editing-and-runs.md`](ui/60-live-editing-and-runs.md), decision
  0012). The window repaints on `TICK` (16 ms, [`crates/waveduderust_app/src/window.rs:9`](../crates/waveduderust_app/src/window.rs#L9)) only while something moves,
  faster on `POLL` while a picture is due, and otherwise sleeps until a worker wakes it (`waveduderust_app::wake`).
- Colours come from the `Palette` tokens (`Palette::of(ctx.theme())`, [`frontends/classic/src/ui/theme.rs:131`](../frontends/classic/src/ui/theme.rs#L131));
  fixed colours are for the preview overlays, graph boxes and colour picker.
- An interactive element states its role and label through `widget_info`, so it is in egui's AccessKit tree, where
  the tests find it ([`ui/preview/transport.rs`](../frontends/classic/src/ui/preview/transport.rs), `tool_button`).

## Strings

- Every UI string is a key in all four tables `crates/waveduderust_app/assets/strings/{en,de,fr,nl}.json`;
  [`crates/waveduderust_app/tests/strings.rs`](../crates/waveduderust_app/tests/strings.rs) fails on a key missing from a language or unused in the source.
- Keys are literal `"Key"` strings in the code (the test searches for them); built keys use the prefixes
  `Label.`, `Hint.`, `Enum.`, `Kind.` through `Strings::label`, `hint`, `enum_word`, `kind`. `Label.`, `Hint.` and
   `Enum.` keys are `TYPE_NAME.<field or member name>` of a model type or `Word` (`strings::model_keys`), the test
   counts those as used.
- Lookup: `s.get("Key")` on the `Strings` of `app.strings` ([`crates/waveduderust_app/src/strings.rs:190`](../crates/waveduderust_app/src/strings.rs#L190));
  placeholders `{0}`, `{1}` with `s.format("KeyFormat", &[&a, &b])`, keys ending in `Format`
  ([`ui/memory_warning.rs:50`](../frontends/classic/src/ui/memory_warning.rs#L50)).
- Plurals are separate keys chosen in code: `ItemCountZero`, `ItemCountOne`, `ItemCountMany`
  ([`frontends/classic/src/ui/properties/mod.rs:186`](../frontends/classic/src/ui/properties/mod.rs#L186)). Menu texts mark the mnemonic with `_` (`"_File"`).

## Tests

- Unit tests sit in a `#[cfg(test)] mod tests` at the end of the file ([`crates/waveduderust_app/src/strings.rs`](../crates/waveduderust_app/src/strings.rs)).
- Integration tests per crate in `tests/`: physics against [`docs/physics.md`](physics.md) in
  [`crates/waveduderust/tests/physics.rs`](../crates/waveduderust/tests/physics.rs), file IO with `proptest` in [`tests/io.rs`](../crates/waveduderust/tests/io.rs), GPU against CPU in
  [`tests/gpu.rs`](../crates/waveduderust/tests/gpu.rs), export decoding in [`crates/waveduderust/tests/`](../crates/waveduderust/tests/).
- The model's fields are tested as tables: the adapter's coverage tests iterate `Field::all()` of every type
  ([`crates/waveduderust/src/io/wdproj.rs`](../crates/waveduderust/src/io/wdproj.rs), `covered_but`), the strings tests iterate it for labels and hints
  (`strings.rs`).
- Snapshots are text (`insta::assert_snapshot!` of hex or ASCII pictures, [`crates/waveduderust/tests/render.rs:268`](../crates/waveduderust/tests/render.rs#L268)),
  stored in `tests/snapshots/`, reviewed with `cargo insta review`.
- Tests that need hardware or tools return early with an `eprintln!` reason: no GPU adapter (`gpu_available`,
  [`crates/waveduderust/tests/gpu.rs:25`](../crates/waveduderust/tests/gpu.rs#L25)), no ffmpeg (`has_ffmpeg`, [`crates/waveduderust/tests/common/mod.rs:116`](../crates/waveduderust/tests/common/mod.rs#L116)).
- `#[ignore = "group: how to run it"]` tests live in a module named for the group, `ignored_timing`, `ignored_slow`
  or `ignored_network`, because `cargo test` filters on the module path: `-- --ignored ignored_timing::` runs
  one group ([`crates/waveduderust/tests/render.rs:338`](../crates/waveduderust/tests/render.rs#L338)).
- App-state tests drive `AppState<FakeBackend>` (`live::fake::FakeBackend`, `live.rs:1298`, feature `test-support`, which app-state's dev-dependency on itself enables) with a fake clock
  ([`crates/waveduderust_app/tests/state.rs:20`](../crates/waveduderust_app/tests/state.rs#L20)).
- The window is tested headless with egui_kittest on wgpu: widgets found by role and label, clicked and typed into
  through AccessKit, with no image snapshots ([`frontends/classic/src/ui/tests/common.rs`](../frontends/classic/src/ui/tests/common.rs),
  [`frontends/classic/src/ui/tests/dialogs.rs`](../frontends/classic/src/ui/tests/dialogs.rs)); a widget alone in its module's tests
  ([`frontends/classic/src/ui/number_box.rs`](../frontends/classic/src/ui/number_box.rs)).
- A behaviour fix lands with the test that failed before it, in the same commit.

## Docs duties

- A behaviour change updates [`docs/spec`](spec/) or [`docs/ui`](ui/) in the same commit.
- A design decision is a numbered ADR `docs/decisions/NNNN-slug.md`: `# NNNN Title`, a `Status: accepted, <date>`
  line, the decision, later changes as `## Topic (added <date>)` sections and noted on the Status line
  ([`docs/decisions/0004-file-formats.md`](decisions/0004-file-formats.md)); mostly, `0012` has `Date: ... Status: ...` and Context, Decision,
  Consequences sections.
- A deliberate difference from WaveDude Next is a row of the [`docs/divergences.md`](divergences.md) table
  (Area, WaveDude Next, WaveDudeRust, Where), pointing to the decision.
- Performance numbers go to [`docs/measurements.md`](measurements.md) with the date, the machine and the command that made them.

## Dependencies

- Versions shared by several crates live in `[workspace.dependencies]` and are used as `dep.workspace = true`;
  a dependency of one crate is declared in that crate (`eframe`, `rfd` in [`frontends/classic/Cargo.toml`](../frontends/classic/Cargo.toml)).
- Optional parts are features with a comment on why (`mp4`, `openh264-source` in
  [`crates/waveduderust/Cargo.toml`](../crates/waveduderust/Cargo.toml)); `openh264-source` is for development and tests, not distributed builds.
- GitHub Actions are pinned to a commit hash with the version as a comment
  (`actions/checkout@3d3c42e... # v7.0.1`, [`.github/workflows/ci.yml`](../.github/workflows/ci.yml)); downloaded tools are checked against a digest
  (`FFMPEG_SHA256`).
