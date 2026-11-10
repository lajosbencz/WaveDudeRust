# Project pane (tree)

Pointers: `Panes/ProjectTree.xaml(.cs)`, `MainWindow.Project.cs`, `MainWindow.Clipboard.cs`,
`Controls/InsertionAdorner.cs`, `Converters/LensIconConverter.cs`, figure `Assets/project-tree.svg`.

## Layout

```
[ Project pane title bar: "Project"            v  x ]
[ +Source  +Probe  +Part  |  ^  v  -              ]   toolbar
[ tree ...                                         ]
```

### Toolbar (all tab stops, left to right)

| Button | Look | Tooltip | Action |
|---|---|---|---|
| Source | source icon + plus badge + "Source" | Add a source (appended at the end) | Add source. |
| Probe | probe icon + plus badge + "Probe" | Add a probe (appended at the end) | Add probe. |
| Part | part icon + plus badge + "Part" | Add an optical part (a menu of kinds; appended at the end) | Opens the Add part menu under the button. |
| Move up | chevron up glyph | Move up (Alt+Up); drag to reorder | Disabled (and skipped by Tab) when not applicable. |
| Move down | chevron down glyph | Move down (Alt+Down); drag to reorder | Same. |
| Remove | minus glyph | Remove (Del) | Same. |

Buttons are flat; hover shows a background. The plus badge is a 5 px plus in the icon's bottom-right corner,
accent colour (text colour when pressed or disabled). Tab order: Source > Probe > Part > (enabled move/remove)
> tree.

## Tree structure

All branches expanded by default; a chevron opens/closes a group.

```
Project                         (root; icon Draw:Project)
  Scene                         (icon U+EB9F)
  Simulation                    (icon U+E916)
  Display                       (icon U+E7F4)
  Sources            [+]        (folder; icon Draw:Source)
    [x] (src icon) Source 1   ^ v -
  Parts              [+]        (folder; icon Draw:Part; its + opens the Add part menu)
    [x] (kind icon) Lens 1    ^ v -
  Probes             [+]        (folder; icon Draw:Probe)
    [x] (probe icon) Probe 1  ^ v -
  Colour schemes     [+]        (folder; icon U+E790)
    (gradient) My scheme      ^ v -
```

Rows are 23 px. Folder labels and the fixed row names are localised (Project, Scene, Simulation, Display,
Sources, Parts, Probes, Colour schemes); item rows show the item's Name.

### Item row

Left to right: Enabled check box (sources, parts; Active for probes; none for schemes), kind icon, name,
then right-aligned glyph buttons Move up, Move down, Remove (18 x 18 px).
- Row check box and row buttons are not focusable and not tab stops (toolbar, keys and context menu cover
  them). Tooltips: "Include the source in runs (Space on the row toggles it)", "Paint the part into the scene
  (Space on the row toggles it)", "Sample this probe (Space on the row toggles it)", Move up/down, Remove.
- A disabled (unchecked) item greys its icon and name.
- Colour scheme row: a 36 x 10 px gradient strip of the scheme (thin border), then its name.
- Folder row: icon, label, right-aligned plus button (tooltip "Add an item to this folder"); Parts folder's plus
  opens the Add part menu.

### Row icons

| Item | Icon |
|---|---|
| Source | the source handle glyph (ring with crosshair ticks) as 16 px icon |
| Probe | the probe handle glyph (square) |
| Part (non-lens) | the kind's drawing: FresnelLens (a plate with sawtooth faces), Fiber (S-bent fibre), BraggReflector (alternating layers), GrinLens (block with lines thickening toward the axis), ZonePlate (plate open in the middle, blocks narrowing outward), Polygon (hexagon), Rectangle, Ellipse, Triangle, Outline (free outline), XorGate/AndGate/OrGate (ANSI symbols) |
| Lens part | its own profile, see below |
| Colour scheme | gradient strip |

Lens profile icon rule: nine drawings `IconLens<Left><Right>`, Left/Right each in {Convex, Flat, Concave}.
Each surface is classed by its sag at the rim, from R1 (front), R2 (back) and the diameter: flat when the sag
is under half a pixel (radius 0 = flat; a huge radius on a small diameter is flat), else convex or concave by the
radius sign convention (R1 > 0 bulges outward, R2 < 0 bulges outward). Follows every edit of R1, R2,
diameter. The Add part menu's Lens/Mirror entry uses a separate fixed icon (biconvex lens beside a concave
mirror).

## Selection model

Shared by tree, preview handles and commands: a `SelectedItems` list plus one current item (`SelectedItem`),
always a member of the list. Properties shows the current item.

| Input | Effect |
|---|---|
| Click, arrows | That row alone (anchor = it). |
| Ctrl+click | Toggle the row in the selection; it becomes current. |
| Shift+click, Shift+arrows | Range from the anchor (last plain selection) to the row, in tree order (sources, parts, probes as one sequence). |
| Insert, Ctrl+Space | Toggle the focused row; when the current item is taken out, the current moves to the last one left. |
| Ctrl+A | Every source, part and probe; the last becomes current. |
| Selection made on the pictures | Mirrored in the tree; the tree row does not take focus (the handle keeps it, so Tab continues through handles). |
| Click on empty preview space | Deselects; the tree lets go too. |

Highlight: current row = selection colour (inactive-selection colour when the tree lacks focus, standard
behaviour); other selected rows = inactive-selection colours. Groups, the root and settings rows can be
current; sources, parts, probes and colour schemes join a multi-selection (schemes for cut, copy, delete,
duplicate; a group move ignores them; decision 0008).

## Commands on the selection

Explorer rule: a command whose target row is one of several selected items acts on the whole selection, else
on the target alone.

| Command | Behaviour |
|---|---|
| Add source / probe / part | Appended at the end of its list, placed at the centre of the canvas the preview shows (image scene: its own size). A new probe's line end B lies 100 px to the right of A. The new item becomes the selection and the caret goes into its first editor (Name, text selected). |
| Add on a folder plus | Adds to that folder (schemes folder: a new project colour scheme with the model's default stops). |
| Enabled (Space) | Each selected item flips on its own (not set-all). |
| Duplicate (Ctrl+D) | A copy after each original; copies become the selection, the last one current with the caret in its name. Copy named "<name> copy". |
| Remove (Del) | Removes target or selection; the user stays at the slot of the last removed: the item that took the slot is selected, else the folder. |
| Move up / down (Alt+Up/Down) | Within the item's own list; disabled at the ends. Selection kept. |
| Cut / Copy / Paste | See below. |
| Export probe data as CSV | Probe rows, active probes only. |

## Drag and drop

- A drag starts from a row (not from its buttons) after the system threshold.
- The drag carries the item as Copy writes it (app clipboard format + plain XML text), nothing else.
- Drop target decides:
  - Same tree: reorders within the item's own list. On a sibling row: before (upper half) or after (lower
    half), shown as a line along the row's top or bottom edge. On its own folder: append, shown as an outline
    around the folder row. A position that moves nothing is not a target.
  - Another window's tree: paste there (before/after a row by half, at the end of a folder's list, or onto
    Scene/Simulation/Display/root as a settings paste).
  - A preview picture (own window or another): pasted at the end of the lists, then moved as a group so the
    first object sits on the canvas pixel under the drop, the group held inside the canvas; dropped objects
    become the selection; one undo step. From the own tree this duplicates.
  - A text editor: receives the XML.
- The tree and the pictures claim a drag only when they take it. Anything else, a file above all, goes to the
  window: a dropped project file (`.wdproj`, `configuration.json`, `.wdscheme`) opens.
- Settings rows (Scene, Simulation, Display) can be dragged (carry their XML); they do not reorder.

## Clipboard (from the tree's point of view)

- Copy: selected sources, parts, probes, schemes, or the Scene/Simulation/Display object, as a fragment of
  their project-file elements; written in the app's format and as plain text.
- Paste: reads app format or plain text (so XML typed in an editor pastes). List items go behind the current
  item (before it for a drop on a row's upper half); renamed only on a name clash; composite emitters re-derived
  from the target project's seed. A settings object replaces only when pasted on its own row or the project root.
  Pasted objects become the selection. One undo step.
- Cut = Copy + Remove, list items only.
- "Can paste" is re-evaluated once per clipboard change.
- A focused text box handles Cut/Copy/Paste for its text first.

## Properties pane coupling

Selecting a folder shows its label and the item count ("No entries", "1 entry", "N entries"). Selecting the
root shows the Project form; settings rows show their forms (`30-properties-pane.md`).

## Automation ids (keep as test ids)

Toolbar: `ToolAddSource`, `ToolAddProbe`, `ToolAddPart`, `ToolMoveUp`, `ToolMoveDown`, `ToolRemove`.
Rows: `Tree:<name>`; check boxes `Enabled:<name>`; selection marker status `InSelection`.
Menu: `AddPart:<Kind>` (Lens, FresnelLens, Fiber, BraggReflector, GrinLens, ZonePlate, Polygon, Rectangle, Ellipse,
Triangle, Outline, OpticalLogic, XorGate, AndGate, OrGate), `ContextToggleSelected`, `ContextSelectAll`,
`ContextExportProbeCsv`. Folder count: `CollectionCount`.
