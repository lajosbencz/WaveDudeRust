# wavedude-mineral: design

A new WaveDude UI on egui-mineral (`egui_mineral` core and `egui_mineral_material`, the MD3 stylekit), over the
UI-free state of `waveduderust_app`. Parity yardstick: [PARITY.md](PARITY.md) (ids below). Library:
`egui_mineral`, `egui_mineral_macros` and `egui_mineral_material` 0.9.0 from crates.io, versioned per crate (changes go
through Library gaps and Blockers below).

The layout follows what users do (section 1), not the component library's navigation patterns: the workspace is the
app and every other surface a page over it. Built in five phases (ff18aa9 to 6607ed1); it is the Material
frontend, one of two egui frontends ([decision 0031](../../docs/decisions/0031-modular-core-and-frontends.md)).

## 1. Users and tasks

| User | Comes for | Device | Session |
|---|---|---|---|
| Student | see a phenomenon (interference, diffraction, focus, waveguides) | laptop, browser, tablet | open an example, run, scrub, change one number, run again |
| Teacher | demonstrate in class | projector, tablet | open a prepared project, run with the canvas large, pause, scrub back, tweak while explaining, export a clip for slides |
| Hobbyist (Huygens Optics audience) | rebuild and extend a video's setup (logic gates, fibres, gratings) | desktop, browser | open the matching example, move parts, watch the intensity, export a video |
| Researcher / engineer | design a part (grating, metalens, gate) and measure it | desktop | build a scene from parts or an image, place probes, long runs, many small edits, probe CSV, frame stills; keyboard heavy |

Examples carry the teaching: 53 projects in 6 topics ([EXAMPLES.md](../../EXAMPLES.md)), each with a description
of what it shows. Next users (the README's drop-in promise) know the menu bar and its keys
([docs/ui/10](../../docs/ui/10-menus-and-shortcuts.md)).

Core loop:

```
pick (example / file / blank)
  -> place and tune (sources, parts, probes on the canvas; numbers in the inspector)
  -> run (F5)
  -> watch (field + intensity over time, probe graphs)
  -> tweak live (edit while running; the run converges on the edit)
  -> compare (field vs intensity, scrub, undo / redo flip, probe graphs)
  -> export (video, frame, probe CSV)
most of a session loops tweak <-> watch
```

| Task | Frequency | Weight | Must be |
|---|---|---|---|
| Run / Stop / Pause run | many per session | critical | one visible button and one key in every window class |
| Play / Pause, step, scrub | constant | critical | always visible under the canvas |
| Select an item | constant | critical | a click / tap on the canvas; the outline for hidden, overlapping, disabled items |
| Move / rotate / resize | constant | high | a drag on the canvas |
| Edit the selected item's numbers | constant | high | beside the canvas on desktop, one tap on touch; the canvas stays visible while editing |
| Brightness / Auto | frequent | high | on the picture it affects |
| Add a source / part / probe | frequent (builders) | high | one button on the canvas; `A` at the pointer |
| Undo / Redo | frequent | high | keys; buttons on touch |
| Open an example | each session start (students, teachers) | high | the first surface at launch; reachable mid-session; by name from the palette |
| Open / Save | each session | high | keys, File menu |
| Export | per result | medium | one button (Export last) plus a menu |
| Field / intensity / both, layout | per project | medium | canvas toolbar |
| Messages | when stuck | low | a click on the status line |
| Settings, About, updates | rare | low | menu and palette only; never on screen |

One gesture away: Run / Stop, Play / Pause, scrub, select, move, edit the selected item, brightness, add, undo,
command palette.

## 2. Principles

- The workspace is the app: canvas, run bar and status line are always on screen; only an overlay page the user
  closes (gallery, Settings, About) covers them.
- Selection drives editing: the inspector always shows the current item; there is no edit mode.
- Watching never stops: no surface stops a run or playback; only questions (`Dialog`) are modal.
- One command vocabulary (`waveduderust_app::menus::MenuAction` + `menus::enabled`, plus this crate's view commands)
  behind every entry point: menu bar, palette, toolbars, context menus, keys.
- Next's keys stay ([docs/ui/10](../../docs/ui/10-menus-and-shortcuts.md)); new keys only add.
- State lives in `AppState`; views read it by reference and emit intents; off-thread work goes through mineral's `Bus`.
- Layout by MD3 window class (`adaptive::WindowClass::of`), never fixed widths; touch is a pointer model, not a class.
- A behaviour that differs from `docs/ui` is a PARITY row `changed: <note>` with its reason from section 1.

## 3. Workspace

Regions in every class: top bar, canvas (pictures, canvas toolbar, graph boxes, selection bar), run bar, status
line, outline, inspector, messages. The window class decides where outline, inspector and messages sit.

| Region | Expanded+ (>= 840) | Medium (600..840) | Compact (< 600) |
|---|---|---|---|
| Top bar | `MenuStrip` (File Edit View Simulation Help), title, palette field, Export `SplitButton` | `TopAppBar` small: menu button (the same groups as submenus), title, Undo, Redo, palette, Export | `TopAppBar` small: menu button, title, palette, overflow (Undo, Redo, Export) |
| Canvas | centre | full width | full width |
| Run bar | docked under the canvas column; one row, two below 720 points (timeline on its own row) | docked bottom, two rows | timeline strip over a `BottomAppBar` (Play / Pause, previous, next, Outline, Inspector) with `Fab` Run / Stop |
| Outline | left column (>= 1200); top of the right column, `SplitPane` 1 : 2 over the inspector (840..1200) | modal `SideSheet` from the left | modal `BottomSheet` |
| Inspector | right column, resizable 320..480 | `BottomSheet`, detents 0.4 / 0.9, opens on a canvas selection | `BottomSheet`, detents 0.35 / 0.6 / 0.95 |
| Messages | bottom panel above the status line | modal `SideSheet` | full-screen `Dialog` |
| Status line | bottom strip | bottom strip | none: run status in the timeline strip, warnings and errors as `Snackbar` |

- Columns are egui `Panel::left/right/bottom` with token `Frame`s (showcase pattern) until gap 18 (docked
  `SideSheet`) lands; widths and visibility last for the session (SET-11), never in the shared settings file.
- View > Outline (Alt+1), Inspector (Alt+2), Messages (Alt+3), Canvas only (Alt+0: hides outline and inspector for
  projectors and small laptops; again restores them).
- Standard sheets have no scrim: the canvas stays interactive, so a number edited in the sheet shows in the run above.
- Touch-first devices keep UI zoom 1x and density 0 (touch targets come from density), so a landscape tablet stays
  Expanded: the desktop layout with the touch pointer model (section 4).

```
Expanded+ desktop (>= 1200; at 840..1200 the outline moves above the inspector in the right column)
+------------------------------------------------------------------------------------------------------+
| File Edit View Simulation Help          Lens demo *                  [Search  Ctrl+K]  [Export |v]   |
+--------------+-----------------------------------------------------------------+---------------------+
| OUTLINE      | [Field|Both|Intensity] [=|] [fit - +] [handles labels] [+Add v] | [o] Lens 1  [on] :  |
|  Scene       |                                                                 | Lens: biconvex ...  |
|  Simulation  |  +------------------------------+------------------------------+| v Position          |
|  Display     |  | Field       [1.20] [A]       | Intensity   [0.80] [A]       || [X   400] [Y   300] |
| Sources 2  + |  |                              |                              || [Rotation      0.0] |
|  [x] Laser   |  |      o---- )  (              |      .:::::.   .:::.         || v Shape             |
|  [x] Point 2 |  |                              |                              || [R1  120] [R2 -120] |
| Parts 3    + |  |  +---------+                 |                              || [Diameter       80] |
|  [x] Lens 1  |  |  | probe A |~~~~             |                              || v Material          |
|  [ ] Wall    |  |  +---------+                 |                              || [Index         1.5] |
| Probes 1   + |  +------------------------------+------------------------------+|                     |
| Schemes 1  + +-----------------------------------------------------------------+                     |
|              | (>Run) (||) | |< [>] >| |===[#####|####]====------| 120/500 step 1200  Live Loop 1x : |
+--------------+-----------------------------------------------------------------+---------------------+
| Saved lens.wdproj       Running: 1200 / 5000 steps, GPU: RTX    Computing frame 121 [==   ]  RAM 1.2/4 |
+------------------------------------------------------------------------------------------------------+
```

```
Medium (600..840), inspector sheet at 0.4
+----------------------------------------------------------+
| [=] Lens demo *                  [undo] [redo] [Q] [Exp] |
+----------------------------------------------------------+
| [F|B|I] [fit] [+Add]                                     |
|  +------------------------+------------------------+     |
|  | Field    [1.20] [A]    | Intensity [0.80] [A]   |     |
|  |    o--- )  (           |     .:::::.            |     |
|  |  [Lens 1 on dup del :] |                        |     |
|  +------------------------+------------------------+     |
+----------------------------------------------------------+
| |===[########|####]======------|  120/500 step 1200      |
| (>Run) (||)  |< [>] >|  Live Loop 1x  [outline]  :       |
+----------------------------------------------------------+
|                         ----                             |
| [o] Lens 1                         [on] [dup] [del] :    |
| v Position   [X 400] [Y 300]                             |
| v Shape      [R1 120] [R2 -120]          (drag up: 0.9)  |
+----------------------------------------------------------+
| Running: 1200 / 5000 steps, GPU            RAM 1.2/4     |
+----------------------------------------------------------+
```

```
Compact (< 600)
+--------------------------------+
| [=] Lens demo *       [Q]  :   |
+--------------------------------+
|                        [fit]   |
|  Field         [1.2] [A]       |
|     o--- )  (                  |
|   [Lens 1  on  dup  del  :]    |  <- selection bar
|  Intensity     [0.8] [A]       |
|     .:::::.                    |
|                        [+]     |
+--------------------------------+
| |==[####|##]==---|  120/500    |
+--------------------------------+
| [>] [|<] [>|] [list] [tune] (O)|  <- BottomAppBar, Fab Run / Stop
+--------------------------------+
```

## 4. Canvas

- Pictures: field and intensity, side by side or stacked, `SplitPane` between them (PREV-01..04). One
  `MineralWidget` per picture (click + drag, focusable): texture or GPU paint callback, scene outline, edge line,
  overlay shapes (`waveduderust::unstable::view::overlay`), graph boxes, busy dim (`LoadingIndicator`), marquee. Logic from
  `waveduderust_app::preview::{handles, overlay, graph_boxes, view, timeline}`.
- Picture header: a strip floating on the picture's top edge: title, brightness `NumberField` (log, gap 9), Auto
  `IconButton`; when narrow the title goes before the field; never turned (PREV-05).
- Canvas toolbar: `Toolbar::floating`, top-left: `SegmentedButton::single_optional` Field / Both / Intensity, sized
  to their labels (= Display layer Preview; none selected when both are hidden), layout toggle (side by side / stacked), Fit, Zoom out, Zoom in,
  Handles, Labels (`IconAction::selected`), Add (`Menu` with the outline's add menus). Compact:
  `Toolbar::floating(..).axis(Axis::Vertical)` on the right edge with Fit and Add; the rest is in View.
- Selection bar (touch-first devices, and Medium / Compact): `Toolbar::floating` above the selection's bounds (below
  them at the top edge, hidden during a drag): name, Enabled, Duplicate, Remove, more (`Menu`: Cut, Copy, Move up,
  Move down, Export probe data as CSV). On desktop the inspector header and the context menu hold the same actions.
- Handles: each focusable handle is its own small `MineralWidget` at its rect, agent key
  `canvas-<layer>-<item>-<handle>`, Tab order = overlay order (HND-14, WIN-10). Emitter dots and outlines are paint.
- Mouse: press on a handle = select + drag; Ctrl / Shift click toggles / adds; drag on empty space = marquee; click
  on empty space deselects; wheel zooms about the pointer; middle drag or Space+drag pans; Esc reverts a drag
  (HND-09..13, PREV-08).
- Touch: tap selects; drag on a handle moves; one-finger drag on empty space pans when zoomed, else marquee after a
  long press; pinch zooms; a second finger cancels a drag and reverts it (PREV-09); long press on an item opens the
  selection bar's more menu.
- Context menu (`Menu::show_context`): on a picture Brightness, Auto brightness, Export this frame, Export field /
  intensity / both (PREV-16); on a handle the outline row's menu; on a graph box Export probe data as CSV (GRAPH-07).
- Add at the pointer: `A` with the canvas focused opens the Add menu at the pointer; the item lands there
  (`Command::Add* { at: Some(point) }`). The toolbar's Add places at the canvas centre (TREE-18).
- Drop of outline rows (`dnd::drag_source`) onto a picture pastes at the drop pixel (TREE-23).
- Focus: the canvas takes keyboard focus after a project opens and after a click on it, so Space plays and F5 runs.
- Both pictures hidden: `EmptyPlaceholder` with a Show field action.

## 5. Run bar, timeline, status

Order (also the Tab order): Run / Stop (`PushButton` filled, play / stop icon by run state; `Fab` on Compact), Pause run
(`IconButton`, selected while paused), divider, Play / Pause (one `IconButton`, icon flips), previous, next,
timeline (flex), read-out "frame / last  step N", Live (`IconButton` selected; disabled without a run), Loop
(`IconButton` selected), speed (`Chip::assist` "1x" -> `Menu`: presets 0.25..4 and Custom, a `Popover` with a
`NumberField` 0.01..100), overflow `Menu`: Set in, Set out, Clear range, Discard frames.

- Timeline: app `MineralWidget` on `range::RangeModel::new(0, last)` (keys, AccessKit `SetValue` / `Increment` /
  `Decrement`), drawn with `waveduderust_egui_preview::timeline`: ticks, in / out brackets (draggable), held frames
  shaded, playhead, hover tooltip (TL-01..08). The shaded span is the run's progress.
- Run status in the status line ("Running: 1200 / 5000 steps, GPU: ...", RUN-12); on Compact in the timeline
  strip's tooltip and a `Snackbar` on Completed / Failed.
- Busy: status `LinearProgressIndicator::indeterminate` "Computing frame n"; picture dim after `preview::BUSY_DELAY`.
- Memory warning, Discard confirmation, export waiting: `Dialog`s from the modal queue (MEM-01..04, RUN-04).
- Status line: last log line (warning / error colours from tokens), run status, busy text + `LinearProgressIndicator`, RAM
  `Chip` "1.2 / 4 GiB" (MEM-05). A click opens Messages (LOG-04). One priority: a busy item (download, then export,
  then the frame being computed; `waveduderust_app::status::busy`) takes the log line's place with its bar and hides the
  run status; without one the log line and the run status show.
- Tick: `AppState::tick(now)` every pass; `request_repaint_after(window::TICK)` only while a run, playback or export
  is active; `waveduderust_app::wake` requests a repaint for worker events.

## 6. Outline and inspector

Outline (`TreeView`, BodyMedium rows, TitleSmall sections; with a pointer at `Density::MIN`, rows 40 px indented 24; touch-first keeps
the library's rows, at least the 48 dp touch target):
- Pinned rows Scene, Simulation, Display; then sections Sources, Parts, Probes, Colour schemes, each with a count
  and a `+` `IconButton` (Parts: kind `Menu`; Probes: detector `Menu`; anchored at the `+` (`Menu::show_at`); kind
  descriptions as item tooltips; TREE-03 / 04).
- Item row: Enabled check box (`ListSlot::checkbox`, gap 4), kind icon (lens profile painted, gap 5), name; scheme row: gradient strip.
  No per-row up / down / remove buttons: those are in the context menu, the inspector header and the keys.
- Selection mirrors `AppState.selection` (TREE-11 / 12): click, Ctrl, Shift, Insert; reorder by drag
  (`ReorderableList`, gap 3) with a mouse; click only on touch (TREE-25).
- Row context menu = docs/ui/10 Project tree row (TREE-19), `Menu::show_context` (gap 11).

Inspector:
- Header: kind icon, name `TextField` (buffered, gap 10), Enabled `Switch`, Duplicate, Remove, overflow (Cut, Copy,
  Move up, Move down, Export probe data as CSV, my schemes). Several selected: "Lens 1 and 2 more"; the actions apply
  to all; the form is the current item's.
- Kind description (`Kind.*`) collapsible under the header, its state persisted (PROP-17).
- Body: one `Accordion` panel per form section; fields full width with the label inside; grid
  `columns = clamp(w / 180, 1, 2)`; pairs (X / Y, Width / Height, R1 / R2, A x / A y) share a row when two fit.
- No selection: the Project form, description first (an example's description says what to try).
- Fields: `NumberField` (compact, scrub, wheel, log: gap 9), `Select` (`label_visible(false)` in grids, gap 7),
  `CheckButton`, `Switch`, connected `ButtonGroup` (2..4 choice enums), colour swatch -> `Popover` + `ColorPicker`
  (gap 13; Esc reverts, gap 12), gradient bar (app widget), point / node / layer lists (`ReorderableList`),
  read-outs as `DescriptionList`, hints as `PlainTooltip`.
- Edits apply live and commit on Enter / blur; an action that reads the model commits the typed text first (PROP-06,
  gaps 9 / 10). Esc reverts the field and focus returns to the canvas.
- Enter on a canvas or outline selection moves focus to the inspector's first field (TREE-26); F2 opens the name for editing.

## 7. Overlay pages: gallery, Settings, About

Routes (`nav::Router<Route>`): `/` workspace (always drawn), `/examples` gallery, `/settings/{section?}`, `/about`.
A non-root route draws a full-window page over the workspace (its own `Panel` stack, a `TopAppBar` with a back
arrow, the body in `scroll::page`: a solid bar in its own gutter); Back, Esc or the same command again returns. A run goes on underneath.

Gallery (`/examples`; File > Examples..., File > Start page..., palette, the empty workspace's action):
- Shown at launch while `ShowStartup` is on and no file argument opened a project.
- Start strip: New, Open... (`PushButton`s); Recent (up to 5 `Chip::assist`; File > Recent has all 10); Preview quality
  `Card` (RAM line, `ButtonGroup` Low / Medium / High / Auto, levers line); Show at launch `Switch`.
- Filter: `SearchBar` (titles and descriptions) and topic `Chip::filter`s (All + 6 topics; wrapping; one scrolled row
  on Compact); a search without a hit in the topic falls back to All with a note.
- Cards: `Card` (filled, clickable) in a grid `columns = clamp(w / 280, 1, 4)`: scene thumbnail (the scene
  painted by `waveduderust::unstable::scene` from the project, off-thread, cached), title, topic, description clamped to 3
  lines (full text in a `PlainTooltip` and in the inspector once opened). Click or Enter opens (guarded open);
  arrows move in the grid.
- The palette's "Open example: <title>" opens one without the gallery.

```
Gallery, Expanded+
+--------------------------------------------------------------------------------------+
| <-  Examples                                                      [Search examples]  |
+--------------------------------------------------------------------------------------+
| [+ New]  [Open...]   Recent: (lens.wdproj) (xor-test.wdproj) (grating.wdproj)        |
| +-Preview quality-------------------------------------------------+ [x] Show at launch|
| | RAM 16 GiB  [Low][Medium][High][Auto]  full resolution, every frame, 8-bit |      |
| +-----------------------------------------------------------------+                  |
| (All) (Sources) (Lenses) (Gratings) (Metasurfaces) (Guides, logic) (Detectors)       |
| +--------------+ +--------------+ +--------------+ +--------------+                  |
| | [thumbnail]  | | [thumbnail]  | | [thumbnail]  | | [thumbnail]  |                  |
| | Single fiber | | Coupled fib. | | XOR          | | Lens vs      |                  |
| | Guides       | | Guides       | | Guides       | | Lenses       |                  |
| | A wave in a  | | Two cores    | | Two inputs   | | Fresnel ...  |                  |
| +--------------+ +--------------+ +--------------+ +--------------+                  |
+--------------------------------------------------------------------------------------+
```

```
Gallery, Compact
+--------------------------------+
| <-  Examples              [Q]  |
+--------------------------------+
| [+ New] [Open...]              |
| Recent: (lens) (xor-test)  ->  |
| Quality [Low][Med][High][Auto] |
| (All)(Sources)(Lenses)(Gra.. ->|
| +----------------------------+ |
| | [thumbnail]                | |
| | Single fiber      Guides   | |
| | A wave in a single core... | |
| +----------------------------+ |
| +----------------------------+ |
| | ...                        | |
+--------------------------------+
```

Settings (`/settings`; File > Settings..., Ctrl+,): list-detail at Expanded+, list then
section page below; `SettingRow` stacks label over control below 560 points; `SearchBar` over labels, supporting
text and sections; `EmptyPlaceholder` "No setting matches"; every change is an `Event::Set*` (applies and saves, SET-01).
Language is not shown (SET-03 dropped).

About (`/about`; Help > About): version, credits, OpenH264 notice, system info + Copy; licences in
an `Accordion` rendered with `egui_commonmark`.

## 8. Commands, menus, keys

The menu bar (Expanded+) comes from `waveduderust_app::menus::MENU_BAR` with this window's changes; Medium and Compact
show the same groups as submenus of the top bar's menu button.

| Menu | Entries |
|---|---|
| File | New, Open..., Recent >, Examples..., Start page..., Save, Save As..., Close, Export > (field, intensity, both, field frame, intensity frame, probe data as CSV), Export last, Settings..., Exit |
| Edit | Undo, Redo, Cut, Copy, Paste, Delete, Select all, Duplicate, Enabled, Move up, Move down, Add source, Add part >, Add probe > |
| View | Outline, Inspector, Messages, Canvas only, Field / Both / Intensity, Side by side / Stacked, Handles, Labels, Fit, Zoom in, Zoom out, UI zoom >, Command palette |
| Simulation | Run, Pause run, Stop, Discard frames, Play / Pause, Live, Loop, Set in, Set out, Clear range |
| Help | Project page on GitHub, Keyboard shortcuts, Check for updates..., About |

- Export `SplitButton` in the top bar: main action = Export last (Ctrl+E) once there is a last export, else Export
  both...; its menu = File > Export.
- Keys: every window-wide key of docs/ui/10 (files, edit, export, UI zoom, run F5 / F6 / Shift+F5 / Esc, playback F7,
  Settings Ctrl+,). Canvas focus scope: Space , . I O X L F Home End (TR-12), `A` add at the pointer, Enter to the
  inspector, arrows / Del on handles (HND-14). New window-wide: Ctrl+K and Ctrl+Shift+P palette, Alt+0..3 views,
  F1 Project page on GitHub, F2 rename.
- Binding: `shortcut::bind(ctx, Scope::Global, key, ActionId)` + `shortcut::take_action`; canvas letters in
  `Scope::Focus(canvas_id)`; dialogs in `Scope::Modal(id)`; one table (`keys.rs`) `ActionId` -> command. A key shown
  on a menu item and on a palette entry is bound once (gap 1). `shortcut::label` renders key text.
- egui zoom and quit keys off (`look::install`); Ctrl+= / - / 0 and Alt+F4 are app commands. Mnemonics and readline
  keys follow the runtime's platform policies.
- Apps / Shift+F10 opens the focused element's context menu (KEY-02); Alt+letter opens a menu (`MenuStrip` mnemonics);
  F10 / lone Alt: gap 23.
- Palette groups: File, Edit, Simulation, Export, View, Settings rows, Examples ("Open example: <title>"), Items
  ("Go to Lens 1": selects it and focuses the inspector); disabled entries greyed by `menus::enabled`.
- Help > Keyboard shortcuts: a `Dialog` listing the key table with `Kbd`.

## 9. Material where it serves, domain widgets where it does not

| Need | Choice |
|---|---|
| Menus, keys, mnemonics | `MenuStrip`, `Menu`, `MenuItem` (shortcut, mnemonic, submenu), `CommandPalette` |
| Run / Stop | `PushButton` filled on a bar; `Fab` in `BottomAppBar` on Compact (a FAB has no place on a dense desktop bar) |
| Toggles on bars | `IconButton::toggle`, `IconAction::selected`, connected `ButtonGroup` |
| Side columns, sheets | egui `Panel` + token `Frame` (gap 18); `SideSheet::show_modal`, `BottomSheet` on Medium / Compact |
| Inspector forms | `Accordion`, `NumberField`, `TextField`, `Select`, `CheckButton`, `Switch`, connected `ButtonGroup`, `ColorPicker` in `Popover`, `DescriptionList` |
| Gallery | `Card`, `Chip::filter`, `Chip::assist`, `SearchBar`, `ButtonGroup`, `Switch`, `EmptyPlaceholder` |
| Feedback | `SnackbarHost`, `Dialog`, `LinearProgressIndicator`, `Chip` |
| Domain (no MD3 equivalent) | pictures and their zoom, handles and overlays, graph boxes, timeline with range brackets and held frames, gradient stop editor, lens-profile row icons, status line |

## 10. Theming

- Fonts: `ctx.set_fonts(waveduderust::unstable::view::fonts::definitions())` first, then `Runtime::new(ctx, Material::new(..))`
  (the stylekit's icon fonts), then `fonts::install_medium` with the bundled bold face.
- Scheme: `SchemeSpec::new(#007ACC)`, variant `TonalSpot`, through `rt.kit().apply`; light / dark through
  `rt.kit().set_preference`. The runtime styles its own root `Ui`s only, so `look.rs` copies `rt.style()` into egui's
  styles for egui's tooltips and popups.
- `Theme::System` follows the OS: native through `waveduderust_egui_platform::os_theme`, browser through `prefers-color-scheme`
  (WIN-05).
- Overlay, edge line and graph box colours stay fixed (not themed). Status warning / error colours from tokens.
- Density: `rt.kit().set_density(Density::new(-2))` with a mouse, `0` on a touch-first device.

## 11. Strings (English only)

Reuse `waveduderust_app::strings::Strings`, pinned to `Language::English`. The runtime's locale (mineral's own
messages and number formats) follows the Language setting resolved by `Language::resolve`: `rt.i18n().set_locale`
with en-US, de-DE, fr-FR or nl-NL; the library ships en-US messages only, the others fall back to them.

Why: 963 keys exist, 450+ reflection-keyed (`Label.<Type>.<Member>`, `Hint.*`, `Enum.*`, `Kind.*`) and looked up
from the model's `Fields` names; `AppState` formats titles, run status and messages through `Strings`. A Fluent copy
would duplicate and drift. New UI keys go into `waveduderust_app/assets/strings/en.json`. The shell calls
`app.strings.set_language(Language::English)` after start and never sends `SetLanguage`.

## 12. State wiring

```
Shell {
    rt: Arc<Rt>,                      // Runtime<Material>; `rt.pass` wraps each pass
    app: waveduderust_app::App,      // AppState<CoreBackend>: the only model
    shell: ShellState,                // waveduderust_app::shell: modals, exports, updater, close flow, tick
    jobs: Jobs,                       // waveduderust_egui_platform::jobs: off-thread work over the bus
    router: Router<Route>,            // `/` workspace; overlay pages
    view: ViewState,                  // UI only: column visibility and widths, sheet detents, outline mirrors,
                                      // canvas View (zoom), split fraction, pending field commit, palette
    preview: PreviewState,            // Presenter, Delivery, SceneWatch, textures
}
```

Pass order:
1. `eframe::App::ui` runs the pass inside `rt.pass(ui, ..)`; dropped files and paste text are read from egui's input.
2. Drain `bus` (`Done` -> intents); `app.tick(now)`; `app.sync_revisions()` -> view caches
   keyed by `Revisions` rebuild only when their counter moved.
3. Bind shortcuts; `take_action` -> `Intent::Act(MenuAction)` or `Intent::View(ViewCommand)`.
4. Workspace regions, then the overlay page if any; views take `&App` and `&mut ViewState` and push `Intent`s:
   `Send(Event)`, `Act(MenuAction)`, `Start(Job)`, `Go(Route)`, `View(..)`. No view mutates `AppState`.
5. Apply intents in order: pending field commit first (PROP-06), `Act` -> `menus::enabled` check -> events / jobs;
   `Send` -> `app.handle(e, now)` -> `Response` -> modal queue / clipboard / `Job`.
6. Modal top, snackbar host, palette; repaint scheduling; `rt.pass` ends the runtime's pass.

`Job`s (file dialogs, file IO, export jobs, update check / download, codec download, browser clipboard read, gallery
thumbnails) run on the spawner; their `Done` re-enters as `Event`s next pass.

Selection: `AppState.selection` is the truth; outline, canvas and inspector read it; their clicks become `Command`s.

## 13. Library gaps

Items 1..22 were in the library revision brief. "Until then" is what this crate does; a closed gap whose "Until then"
is not "-" keeps that workaround until this crate adopts the library's answer. "Not rechecked" parts were open at
24995a1 and are not verified against 0.3.0.

| # | Gap | Status at 0.9.0 | Until then |
|---|---|---|---|
| 1 | A key on a menu item and a palette entry is bound twice (panic) | closed: same-command rebind is a no-op; `MenuItem::command`, `PaletteCommand::command` | - |
| 2 | `TreeView` rows: context menu, slots, dnd | closed: row map, check, actions, moves | - |
| 3 | `ReorderableList` drops per-row responses | closed: `ReorderableListResponse::rows` | - |
| 4 | List check box paint only | closed: `ListItemOutput::leading_toggled` | - |
| 5 | `ListItem` one leading slot, no custom paint | closed: `ListSlot` composes a check box and an icon | `TreeView` for the outline |
| 6 | `MenuItem` / `Select` options: no custom leading | closed: `leading` takes a `LeadingPaint` | - |
| 7 | `Select` without its label | closed: `label_visible(false)` | - |
| 8 | `Select` per-option disable | closed: `SelectOption::disabled` (H.264 profile) | - |
| 9 | `NumberField`: log scale, scrub, wheel, compact, commit modes | partly: log, scrub, commit modes, `edit::commit_pending`, wheel over the focused field; compact size and a hidden label not rechecked | the brightness boxes are Outlined at `Density::MIN` (`with_density`); Undo steps repeats its row label |
| 10 | `TextField` buffered edit, commit on demand | closed: `commit_on_submit` | - |
| 11 | `Menu::is_context_open` | closed | - |
| 12 | `PopoverResponse::dismissed` (Esc vs outside press) | closed: `Option<Dismiss>` | - |
| 13 | `ColorPicker::rgb_fields` | closed | - |
| 14 | `Accordion` header height fixed 56 dp | closed: density | - |
| 15 | `TopAppBar` action disabled flag and response | closed: `IconAction` | - |
| 16 | `Dialog` mnemonics, Enter default, size | closed: `DialogAction::mnemonic`, `default_action`, `Dialog::width` | `Dialog::full_screen` for export on Compact |
| 17 | `ButtonGroup` wrap | closed: a standard group wraps | the quality presets fit one row at the page's widths |
| 18 | Docked resizable `SideSheet` | closed: `SideSheet::width_range`, `resizable` | egui `Panel` + token `Frame` for the columns (gaps 66, 67) |
| 19 | Path-based pickers, streaming save | closed: `pick_file_reader`, `save_file_writer` | WaveDude's rfd / browser glue (gap 69) |
| 20 | `shortcut::bind` swallows paste text | closed: `shortcut::take_paste` | - |
| 21 | Settings row and status line components | closed: `SettingRow`, `StatusBar` with `StatusItem::progress` | `status.rs` (gaps 64, 65) |
| 22 | Material crate not in wasm32 CI | closed | - |
| 23 | `MenuStrip` has no F10 / lone Alt entry (Alt+letter works) | closed: F10 / lone Alt | - |
| 24 | Domain widgets: canvas, handles, GPU host, timeline, gradient editor | library-independent | app widgets over `waveduderust_egui_preview::{pictures, timeline}`; no library change |
| 25 | Markdown view | library-independent | `egui_commonmark` via interop |
| 26 | `Popover` opens only on its trigger's click | closed: the app owns `open` | speed Custom: `Popover::show_at` (gap 68) |
| 27 | `SettingRow` grows each pass around a control placed straight into its row layout | closed: the control column is top-aligned, centred on last pass's size | - |
| 28 | A tree row action cannot anchor a `Menu` | closed: `TreeAction::rect`, `Menu::show_at` | - |
| 29 | `MenuItem` has no tooltip | closed: `MenuItem::tooltip` | - |
| 30 | `NumberField` gives no focus handle (its response is the row) | closed: output `field` | - |
| 31 | `SideSheet` / `BottomSheet` never open after a pass shown closed: `presence_in` resets the animation and reopens it in the same pass, so egui's tick-based `animate_bool` advances 0 s and stays at 0 | closed: sheets open | - |
| 32 | An outlined field's floating label is painted half its height above the field's rect, outside its allocation | closed: the label's cutout is in the field's top margin | - |
| 33 | `Menu::show_context` focuses the first item even when a pointer opened it, and the focus ring shows | closed: a pointer-opened menu keeps focus | - |
| 34 | `NumberField` scrub and log steps: the field width spans the whole range (no per-pixel rate), no Ctrl coarse; log steps are a share of the log track, not a percentage with page fifths and significant digits | partly: Ctrl coarse scrub, `RangeModel::ratio`, `RangeModel::significant` | accepted; a scrub is one history step (`MouseDown` at its start, `MouseUp` at its edit event or release) |
| 35 | `Toolbar::show` returns the clicked action index only, no per-action `Response`, so no `Menu` anchors on a toolbar action | closed: `ToolbarResponse::actions` rects, `Menu::show_at` | - |
| 36 | A `Menu` opens only on its anchor's click or a context trigger, never from code at a point | closed: `Menu::show_at` | - |
| 37 | `TreeView`'s drag payload is private, a picture cannot read the dragged rows | closed: `TreeDrag` | the pictures take the drop (TREE-23) |
| 38 | `Menu::show_context` opens on Shift+F10, not on the Apps (`Key::Menu`) key | closed | - |
| 39 | A floating `Toolbar` sizes its pill for 48 px items but packs the 40 px icons with no spacing (`actions` spacing `gap - (ITEM - ITEM_VISUAL)` is 0), leaving an empty strip of 8 px per action at its right end | closed: the actions fill the pill | - |
| 40 | A `Select` inside a `Dialog` opens its list behind the dialog once the dialog was shown and closed before: the list's `Area` is moved to the top only when new, the reopened modal is newer; neither pointer nor keyboard reaches the list | closed: popups kept above reopened modals | - |
| 41 | A `Dialog` keeps its tallest height while shown: content that shrinks (the export dialog's MP4 notes going with another file type) leaves empty space above the actions | closed: dialogs shrink to their content | - |
| 42 | `TextField` shows the start of an overflowing value while unfocused (path file names) | closed: `TextField::elide` | - |
| 43 | `TreeView` rows are list-item metrics: 56 dp less 4 per density step, indent 40 per level, BodyLarge labels | closed: `row_height`, `indent`, `label_style` | - |
| 44 | No segmented button: `ButtonGroup` `Connected` draws separate pills with a gap, its selected toggle filled | closed: `SegmentedButton` | - |
| 45 | `Accordion` headers and `TextField` input have one text style each (BodyLarge, 24 dp lines) | closed: `AccordionPanel::title_style`, `TextField::text_style` | the inspector's section titles are TitleMedium, the Description box BodyMedium |
| 46 | `NumberField` parses in the runtime's locale only: in de-DE, fr-FR and nl-NL a typed `.` is a group separator (`0.6` commits 6) | closed: a typed `.` is the decimal point unless it reads as digit groups | the inspector shows the locale's decimal separator |
| 47 | A control's label stays on one line, wider than a dialog | closed: control labels wrap | - |
| 48 | Density is the runtime's alone: a denser widget sets and restores it around itself | closed: `MaterialUiExt::with_density` | - |
| 49 | A `TextField`'s supporting text elides its end only | closed: `TextField::elide` applies to it | - |
| 50 | A typed `.` in de-DE read as digit groups whenever three digits followed it (`0.123` committed 123) | closed: groups need a lead of 1..3 digits without a leading 0 | - |
| 51 | A value snapped to a 0.01 grid from -180 kept float noise (0.5999999999999943) | closed: the grid rounds to the decimals of min and step | - |
| 52 | A control in a narrow row measured below its size, so the next widget overlapped it | closed: controls never measure below their size | - |
| 53 | `NumberField` snaps typed values to the step grid (`0.123` with step 0.01 commits 0.12; docs/ui/30, Number box: typed text is not limited by the box) | closed: `NumberField::snap_typed(false)` | - |
| 54 | `SegmentedButton` segments share the widest one's width; a single choice cannot show none selected, so the layer switch was a `multi` row | closed: `SegmentedButton::equal_width`, `single_optional` | - |
| 55 | `TreeView` paddings are the list's fixed 16 dp at any density, truncating "Colour schemes (0)" in the default column | closed: paddings narrow 4 dp per density step | - |
| 56 | A `Scale::Log` range has no significant-digit grid: steps and scrubs land on float noise (2.749041183321324), and the focused field's draft keeps it after the bound value is rounded | closed: `RangeModel::significant` | - |
| 57 | `NumberFieldOutput` has no input source: a commit of typed text and one of a step, a wheel notch or a scrub release look alike | closed: `NumberFieldOutput::source` | - (unused: the range carries the spec's bounds and precision, so every source's value is applied as it comes) |
| 58 | `SegmentedButton`: after a pointer click Left / Right do nothing; with keyboard focus they move and select | closed: press-to-focus follows a click whose press and release arrive in one pass | - |
| 59 | One wheel notch steps a focused `NumberField` twice (seen with xdotool on Xvfb, up and down) | upstream: winit 0.30.13 on X11 reports a synthetic notch as two; real mice step once | - |
| 60 | A linear `RangeModel` steps on a grid anchored at its minimum: with minimum 0.01 and step 1, 1.5 Up gave 2.01 where the classic window adds the increment (2.5) | closed: `RangeModel::increment` adds to the value, `fraction_digits` rounds to the decimals | - |
| 61 | A focused field kept its draft when the app changed the bound value during the edit (typed 0 in a box with minimum 0.01 showed 0 until the focus left) | closed: the field adopts a bound value it did not write (`EditBuffer::wrote`) | - |
| 62 | Collection items were keyed by index, so inserting one renamed the agent keys and egui ids of the items after it | closed: items take a salt, by default their command's `ActionId`; the canvas toolbar, the export action, action menu items and palette entries without a command set one, fixed lists keep the index | - |
| 63 | A `Scale::Log` step starts from the value snapped to its significant digits (typed 1.234 Up: 1.25, `NumberSpec::step` 1.26), a page multiplies by the ratio only (whole box 10 PageUp: 12, the spec 20), a tie of the last digit falls by float noise, and n stalled whole steps in one pass move 1 (AccessKit Increment xN, several wheel notches) | closed in 0.4.0: log steps start from the current value, ties round away from zero, `jump` and stalled steps are honored (the spec's float noise still differs on ties) | the library's steps |
| 64 | `StatusBar` paints one container and content colour: no item or bar tone for the newest log line's warning or error (LOG-01) | closed, adopted (0.4.0 `StatusItem::tone` and `colors`) | `status.rs` |
| 65 | `StatusBar` drops a start item that does not fit instead of eliding it: a long log line vanishes in a narrow window | closed, adopted (0.4.0 `StatusItem::elide`) | `status.rs` |
| 66 | A standard `SideSheet` docks at the logical end edge only, so the outline column (start edge) cannot use it | closed: `SideSheet::edge(Edge::Start)` | - |
| 67 | A standard `SideSheet` puts the content in its own `ScrollArea` with 24 dp side padding: the inspector's fixed header over its own scroll area and the outline-over-inspector split (840..1200, a third of the column height) lose the column's height | closed: `SideSheet::scroll(false)` and `content_padding`; the outline and inspector columns are docked, resizable `SideSheet`s | - |
| 68 | `Popover::show` anchors at a trigger `Response` whose clicks toggle it: a popover opened from a menu item and anchored at the chip that opens the menu also toggles on each chip click | closed: `Popover::show_at` (0.4.0) | speed Custom opens an anchored popover at the chip |
| 69 | The pickers came only with `platform-native` (`arboard`, `notify-rust`), and `FileFilter` set no start file name for an open picker | closed: adopted, `platform-pickers` (0.4.0) only; native flows use `PlatformPathPickers` and `pick_save_path`; the browser keeps `waveduderust_app::dialogs` (Source::Bytes upload, Target::Download), shared with the classic window | - |
| 71 | Native open pickers read the whole file or open it; none returned only the chosen path | closed: adopted, `pick_file_path` (`PlatformPathPickers`) | - |
| 72 | `FileFilter` took a start folder and a start file name separately; a proposed path was split by the caller | closed: adopted, `FileFilter::start(path)`; the filters come from `waveduderust_app::dialogs::filters`, shared with the classic window | - |
| 73 | The runtime styles only its own windows: egui's root tooltips and popups keep egui's global styles, and the window decorations need `ViewportCommand::SetTheme`, so the app copies the kit's style into `all_styles_mut`, calls `ctx.set_theme` and sends `SetTheme` when the resolved theme changes | closed, adopted (0.5.0): `GlobalStyle::Kit` writes the kit's style and theme preference into egui's globals and the window theme | - |
| 74 | `StatusItem` shows progress only as a circular indicator: no linear bar for a long job (export, run build) in the status line | closed, adopted (0.5.0): `StatusItem::progress(StatusProgress::Linear)` (0.6.0) | - |
| 75 | `Popover::show_at` dismisses on the tap that opened it when the app opens it in the pick frame of a menu item (`outside_press` sees the same-pass tap, the first-pass rect is unplaced) | closed, adopted (0.5.0): a popup ignores presses of its opening frame and `show_at` focuses its first field | - |
| 76 | A `Dialog`'s action mnemonics bind in `Scope::Global` on the dialog's first pass: `scope_of` asks egui's `top_modal_layer`, which egui sets at the end of the pass, so the opening pass clashes with the menu bar's mnemonics ("mnemonic 'S' bound twice in Global": menu bar and Save changes?) | closed (0.5.0): library modals bind in their own scope from the pass they open in | - |
| 77 | `TreeView::text_style` sets one style for every row and `TreeData` gives only `label()`: folder rows cannot be emphasized over leaf rows | closed, adopted (0.7.0): `TreeView::node_text_style` | - |
| 78 | `Toolbar` keeps a 64 dp container and 48 dp action slots at every density: a desktop view toolbar cannot be 40 dp | closed (0.7.0): toolbar heights follow the density | - |
| 79 | No tabular figures: a changing read-out (`12 / 1249`) shifts with proportional digits | closed, adopted (0.7.0): `kit::text::tabular_figures` (run bar counter), `NumberField::tabular_figures`, `StatusItem::tabular_figures` | - |
| 80 | `TextField` / `NumberField` have no tooltip setter; with `label_visible(false)` sighted users get no hint | closed, adopted (0.8.0): `tooltip` on text, number and select fields, also their accessible description | - |
| 81 | A docked `SideSheet` seeds its width at 360 dp clamped into `width(range)` and its persisted width id is private: no 300 dp default column | open: `SideSheet::initial_width` (0.7.0), but a docked sheet fills `SurfaceContainerLow` and the tree rows `surface` (84) | the columns are `egui::Panel`s with a 1 dp divider (no sheet chrome, no keyboard resize) |
| 82 | `TextField` / `NumberField` have no text alignment: numbers cannot be end-aligned in a property grid | closed, adopted (0.7.0): `text_align(Edge::End)` | - |
| 83 | `InlineEdit` keeps its edit state private: an app cannot start editing from code (F2, a new item's name) | closed, adopted (0.7.0): `InlineEdit::editing`; F2 and a new item open the name's editor | - |
| 84 | `TreeView` rows are hard-wired to `surface`: a tree in a pane of another tone shows as a block | open | the panes use `surface` |
| 85 | `NumberField` has no leading icon slot (`no_icon` chrome) | closed, adopted (0.8.0): `NumberField::icon`; the brightness box leads with its icon and tips its name | - |
| 86 | `CheckButton` keeps the 48 dp touch target under a pointer at every density (`control_slot` uses `MIN_INTERACTIVE`) | closed (0.7.0): check, radio and switch targets follow the density | - |
| 87 | `subsecond` 0.7.10 patches only the crate holding `main.rs`: UI in a `lib.rs` keeps running the old code while dx reports the patch. `Runtime::pass`'s one `subsecond::call` is enough for a bin-only crate; dx 0.7.10 needs `main.rs` touched before `dx serve` (mineral `docs/blockers.md`) | closed: this crate is bin-only; a nested edit (`pages::gallery::start_strip`) patched live in 1.5 s, same process (2026-11-21) | `just hot` |
| 88 | `egui_mineral_eframe::MineralApp` has no hook before the pass (0.8.0 forwards `raw_input_hook` and the `eframe::Frame`) | open | the shell implements `eframe::App` around `Runtime::pass` itself: the look follows the settings before the pass styles its root |
| 89 | An end-aligned `NumberField` value sits flush against its suffix (`280px`) | open | - |
| 90 | `Menu::widen` is crate-private, and a submenu's parent row takes a value of the menu's type though it never activates | open | `menus::keyed` widens with an empty `items`; `MenuKey::Sub` values the parent rows |

## 14. Shared crates

The window depends on no other window: what it shares with `waveduderust_classic` lives in `waveduderust_app`,
`waveduderust_egui_preview` and `waveduderust_egui_platform` (decision 0031).

| Crate | Modules | What |
|---|---|---|
| `waveduderust_app` | `menus` | `MenuAction`, `MENU_BAR` (icons as `menus::Icon`), `PART_KINDS`, `enabled`, `target`, `can_move` |
| `waveduderust_app` | `settings_table`, `about` | settings rows, system lines |
| `waveduderust_app` | `preview::{presenter, view, timeline, handles, overlay, graph_boxes}` | presenter thread, request / delivery, zoom, timeline maths, handle hit test and drag, overlay edits, graph data |
| `waveduderust_app` | `properties`, `form`, `outline`, `messages`, `status` | read-outs, bounds, forms, tree keys and drags, the log's lines, the status line |
| `waveduderust_app` | `export::{mod, session, native, web, delivery}` | export requests, plans, jobs, browser delivery |
| `waveduderust_app` | `dialogs`, `codec`, `downloads`, `update`, `window` | filters, OpenH264, downloads, updater, `TICK`, `MIN_SIZE`, `CANVAS` |
| `waveduderust_egui_preview` | `pictures`, `overlay`, `graph_boxes`, `timeline`, `gradient_editor`, `drawn` | the pictures on the renderer's device, handle and box interaction, the timeline, the gradient bar; colours passed in |
| `waveduderust_egui_platform` | `input`, `keys`, `icons`, `theme`, `os_theme`, `placement`, root | key tables, menu icons, theme resolution, OS scheme, window place, clipboard, leave guard, `share_device`, `renderer_gpu` |

## 15. Crate and dependencies

[`Cargo.toml`](Cargo.toml) (bin-only `wavedude-mineral`, unit tests in its modules):

- `egui_mineral` without `platform-native` (file dialogs and clipboard stay WaveDude's) and without `persist-fs`; ICU
  for locale number formats. Icons from `egui_mineral_material::icons` (`bundled-icons`, the same
  `egui_material_icons` 0.9.0 as the classic window: one copy of its fonts in a build).
- One egui / eframe / egui-wgpu / epaint 0.37.0, wgpu 30.0.1, accesskit 0.25.2; adds taffy 0.15.1, egui_taffy 0.16.0,
  ICU4X 2.x, fluent, nucleo. The library's lock has wgpu 30.0.2; ours stays 30.0.1 (semver-compatible).
- Native: `eframe::run_native` with `waveduderust_egui_platform::share_device`, `renderer_gpu` -> `engine::gpu::set_context`,
  `placement::viewport`, icon, panic hook and log as `waveduderust_classic`'s `main.rs`.
- wasm32: `eframe::WebRunner` on its own page; reuses `wavedude-run-worker`, `wavedude-spill-worker` and app-state's
  `Store`; `tools/build-web.sh` gains an app switch. Clippy with `CLIPPY_CONF_DIR=tools/clippy-wasm32`.

## 16. Blockers and open decisions

| Item | Effect | Proposed |
|---|---|---|
| Settings file shared with the old app | both apps read / write `~/.config/WaveDudeRust` | share it (recent files, quality, theme); never write `Language` or `Layout`; column state stays in the session |
