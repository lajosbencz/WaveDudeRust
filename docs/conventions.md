# Conventions

What new code follows so that it fits the code already here. Each rule names one place that shows it.

## Toolchain and checks

- Edition 2024, resolver 3, MSRV `rust-version = "1.89"`, all in the root `Cargo.toml` `[workspace.package]`;
  crates inherit them with `edition.workspace = true` etc. (`crates/wavedude-core/Cargo.toml`).
- The toolchain is pinned in `rust-toolchain.toml` (`channel = "1.99.1"`, clippy and rustfmt); bump it there.
- `rustfmt.toml`: `hard_tabs = true`, `max_width = 120`, `use_small_heuristics = "Max"`. Indent with tabs,
  including in TOML arrays (`crates/wavedude-app/Cargo.toml`, `file-associations`).
- The lint policy is the `[workspace.lints]` table of the root `Cargo.toml`, inherited by every crate
  (`[lints] workspace = true`); CI runs clippy with `-D warnings`, so each listed lint fails the build. The cast
  lints and `missing_docs` are left out on purpose. A needed `#[allow]` carries a `reason`
  (`crates/wavedude-widgets/src/picture.rs:275`).
- `cargo doc --workspace --no-deps` builds without warnings (`RUSTDOCFLAGS=-D warnings` in CI): a doc comment
  links only to public items and wraps generic names like `<n>` in backticks.
- `typos` (`_typos.toml` holds the exceptions: format and literature terms, the translated string tables) and
  `cargo deny check` (`deny.toml`: advisories of our own dependencies, the licence allow-list, no wildcard or
  unknown-source dependencies) run on every change in `.github/workflows/hygiene.yml`; `cargo hack check
  --workspace --feature-powerset --depth 2` and `cargo machete` run in the Linux CI job.
- Before a push, run what `.github/workflows/ci.yml` runs:
  `cargo fmt --all --check`;
  `cargo clippy --workspace --all-targets --features $F -- -D warnings` and `cargo test --workspace --features $F`
  with `F=wavedude-export/openh264-source,wavedude-cli/openh264-source`;
  `cargo check --workspace`, `cargo check -p wavedude-export --no-default-features`,
  `cargo check -p wavedude-core --no-default-features`; and `cargo +1.89 check --workspace --all-targets` (MSRV job).
- Commits and pull request titles are conventional commits (`CONTRIBUTING.md`, "Commit messages").

## Crates and dependency direction

- `wavedude-core`: model, `.wdproj` and scheme IO, scene painter, engines (CPU, wgpu behind feature `gpu`),
  runs and frame store, renderer. Depends on no workspace crate (`crates/wavedude-core/Cargo.toml`).
- `wavedude-export`: sinks, overlays, CSV, the export job. Depends on core alone.
- `wavedude-widgets`: Freya widgets. Depends on `freya`, `serde`, `regex` only: no core, no strings; labels and
  tips come in as data (`MessageLabels`, `PickerTips` in `crates/wavedude-widgets/src/lib.rs`).
- `wavedude-app-state`: document, history, selection, commands, live runs, settings, strings, log. Depends on
  core and export (no default features); no Freya, no rfd (`crates/wavedude-app-state/Cargo.toml`).
- `wavedude-app`: Freya views, file dialogs (`rfd`, `crates/wavedude-app/src/ui/dialogs.rs:105`), the updater.
  Depends on all of the above.
- `wavedude-cli`: binary only (no `lib.rs`); core, export, and app-state behind feature `mp4`.
- `wavedude-testing`: a dev-dependency of widgets and app only: finding and clicking in the headless Freya tests.
  A click goes through its `click`/`click_at`, which assert the target lies inside the window first; a test that
  clicks a bare point must prove the element is there some other way.

## Where new things go

- Model property: a field on the `model` type, its bound in `crates/wavedude-core/src/model/ranges.rs` (`ALL`,
  or `UNBOUNDED`/`DEPENDENT`), its clamp in `Normalize` (`model/normalize.rs:17`), its attribute in the
  `Table` of `crates/wavedude-core/src/io/wdproj.rs` (`impl Table for Simulation`), and `Label.<Type>.<Member>` /
  `Hint.<Type>.<Member>` in the four string tables (enforced by `every_model_property_has_label_and_hint`).
- Model edit: a variant of `Command` (`crates/wavedude-app-state/src/commands.rs:181`), a struct variant with
  named fields and a `///` line, handled in `commands::apply` (`commands.rs:408`), which returns a `Change`
  whose `ChangeKind` (`commands.rs:249`) tells the live controller what to redo (`LiveController::on_change`,
  `live.rs:526`). A property edit needs no new variant: `Command::Set` addresses it by serde path.
- Views send edits as `Event::Command(...)` through `Cx::send` (`crates/wavedude-app/src/ui/properties.rs:207`);
  every other user action is an `Event` variant (`crates/wavedude-app-state/src/lib.rs:67`) handled in
  `AppState::handle` (`lib.rs:549`), answered with a `Response`.
- Export format: a variant of `ExportFormat` with its line in `ALL` and `extensions`
  (`crates/wavedude-export/src/lib.rs:32`), a module under `src/sinks/` implementing `FrameSink`
  (`sinks/mod.rs:27`), and its arm in `sinks::open`; the matches are exhaustive, so the compiler lists the rest.
- CLI subcommand: a variant of `Command` in `crates/wavedude-cli/src/main.rs:34` (clap derive), a module with
  `pub struct Args` and `pub fn run(args: &Args) -> Result<()>` (`crates/wavedude-cli/src/export.rs:18,131`),
  and its arm in `main`.
- Widget: one module per widget in `crates/wavedude-widgets/src/` named after it (`text_box.rs` -> `TextBox`),
  `pub mod` plus `pub use` in `lib.rs`, shown in `examples/gallery.rs`, tested in `tests/headless.rs`.
- Project format quirks (WaveDude Next's units, versions, sparse writing) stay in `crates/wavedude-core/src/io/`
  (`BRIGHTNESS_GAIN`, `wdproj.rs:219`); the model holds this program's units.
- Example projects are `.wdproj` files under `examples/<topic>/` with a numeric prefix
  (`examples/gates/07-xor.wdproj`), embedded with `include_bytes!` in `crates/wavedude-app-state/src/examples.rs`.

## Naming

- Hooks are `use_*` and return a handle (`use_delayed`, `use_pending_edit`, `use_wave_theme`, `use_menu_bar`).
- Widgets are structs with `new(required...)` and by-value builder methods returning `Self`
  (`TextBox::new(value, on_apply).placeholder(...)`, `crates/wavedude-widgets/src/text_box.rs:52`;
  `Button::new().flat()`), rendered by `impl Component`.
- Test names are snake_case sentences stating the behaviour (`the_caret_scrolls_into_view_vertically`,
  `crates/wavedude-widgets/src/text_field.rs:545`); mostly, as older ones are short noun phrases (`limits`,
  `probe_rules`).
- Tunables are named `const`s with a `///` line (`QUIET`, `crates/wavedude-app-state/src/history.rs:14`; `POLL`,
  `crates/wavedude-app/src/ui/preview/mod.rs:54`).

## Visibility and exports

- Each `lib.rs` opens with a `//!` summary, declares `pub mod`s and re-exports the main types with `pub use`
  (`crates/wavedude-widgets/src/lib.rs`, `crates/wavedude-export/src/lib.rs`).
- Internal helpers are private modules (`mod edit; mod paint; mod util;` in widgets, `mod bits; mod yuv;` in
  export) or `pub(crate)` items (`pub(crate) fn mur_weights`, `crates/wavedude-core/src/engine/cpu.rs:64`).
  app-state and cli have no `pub(crate)`: their modules are the public API or private to a binary.
- Imports: `std`, then external and workspace crates, then `crate::`/`super::`, as blank-line separated groups
  (`crates/wavedude-core/src/io/wdproj.rs`).

## Errors and panics

- Library crates define `thiserror` enums, messages lower-case but for proper nouns (`ExportError`,
  `crates/wavedude-export/src/lib.rs:191`; `CommandError`, `commands.rs:306`; `IoError`,
  `crates/wavedude-core/src/io/mod.rs:95`) and a `Result` alias (`io/mod.rs:110`, `export/src/lib.rs:229`).
- `anyhow` is for binaries and setup: every `wavedude-cli` module, `crates/wavedude-app/src/main.rs`,
  `log::init` (`crates/wavedude-app-state/src/log.rs:146`).
- A broken invariant panics through `expect` whose message states the invariant
  (`.expect("a dock layout serialises")`, `crates/wavedude-app/src/ui/app.rs:267`;
  `.expect("an embedded string table is valid JSON")`, `strings.rs:90`). Bare `unwrap()` is for tests and
  mutex locks (`crates/wavedude-app/src/ui/preview/presenter.rs:217`).
- A missing string key is a bug and panics (`Strings::get`, `crates/wavedude-app-state/src/strings.rs:185`).
- No silent fallbacks: a fallback is logged and stated. The GPU engine falls back to the CPU with a warning
  (`crates/wavedude-core/src/engine/mod.rs:83`); an unreadable settings file is logged with its text and kept
  in `unreadable` (`settings.rs:200`); a failed command is logged and returned as `Response::Error` for a modal
  (`crates/wavedude-app-state/src/lib.rs:557`).
- Out-of-range values are clamped by `normalize` after load, import, every edit and before a run; it is
  idempotent (`model/normalize.rs`, `Settings::normalize` at `settings.rs:217`).

## Logging

- `tracing` macros, always path-qualified (`tracing::warn!`), messages lower-case clauses naming the file or
  value (`crates/wavedude-app-state/src/document.rs:141`).
- Levels: `error` for a failed user action, `warn` for a fallback or a dropped request, `info` for what the
  user did or got (`opened '...'`, `document.rs:94`), `debug` for diagnostics, `trace` for every edit as
  `old -> new` (`log.rs:235`).
- Info and up reach the status bar and the Messages pane through the `LogRing` (`log.rs:39`, `RING_LINES`);
  the file log is at Trace in debug builds, Debug in release (`log.rs:142`). Libraries never print; the CLI
  prints its results with `println!`.

## Comments and docs in code

- Every library module opens with `//!`: what it is, units and invariants, and a `Threads:` line where threads
  are involved (`crates/wavedude-app-state/src/lib.rs:1`, `crates/wavedude-app/src/ui/shared.rs`); mostly, the
  `model` and `scene/shapes` files and CLI modules have none.
- `///` on public items is a terse noun phrase for types (`/// What the views send.`, `lib.rs:65`) and a
  third-person clause for functions (`/// Applies and clears the pending text.`, `edit.rs:20`); present state
  only; references the spec by path (`(docs/ui/60-live-editing-and-runs.md, Edit classes)`, `commands.rs:2`).
- Inline `//` comments are rare and explain a non-obvious why; no TODO or FIXME is left in `src/`.
- Source and docs are ASCII (`->`, `...`, `x`, `deg`); only the translated string tables hold other characters.

## State and views

- `AppState` (`crates/wavedude-app-state/src/lib.rs`) holds everything testable; the app crate is thin
  `impl Component` views over it.
- Views reach the state through `Cx::get()` (`crates/wavedude-app/src/ui/app.rs:237`) and `Shared`
  (`ui/shared.rs`): `read(topics, f)` subscribes to `Topics`, `peek` reads without subscribing, `update`
  mutates and re-renders the subscribed parts. Derived view data is a `use_memo` over `shared.read(...)`
  (`crates/wavedude-app/src/ui/tree.rs:447`). Contexts are `use_provide_context`/`consume_context` by type.
- Typed text applies through the one `PendingEdit` (`crates/wavedude-widgets/src/edit.rs:17`): an editor offers
  it, the window flushes it before a command, save, run or selection change, a model-writing press flushes first
  (`use_pending_edit`, `edit.rs:55`), an editor flushes on focus loss and unmount.
- Focus moves by a counter, served once the editor is laid out (`FocusRequest`, `use_focus_on_layout`,
  `edit.rs:132`).
- Text editing uses the own `TextField` on `freya-edit`, never Freya's `Input`
  (`crates/wavedude-widgets/src/text_field.rs:1`); `TextBox` and `NumberBox` build on it.
- Runs converge on a goal without debounce or quiet timers (`docs/ui/60-live-editing-and-runs.md`, decision
  0012). Views poll the state on `TICK` (16 ms, `app.rs:32`) and faster on `POLL` only while a picture is due.
  A deliberate delay is a named const run through `Delayed` (`crates/wavedude-widgets/src/delay.rs:9`,
  `HOVER_TIP_DELAY` in `ui/preview/pictures.rs`).
- Colours come from the `Theme` tokens (`use_wave_theme()`, `crates/wavedude-widgets/src/theme.rs:33`); fixed
  colours are for the preview overlays, graph boxes and colour picker.
- Interactive elements carry a stable test id through `TestIdExt::test_id` (`crates/wavedude-widgets/src/util.rs:7`).

## Strings

- Every UI string is a key in all four tables `crates/wavedude-app-state/assets/strings/{en,de,fr,nl}.json`;
  `crates/wavedude-app/tests/strings.rs` fails on a key missing from a language or unused in the source.
- Keys are literal `"Key"` strings in the code (the test searches for them); built keys use the prefixes
  `Label.`, `Hint.`, `Enum.`, `Kind.` through `Strings::label`, `hint`, `enum_word`, `kind`.
- Lookup: `s.get("Key")` on a `Strings` from `cx.strings()` (`crates/wavedude-app/src/ui/app.rs:261`);
  placeholders `{0}`, `{1}` with `s.format("KeyFormat", &[&a, &b])`, keys ending in `Format`
  (`ui/memory_warning.rs:73`).
- Plurals are separate keys chosen in code: `ItemCountZero`, `ItemCountOne`, `ItemCountMany`
  (`crates/wavedude-app/src/ui/properties.rs:653`). Menu texts mark the mnemonic with `_` (`"_File"`).

## Tests

- Unit tests sit in a `#[cfg(test)] mod tests` at the end of the file (`crates/wavedude-app-state/src/strings.rs`).
- Integration tests per crate in `tests/`: physics against `docs/physics.md` in
  `crates/wavedude-core/tests/physics.rs`, file IO with `proptest` in `tests/io.rs`, GPU against CPU in
  `tests/gpu.rs`, export decoding in `crates/wavedude-export/tests/`.
- Snapshots are text (`insta::assert_snapshot!` of hex or ASCII pictures, `crates/wavedude-core/tests/render.rs:268`),
  stored in `tests/snapshots/`, reviewed with `cargo insta review`.
- Tests that need hardware or tools return early with an `eprintln!` reason: no GPU adapter (`gpu_available`,
  `crates/wavedude-core/tests/gpu.rs:25`), no ffmpeg (`has_ffmpeg`, `crates/wavedude-export/tests/common/mod.rs:89`).
- Timing tests are `#[ignore = "how to run it"]` (`crates/wavedude-core/tests/render.rs:281`).
- App-state tests drive `AppState<FakeBackend>` (`live::fake::FakeBackend`, `live.rs:1015`) with a fake clock
  (`crates/wavedude-app-state/tests/state.rs:21`).
- Widgets and views are tested headless with `freya_testing` (`launch_test`, `sync_and_update`, `click_cursor`,
  `press_key`): widgets in `crates/wavedude-widgets/tests/headless.rs`, the window in
  `crates/wavedude-app/tests/shell.rs` and `crates/wavedude-app/src/ui/tests.rs` on an example project.
- A behaviour fix lands with the test that failed before it, in the same commit.

## Docs duties

- A behaviour change updates `docs/spec` or `docs/ui` in the same commit; a user-visible one also
  `crates/wavedude-app/assets/manual.md`.
- A design decision is a numbered ADR `docs/decisions/NNNN-slug.md`: `# NNNN Title`, a `Status: accepted, <date>`
  line, the decision, later changes as `## Topic (added <date>)` sections and noted on the Status line
  (`docs/decisions/0004-file-formats.md`); mostly, `0012` has `Date: ... Status: ...` and Context, Decision,
  Consequences sections.
- A deliberate difference from WaveDude Next is a row of the `docs/divergences.md` table
  (Area, WaveDude Next, WaveDudeRust, Where), pointing to the decision.
- Performance numbers go to `docs/measurements.md` with the date, the machine and the command that made them.

## Dependencies

- Versions shared by several crates live in `[workspace.dependencies]` and are used as `dep.workspace = true`;
  a dependency of one crate is declared in that crate (`freya`, `rfd` in `crates/wavedude-app/Cargo.toml`).
- Optional parts are features with a comment on why (`mp4`, `openh264-source` in
  `crates/wavedude-export/Cargo.toml`); `openh264-source` is for development and tests, not distributed builds.
- GitHub Actions are pinned to a commit hash with the version as a comment
  (`actions/checkout@3d3c42e... # v7.0.1`, `.github/workflows/ci.yml`); downloaded tools are checked against a digest
  (`FFMPEG_SHA256`).
