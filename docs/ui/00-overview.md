# UI overview

Source of truth: WaveDude Next WPF app (`WaveDudeNext.WPF/`), its `MANUAL.md`, `CLAUDE.md` and the
figures in `Assets/` (badge legend: `Assets/Legend.md`). File names below are lookup pointers only.

## Main window

Top to bottom (`MainWindow.xaml`):

| Area | Content |
|---|---|
| Menu bar | File, Edit, View, Simulation, Help. Not in the Tab cycle; F10 or Alt enters it. See [`10-menus-and-shortcuts.md`](10-menus-and-shortcuts.md). |
| Dock host | Tool panes around one fixed centre area. Centre area = the Preview (pictures, timeline, transport bar). |
| Status bar | 22 px, padding 4,0,16,0, items padded 4,2: last log line (left), run status while a run exists (right, no frame read-out), busy text + 120 x 12 progress bar (far right; a download first: an update, the codec), 16 px resize grip in the corner. |

No toolbar on the window. The Project pane has its own toolbar ([`20-project-tree.md`](20-project-tree.md)); the Preview has the
transport bar ([`50-timeline-transport.md`](50-timeline-transport.md)).

Window title: `WaveDude Next - <file>` with `*` appended for unsaved edits. Without a file the suggested
name of an unsaved import or example stands in; with neither, `Untitled`. Just `WaveDude Next` when no file
and nothing to suggest (the app name alone with no project; decision 0008).
File part = file name by default, the full path by a setting (decision 0008).

Window: min 800 x 500 DIP, default 1280 x 800, centred on first start, resizable with grip. Size, position
and state (never minimised) are persisted ([`80-settings.md`](80-settings.md)). A window is kept inside its monitor's work
area at start and after a zoom change.

## Panes and default arrangement

| Pane id | Title | Default side | First start |
|---|---|---|---|
| `project` | Project | Left | shown, left column upper third |
| `properties` | Properties | Right | shown, left column lower two thirds (below Project) |
| `messages` | Messages | Bottom | hidden |
| (centre) | Preview | - | always shown, the remaining width |

First-start layout: horizontal split, left column 1 share (vertical split Project 1 : Properties 2), centre
area 3 shares.

Docking behaviour (`Docking/`):
- Layout is a tree: splits (horizontal/vertical, star weights) with splitters, tabbed groups as leaves, one
  centre area that cannot be removed.
- Pane title bar: title, a drop-down button "Window position" (menu: Move to Left / Right / Top / Bottom,
  Close), and a close button. Close only hides the pane.
- Drag a title bar (whole group) or a tab (single pane): past the system drag threshold an overlay shows four
  outer-edge indicators, a five-way cluster (centre + 4 sides) on the hovered pane or centre area, a
  translucent preview of the drop area and a ghost label. Drop on edge = new pane at outer edge; centre of a
  group = tab into it; side = split next to it. Not into the centre area as a tab; not into own group.
- Edge drop takes a default share of the host; side drop a share of the hovered pane (exact fractions in
  `DockDropAdorner.cs`, `DockHost.cs`).
- Multiple panes in a group show a tab strip; the active pane's title bar is accent-coloured with white text
  and a light-blue grip, others inactive.
- Default arrangement: left column a quarter of the window (1 : 3 against the preview), Project over
  Properties 1 : 2.
- No floating windows.
- View > Panes lists every registered pane with a check mark; click toggles shown/hidden. A pane shown
  without a saved position goes to its default side.
- Status bar double-click opens the Messages pane, or focuses it when shown.
- The arrangement (incl. sizes) is saved on close and restored at start; unknown ids in a saved layout are
  ignored.
- View > Panes lists Preview too (decision 0008); in the WPF code the Preview is the centre area
  and not a registered pane, so only Project, Properties, Messages are listed.

## Look

dnSpy / Visual Studio tool-window look (look only, nothing copied).

- Font: Segoe UI 12 DIP (9 pt) everywhere; icons from the platform icon font (Segoe Fluent Icons / MDL2)
  or the app's own 16 px stroked drawings. Rust port: Segoe UI when installed, else the bundled Selawik
  (Microsoft's metric-compatible stand-in, SIL OFL), 12 px; 16 px icons.
- Themes: System (follows OS), Light, Dark; live switch, native title bars follow. Colour tokens live in
  `Themes/Dark.xaml` and `Themes/Light.xaml` (window, menu, status bar normal/warn/error, tool window,
  title bar active/inactive, tabs, tree, scroll bars, splitter, drop indicator, buttons). Dark examples:
  window `#2D2D30`, tool window `#252526`, accent `#007ACC`, status bar `#007ACC`, warn `#CA5100`,
  error `#B4211D`, tree selection `#3399FF`, inactive selection `#3F3F46`.
- Every tree row and toolbar button is 23 px high, the menu bar headers and tabs too; drop-down menu entries
  are 22 px with Selawik (2 + 2 + the text line + 2 + 2).
- Icons: 16 px, 1 px strokes on half pixels, ink within rows/columns 0..14 so they centre like text; drawn
  in the inherited text colour so a disabled row greys its icon with its name; greyscale (no subpixel AA).
- Keyboard focus is an accent border, never a dotted rectangle.
- Preview overlays (handles) use fixed colours, not theme colours: light grey over a dark halo, selected
  item bright yellow, keyboard focus accent fill.
- Lines drawn by the app (timeline, graph boxes) are whole device pixels at any scale.

## Keyboard first

- Every control reachable with Tab and arrows, focus visible. A mouse-only control is a bug.
- Menu bar out of the Tab cycle (F10/Alt).
- Every menu entry has a unique mnemonic within its menu, in all four languages (incl. shared strings such as
  Cut, Copy, Paste, Select all, the export entries, in every menu they appear in).
- Commands pressed by automation have an unmodified key.
- Everything automation needs has a stable id (WPF AutomationId). Rust port: keep the ids as widget test ids
  (Freya exposes an AccessKit tree, see decision 0002).

## UI zoom and DPI

- View > UI zoom: 0.5x, 0.75x, 1x, 1.25x, 1.5x, 2x, 2.5x, 3x. Live. Persisted (`UiZoom`, default 1).
  Command-line `--zoom=<factor>` (0.5..8) sets it at start.
- Zoom scales every window's content (layout scale), including dialogs, popups, menus, tooltips and context
  menus. Window sizes are multiplied by the change; the window is then kept on its monitor.
- Layout rounds to whole device pixels at the zoomed DPI: a line is 1 or 2 device pixels, never cut.
- Process is per-monitor DPI aware: each window renders at its monitor's scale; moving to a monitor of
  another scale resizes in pixels. Title bar stays at the monitor's scale.
- The 16-bit preview pictures are rendered at their device-pixel size and drawn 1:1 at any zoom; 8-bit
  pictures are scaled bitmaps.
- Handle glyphs, dashed probe lines and the marquee are antialiased by design.
- `--greyscale-text`: text without subpixel AA (used for manual figures).
- The manual window reloads its document when a zoom change changes the text rendering mode (text sharpens a
  moment after the change, about 0.5 s).

## Languages

- English, Deutsch, Francais, Nederlands, plus System (OS display language, English fallback).
- View > Language switches live: menus, tooltips, property labels and hints, tree folder names, pane titles,
  status words, enum words. Persisted (`Language`, "" = System).
- Spelling: British in the UI, except "license".
- Four string tables `Strings/Strings.{EN,DE,FR,NL}.xaml`: menu headers carry their mnemonic underscore;
  property labels/hints/enum words under `Label.<Type>.<Member>`, `Hint.<Type>.<Member>`,
  `Enum.<Type>.<Member>`; kind descriptions under `Kind.<Type>`.
- A key missing from a language falls back to English; a key missing from English is a bug and panics with the
  key.
- Converter output that contains words (run status, item counts, RAM cache entries) re-evaluates on a switch.
- Log messages are not localised; numbers in log lines use a decimal point always.
- The manual itself is English only.

## Platform-specific behaviour in the WPF app

Map or drop in the cross-platform port:
- Taskbar button progress (busy indicator mirror).
- File type registration per user (`.wdproj`, `.wdscheme`) at start-up; recent files also go to the taskbar
  jump list.
- A click on the title bar while a menu is open closes the menu and still lands (`Theming/MenuClickThrough.cs`).
- WIC/Media Foundation details belong to export (core spec).

## Startup

- Optional first argument: a `.wdproj`, `configuration.json` (import) or `.wdscheme` file to open.
- Switches: `--zoom=<f>`, `--greyscale-text`; unknown `--` switches are logged.
- First start (no settings file): preview quality chosen from the machine ([`70-dialogs-and-windows.md`](70-dialogs-and-windows.md)).
- Status bar's first text: "Ready", or the last start-up log line.
