# Preview pane (centre area)

Pointers: `Panes/PreviewPane.xaml(.cs)`, `Controls/SceneOverlay.cs`, `Controls/ProbeGraphOverlay.cs`,
`Controls/ProbeGraphDrawing.cs`, `Controls/MarkerThumb.cs`, `Controls/HeaderStrip.cs`,
`Controls/PictureFrame.cs`, `Controls/CanvasImage.cs`, `Rendering/FramePresenter.cs`,
`MainWindow.Display.cs` (Auto brightness), figure `Assets/preview.svg`.

## Layout

```
+------------------------------------------------------------+
| [Field header strip .................. Brightness [NB][Auto]]|
| +--------------------------------------------------------+ |
| | field picture + outline + handles + graph boxes        | |
| +--------------------------------------------------------+ |
| ============== splitter (4 px) ===========================  |
| [Intensity header strip ............. Brightness [NB][Auto]]|
| | intensity picture ...                                  | |
+------------------------------------------------------------+
| timeline (full width)                                      |
| transport bar                                              |
+------------------------------------------------------------+
```

- Two picture cells: Field and Intensity. Split: side by side (columns) or stacked (rows, field above).
  User setting `PreviewOrientation`, default stacked (`Vertical`). Toggled by the transport bar's two radios.
- Splitter between the cells: a tab stop; drag or arrow keys (10 px per key) resize; double-click or Enter
  resets to half. Tooltip "Drag or use the arrow keys to resize; double-click or Enter resets to half".
- A hidden picture (Display layer's Preview off, or the transport toggle) gives all its room to the other; the
  splitter collapses; showing it again restores the previous split.
- Each picture is fitted uniformly (aspect kept) into its cell, centred, with a 4 px margin; the cell is sized to
  the picture so the edge line hugs it.
- Without a run or frame: black at the canvas size; an image scene shows the scene image itself.

## Header strip

One strip per picture: title ("Field" / "Intensity") at the start; at the far end the caption "Brightness",
the brightness number box (64 px, logarithmic, 0.0001..100000, same value as Display > layer > Brightness;
tooltip = the brightness hint) and an "Auto" button.
- Placement: above the picture when the cell is taller than the picture's aspect (spare room above/below);
  rotated 90 degrees on the picture's left when the cell is wider (spare room at the sides), ending at the
  picture's bottom edge.
- When short: the caption is dropped first; then the title is trimmed (ellipsis) to what the tools leave.
- Auto: one-shot fit of the frame shown: the 99.5th percentile of |field| (or sqrt(intensity)) maps to the top
  of the colour scheme. The brightness then stays. With 8-bit samples the frame is computed once more in 16-bit
  (run paused meanwhile) and fitted when it arrives. The fit is an ordinary brightness edit (8-bit: a
  simulation edit). Nothing to fit: says so (status bar).
- Brightness box keys: Up/Down two percent, PageUp/PageDown a fifth; three significant digits.

## Edge line

1 px line around each picture in the colour of what a wave meets at the canvas edge:

| Scene | Colour |
|---|---|
| damping rim outside the canvas (negative damping edge) | pure blue `#0000FF` (wins over a wall) |
| reflective edges, no outside rim | pure red `#FF0000` |
| open edges, or a damping band inside the canvas | none (transparent) |

Fixed colours, not themed; preview only, never exported.

## Zoom

Both pictures share one zoom over the fit (`View` in `wave-widgets/src/picture.rs`, `PaneStates.view`): the
wheel over a picture zooms in by a factor of 1.25 a notch, out by the same, never below the fit, up to 64x; the
canvas point under the pointer stays where it is. A middle-button drag pans a zoomed view. The zoomed picture is
larger than the cell's area and is held so that no area is left empty; the handles, the graph boxes and the
marquee follow the transform. A zoomed 16-bit picture is rendered for its visible part alone (a source window of
the screen renderer, `LayerRequest.window`), at the area's device size, so the cost does not grow with the zoom;
an 8-bit picture is the canvas-sized bitmap scaled. The zoom is session state, not persisted; opening another
project keeps it.

## What a picture shows

- A picture changes only when a frame for the current position arrives (scratch frame for the position, else
  the run's stored frame). Otherwise the last picture stays; never black, never another frame, never a replay.
- While a frame for the position is computed for longer than 150 ms, both pictures dim (black at 45 %) under a
  spinner (three-quarter ring turning once a second); not hit-testable. Mirrored on the taskbar button
  (indeterminate) and the status bar ("Computing frame n", indeterminate bar).
- Overlays in z-order: picture, scene outline (translucent glowing-edges layer, when enabled for the layer),
  handle overlay (lines, emitter dots, part outlines, labels, handles), probe graph boxes, busy dim.

### 8-bit vs 16-bit (user's view)

- 8-bit samples (View > Preview samples, default): stored palette indices at the store's resolution, scaled to
  the picture. Brightness is baked in when the run starts; a brightness change is a simulation edit (stale run,
  re-run). Dithering unavailable.
- 16-bit samples: stored half floats; the picture is rendered at its exact size in device pixels (value-space
  resampling: box mean when shrinking, bilinear when enlarging, then the colour table), drawn 1:1. Brightness
  changes recolour instantly without a new run; every gradient level shows.
- Dithering (16-bit only, View menu): colours at 10/12/14/16 bits per channel reduced to 8 through a Bayer
  matrix (2/4/8/16 cells a side), fixed to screen pixels; preview only.

## Handles and overlays

Drawn over the pictures in canvas pixels, clipped to the picture. Colours: light grey (`PictureHandle`) over a
dark halo; the selected item's handle bright yellow (`PictureSelection`); the keyboard-focused handle the accent
fill; hover and focus thicken the stroke. Glyphs are odd-sized (21, 15, 13 px) with the point on the centre
pixel; strokes 1.5 px antialiased.

| Handle | Glyph | On | Shown |
|---|---|---|---|
| Source position | ring with four crosshair ticks outside, gap at the pixel (21 px) | both pictures | always (subject to Handles setting) |
| Composite emitters | dots (not focusable) | both | always |
| Source rotation lever | handle on the array's row axis, a lever line from the centre | both | current item only |
| Part centre | ring with inner ring + ticks (21 px) | both | always |
| Part outline | the painted outline in the selection colour | both | selected parts |
| Part rotation lever | on the part's axis beyond its reach | both | current item only |
| Part size grip | small filled square (15 px) on a corner/vertex | both | current item only |
| Lens surface handles | one per surface vertex | both | current item only |
| Outline point handles | small ring per point | both | current item only |
| Fibre node handles + Bezier in/out handles | node ring; handle = small filled dot (13 px) at the end of a dotted line from the node | both | current item only (first node's in-handle and last node's out-handle omitted) |
| Probe point A / end B | square with ticks; line probe: dark line under a dashed light line between A and B | only the picture of the probe's layer | always |
| Probe graph box | see below | probe's layer | active probes |
| Label | item name beside the main handle (below it for composites and parts), dark halo under the text, kept inside the picture | | subject to Labels setting |

Display settings (project, Display > Preview):
- Handles in the preview (default on): off hides every item's handles, lines, dots and levers, except the
  selected item's. Hidden handles leave the Tab cycle.
- Labels in the preview (default on): off hides names except the selected item's.
- Graph boxes are unaffected by both.

Z-order rules: graph boxes lie over every handle and dot (a handle under a box is reached by Tab or the tree);
the selected item is drawn last among its kind; handles themselves keep their order (no z-index games that
would break the Tab cycle). A rotation lever may extend beyond the picture and is clipped; other handles sit on
positions clamped inside the canvas.

### Interaction

| Input | Effect |
|---|---|
| Click on a handle | Its item becomes the selection (unless it is one of several selected: then a drag moves all). Tree and Properties follow. |
| Ctrl+click on a handle | Toggle its item in the selection; no drag. |
| Shift+click on a handle | Add its item; no drag. |
| Drag a handle | Moves it; writes canvas coordinates live. Measured from the press point against the absolute pointer position (no drift when clamped). |
| Drag a handle of one of several selected items | Moves all by the same delta (whole-item handles only: source, part centre, probe A/B), limited so none leaves the canvas; the formation holds. A drag that ends where it began makes that item the selection. |
| Drag a rotation lever | Turns the array or part toward the pointer, whole degrees; Shift snaps to 15 degrees. Angle kept in (-180, 180]. |
| Drag a grip | Sets the extents from the grip's absolute position, whole pixels, never below the kind's minimum; linked rectangle/ellipse: the farther coordinate sets both; polygon vertex stays on its ray. |
| Drag a lens surface handle | Along the axis: outward bulges, inward flattens then hollows; the rim and the other vertex stay. |
| Drag a fibre node | Moves the node with its handles. Drag a Bezier handle: shapes the segment; a smooth node turns the opposite handle keeping its length. |
| Drag a probe A onto its detached graph box | Picks the box up (attaches). |
| Esc during a drag | Puts the item or group back. |
| Click on empty picture space | Deselects (unless Shift/Ctrl held); focuses the pane so Space plays. |
| Drag on empty space | Marquee (faint accent fill, dashed accent edge): selects items whose whole-item handles lie inside; Shift adds, Ctrl toggles, else replaces; last becomes current. |
| Tab | Through the handles; focusing a handle selects its item (the tree mirrors without taking focus). |
| Arrows / Shift+Arrows on a focused handle | 1 / 10 canvas pixels (group if selected with others); lever: 1 / 10 degrees, Right and Up counter-clockwise. A lens surface handle keeps its rim fixed across a run of key presses. |
| Del on a focused handle or box | Removes its item, or the selection it belongs to. |
| Right-click / Apps on the picture | Picture context menu (`10-menus-and-shortcuts.md`). |
| Drop of objects (from a tree) | Paste and place at the drop pixel (`20-project-tree.md`). |
| Drop of a file | Not claimed; the window opens it. |

Tooltips on handles (format strings `Tip*Format`): "<name>: drag to move; with keyboard focus the arrow keys
move it by a pixel, ten with Shift"; lever: "...drag to turn the array (Shift snaps to 15 degrees)..."; grip,
surface, point, node, in-handle, out-handle, graph box, graph corner have their own.

Pointer resting on a picture must cost nothing (the drawing layers are not hit-testable; with thousands of
emitter dots the source layer is cached as a bitmap).

### Probe graph box

- Translucent dark box (fixed author colours, not themed) with axes, labels, the curve and a title strip with
  the probe name and a live readout (right-aligned, own colour). Fixed label text size: the text does not scale
  with the box.
- Position/size from the probe: Box x/y in canvas pixels; size from Box size (Tiny 200 x 100 ... Extra large,
  Custom = Box width/height). A box larger than the canvas shrinks to fit, aspect kept. Mapped with the picture's
  scale; edges on whole pixels.
- Point probe: value over steps; step axis Running (sliding window of Window steps) or Full; Auto-scale steps
  lets the axis follow the recorded samples. Line probe: values along A..B at the frame shown.
- Value axis: field amplitude always linear and symmetric about zero; intensity linear or logarithmic
  (Decades below the fitted top, powers-of-ten ticks, zero and smaller on the bottom line). Auto-scale values
  fits to the data, else Value minimum/maximum.
- Data: while the picture shows a scratch frame, that frame's recording; else the run's recording as it stood at
  the frame shown (series cut at that step). A stale run's data is shown while the stored frame is shown (no
  empty-axis flicker on scrub).
- Selected probe's box: accent (selection) border; drawn last.
- Drag the box to move (kept inside the canvas). Snap: a box whose anchor comes within a radius (screen px)
  of the probe's point A lands on it and is attached (then follows the probe); it detaches only when dragged
  beyond a larger release distance (hysteresis).
- Bottom-right corner grip: drag resizes, snapping to the fixed sizes, else Custom. Arrows on the grip
  resize by 1 px (Shift 10), making it Custom.
- Arrows on the box move 1 px (Shift 10); Tab reaches boxes; focusing a box selects its probe.
- Moving a box or a probe never marks the run stale (no new run); the graph refreshes from a scratch frame.
- Context menu: Export probe data as CSV...
- Hover and focus show a border on the box.

## Busy, stale and taskbar

- Busy dim + spinner: see above (150 ms delay so fast machines do not strobe).
- Timeline shades the frames a run has produced (accent; grey when the run is stale).
- Taskbar progress (platform-specific): export determinate, waiting/scratch indeterminate, idle none.

## Automation ids

Pictures: `PreviewFieldImage`, `PreviewIntensityImage`, with `<id>Brightness`,
`<id>AutoBrightness`; `PreviewSplitter`; context menu `ContextBrightness`, `ContextAutoBrightness`,
`ContextExportFrame`, `ContextExportField`, `ContextExportIntensity`, `ContextExportBoth`; graph box menu
`ContextExportProbeCsv`.
