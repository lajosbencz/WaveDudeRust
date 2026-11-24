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

## Progress

| Phase | Done | Left |
|---|---|---|
| Skeleton | window, theme tokens, menus with shortcuts, pane layout, status bar, `just hot` | splitters, docking, Selawik font, icons |
| Document | open/save (`.wdproj`, Wave Dude 2D import), title, undo/redo, tree rows, check, move, remove, select | multi-selection, toolbar, add items, drag and drop, clipboard, recent files, unsaved edits prompt |
| Properties | a form per object from its reflected fields (`model::fields`): text, number, check and choice editors, edit errors | sections, units, number stepping, colour swatches, gradient rows, read-outs, part kind fields; Display edits skip undo |
| Preview | one wgpu device for engine and window, Run (F5) and Stop, re-run on edit, field and intensity drawn into textures with no readback, run state in the status bar | header strip, zoom, handles and overlays, probe graph boxes, busy/stale marks, frame at the playback position |
| Timeline and transport | playback at the frame rate and speed, loop, in/out range, live follow, scrub, wheel and arrow keys, run pause, frame/step read-out, the frame at the position drawn | tick labels, brackets drag, pane-scoped keys (`,` `.` I O X L F), field/intensity toggles and layout, editable speed |
| Messages | log ring (5000 lines) beside stderr, level toggles with counts, search, clear, newest Info line in the status bar, double-click on the status bar opens the pane | regex, auto-scroll, columns, copy, local time |
| Prompts | message window (info, question, error; Enter/Esc), unsaved edits prompt before New, Open, Close, Exit and window close, open/save errors, About | links and license toggle in About, export-running quit prompt |
| Settings | settings loaded and saved through `waveduderust_platform`, Settings window (sections, rows on the property editors), theme mode, undo steps, preview quality, auto re-run, recent files recorded | language, UI zoom, RAM read-out, presets, samples and dithering, OpenH264 rows, search, Recent Projects menu, window size and layout restored |
| Exports | Export field, intensity, both, field and intensity frame, Export last; save dialog with every format, MP4 through OpenH264, a thread per export, progress in the status bar, Stop cancels, errors as a message | the Export Save dialog's form (range, overlays, archive), probe CSV, the browser's exports |
