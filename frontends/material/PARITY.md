# Feature parity: waveduderust-material vs waveduderust

Yardstick of every user-visible feature of the Next-based app (`frontends/classic`, spec `docs/ui`).
Ref = `docs/ui` file number and section. Status: `todo`, `done`, `changed: <note>`, `dropped: <note>`;
`dropped (brief)` = out of scope by the brief (no manual, English only). `changed: <note>` = a deliberate
difference decided by [DESIGN.md](DESIGN.md) (section 1, Users and tasks, gives the reason). Section UX lists what this UI adds beyond the Next-ported one. Library:
the egui-mineral release DESIGN.md names; a gap cited as a reason may be closed there with
its workaround kept (DESIGN.md section 13).

The UX differs from the Next-ported app on purpose: one workspace with the canvas and the run bar always on screen
(no dock, no routes between panes), a menu bar on desktop plus a command palette, selection-driven inspector, a
gallery as the start surface, Settings and About as pages over the workspace.

## Window, look, startup (WIN)

| ID | Feature | Ref | Status |
|---|---|---|---|
| WIN-01 | Title: app name; file name or suggested name (import, example); `*` when dirty; `Untitled` | 00 Main window | done |
| WIN-02 | Title shows the full path (setting `TitleFullPath`) | 00 Main window | done |
| WIN-03 | Min size 800 x 500, default 1280 x 800, centred on first start | 00 Main window | done |
| WIN-04 | Window size, position, maximised state persisted; kept on its monitor at start and after zoom | 00 Main window, 80 | done |
| WIN-05 | Theme System / Light / Dark, live switch, System follows the OS (portal on Linux) | 00 Look, 80 | done |
| WIN-06 | UI zoom 0.5x..3x steps; Ctrl+= / Ctrl+- / Ctrl+0; persisted sparse | 00 UI zoom | done |
| WIN-07 | `--zoom=<0.5..8>` switch wins over the setting | 00 UI zoom | done |
| WIN-08 | Touch-first device (browser coarse pointer): default zoom 2x, tree drag off | 00 UI zoom | changed: touch-first keeps UI zoom 1x and density 0, so a landscape tablet keeps the Expanded layout with the touch pointer model; outline drag off on touch |
| WIN-09 | Every control reachable by keyboard, focus visible; no mouse-only control | 00 Keyboard first | partial: pages and panes audited by code; library widgets, outline rows, messages list, canvas, status line and colour swatch take focus with a ring; messages column grips take the focus and resize with the arrows (Splitter role, value); handles nudge with arrows, a focused picture pans with arrows |
| WIN-10 | Stable accessible ids and names on every element (AccessKit) | 00 Keyboard first | changed: audited; salts are literals, item keys or `Salt` (no displayed text, Debug or reorderable index), the inspector form's salt is the row's, handle agent keys are spelled out, context menus and memory levers are named. Every menu item carries an explicit salt. Open: read-out terms without a label are unnamed |
| WIN-11 | First argument opens a `.wdproj` / `configuration.json` / `.wdscheme`; unknown `--` switches logged | 00 Startup | done |
| WIN-12 | `--greyscale-text` (text without subpixel AA) | 00 UI zoom | n/a: egui draws text without subpixel AA, and the manual figures it served are gone (decision 0028) |
| WIN-13 | Status line first text "Ready" or the last start-up log line | 00 Startup | done |
| WIN-14 | Dock: panes Project, Properties, Messages around the Preview; drag to edge / split / tab; close hides | 00 Panes | changed: no dock and no routes between panes: one workspace (outline, canvas, run bar, inspector, status line) placed per window class; the canvas and the run never leave the screen (DESIGN 3) |
| WIN-15 | View > Panes toggles each pane; arrangement persisted (`Layout`) | 00 Panes, 10 View | changed: View > Outline / Inspector / Messages (Alt+1..3), Canvas only (Alt+0, for projectors); widths and visibility per session, `Layout` never written (settings file shared) |
| WIN-16 | Pane title bar menu: Move to Left / Right / Top / Bottom, Close | 10 Context menus | changed: no pane title bars; columns collapse through View |
| WIN-17 | Bundled UI font (Selawik + Noto Sans Math subset), 16 px icons | 00 Look | changed: bundled Selawik, Material icons |

## Menus (MENU)

| ID | Feature | Ref | Status |
|---|---|---|---|
| MENU-01 | Menu bar File, Edit, View, Simulation, Help with icons and shortcut text | 10 | changed: `MenuStrip` from `MENU_BAR` on Expanded+ (Next users' habit, desktop discoverability); Medium / Compact: the same groups under the top bar's menu button; palette everywhere |
| MENU-02 | One enabled table for menu bar, context menus, toolbar, Run and dispatcher; disabled shortcut does nothing | 10 Conventions | done |
| MENU-03 | Menu keyboard: F10 / lone Alt, Alt+letter, arrows, letters, Home/End, Esc returns focus | 10 Conventions | done: F10 / lone Alt, Alt+letter, arrows, letters and Esc by the library's `MenuStrip`; palette Ctrl+K / Ctrl+Shift+P |
| MENU-04 | Unique mnemonic per menu entry | 10 Conventions | done |
| MENU-05 | Menu bar greyed while a modal shows | 10 Conventions | changed: window-wide keys and the menu bar blocked while a modal shows |
| MENU-06 | Help > About | 10 Help | changed: Help > About opens the About page over the workspace |
| MENU-07 | Help > Manual (F1) | 10 Help | changed (decision 0028): Help > Project page on GitHub (F1) opens the repository through `platform::open`; a failure shows OpenError with the OS error and is logged |
| MENU-08 | Help > Check for updates (updater builds) | 10 Help | done (offered where the build has an updater) |

## Project and files (PRJ)

| ID | Feature | Ref | Status |
|---|---|---|---|
| PRJ-01 | New project (Ctrl+N): blank canvas, open edges, damping rim -32, no sources; timeline to 0 | 10 File 1 | done |
| PRJ-02 | Open (Ctrl+O): filters wdproj, Wave Dude 2D `configuration.json`, XML, All | 10 File 2 | done |
| PRJ-03 | `configuration.json` imported as a new unsaved project with a suggested name | 10 File 2 | done |
| PRJ-04 | Recent projects: max 10, newest first, full path, click opens | 10 File 3, 80 | changed: File > Recent submenu (10), gallery start strip (5), palette group |
| PRJ-05 | Open an example: unsaved, named after its file; Save As proposes it | 10 File 4 | done |
| PRJ-06 | Save (Ctrl+S); Save As when no file | 10 File 5 | done |
| PRJ-07 | Save As (Ctrl+Shift+S): wdproj / xml, proposes the suggested name beside its origin | 10 File 6 | done |
| PRJ-08 | Close project; disabled on the placeholder project | 10 File 9 | done |
| PRJ-09 | Exit (Alt+F4), window close box | 10 File 10 | done |
| PRJ-10 | "Save the changes to X?" Save / Don't save / Cancel before New, Open, Close, Exit, example, recent, drop, restart | 70 Unsaved edits | done |
| PRJ-11 | "An export is running. Stop it and quit?" on Exit, close, restart | 70 Unsaved edits | done |
| PRJ-12 | Dropped file: image becomes scene image; project or scheme opens (after prompt); other type reports Open error | 10 File dropped | done |
| PRJ-13 | `.wdscheme` opened or dropped: added to the project's schemes and selected | 60 Files and drop | done |
| PRJ-14 | Scene image file differs from embedded: "Load the file?" Load / Keep embedded | 60 Files and drop, 70 | done (shared Document logic; the window asks and withdraws the question; the browser has no watch) |
| PRJ-15 | Scene image file watched (400 ms debounce, retries, withdraws when moot, one question at a time) | 60 Files and drop | done (shared Document logic; the window asks and withdraws the question; the browser has no watch) |
| PRJ-16 | Scene image looked up beside the project and in `scene_images`; relative path inside the project folder | 60 Files and drop | done (shared Document logic; the window asks and withdraws the question; the browser has no watch) |
| PRJ-17 | Open / save errors: one-line message, detail in the log | 70 Message window | done |
| PRJ-18 | Browser: page asks before leaving with unsaved edits | 10 File dropped | done: the page asks while the project has unsaved edits or an export runs |
| PRJ-19 | macOS Finder open requests; file associations by the installers | 70 File type registration | todo |
| PRJ-20 | Off-canvas objects logged once per change of the list | 60 Objects off the canvas | done: the shared live controller (`LiveController::respec`) warns, the log ring feeds the Messages pane |

## Project tree (TREE)

| ID | Feature | Ref | Status |
|---|---|---|---|
| TREE-01 | Rows: Project root, Scene, Simulation, Display, folders Sources, Parts, Probes, Colour schemes; expand / collapse | 20 Tree structure | changed: outline rows Scene, Simulation, Display pinned, then sections with counts; no Project root row: no selection shows the Project form |
| TREE-02 | Toolbar: + Source | 20 Toolbar | changed: no tree toolbar: Sources section `+`, row context menu, Edit > Add, the canvas Add button (`A` at the pointer) |
| TREE-03 | + Probe menu: Standard; Probabilistic > Counting detector, Imaging detector | 20 Toolbar | done: `+` on Probes opens the detector menu; also in the row context menu and Edit > Add |
| TREE-04 | + Part menu: optics, shapes, Optical logic submenu (21 kinds), kind description tooltips | 10 Add part, 20 Toolbar | done: `+` on Parts opens the kind menu with the Optical logic submenu and kind description tooltips; also in the row context menu and Edit > Add |
| TREE-05 | Toolbar Move up, Move down, Remove with enabled rules (skipped by Tab when disabled) | 20 Toolbar | changed: Move up / down and Remove in the inspector header, row context menu, Edit menu and keys |
| TREE-06 | Item row: Enabled / Active check box, kind icon, name, row buttons up / down / remove; unchecked greys | 20 Item row | changed: row = Enabled box, kind icon, name; up / down / remove buttons move to the inspector header and context menu (rows stay narrow beside the canvas) |
| TREE-07 | Lens row icon follows R1, R2, diameter (9 profiles) | 20 Row icons | done: `LeadingPaint` draws the lens profile from the shared `waveduderust_egui_preview::drawn` |
| TREE-08 | Kind icons for every part kind, source, probe | 20 Row icons | changed: a Material icon per part kind, source and probe (gap 5) |
| TREE-09 | Colour scheme row: gradient strip and name | 20 Item row | changed: the 24dp leading slot holds the gradient as a swatch, no wider strip; the name follows |
| TREE-10 | Folder row plus: adds to that folder (Parts: kind menu; schemes: default scheme) | 20 Item row | changed: `+` on Sources (adds a source), Parts (kind menu), Probes (detector menu) and Colour schemes (default scheme) |
| TREE-11 | Selection: click, Ctrl+click toggle, Shift range in tree order, Insert / Ctrl+Space toggle; one current item | 20 Selection model | done |
| TREE-12 | Selection shared with the pictures; tree mirrors without taking focus | 20 Selection model | done |
| TREE-13 | Opened project selects the root; tree takes keyboard focus | 20 Selection model | changed: an opened project selects nothing (inspector shows the Project form with its description) and the canvas takes focus, so Space plays and F5 runs at once |
| TREE-14 | Enabled toggle (Space) flips each selected item on its own | 20 Commands | done |
| TREE-15 | Duplicate (Ctrl+D): "<name> copy" after each original, copies selected, caret in last name | 20 Commands | done |
| TREE-16 | Remove (Del): selection lands on the slot rule; tree takes focus | 20 Commands | changed: Remove keeps focus where it was (canvas or outline); selection by the slot rule |
| TREE-17 | Move up / down (Alt+Up / Alt+Down) within the list, selection kept | 20 Commands | done |
| TREE-18 | Add: appended, placed at the canvas centre, selected, caret in Name (probe B 100 px right of A) | 20 Commands | changed: added at the canvas centre and selected, caret in Name; `A` on the focused canvas places it at the pointer (UX-07) |
| TREE-19 | Row context menu: add source / part / probe, selection toggle, select all, Enabled, Duplicate, Cut, Copy, Paste, Remove, Move up / down | 10 Context menus | changed: a right click selects the row unless it is in the selection, then the menu acts on the selection |
| TREE-20 | Scheme rows: Add to / Remove from my schemes | 20 Commands | done |
| TREE-21 | Probe rows: Export probe data as CSV (active probe, no export running) | 10 Context menus | done |
| TREE-22 | Drag reorder within a list: line above / below a row, outline on its folder | 20 Drag and drop | done |
| TREE-23 | Drag rows onto a picture: pasted and placed at the drop pixel, one undo step | 20 Drag and drop | done: the canvas reads `TreeDrag` on release and pastes the dragged rows (the selection when several) at the drop pixel (`drop_paste`) |
| TREE-24 | Drag carries XML text out (other window, text editor); settings rows draggable | 20 Drag and drop | todo: drags stay inside the window, no OS drag in winit ([`docs/ui/00-overview.md`](../../docs/ui/00-overview.md), Drag out of the window: TODO) |
| TREE-25 | Touch-first: rows click-only, no drag from the tree | 20 Drag and drop | done |
| TREE-26 | Keys: arrows, Home / End, Shift+arrows, Enter moves focus into the row's form | 10 Project tree | done: Enter focuses the form's first editor, a text or number box with its text selected |

## Properties (PROP)

| ID | Feature | Ref | Status |
|---|---|---|---|
| PROP-01 | Form of the current item; "No selection" | 30 Pane behaviour | changed: no selection shows the Project form, description first (an example's description says what to try), not "No selection" |
| PROP-02 | Folder: label and count ("No entries" / "1 entry" / "N entries") | 30 Pane behaviour | changed: a section header row selects nothing; its count is on the row |
| PROP-03 | Sections with headers; vertical scroll only | 30 Pane behaviour | changed: each section is an accordion panel; vertical scroll only |
| PROP-04 | Read-outs follow their inputs; "~ " estimates with one decimal | 30 Pane behaviour | done |
| PROP-05 | Edits apply live; name per keystroke renames the tree row | 30 Pane behaviour | done |
| PROP-06 | Commit on Enter, focus loss, and before any action reads the model (`PendingEdit`); Esc reverts | 30 Text box | changed: Enter leaves the field (egui `TextEdit`) instead of selecting the text |
| PROP-07 | New item: caret in its first text box, text selected | 30 Pane behaviour | changed: caret in Name, text selected (the name heads every item form); a form starting with a number selects its text |
| PROP-08 | Number box: Up / Down, PageUp / PageDown, wheel (Shift x10), repeat buttons, clamp, decimals | 30 Number box | changed: Up / Down add the increment and PageUp / PageDown ten, rounded to the decimals and clamped; typed text keeps its digits, clamped at once; the wheel steps a focused box one step a notch, without Shift x10; no repeat buttons; a step from text that does not parse starts from the last value, not 0 |
| PROP-09 | Number box horizontal drag scrub (Shift fine, Ctrl coarse), Esc restores | 30 Number box | changed: horizontal drag scrubs with Shift fine, Ctrl coarse and Esc restoring, rounded to the box's precision; the field width spans the whole range, no per-pixel rate (DESIGN 14, gap 34) |
| PROP-10 | Logarithmic number box (brightness): 2 % steps, a fifth per page, 3 significant digits | 30 Number box | changed: 2 % steps, a fifth a page, 3 significant digits; a value with more digits steps from its 3-digit rounding (DESIGN 14) |
| PROP-11 | Rows disabled by another setting grey out and take no focus | 30 Number box | done |
| PROP-12 | Enum combos show the words of the string table | 30 Check box, combo box | done |
| PROP-13 | Hints (`Hint.*`) as tooltips | 30 | done |
| PROP-14 | Colour scheme picker: project / shipped / user schemes with swatch, "(not found)", Load from file, Edit a copy, Remove from my schemes | 30 Colour scheme picker | done |
| PROP-15 | Colour swatch and picker popup: SV square, hue strip, R G B, hex, keys, Enter keeps, Esc reverts, live preview | 30, 70 Colour picker | changed: swatch opens a popover `ColorPicker` with SV square, hue strip, R G B and hex; edits apply live, Esc reverts to the colour at opening |
| PROP-16 | Link toggle for rectangle / ellipse length and width | 30 Link toggle | done |
| PROP-17 | Part description header, open state persisted (`PartDescriptionExpanded`) | 30 Description header | done |
| PROP-18 | Project form: format version, seed + New seed, description | 30 Project | done |
| PROP-19 | Scene form: mode, image name / pick / save / remove, image file, embedded image, size, edges, damping, index, image sources | 30 Scene | done |
| PROP-20 | Simulation form: steps, visualize every, 4th-order Laplacian, wave speed, decay, frequency scale, intensity scale | 30 Simulation | done |
| PROP-21 | Display form: layer sub-forms (scheme, brightness, black level, preview, outline), handles, labels, export range, markers, frame rate, quality | 30 Display | done |
| PROP-22 | Source form: general, position, wave, timing, array (composite), random offsets, wavelength read-out | 30 Source | done |
| PROP-23 | Part shared rows and glass / mirror material rows; Lens <-> Mirror rename on the box | 30 Shared, Material | done |
| PROP-24 | Lens and Fresnel lens forms (Concave flips radii, edge thickness) | 30 Kind-specific | done |
| PROP-25 | Polygon, rectangle, ellipse, triangle forms | 30 Kind-specific | done |
| PROP-26 | Outline: point list with add / remove | 30 Kind-specific | done |
| PROP-27 | Diffuser, facetted grating, simple 2D grating forms | 30 Kind-specific | done |
| PROP-28 | Interference filter: layer list with grip reorder, up / down / remove, Add layer, Remove all | 30 Kind-specific | changed: layer rows have up / down / remove buttons instead of the grip (`ReorderableList` takes `ListItem`s only); Add layer, Remove all |
| PROP-29 | Meta lens / prism / axicon forms | 30 Kind-specific | done |
| PROP-30 | Fibre: core, cladding, ends, node list with Smooth, In / Out handles, Add node | 30 Kind-specific | done |
| PROP-31 | Bragg reflector, GRIN lens, zone plate forms | 30 Kind-specific | done |
| PROP-32 | Gate (XOR / AND / OR) form: design, material, port spacing, outputs, layout and port read-outs | 30 Kind-specific | done |
| PROP-33 | Probe form with its enabled rules (detector, layer, measurement, kind, A / B, box, axes) | 30 Probe | done |
| PROP-34 | Colour scheme form: name (repoints layers), kind, description, my schemes, Based on | 30 Colour scheme | done |
| PROP-35 | Gradient bar: stop markers drag, drag off or double-click removes; stop rows (position, swatch, hex, remove); Add stop | 30 Colour scheme | changed: gradient bar with stop markers (drag, drag off or double-click removes); stop rows have position, swatch and remove, the hex is in the picker; Add stop |

## Preview pictures (PREV)

| ID | Feature | Ref | Status |
|---|---|---|---|
| PREV-01 | Field and intensity pictures, side by side or stacked (`PreviewOrientation`) | 40 Layout | done |
| PREV-02 | Splitter between pictures: drag, arrows, double-click / Enter resets to half | 40 Layout | changed: library `SplitPane`: drag and arrow keys, split kept per session; no double-click / Enter reset |
| PREV-03 | Hidden picture gives its room to the other; split restored when shown | 40 Layout | done |
| PREV-04 | Uniform fit with 4 px margin; black at canvas size without a frame; image scene shows its image | 40 Layout | done |
| PREV-05 | Header strip: title, Brightness box, Auto; turned on the left side when there is no room; caption dropped, title trimmed | 40 Header strip | changed: a header row over each picture: title, brightness `NumberField` (log), Auto; never turned |
| PREV-06 | Auto brightness: 99.5th percentile fit; 8-bit refit in 16-bit; "nothing to fit" | 40 Header strip | done |
| PREV-07 | Edge line: blue for an outside rim, red for reflective edges, none otherwise | 40 Edge line | done |
| PREV-08 | Shared zoom: wheel 1.25x about the pointer, fit..64x; middle-drag pan | 40 Zoom | done |
| PREV-09 | Pinch zoom and two-finger pan; second finger cancels a drag | 40 Zoom | done: a second finger cancels and reverts a handle or box drag |
| PREV-10 | Zoomed 16-bit picture rendered for its visible window at device size | 40 Zoom | done |
| PREV-11 | Picture changes only when the position's frame arrives; never black, never another frame | 40 What a picture shows | done |
| PREV-12 | Busy dim and spinner after 150 ms while the position's frame computes | 40 What a picture shows | done |
| PREV-13 | Scene outline (glowing edges) per layer | 40 What a picture shows | done |
| PREV-14 | 8-bit palette vs 16-bit half-float pictures; dithering 10..16 bits | 40 8-bit vs 16-bit | done |
| PREV-15 | GPU-coloured pictures on the renderer's device; CPU fallback with a warning | 40 8-bit vs 16-bit | done |
| PREV-16 | Picture context menu: Brightness, Auto brightness, Export this frame, Export field / intensity / both | 10 Preview picture | changed: right click, long press or Shift+F10 on a picture: Brightness (focuses the header's field), Auto brightness and the four exports; on an item the outline row menu |
| PREV-17 | Click on a picture focuses the preview so Space plays | 50 Playback | changed: the canvas takes focus on any click and after a project opens |

## Handles and overlays (HND)

| ID | Feature | Ref | Status |
|---|---|---|---|
| HND-01 | Source handle, composite emitter dots (thousands cached) | 40 Handles | done |
| HND-02 | Rotation levers (source array, part): whole degrees, Shift snaps 15 | 40 Interaction | done |
| HND-03 | Part centre, selected part outline, size grips (linked; polygon vertex on its ray) | 40 Handles | done |
| HND-04 | Lens surface handles | 40 Handles | done |
| HND-05 | Outline point handles | 40 Handles | done |
| HND-06 | Fibre node and Bezier in / out handles, smooth nodes | 40 Handles | done |
| HND-07 | Probe A / B handles, line probe line; only on the probe's layer | 40 Handles | done |
| HND-08 | Labels beside handles; Display Handles / Labels settings (selected item always shown) | 40 Handles | done: View > Handles / Labels and the canvas toolbar toggle the Display settings |
| HND-09 | Click selects; Ctrl+click toggles; Shift+click adds | 40 Interaction | done |
| HND-10 | Group drag of selected items, formation held inside the canvas | 40 Interaction | done |
| HND-11 | Marquee on empty space: replace / Shift add / Ctrl toggle | 40 Interaction | done |
| HND-12 | Click on empty space deselects | 40 Interaction | done |
| HND-13 | Esc during a drag puts everything back | 40 Interaction | done: Esc or a second finger; Esc also cancels a graph box drag |
| HND-14 | Tab through handles (focus selects); arrows 1 / 10 px, levers 1 / 10 deg; Del removes | 40 Interaction | changed: each handle is a focusable widget (agent key `canvas-<layer>-<item>-<handle>`), Tab order = overlay order; arrows, Shift+arrows, Del as before |
| HND-15 | Handle tooltips per kind | 40 Interaction | done: grab cursor over a handle, grabbing while dragging |
| HND-16 | Fixed overlay colours, selected drawn last, clipped to the picture | 40 Handles | done: focus emphasis in the theme primary |

## Probe graph boxes (GRAPH)

| ID | Feature | Ref | Status |
|---|---|---|---|
| GRAPH-01 | Graph box drawn by the export's layout: grid, labels, curve, title and live read-out | 40 Probe graph box | done |
| GRAPH-02 | Point probe over steps (Running / Full, auto-scale steps); line probe along A..B | 40 Probe graph box | done |
| GRAPH-03 | Value axis linear / log with floor rules; auto-scale values | 40 Probe graph box | done |
| GRAPH-04 | Drag the box; snap and attach to probe A with hysteresis | 40 Probe graph box | done |
| GRAPH-05 | Corner grip resize snapping to sizes, else Custom; arrows resize | 40 Probe graph box | done |
| GRAPH-06 | Box keys: arrows move, Tab reaches, focus selects its probe | 40 Probe graph box | done |
| GRAPH-07 | Box context menu: Export probe data as CSV | 10 Probe graph box | done |
| GRAPH-08 | Selected box accent border; hover and focus border | 40 Probe graph box | done |

## Timeline (TL)

| ID | Feature | Ref | Status |
|---|---|---|---|
| TL-01 | Frame scale: tick labels 1, 2, 5, 10 ... at least 64 px apart, major / minor ticks | 50 Timeline | done |
| TL-02 | Playhead with frame number | 50 Timeline | done |
| TL-03 | In / out range band with brackets; drag a bracket | 50 Timeline | done |
| TL-04 | Frames the run holds shaded | 50 Timeline | done |
| TL-05 | Hover line, tooltip | 50 Timeline | done |
| TL-06 | Click / drag scrubs to the nearest frame; Live off, playback paused | 50 Timeline | done |
| TL-07 | Keys: Left / Right, PageUp / PageDown, Home / End, wheel, Space | 50 Timeline | done |
| TL-08 | Accessible slider with range value and "range in..out" | 50 Timeline | done |

## Transport (TR)

| ID | Feature | Ref | Status |
|---|---|---|---|
| TR-01 | Run / Stop button | 50 Transport bar | changed: filled Run / Stop button leads the run bar; Compact keeps the run bar (no `BottomAppBar` with a `Fab`) |
| TR-02 | Pause / Resume run | 50 Transport bar | done |
| TR-03 | Play / Pause playback | 50 Transport bar | done |
| TR-04 | Previous / next frame | 50 Transport bar | done |
| TR-05 | Set in, set out, clear range (in past out clears out, and back) | 50 Transport bar | changed: Set in / out / Clear range in the run bar's overflow menu, keys I / O / X and bracket drag on the timeline (rare beside play and scrub) |
| TR-06 | Loop toggle | 50 Transport bar | done |
| TR-07 | Live toggle (only with an active run) | 50 Transport bar | done |
| TR-08 | Read-out "frame / last" and "step N" | 50 Transport bar | done |
| TR-09 | Speed: presets 0.25..4, typed 0.01..100, not persisted | 50 Transport bar | changed: speed chip "1x" with a presets menu and Custom, a `NumberField` 0.01..100 in a `Popover` anchored at the chip (`show_at`, gap 68, gap 75); Enter, blur or an outside press applies the typed value, Esc reverts, Enter closes; not persisted |
| TR-10 | Show field / show intensity toggles (= Display layer Preview) | 50 Transport bar | changed: canvas toolbar single-choice `SegmentedButton` Field / Both / Intensity, arrow keys select (= Display layer Preview); both off only from Display, the canvas then shows an empty state |
| TR-11 | Layout: side by side / stacked | 50 Transport bar | changed: layout toggle on the canvas toolbar (it is a property of the pictures, not of playback) |
| TR-12 | Preview keys: Space, `,` `.`, I, O, X, L, F, Home / End, Ctrl+A; letters consumed | 10 Preview pane | changed: Space , . I O X L F Home End, bare or with Shift alone, while a picture has the focus (`Scope::Focus`); Ctrl+A with the edit keys |

## Runs and live editing (RUN)

| ID | Feature | Ref | Status |
|---|---|---|---|
| RUN-01 | Run (F5): from step 0, Live on, frame 0; disabled during an export | 10 Simulation, 50 | done |
| RUN-02 | Pause / Resume run (F6), label follows | 10 Simulation | done |
| RUN-03 | Stop (Shift+F5, Esc): Esc cancels an export first; frames stay | 10 Simulation | done |
| RUN-04 | Discard frames, after "Discard the frames of the run?" (No default) | 10 Simulation | done |
| RUN-05 | Play / Pause playback (F7) from anywhere | 10 Simulation | done |
| RUN-06 | Playback at frame rate x speed, wall clock; between in and out; stop or loop | 50 Playback | done |
| RUN-07 | Play with missing frames starts a run (memory warning first) and holds at the newest frame | 50 Playback | done |
| RUN-08 | Live follow rules (trail by one picture, final follow, off on scrub / step / Play) | 50 Live follow | done |
| RUN-09 | Position rules (new / open -> 0, auto re-run keeps it) | 50 Position rules | done |
| RUN-10 | Edit classes: simulation, probe, display, scheme edits and their effects | 60 Edit classes | done |
| RUN-11 | Automatic re-run after edits (setting) | 60 Automatic re-run | done |
| RUN-12 | Run status "<State>: done / total steps, <engine>" | 50 Run states, 60 | changed: status line from Expanded on, the top bar's supporting line on Medium, a snackbar on Completed / Failed on Compact |
| RUN-13 | Run summary log line on completion / stop | 50 Run states | done |
| RUN-14 | Run start errors (no image file, save first, browser worker errors), Failed state | 60 Run start errors | done |
| RUN-15 | Opening a project makes frame 0 and nothing more | 60 The live run | done |
| RUN-16 | Export waiting / holding the run: refusals of New, Open, Close, F5 reported in the status line | 60 Automatic re-run | done: the shared state refuses and reports them |

## Memory (MEM)

| ID | Feature | Ref | Status |
|---|---|---|---|
| MEM-01 | Memory warning before a preview run that exceeds the RAM cache | 60 Memory warning | done |
| MEM-02 | Warning levers: RAM Auto (browser), halve resolution, half frames, 8-bit; greyed when moot | 60 Memory warning | done |
| MEM-03 | Pre-tick rule, live need / budget bar and line | 60 Memory warning | done |
| MEM-04 | "Do not warn me again"; Run applies ticks; Cancel keeps frames and logs | 60 Memory warning | done |
| MEM-05 | Frame cache read-out "RAM used / RAM cache", "Spill n", tooltip | 00 Main window | changed: RAM `Chip` in the status line, its tooltip with the counts and the spill |

## Undo and clipboard (EDIT)

| ID | Feature | Ref | Status |
|---|---|---|---|
| EDIT-01 | Undo (Ctrl+Z) / Redo (Ctrl+Y, Ctrl+Shift+Z), unlimited, 0.5 s step grouping | 60 Undo and redo | done |
| EDIT-02 | A focused text box keeps its own undo, cut, copy, paste, select all, Delete | 10 Edit | done |
| EDIT-03 | Cut / Copy / Paste objects as XML (app format and plain text) | 60 Clipboard | done |
| EDIT-04 | Paste behind the current item; settings object replaces on its row or the root; name clash " copy" | 20 Clipboard | done |
| EDIT-05 | Paste enabled per clipboard change | 20 Clipboard | changed: Paste stays enabled; a clipboard without objects pastes nothing |
| EDIT-06 | Select all (Ctrl+A) in tree and pictures | 10 Edit | changed: Ctrl+A selects all items wherever a text box has no focus; the pictures follow the selection |
| EDIT-07 | Delete (Del) removes the selection | 10 Edit | done |
| EDIT-08 | Selection follows undo by name, else index; else root | 60 Undo and redo | done |

## Export (EXP)

| ID | Feature | Ref | Status |
|---|---|---|---|
| EXP-01 | Export field / intensity / both (Ctrl+Shift+E / I / B) | 10 File > Export | done |
| EXP-02 | Export field / intensity frame (Ctrl+Shift+F / G) | 10 File > Export | done |
| EXP-03 | Export last (Ctrl+E): same outputs, no dialog, replaces silently | 10 File 8 | done |
| EXP-04 | Formats: MP4, MPEG-1, AVI, MOV Animation, MOV PNG, PNG / JPEG / EXR (I, sqrt I) sequences | 70 Export Save dialog | done |
| EXP-05 | One path box per output with "..."; layer labels; OK rules (name, no shared output) | 70 Export Save dialog | done |
| EXP-06 | Archive into one ZIP (setting `ExportArchive`), atomic archive | 70 Export Save dialog | done |
| EXP-07 | OpenH264: Cisco notice, consent box, download at first MP4, Delete the downloaded codec | 70 Export Save dialog | done |
| EXP-08 | H.264 profile with unsupported profiles greyed and the reason | 70 Export Save dialog | done |
| EXP-09 | Remembered format; start folder per kind and project (`ExportHints`, `LastExportFolder`) | 70 Export Save dialog | done |
| EXP-10 | Replace prompts (folder with files, file, archive) | 70 Export Save dialog | done |
| EXP-11 | Full-quality run first ("waiting for the run"), then "Exporting n / N frames"; Esc cancels | 70 Export Save dialog | done |
| EXP-12 | A still renders what a video would show at that frame | 70 Export Save dialog | done |
| EXP-13 | Probe CSV export (point: step rows, line: frame rows) | 70 Export Save dialog | done |
| EXP-14 | Browser: name-only boxes, always ZIP for many files, WebCodecs profile query, OPFS requirement | 70 Browser differences | done |
| EXP-15 | Browser: "The export is ready" with Download (user click starts it) | 70 Browser differences | done |
| EXP-16 | End, cancel, failure reported in the status line / message window | 70 Export Save dialog | done |

## Dialogs and windows (DLG)

| ID | Feature | Ref | Status |
|---|---|---|---|
| DLG-01 | Message window: error / question disc, button sets, Enter default, Esc cancel, mnemonics without Alt | 70 Message window | changed: Dialog actions take their mnemonic with Alt (`DialogAction::mnemonic`; the bare letter in the classic window); Enter the default action; Esc cancels |
| DLG-02 | One modal at a time; later ones queue; close box during a modal acts after it | 70 | done |
| DLG-03 | Welcome page: quality text, RAM row, presets Low / Medium / High / Auto, levers line, Settings... / Examples..., show at launch, Close | 70 Welcome page | changed: the gallery's start strip (New, Open, Recent, preview quality with RAM, presets and levers line, show at launch) replaces the welcome page; shown at launch over the workspace; File > Start page... reopens it |
| DLG-04 | First start: quality chosen from the machine, settings written, Info line, welcome page | 70 First start | done |
| DLG-05 | Examples: 6 topic tabs, search filtering and dimming tabs, title + description rows, keys, Open / Cancel / Welcome page | 70 Examples dialog | changed: gallery page over the workspace: topic filter chips (All + 6), search over every topic, card grid, Back / Esc returns; palette opens an example by name. Students and teachers start here |
| DLG-06 | About: icon, name, version (hash7), tagline, copyright, credits with links | 70 About dialog | changed: About page over the workspace; credits as text |
| DLG-07 | About: license toggle with LICENSE.md rendered; OpenH264 notice and licence toggle | 70 About dialog | done: Text buttons show and hide the rendered LICENSE.md and the OpenH264 licence in scrolling boxes |
| DLG-08 | About: System info box, Copy system info | 70 About dialog | done |
| DLG-09 | Updates: check at launch (setting) and on demand; download with progress, verify, restart prompt; manual download text | 70 Updates | done (check and download on threads, progress in the status line, restart through the unsaved-changes prompt) |
| DLG-10 | Crash report at next start: path, Copy path / browser report text, Copy report | 70 Crash report | done |
| DLG-11 | Manual window (contents tree, rendered MANUAL.md, figures with badges) | 70 Manual window | dropped (brief): the old app drops its manual window too (decision 0028); Help > Project page on GitHub opens the repository |

## Messages and status (LOG)

| ID | Feature | Ref | Status |
|---|---|---|---|
| LOG-01 | Status: latest Info+ log line of WaveDude's own crates (dependencies only in Messages and the log file); warning orange, error red until the next Info | 70 Status bar | changed: status line on Expanded / Medium; on Compact warnings and errors as snackbars; changed: warning in the tertiary container colours, error in the error tone |
| LOG-02 | Status: run status | 70 Status bar | done |
| LOG-03 | Status: busy text with determinate / indeterminate bar (export, waiting, computing, downloads) | 70 Status bar | done |
| LOG-04 | Status double-click opens / focuses Messages | 70 Status bar | changed: a click on the status line or Alt+3 opens Messages: a bottom panel on Expanded and up, a full-screen dialog on Compact; Medium's side sheet never opens (gap 31) |
| LOG-05 | Messages: level toggles with counts, search, Regex, Auto-scroll, Clear | 70 Messages pane | done |
| LOG-06 | Messages: columns Time, Level, Source, Message; level colours; resizable columns | 70 Messages pane | done |
| LOG-07 | Messages context menu: Copy, Copy all, Clear (tab-separated) | 10 Messages pane list | done |
| LOG-08 | 5000-entry ring, batched; tooltips with exception text and full logger | 70 Messages pane | done |
| LOG-09 | Trace / Debug visible in debug builds only | 70 Messages pane | done |

## Settings (SET)

| ID | Feature | Ref | Status |
|---|---|---|---|
| SET-01 | Settings (Ctrl+,): search, sections, rows; change applies and saves at once | 70 Settings | changed: Settings page over the workspace (Back returns, a run goes on), list-detail by window class |
| SET-02 | General: Theme, UI zoom (with `--zoom` value), Full path in the title, Show the welcome page, Undo steps | 70 Settings | done: Undo steps is a log-scale whole-number `NumberField`, set on Enter or focus loss, its tip as supporting text; log ties differ by float noise (DESIGN.md section 13) |
| SET-03 | General: Language | 70 Settings | dropped (brief) |
| SET-04 | Preview and memory: RAM (native read-only, browser sizes), presets | 70 Settings | changed: native read-only RAM only; the browser's sizes in the web build |
| SET-05 | Preview resolution, frames, samples, dithering (only with 16-bit), Use the GPU | 70 Settings | changed: Use the GPU is the Engine drop-down of the GPU and the CPU backends (port) |
| SET-06 | Simulation: Re-run automatically, Warn when a run does not fit | 70 Settings | done |
| SET-07 | Export: Use OpenH264 (absent when compiled in), H.264 profile, Archive (native) | 70 Settings | changed: a profile the encoder lacks is named in the supporting text and not taken, not greyed (library) |
| SET-08 | Updates: Look at each launch, Check now (updater builds) | 70 Settings | done |
| SET-09 | Search by label and section; grouped results; "No setting matches"; keys | 70 Settings | changed: palette entries open the row's section, not scrolled to the row |
| SET-10 | Settings file: sparse JSON, versioned, atomic write, unknown keys kept, bad values logged | 80 | done |
| SET-11 | Session-only state: speed, loop, Live, split, Export last target | 80 | done |

## Keyboard (KEY)

| ID | Feature | Ref | Status |
|---|---|---|---|
| KEY-01 | Every window-wide key of ui/10 (files, edit, export, zoom, run, playback, settings) | 10 Window-wide | changed: file, zoom, settings, run, playback and edit keys bound; export keys. New: Ctrl+K and Ctrl+Shift+P palette, Alt+0..3, F2 rename, `A` add at the pointer |
| KEY-02 | Apps / Shift+F10 opens the focused element's context menu | 10 Window-wide | done: Shift+F10 or the Apps key opens the menu of the focused picture, handle, graph box or outline row |
| KEY-03 | Esc order: drag, text revert, export cancel, run stop | 10 Window-wide | done |
| KEY-04 | Tab / Shift+Tab through controls in a stable order | 10 Window-wide | partial: creation order is fixed (no hover- or hash-dependent widgets; the one HashMap is a lookup); not verified by a Tab walk in the GUI |
| KEY-05 | Colour picker and swatch keys | 10 Colour swatch | done: the swatch is focusable (Space, Enter, Down open), Enter in the picker keeps and closes, Esc reverts; square and hue arrows by `ColorPicker` |
| KEY-06 | Dialog keys (Enter default, Esc cancel) | 10 Dialogs | done: `Dialog::default_action` and Esc dismiss |
| KEY-07 | Picture splitter and layout radios keys | 10 Picture splitter | changed: the split's arrow keys by the library's `SplitPane`; the layout toggle is a canvas toolbar action |

## Language (I18N)

| ID | Feature | Ref | Status |
|---|---|---|---|
| I18N-01 | English UI, British spelling except "license" | 00 Languages | done |
| I18N-02 | Missing English key is a bug (panics with the key) | 00 Languages | done |
| I18N-03 | Deutsch, Francais, Nederlands, System language | 00 Languages | dropped (brief) |
| I18N-04 | Live language switch | 00 Languages | dropped (brief) |
| I18N-05 | Log lines not localised, decimal point | 00 Languages | done |

## Platform (PLAT)

| ID | Feature | Ref | Status |
|---|---|---|---|
| PLAT-01 | Native build on Linux, macOS, Windows; icon, app id, single binary | 00 | changed: native launcher verified on Linux; app id waveduderust-material |
| PLAT-02 | Browser build (wasm32): page, run worker, spill worker | 00, 60 | done: `WAVEDUDE_WEB_APP=waveduderust-material tools/build-web.sh`, the page and the workers of waveduderust |
| PLAT-03 | Settings in browser storage; spill to the origin private file system | 80, 00 | done: app-state's `Store` and spill worker |
| PLAT-04 | Browser open by upload, save / export by download | 70 Browser differences | done |
| PLAT-05 | Engines share the renderer's GPU device natively; WebGPU in the browser; WebGL2 fallback warning | 40 8-bit vs 16-bit | done: as waveduderust (`waveduderust_egui_platform::share_device`) |
| PLAT-06 | Logs to file natively (log folder), panic hook | 70 Crash report | done |
| PLAT-07 | Updater (cargo-packager) native only | 70 Updates | done (`waveduderust_app::shell`, native only) |
| PLAT-08 | Native file dialogs off the UI thread | 70 Export Save dialog | done |
| PLAT-09 | Touch input: tap, long press, pinch, scroll by drag | 00 Touch | partial: tap and pinch work in Chrome's touch emulation; long press and scroll by drag not checked |
| PLAT-10 | Browser clipboard copy / paste of objects | 60 Clipboard | done: the browser's paste event feeds Paste |

## Added by this UI (UX)

| ID | Feature | Ref | Status |
|---|---|---|---|
| UX-01 | Workspace always on screen; gallery, Settings, About are pages over it; a run goes on underneath | DESIGN 3, 7 | done |
| UX-02 | Selection bar over the canvas selection on touch-first devices and Medium / Compact (Enabled, Duplicate, Remove, more) | DESIGN 4 | changed: a floating `Toolbar` (name opens the inspector, Enabled, Duplicate, Remove, more), the more menu anchored at its action; hidden during a drag |
| UX-03 | Column toggles Alt+1..3 and Canvas only (Alt+0) | DESIGN 3 | done |
| UX-04 | Gallery: start strip, topic chips, search, card grid; palette "Open example: <title>" | DESIGN 7 | done |
| UX-05 | Inspector header: name, Enabled, Duplicate, Remove, overflow; "<name> and n more" for several selected | DESIGN 6 | done |
| UX-06 | Enter on a selection focuses the inspector; F2 focuses the name; Esc in a field returns to the canvas; palette "Go to <item>" | DESIGN 6, 8 | changed: Enter on a row focuses the form, F2 the name, palette "Go to <item>"; Esc in a field reverts it and keeps the focus (Esc to the canvas todo) |
| UX-07 | `A` on the canvas opens the Add menu at the pointer; the item lands there | DESIGN 4 | done: with the canvas focused |
| UX-08 | Gallery cards show the scene painted from the project (off-thread, cached) | DESIGN 7 | done: each card shows its project's scene as the scene view shows it (an image scene, else its glowing edges) at most 320x180, painted on one worker thread (one card per frame in the browser) and cached as a texture by the example's stem; the text shows until it is ready |
| UX-09 | Help > Keyboard shortcuts: the key table in a dialog | DESIGN 8 | done |
| UX-10 | Export split button in the top bar: Export last, else Export both; menu = File > Export | DESIGN 8 | done |
| UX-11 | Below Expanded: the outline in a modal side sheet, the inspector in a bottom sheet (detents 40 % / 90 %) | DESIGN 3, 6 | done |
