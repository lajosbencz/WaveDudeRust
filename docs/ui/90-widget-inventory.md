# Widget inventory of `wavedude-app` (egui)

Derived from files 00..80. Each widget: required behaviours, then what it is built on in
[`crates/wavedude-app/src/ui`](../../crates/wavedude-app/src/ui/) (decision 0019).

Cross-cutting requirements for every widget:
- Keyboard first: focusable where interactive, visible focus (accent border), Tab order = visual order.
- Theme tokens only (no hard-coded colours) except the preview overlays and graph boxes (fixed colours).
- Re-labels live on a language switch; scales with the UI zoom; lines on whole device pixels.
- Its role and label in egui's AccessKit tree, through which the window's tests find it.
- 23 px row height for rows, toolbar buttons, the menu bar and tabs; 22 px for drop-down menu entries, text
  boxes, number boxes and combo boxes; 16 px icons in text colour (greys when disabled).
- WPF's box model: square corners everywhere; buttons padding 10,3 inside a 1 px border (toolbar text buttons
  6,1,6,0), text fields padding 3,2 inside a 1 px border that turns to the hover colour, then the accent on
  focus; the UI font is Segoe UI, else the bundled Selawik, at 12 px.

## 1. Dock host

Behaviours:
- Layout tree: splits (h/v, star weights), tabbed groups as leaves, one fixed centre area.
- Splitters between siblings: drag, arrow keys.
- Pane title bar, 21 px: title, a dotted drag grip filling the rest, "Window position" drop-down (Move to
  Left/Right/Top/Bottom, Close), close button (16 px glyph buttons); active/inactive colouring by focus, the
  first group active at start.
- Tab strip when a group holds several panes; a tab is a row high (23 px), its text 10 px from either edge;
  the selected tab on the tool window's background with a side and bottom border and accent-blue text.
- Drag a title bar (group) or tab (pane) past a threshold: overlay with 4 edge targets, 5-way cluster on the
  hovered pane/centre, translucent drop preview, ghost label; drop = dock at edge, tab into group, or split.
- Show/hide panes (hide = keep registration); default side for a pane shown without a saved place.
- Serialize/deserialize the layout by pane id (unknown ids ignored); default layout definition.
- Sizes survive rebuilds; hosted content keeps its state across re-parenting.
Base: egui_tiles, with the app's title bars, tabs, drop overlay and saved arrangement (`dock.rs`).

## 2. Menu bar and menus

Behaviours:
- Top-level headers with mnemonics (underlined letter, Alt+letter, F10/Alt to enter, arrows to walk, letter to
  activate when unique).
- Entries: icon slot (glyph or drawing), text with mnemonic, right-aligned shortcut text, submenu arrow,
  check mark / radio dot, separator, disabled state, tooltip.
- Header text and check state bound to state (Pause/Resume, Play/Pause playback).
- Dynamic item lists (recent files, examples, panes, languages, RAM cache options).
- Context menus from right-click, Apps key and Shift+F10 at the focused element; same entry model.
- Keyboard: Esc closes a level; first entry focused when opened by keyboard.
- A click outside while open closes and still lands on its target (incl. window title bar where the platform
  allows).
Base: egui's menus and context menus; the icon and shortcut columns, mnemonics and key walk are the app's
(`menu_bar.rs`, `menu_keys.rs`).
Port: an entry carries a typed action (the app's `MenuAction`, the preview's `PictureMenu`) beside its string
test id; activation reports the action, never the id.

## 3. Tree view (project tree)

Behaviours:
- Hierarchical rows from heterogeneous nodes (root, settings rows, folders, items), expanded by default,
  chevrons to collapse.
- Row template per node kind: check box (not focusable), icon, text, right-aligned glyph buttons (not tab stops);
  folder plus button (one opens a menu).
- Disabled-item styling (icon + text greyed).
- Single selection + shared multi-selection: Ctrl+click toggle, Shift+click/Shift+arrows range from anchor,
  Insert/Ctrl+Space toggle, Ctrl+A; current row vs other-selected rows styled differently.
- Selection set from outside without stealing focus; scroll into view.
- Keys: arrows, Home/End, Space, Del, Ctrl+D, Alt+Up/Down.
- Drag source (row, not its buttons) carrying text; drop target with insertion feedback: line above/below a row
  (by half), outline around a folder; claims only drags it accepts.
- Per-row context menu with the node as target.
Base: own rows in a ScrollArea, egui's drag and drop payloads (`tree.rs`).

## 4. Toolbar buttons

Behaviours: flat text+icon button with plus badge; flat 16/18 px glyph button; tooltip; disabled skipped by Tab;
toggle variant (pressed while checked); radio variant (each button a tab stop).
Base: own painted buttons (`tree.rs`, `preview/transport.rs`).

## 5. Property form (form panel and rows)

Behaviours:
- Two-column layout: label column as wide as the widest label (min width), editors fill the rest; full-width rows.
- Rows at a 28 px pitch (24 px row, 4 px spacing) as WPF lays out its 23 px editors.
- Section headers (name over a rule); gaps after groups.
- Conditional enable (greyed) and visibility per row.
- Read-out rows (text bound to several inputs).
- Repeated row lists with per-row remove button and an Add button (points, nodes, stops).
- Collapsible description header with chevron (Space toggles, persisted).
- Vertical scroll, never horizontal; caret into the first text box on a new item.
Base: own two-column layout in a ScrollArea (`properties/`).

## 6. Text box

Behaviours: Enter commits to the bound value and selects all; Esc reverts when changed (consumed only then);
per-keystroke binding option; multiline option; own undo (Ctrl+Z) and clipboard handled before window commands;
placeholder text (Messages search); option to select all text when the box gains focus (the object Name box).
Base: egui's TextEdit with the commit policy of the pending edit (`text_box.rs`, `edit.rs`); the box selects its
text when it gains the focus.

## 7. Number box (linear and logarithmic)

Behaviours:
- Text + two repeat buttons (auto-repeat; clicking focuses).
- Up/Down step, PageUp/PageDown 10 steps, wheel 1 (Shift 10) while focused.
- Horizontal drag scrub on an unfocused box past a threshold: per-pixel step, Shift finer x0.1, Ctrl coarser x10;
  Esc restores; press without move = caret click.
- Logarithmic mode: x1.02 per step, 10 steps per page, doubling per N pixels on drag, three significant digits.
- Min/max clamp on steps; steps and drags round to the increment's decimals; the value shows in its shortest
  form ("120", "1.5", "0.125"); typed text keeps its digits, clamped; culture-aware parse/format.
- Blank mode: an empty box for a value that does not apply.
- Look: 22 px, padding 3,2, 1 px border (hover, focus accent), 14 px spin column with 4 x 2 chevrons, hover and
  pressed backgrounds; disabled: window background, grey text, no spin buttons.
- Writes through to the bound value on every step; selects text after a step.
Base: egui's TextEdit with own steps, drag and spin buttons (`number_box.rs`).

## 8. Check box, combo box, editable combo

Behaviours: check box with label content; enum combo with localised words; editable combo (speed box) with preset
list and free typing, swallowing Space; keyboard open/close and arrow selection.
Base: own check box and combo box over egui's Popup (`properties/mod.rs`); the speed box a number box with a presets
Popup (`preview/transport.rs`).

## 9. Colour scheme picker (combo with gradient rows)

Behaviours: rows with gradient strip + name + grey tag; greyed missing name; italic action rows (Load from file...,
Edit a copy in the project...); list updated in place; value = scheme name.
Base: the combo box with painted rows, the gradient as a mesh (`properties/`, `gradient_editor.rs`).

## 10. Colour swatch button and colour picker popup

Behaviours: swatch toggles a popup below it (Space/Enter/Down); SV square + hue strip (Canvas), markers, mouse drag,
arrow keys with Shift x10; R/G/B number boxes; hex box (Enter applies); live colour commit throttled to 10/s plus on
drag end; Enter keeps, Esc reverts, click outside closes without reopening; focus returns to the swatch.
Base: egui's Popup with painted square and strip (`color_picker.rs`).

## 11. Gradient editor rows (colour scheme form)

Behaviours: gradient bar; stop rows (position number box 0..1, swatch, hex box, remove); Add stop in the widest gap;
Based-on combo copying stops.
Base: composed from 5, 7, 10 (`gradient_editor.rs`).

## 12. Canvas image (preview picture)

Behaviours:
- Sized by the canvas size (aspect), never by the bitmap; uniform fit, centred; reports its device-pixel size.
- Draws an RGBA bitmap of arbitrary size into that rect: 1:1 (16-bit path, rendered at device size) or scaled
  (8-bit path); updated up to 60 times a second from a background renderer without layout changes.
- 1 px edge line around it in a state colour.
- Layered: scene outline image (translucent), handle overlay, graph box overlay, busy dim + spinner.
- Context menu; focusable pane for Space.
- Its hooks and callbacks compare by identity: made once by the parent, with a redraw key that changes with the
  overlay's inputs, the picture does not re-render with its parent. The device size is reported after layout.
Base: a wgpu texture written in place, or the GPU engine's frame coloured in a paint callback (`preview/picture.rs`,
`preview/texture.rs`, `preview/device.rs`).

## 13. Scene overlay (handles)

Behaviours:
- Maps canvas pixels to view coordinates with the picture's uniform scale; clips to the picture.
- Handle kinds (source, part, lever, grip, lens surface, point, node, Bezier in/out, probe A/B) as odd-sized glyphs
  over a dark halo; selected = yellow, focused = accent fill, hover/focus thicker stroke; tooltips.
- Each handle focusable, in Tab order; focusing selects its item; arrows/Shift move, Del removes.
- Drag with absolute-position measurement, clamping, group drag with limit, Shift snapping for levers, Esc cancel.
- Ctrl/Shift click selection; marquee on empty space (Shift adds, Ctrl toggles).
- Non-interactive drawing layers: probe lines (dark under dashed light), emitter dots (thousands, cached as a
  bitmap), part outlines in selection colour, levers, Bezier dotted lines, labels with halo; selected drawn last.
- Visibility rules (Handles/Labels settings, current-item-only handles).
- Drop target for objects (paste at pixel).
- Hit testing must not walk thousands of dots under a resting pointer.
Base: the view's epaint shapes, own hit testing and focus (`preview/overlay.rs`).

## 14. Probe graph box overlay

Behaviours: draws boxes (translucent dark, axes, labels, curve clipped to the plot, title strip with readout) with
fixed text size; linear and symmetric/logarithmic axes; each box draggable and focusable (arrows, Del), corner grip
resize snapping to fixed sizes; snap/attach to probe point with hysteresis; selected border; context menu (CSV);
cached text layout.
Base: the view's epaint shapes (`preview/graph_boxes.rs`).

## 15. Header strip

Behaviours: title + optional caption + tools (number box, Auto button); caption dropped first, title trimmed when
short; can be laid out rotated 90 degrees beside the picture (picture frame decides above vs beside by aspect).
Base: own layout, turned text shapes and an upright editing Area (`preview/header_strip.rs`).

## 16. Picture splitter / split container

Behaviours: two cells side by side or stacked; 4 px splitter, tab stop, arrows move 10 px, Enter/double-click
resets to half; collapses when one cell is hidden and restores the split.
Base: own splitter (`preview/mod.rs`).

## 17. Timeline

Behaviours: custom-drawn bar with tick labels (interval by label width), playhead with frame label, in/out brackets
and band, available-frames shading (accent or grey when stale), hover line and label; click/drag scrub, bracket
drag, keys Left/Right/PageUp/PageDown/Home/End, wheel; cached tick drawing; whole-pixel lines; test value exposure.
Base: painted shapes on whole device pixels (`preview/timeline.rs`).

## 18. Transport bar

Behaviours: glyph buttons and toggles with state-driven glyph/tooltip/command (Run/Stop, Pause/Resume), focus
handoff Play<->Pause, readout text "frame / last" + dim step, speed editable combo, layout radio group.
Base: composed from 4 and 8 (`preview/transport.rs`).

## 19. Tooltip

Behaviours: on hover after 400 ms (WPF's initial show delay), under the pointer and kept inside the window;
a press or leaving hides it; localised text, themed, zoomed, square 1 px border, padding 6,3, UI font; on menu
entries too (shown 400 ms after the entry is highlighted).
Base: egui's tooltips (`on_hover_text`) in the app's visuals.

## 20. Busy indicators

Behaviours: dim rectangle (45 % black) + spinner (three-quarter ring, one turn per second) over the pictures after a
150 ms delay, not hit-testable; status bar progress bar determinate/indeterminate; platform taskbar progress where
available.
Base: painted dim and spinner (`preview/picture.rs`), own progress bar (`status_bar.rs`); no taskbar progress.

## 21. Status bar

Behaviours: left log line with background/foreground switching for warn/error; right run status; far-right busy
text + bar; tooltip; double-click opens Messages; resize grip corner.
Base: own row in egui's bottom panel (`status_bar.rs`).

## 22. Message list (Messages pane)

Behaviours: level toggles with counts and colours, search box with regex option and tolerant parsing, auto-scroll,
clear; columns Time/Level/Source/Message with resizable headers; per-level row colour; multi-select; context menu
Copy/Copy all/Clear; 5000-entry ring buffer with batched updates; virtualised rows; row tooltip. The visible
lines are kept as ids per (log revision, filter), so a hover or a selection change neither copies lines nor
compiles the search again.
Base: own columns and virtualised rows in a ScrollArea (`messages/`).

## 23. Dialog window

Behaviours: modal owned window with themed title bar, icon disc, wrapped text, button sets with default/cancel,
mnemonics without Alt; sized to content; zoomed. Also used for the memory warning (check boxes with greyed states,
live result line) and About (links, collapsible document, read-only text, copy).
Base: egui's Modal over the window (`modal.rs`).

## 24. Document viewer (manual, license)

Behaviours: render Markdown (headings with GitHub anchors, paragraphs, tight lists, emphasis, code, links) and
figures (PNG with numbered badge discs and tooltips) in a scrolling column; whole-pixel scrolling; text selection
and copy; scroll-to-heading; contents tree beside it with a splitter; internal anchor links and external links.
Base: egui_commonmark with own code blocks and figures (`markdown.rs`); the manual a viewport of its own, a
Window in the browser (`manual.rs`).

## 25. Icon view

Behaviours: 16 px icon from an icon font glyph or an app vector drawing (stroke + optional fill), text colour,
optional plus badge; lens profile variants chosen from data.
Base: egui_material_icons, and SVG path data flattened into meshes (`icons.rs`).
