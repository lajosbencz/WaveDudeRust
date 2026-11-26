# Rebuild on Slint

The app as it stood at `master` (v0.4.0), mapped for the rebuild of decision 0032: what each part did and where its
spec lives. Kept: `crates/waveduderust` (core), `crates/waveduderust_macros`, `crates/waveduderust_cli`. The rest is
rebuilt in `frontends/waveduderust_slint`; the old code stays readable at tag `v0.4.0`.

## Layout

Main window (docs/ui/00): menu bar, a dock of panes, status bar. Default arrangement:

| Pane | Spec | Contents |
|---|---|---|
| Project tree | docs/ui/20 | scene items by kind, multi-selection, drag to reorder, context menus |
| Properties | docs/ui/30 | the form of the selected item: number, text, check, combo boxes, colour swatches, gradient rows |
| Preview (field, intensity) | docs/ui/40 | header strip, picture, zoom, handles and overlays, probe graph boxes, busy/stale marks |
| Timeline and transport | docs/ui/50 | timeline with in/out points, transport bar, playback speed and loop, live follow |
| Messages | docs/ui/70 | log lines with counts and filters |

Windows and dialogs (docs/ui/70): message box, unsaved edits prompt, scene image changed on disk, memory warning,
welcome page, settings, updates, export save, about, examples, colour picker popup, crash report, first start.

Look (docs/ui/00, Look): the original's colours and layout; menus and shortcuts as tabled in docs/ui/10.

## Features

| Feature | Spec | Old home in `waveduderust_app` |
|---|---|---|
| Menus, shortcuts, context menus | docs/ui/10 | `menus.rs` |
| Selection shared by tree, handles, commands | docs/ui/20 | `selection.rs`, `outline.rs` |
| Model edits as commands | docs/ui/20, 30 | `commands.rs` |
| Property forms as data, units, number stepping | docs/ui/30 | `form/`, `number.rs`, `properties.rs`, `gradient.rs` |
| Document, file state, undo/redo (`UndoSteps`, decision 0030) | docs/ui/60 | `document.rs`, `history.rs` |
| Clipboard and drag text as `.wdproj` fragments | docs/ui/60 | `clipboard.rs` |
| Live run: re-run on edit, playback, memory warning | docs/ui/50, 60 | `live.rs`, `playback.rs`, `memory_warning.rs` |
| Preview: presenter, view fit/zoom, handles, graph boxes, timeline maths | docs/ui/40, 50 | `preview/` |
| Exports: video, frame, probe CSV; native and browser | docs/ui/70, decisions 0021, 0025 | `export/`, `codec.rs` |
| Settings (one sparse JSON), settings table | docs/ui/80, decision 0027 | `settings.rs`, `settings_table.rs` |
| User colour schemes (`.wdscheme`) | docs/ui/30 | `user_schemes.rs` |
| Strings (four tables, live language switch) | docs/ui/00, Languages | `strings.rs` |
| Log ring, Messages pane, status bar | docs/ui/70 | `log.rs`, `messages.rs`, `status.rs` |
| Modal queue | docs/ui/70 | `modal.rs` |
| Updates from GitHub releases | decision 0011 | `update.rs` |
| Files the OS asks to open | docs/ui/00, Startup | `open_requests.rs` |
| Examples | docs/ui/70 | `crates/waveduderust_examples` |

## Mechanisms

- **State and events.** `AppState` held the document, selection, live controller, settings, strings and log; views
  sent `Event`s and called `tick` about 60 times a second (`lib.rs`, `shell.rs`). On Slint: callbacks in, properties
  and models out, a `slint::Timer` for the tick.
- **Live run.** Natively a `RunWorker` on its own thread, polled each tick. In the browser a pumped worker in a module
  web worker (decision 0024, `remote/`), frames mirrored to the page.
- **Frame storage.** Frames up to the RAM budget in memory, the rest spilled: a file natively, OPFS in a spill worker
  in the browser (decision 0020, `platform/opfs/`).
- **GPU.** wgpu device shared by engine and renderer; the field drawn into a texture with no CPU readback. Slint takes
  the device through `require_wgpu_30(Manual)` and the texture through `Image::try_from` (spike, native and WebGPU).
- **Video.** OpenH264 natively (decision 0005), WebCodecs in the browser (`platform/webcodecs/`).
- **Platform split.** `platform/native.rs` and `platform/web.rs` chosen at compile time: files and threads vs uploads,
  downloads and local storage.
- **Wake.** Other threads wake the window when news arrives (`wake.rs`); on Slint `invoke_from_event_loop`.

## Divergences kept

Settings and other documented divergences stand: docs/divergences.md, decisions 0010 (solver), 0027 (settings), 0028
(no manual), 0030 (undo cap).

## Plan

State per feature: [`parity.md`](parity.md), built 2026-11-24 from WaveDude Next's source and [`ui/`](ui/); on
2026-11-25 827 done, 86 partial, 174 divergence, 3 missing of 1095 rows. A feature is done when its row is checked against Next side by side on a
nested display ([`tools/compare-csharp/README.md`](../tools/compare-csharp/README.md)); the commit that ports it ticks the
row. Each phase is one coding pass, then a separate verification of the diff, the checks and the rows before the commit.

1. Foundations every area uses: Next's icons as SVG (rows I-), Next's theme values and the bundled Selawik font
   (X-21, X-23), tooltips (X-28), WPF box model and focus border (X-26, X-27), a number box widget (P- rows), resizable
   splitters (X-07), UI strings through `@tr` with the four languages (X-34).
2. Menus and shortcuts (M-): every entry, shortcut text, mnemonic, enabled rule, icons.
3. Project tree (T-): toolbar and add menus, context menus, multi-selection, keys, drag and drop.
4. Properties (P-): sections, editors, every kind-specific form. Next is the feature spec, never the etalon: its
   features become our model's rules and idiomatic Slint, not its setters, converters or styles. After the forms of
   settings, sources, probes and shape, lens and outline parts, a rework before the remaining kinds:
   1. An edit rule per model type in the core: an edited object is told what it was before and makes its fields agree
      (a detector's layer, a linked side, a triangle's preset, a lens's name with its mirror box).
   2. Typed forms: a Slint struct per form, Slint enums for the model's words, editors bound two-way to the struct's
      fields, actions as typed callbacks, a list as a model of structs. Rust converts between model and struct by
      exhaustive matches and applies each commit at once. No string paths, index or
      `-1` conventions, no integer codes. A compact form row component: label, hint once, editor.
   3. Typed boundary elsewhere: commands, tree picks and drop places, run states as Slint enums.
   4. UI text only in `.slint`: Rust sends data (node kinds, states, message codes and arguments); words Rust needs for
      native dialogs are read from a Slint global; numbers use `Platform.decimal-separator`. Logs stay untranslated.
5. Preview (V-): header strips, overlays, handles and drag editing, graph boxes, zoom.
6. Timeline, transport, status bar (L-), live editing and runs (R-), Messages pane (G-).
7. Dialogs and settings (D-, S-), then docking and window persistence (X-).
8. The classic app's architecture and performance decisions, lost with the app crates (the core's are kept). Each
   step ends with the classic measurement it restores, re-run on the Slint app
   ([measurements.md](measurements.md)); the code to port is at `master` before the merge of `rewrite/slint`.
   1. Woken by news only: the core's `RunWorker::set_notify` and the scene image watch wake the window through
      `slint::invoke_from_event_loop`; the timer runs only while a run, playback or a drag is in progress. Measure:
      The idle window (0 wakeups).
   2. The browser's pump: pumped until `Pump::Waiting` with a waker that wakes the window, and the GPU queue kicked on
      `Kicks` while completions are owed (decision 0026). Measure: The browser's GPU latency (Firefox).
   3. The browser's CPU run in a module worker (decisions 0024, 0035): `remote/` and `wavedude-run-worker` of `master`
      as `waveduderust_remote` and `waveduderust_workers`, the frames posted to the page's mirror store and coloured
      on the page; the GPU engine stays pumped on the page; Use the GPU swaps the two. A lost worker in Messages.
      Measure: The browser's live run in a worker; the page's main-thread time during a run, on both.
   4. The browser's frame spill in OPFS through the spill worker (decision 0020, `platform/opfs/` of `master`) and
      playback reading ahead (`FrameStore::prefetch`), parity L-65. Measure: The browser's frame spill, read ahead,
      writes. Done 2026-11-26: `waveduderust_remote::opfs` and `wavedude-spill-worker`, one spill worker and session
      per page shared by both runs; without one the frames past memory are dropped, said once
      ([measurements.md](measurements.md), The browser's frame spill in the Slint app).
   5. Exports from the live run's store, waiting for its full-quality frames (decision 0012, `ExportGate` of
      `master`), not a run of their own (parity D-47, R-25, R-26). Then the browser's exports (decision 0025: names only,
      always ZIP, WebCodecs MP4, the OPFS spool, Download), parity D-51. Done 2026-11-26: the browser's exports read
      the live run's store through the same export hold, the job stepped on the page; `waveduderust_remote::opfs::Spool`
      (feature `spool`) writes a run's files in the spill session's folder, the page's WebCodecs encoder
      (`export/webcodecs.rs`) writes MP4, and a completed export offers its file in "The export is ready" with Download.
   6. Fallbacks: no GPU adapter starts the app on Slint's software renderer and the CPU engine, the pictures coloured
      on the CPU; in the browser WebGL2 and the CPU engine, the reason in Messages (parity V-43); the browser's CPU
      engine in a SIMD build beside the scalar one (decision 0034); a lost GPU device reported and the run rebuilt.
      Done 2026-11-26 in the browser: without a usable WebGPU device (none, a software adapter, no compute shaders)
      the window takes Slint's own device, WebGPU or else WebGL2 through wgpu's GL backend, the run the CPU engine in
      the run worker, the pictures coloured on the CPU, the reason in Messages, About's GPU setting none (parity
      V-43); the GPU engine stays off WebGL2, which runs no compute shaders. The run worker's SIMD build in
      `pkg-simd/`, chosen by `run-worker.js`'s probe (decision 0034).

Known bugs: a shortcut (Save, Run, Undo) pressed while a number box holds typed text acts before the box commits.
