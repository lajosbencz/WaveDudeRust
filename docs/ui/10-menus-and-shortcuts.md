# Menus and shortcuts

Pointers: `MainWindow.xaml` (menu bar), `Panes/ProjectTree.xaml` (tree menus), `Panes/PreviewPane.xaml`
(picture menus, pane keys), `Commands/*.cs` (gestures), `Strings/Strings.*.xaml` (headers, mnemonics).

Conventions:
- Mnemonic column: the underlined letter per language, EN / DE / FR / NL. Must stay unique within each menu
  in every language (also for entries shared between menus).
- Icon: platform icon font code point (Segoe MDL2/Fluent, `Uxxxx`) or an app drawing name (`Draw:<name>`).
  The port needs an equivalent icon for each.
- Every entry is 23 px high, shows its shortcut text right-aligned.
- A disabled entry greys text and icon.
- (port) While a modal shows, the menu bar is greyed and takes no pointer or key.
- Port: menu bar, tree menus and shortcuts dispatch one exhaustive `MenuAction` enum (`wavedude-app` [`menus.rs`](../../crates/wavedude-app/src/menus.rs));
  the ids in this document name the entries.
- Keys in the menus: Alt held underlines the mnemonics, as do keys working the menus. Alt+letter opens that menu
  of the bar on its first entry; F10 or a lone Alt puts the focus on the bar's first header, where Left/Right walk
  the headers and Down, Enter, Space or a letter opens one (Up: on its last entry). In an open menu Up/Down walk the
  enabled entries, Right or Enter opens a submenu, a letter picks its entry (several sharing it take the focus in
  turn), Home/End reach the ends, Left/Right open the neighbouring menu of the bar, Esc closes one level (from a
  menu of the bar back to its header, from there out of the bar) and the focus returns to where the keys took it
  from. The pane title bar's menu takes letters too. The mnemonics are the entries' access keys for assistive
  technology (port: [`ui/menu_keys.rs`](../../crates/wavedude-app/src/ui/menu_keys.rs)).

## File

| # | Entry | Mn EN/DE/FR/NL | Shortcut | Icon | Enabled | Action |
|---|---|---|---|---|---|---|
| 1 | New Project | N/N/N/N | Ctrl+N | U+E8A5 | always | Asks about unsaved edits, then a new empty project: blank canvas, open edges, damping rim outside (-32), no sources. Timeline to frame 0. |
| 2 | Open Project... | O/O-umlaut/O/O | Ctrl+O | U+E8E5 | always | Asks about unsaved edits; file dialog, filter: WaveDude Next projects (*.wdproj), Wave Dude 2D projects (configuration.json; *.json), XML (*.xml), All. A `configuration.json` is imported as a new unsaved project. |
| 3 | Recent Projects > | R/Z/R/R | - | U+E81C | always (empty submenu when none) | One entry per recent file, newest first, full path as plain text (no mnemonics). Click opens. |
| 4 | Examples... | P/P/P/V | - | U+E8F1 | always | Opens the Examples dialog ([ui/70](70-dialogs-and-windows.md)) of 44 projects; id `file:examples`. An example opens as an unsaved project named after its file (title shows it; Save As proposes it). List: see `MANUAL.md` "Examples". |
| - | separator | | | | | |
| 5 | Save | S/S/E/S | Ctrl+S | U+E74E | always | Save; Save As when the project has no file. |
| 6 | Save As... | A/U/S/A | Ctrl+Shift+S | U+E792 | always | Save dialog, filter: *.wdproj, *.xml. Proposes the suggested name (import/example) beside its origin. |
| - | separator | | | | | |
| 7 | Export > | E/E/X/E | - | U+EDE1 | always | Submenu below. |
| 8 | Export last | L/W/D/H | Ctrl+E | U+E8EE | after the first completed run export since the project was opened | Repeats the session's last completed run export: same layers, format, path; no dialog; existing file or sequence folder replaced without asking. A still, a cancelled or a failed export does not count. |
| - | separator | | | | | |
| 9 | Close Project | C/C/F/L | - | U+E711 | unless the document is the empty placeholder project of the start or of an earlier Close (`Document::is_placeholder`) | Asks about unsaved edits, then an empty project. A project made by New, Open, an example or an edit enables it. |
| - | separator | | | | | |
| 10 | Exit | X/B/Q/F | Alt+F4 | U+E7E8 | always | Asks about unsaved edits, quits. |

### File > Export

| Entry | Mn | Shortcut | Icon | Enabled | Action |
|---|---|---|---|---|---|
| Export field... | F/F/C/V | Ctrl+Shift+E | U+EDE1 | no export running and Steps > 0 | Save dialog, then the field over the export range as video or sequence. |
| Export intensity... | I/I/I/I | Ctrl+Shift+I | U+EDE1 | same | Same for the intensity. |
| Export both... | O/B/D/B | Ctrl+Shift+B | U+EDE1 | same | Two outputs from one name (`_field`, `_intensity` appended). |
| - separator | | | | | |
| Export field frame... | R/Z/M/E | Ctrl+Shift+F | U+EB9F | same | The frame shown as one image. |
| Export intensity frame... | M/X/T/D | Ctrl+Shift+G | U+EB9F | same | Same for the intensity. |

Dialog details: [`70-dialogs-and-windows.md`](70-dialogs-and-windows.md).

The browser build has no exports: the Export submenu and Export last are absent, their shortcuts inert (decision 0021).

### File dropped on the window

A file dropped on the window, on both targets, is read as follows; the last file of a frame wins, and nothing is taken while a modal window shows.

- An image (the extensions of the scene image picker) becomes the scene image, as the picker's answer does.
- Any other file goes to the state's `Event::DropFile`: a project (`.wdproj`, `.xml`, `.json`) or a `.wdscheme` opens as File > Open does, after the question about unsaved edits; a file of another type reports the Open error.
- The tree and the pictures claim a drag of their own objects first (`20-project-tree.md`, Drag and drop).

In the browser the page asks before it is left (closed, reloaded, navigated away) while the project has unsaved edits.

## Edit

| Entry | Mn | Shortcut | Icon | Enabled | Action |
|---|---|---|---|---|---|
| Undo | U/R/A/O | Ctrl+Z | U+E7A7 | a step to undo | Project step back. A focused text box keeps its own Ctrl+Z. |
| Redo | R/W/R/I | Ctrl+Y, Ctrl+Shift+Z | U+E7A6 | a step to redo | Step forward. |
| - separator | | | | | |
| Cut | T/A/U/N | Ctrl+X | U+E8C6 | list items selected (sources, parts, probes, schemes) | Copy then remove. Text box first, for its text. |
| Copy | C/K/P/K | Ctrl+C | U+E8C8 | copyable selection (list items, Scene, Simulation, Display) | Objects to clipboard as XML text + app format. |
| Paste | P/E/O/P | Ctrl+V | U+E77F | clipboard parses to objects | Paste behind the current item; a settings object replaces on its own row or the project root. |
| Delete | D/L/S/V | Del | U+E74D | `CanDelete` | In a text box: deletes its selected text. Else removes the selection (as Remove). |
| - separator | | | | | |
| Select all | A/S/T/A | Ctrl+A | U+E8B3 | project has items | Every source, part and probe; last one becomes current. Works in tree and preview; a text box keeps its own Ctrl+A. |

## View

All settings here are user settings, persisted, not in the project.

| Entry | Mn | Icon | Kind | Action |
|---|---|---|---|---|
| Panes > | P/F/P/P | U+E8A0 | checkable list | One entry per pane (Preview, Project, Properties, Messages; decision 0008), check = shown; click toggles. Pane title as header. |
| Theme > | T/D/T/T | U+E790 | radio list | System, Light, Dark. |
| UI zoom > | Z/Z/Z/Z | U+E8A3 | radio list | 0.5x, 0.75x, 1x, 1.25x, 1.5x, 2x, 2.5x, 3x (no mnemonics). |
| Language > | L/S/L/L | U+E774 | radio list | System (in current language), English, Deutsch, Francais, Nederlands (own names). |
| - separator | | | | |
| Preview resolution > | R/A/R/R | Draw:PreviewResolution | radio | Full (F/V/C/V), Half (H/H/M/H), Quarter (Q/I/Q/K). Divisor 1/2/4. |
| Preview frames > | F/B/I/F | Draw:PreviewFrames | radio | Full, Half (every second frame), Quarter (every fourth frame). |
| Preview samples > | S/W/H/S | Draw:PreviewSamples | radio | 16-bit float (1), 8-bit palette index (brightness fixed when the run starts) (8). |
| Preview dithering > | D/H/M/D | Draw:PreviewDither | radio; whole submenu disabled unless samples = 16-bit | Off, 10, 12, 14, 16 bits per channel (mnemonics O,1,2,4,6). |

A change of resolution/frames/samples replaces the frames like a simulation edit. Dithering only redraws.

There is no Display menu. Display settings (handles/labels in the preview etc.) are the project's Display
row in the tree ([`30-properties-pane.md`](30-properties-pane.md)).

## Simulation

| Entry | Mn | Shortcut | Icon | Enabled | Action |
|---|---|---|---|---|---|
| Run | R/S/L/S | F5 | U+E768 | no run active and no export running | Run from step 0 with the project as it is; preview follows (Live). May ask the memory warning first. |
| Pause / Resume | P/P/P/P, Resume R/F/R/H | F6 | U+E769 | a run is active | Toggles pause; header reads Resume while paused. |
| Stop | T/T/A/T | Shift+F5, Esc | U+E71A | a run or an export is active | Esc/Stop cancels an export when one runs, else stops the run; computed frames stay. |
| Discard frames (port) | D/V/S/V | - | U+E74D | the run has frames stored and no export holds or waits for it | Asks "Discard the frames of the run?" (Yes / No, No the default); Yes stops a run and drops its frames from RAM and the spill (disk file, browser storage), the timeline starts over at frame 0, which is made again. The project and its history stay. |
| - separator | | | | | |
| Play / Pause playback | Y/D/E/L | F7 | U+E768 / U+E769 | `CanPlayPause` | The timeline's Play/Pause from anywhere. Header and icon switch while playing. |
| - separator | | | | | |
| Use the GPU | G/G/G/G | - | none | checkable | Engine choice (CPU fallback logged). Default on. |
| Re-run automatically after edits | A/A/U/A | - | none | checkable | Default on. |
| Warn when a run does not fit in RAM | W/W/V/W | - | none | checkable | Default on. |
| RAM cache size > | C/C/C/C | - | Draw:FrameMemory | radio list | Auto (A), 10 %, 20 %, ... 100 % of physical RAM (11 entries), check = current. |

## Help

| Entry | Mn | Shortcut | Icon | Action |
|---|---|---|---|---|
| About WaveDude Next | A/U-umlaut/P/O | - | U+E946 | Modal About dialog. |
| Manual | M/H/M/H | F1 | U+E736 | Opens the manual window, or brings it to the front. |
| Check for updates (port; builds with the updater) | U/U/J/U | - | none | Looks for a newer release now ([docs/ui/70](70-dialogs-and-windows.md), Updates). |
| Check for updates at launch (port; builds with the updater) | L/S/L/S | - | none | checkable; setting `CheckForUpdates`. |

## Context menus

### Project tree row (any row, the node is the target)

| Entry | Mn | Gesture text | Icon | Notes |
|---|---|---|---|---|
| Add source | S/Q/R/B | - | U+E710 | |
| Add part > | R/B/C/O | - | U+E710 | The part kinds submenu (below). |
| Add probe | B/O/N/S | - | U+E710 | |
| - separator | | | | |
| Add to or remove from selection | L/T/I/R | Ins | U+E762 | |
| Select all | A/S/T/A | Ctrl+A | U+E8B3 | |
| - separator | | | | |
| Enabled | E/I/A/I | Space | U+E73A | Toggles Enabled/Active; on a group selection each flips on its own. |
| Duplicate | D/D/D/D | Ctrl+D | U+F413 | A copy after each original; copies become the selection, caret in the last one's name. |
| Cut | T/A/U/N | - | U+E8C6 | |
| Copy | C/K/P/K | - | U+E8C8 | |
| Paste | P/E/O/P | - | U+E77F | |
| Remove | V/F/S/V | Del | U+E738 | |
| - separator | | | | |
| Move up | U/N/M/H | Alt+Up | U+E74A | |
| Move down | W/U/E/L | Alt+Down | U+E74B | |
| - separator (probe rows only) | | | | |
| Export probe data as CSV... | X/X/V/G | - | U+EDE1 | Probe rows only; enabled for an active probe only. |

### Add part (tree toolbar "+ Part", Parts folder "+", row menu > Add part)

Order: optics, separator, shapes, separator, logic submenu. Each entry's tooltip = the kind description
(`Kind.<Type>`).

| Entry | Mn | Icon |
|---|---|---|
| Lens/Mirror | L/L/L/L | Draw:Lens (biconvex lens beside a concave mirror) |
| Fibre | F/F/F/V | Draw:Fiber |
| Bragg reflector | B/B/B/B | Draw:BraggReflector |
| GRIN lens | G/G/G/G | Draw:GrinLens |
| Zone plate | Z/Z/Z/Z | Draw:ZonePlate |
| - separator | | |
| Polygon | P/P/P/P | Draw:Polygon |
| Rectangle | R/R/R/R | Draw:Rectangle |
| Ellipse | E/E/E/E | Draw:Ellipse |
| Triangle | T/D/T/D | Draw:Triangle |
| Outline | O/U/O/O | Draw:Outline |
| - separator | | |
| Optical logic > | I/O/I/I | Draw:OpticalLogic |
| > XOR (Mach-Zehnder) | X | Draw:XorGate |
| > AND | A | Draw:AndGate |
| > OR | O | Draw:OrGate |

"+ Part" without a kind opens this menu under the button (keyboard: Space opens, arrows walk, Enter picks).

### Preview picture (right-click or Apps key on a picture cell)

Same entries on both pictures, parameterised by the layer.

| Entry | Mn | Gesture text | Icon | Action |
|---|---|---|---|---|
| Brightness | B/H/L/H | - | U+E706 | Moves keyboard focus to that picture's brightness box. |
| Auto brightness | A/A/A/A | - | Draw:AutoBrightness | One-shot fit ([`40-preview-pane.md`](40-preview-pane.md)). |
| - separator | | | | |
| Export this frame... | T/D/E/D | Ctrl+Shift+F (field) / Ctrl+Shift+G (intensity) | U+EB9F | Still of this layer. |
| - separator | | | | |
| Export field... | F | Ctrl+Shift+E | U+EDE1 | |
| Export intensity... | I | Ctrl+Shift+I | U+EDE1 | |
| Export both... | O | Ctrl+Shift+B | U+EDE1 | |

### Probe graph box (on a picture)

| Entry | Mn | Icon |
|---|---|---|
| Export probe data as CSV... | X/X/V/G | U+EDE1 |

### Dock pane title bar drop-down ("Window position")

Move to Left (L/L/G/L), Move to Right (R/R/D/R), Move to Top (T/O/H/B), Move to Bottom (B/U/B/O),
separator, Close (C/S/F/S).

### Messages pane list

Copy (C/K/C/K, Ctrl+C, selected lines), Copy all (A/A/T/A, every visible line), separator, Clear
(L/L/E/W). Copied lines are tab-separated.

### Header strip

No own context menu; right-click or Apps on the strip opens the picture's menu (decision 0008).

## Global and focus-scoped keys

### Window-wide (unless a text box takes the key first)

| Key | Action |
|---|---|
| F10, Alt | Enter the menu bar. |
| Apps, Shift+F10 | Context menu of the focused element. |
| Ctrl+N / Ctrl+O / Ctrl+S / Ctrl+Shift+S | New / Open / Save / Save As. |
| Alt+F4 | Exit. |
| Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z | Undo / Redo / Redo (text box: its own). |
| Ctrl+X / Ctrl+C / Ctrl+V | Cut / Copy / Paste objects (text box: its text). |
| Ctrl+A | Select all objects (text box: its text). |
| Ctrl+E | Export last. |
| Ctrl+Shift+E / I / B | Export field / intensity / both. |
| Ctrl+Shift+F / G | Export field frame / intensity frame. |
| F1 | Manual (open or bring to front). |
| F5 | Run. |
| F6 | Pause / resume the run. |
| Shift+F5 | Stop the run. |
| Esc | Cancel a running export, else stop the run. Text box: reverts an edit first (only consumed when it changed something). During a drag: cancels the drag. |
| F7 | Play / pause playback. |
| Tab / Shift+Tab | Next / previous control; menu bar excluded. |

### Project tree (focus in the tree)

| Key | Action |
|---|---|
| Up / Down / Left / Right / Home / End | Standard tree navigation (End reaches the last row, e.g. a scheme). |
| Shift+Up / Shift+Down | Extend the range from the anchor. |
| Space | Toggle Enabled/Active of the selected row(s). |
| Insert, Ctrl+Space | Toggle the focused row in the selection. |
| Ctrl+A | Select all. |
| Ctrl+D | Duplicate. |
| Del | Remove. |
| Alt+Up / Alt+Down | Move up / down within the group. |
| Enter | Focus moves into the row's properties form (decision 0008). |

### Preview pane (focus anywhere in the pane except text boxes)

| Key | Action |
|---|---|
| Space | Play / pause (not in the speed box, where it is swallowed). |
| , (comma) | One frame back. |
| . (period) | One frame forward. |
| I | Set the in point at the frame shown. |
| O | Set the out point. |
| X | Clear in and out points. |
| L | Toggle loop. |
| F | Toggle Live follow (only while a run is active). |
| Home / End | Go to the in point / out point (start / end when unset). |
| Ctrl+A | Select all. |

The pane's letter keys are consumed even when their command cannot run (otherwise they would trigger menu
mnemonics). Text boxes keep their keys.

### Timeline (focused)

Left/Right one frame, PageUp/PageDown ten frames, Home/End in/out point, mouse wheel one frame.

### Handles and graph boxes on the pictures (focused)

| Key | Action |
|---|---|
| Arrows | Move by 1 canvas pixel (all selected items when the handle's item is one of several). Rotation lever: turn 1 degree (Right and Up counter-clockwise). Graph box corner grip: resize 1 px. |
| Shift+Arrows | 10 pixels / 10 degrees. |
| Del | Remove the item, or the selection it belongs to. |
| Esc | During a drag: put everything back. |
| Wheel | Zoom the pictures in or out about the pointer, never below the fit. |
| Middle drag | Pan a zoomed picture. |
| Tab | Next handle; focusing a handle selects its item. |

### Picture splitter (focused)

Arrows move it; Enter (or double-click) resets to half.

### Layout radio group (side by side / stacked)

One tab stop on the checked radio; Left/Right check the other.

### Number box (focused)

Up/Down one step, PageUp/PageDown ten steps, wheel one step (Shift+wheel ten), Enter commits, Esc reverts.
Details: [`30-properties-pane.md`](30-properties-pane.md).

### Colour swatch / picker

Space, Enter or Down open the picker; Enter keeps, Esc reverts and closes. In the picker: square arrows =
saturation (Left/Right) and value (Up/Down) by 1/100, hue strip Up/Down 1 degree, Shift x10; channel boxes
Up/Down.

### Dialogs

Message window: Enter = default button, Esc = cancel button, mnemonics without Alt. About: Esc or Enter
closes. Manual: Esc closes, F1 brings it forward.
