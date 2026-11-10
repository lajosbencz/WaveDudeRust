# Properties pane

Pointers: `Panes/PropertiesPane.xaml(.cs)`, `Views/*View.xaml`, `Controls/NumberBox.cs`,
`Controls/EditBehavior.cs`, `Controls/FormPanel.cs`, `Controls/ColorSchemePicker.cs`, `Themes/Editors.xaml`.
Figures: `Assets/properties-*.svg`; badge-by-badge text: `MANUAL.md` sections "Project settings" to
"Colour schemes" and `Assets/Legend.md`.

Labels, hints (tooltips) and enum words come from the string tables:
`Label.<Type>.<Member>`, `Hint.<Type>.<Member>`, `Enum.<Type>.<Member>`, in all four languages. The hint
column below is a summary; the full text is the `Hint.*` entry (identical to the manual's badge text).

## Pane behaviour

- Shows the form for the type of the current item (`SelectedItem`); "No selection" (grey) when none.
- A folder row shows its label as a section header and the count ("No entries" / "1 entry" / "N entries").
- When a form is built for a newly added item, the caret goes into its first text box with the text selected.
- Each form scrolls vertically; never horizontally.
- Layout: `FormPanel`, two columns: labels left (column as wide as the widest label, a minimum width), editors
  right filling the rest; a check box or block marked full-width spans both columns. Section headers: name over
  a rule across the width; the gap comes after a group's rows.
- Read-outs (computed values) are plain text, bound to every input they depend on. Estimates carry a "~ "
  prefix and one decimal ("~ 32.0 px": wavelengths, the quarter-wave coating, the outermost zone); other pixel
  and angle read-outs show at most one decimal ("12 px", "12.5 px").
- Edits apply live: every keystroke in a name box, every number step, every check box. Number text is applied
  on Enter or focus loss (see Number box). Each edit marks the project dirty and follows the live editing rules
  (`60-live-editing-and-runs.md`).

## Editors

### Text box

Enter pushes the text into the property at once and selects the text; Esc restores the property's value when
the text differs. Both keys are consumed only when they did something (so Esc from an unchanged box reaches
Stop). Multiline boxes keep Enter as newline. Name boxes update per keystroke (tree row renames live).

### Number box (`NumberBox`)

Text box plus a column of two small repeat buttons (up, down) on the right.

| Input | Linear box | Logarithmic box (brightness) |
|---|---|---|
| Up / Down | +/- Increment | x / / 1.02 (two percent) |
| PageUp / PageDown | +/- 10 increments | 10 steps (about a fifth) |
| Wheel (while focused) | 1 step per notch; Shift: 10 | same |
| Repeat buttons | 1 step, auto-repeat; also focus the box | same |
| Horizontal drag on the text | value follows the mouse: DragStep per pixel (default one increment per N pixels); Shift x0.1, Ctrl x10 | a doubling per fixed number of pixels, Shift finer, Ctrl coarser |
| Rounding | to Decimals (default: the increment's decimals; integer boxes never get fractions) | three significant digits |

- Every step clamps to Minimum..Maximum, writes the text into the binding at once and selects it (a source
  moves on the picture with each key press).
- Typing is free text; the model converts and clamps (typed text is not limited by the box). Parsing and
  writing use the current culture (decimal comma in DE/FR/NL).
- Drag: press on an unfocused box, move past the drag threshold, then scrub. A press without movement is a
  normal click that places the caret. A press on an already focused box selects text (no scrub). A drag reaching
  the monitor's left/right edge wraps the pointer to the opposite edge and continues. Esc during a drag restores
  the start value.
- A non-parsing text counts as 0 for a step. A logarithmic box starts from Minimum (positive) if the value
  is not above zero.
- Disabled boxes grey out.
- A row disabled by another setting greys its label and disables its editor: a number box, check box or
  combo box takes neither the pointer nor the focus then.

### Check box, combo box

- Enum combos show localised words (`Enum.*`); the words follow a language switch.
- A combo box fills the editor column, never narrower than its widest word.
- Full-width check boxes carry their label as content.

### Colour scheme picker (combo)

Rows: the project's own schemes first (tag "project", grey), then the shipped and user schemes of the layer's
kind (Field or Intensity); each row a gradient strip and the name (long names trimmed). A name the layer refers
to that no scheme has: greyed "<name> (not found)". Two italic action rows at the end:
- "Load from file..." (`.wdscheme` / `.json`): adds the scheme to the project and selects it. A sampled table
  with many stops is slimmed to its bends.
- "Edit a copy in the project...": copies the current scheme into the project as editable stops, selects the
  copy on the layer and opens it in the tree.

### Colour swatch button (`ColorPickerButton`)

Swatch that opens the picker in a popup below it (`70-dialogs-and-windows.md`, Colour picker).

### Link toggle (chain)

Toggle between two number boxes (rectangle/ellipse Length and Width) showing two linked rings, parted when off;
focus shows as the accent on its underline.

### Description header (parts)

Toggle with a chevron at the left (points right closed, down open) and the label "Description"; Space toggles.
The text under it is the kind description (`Kind.<Type>`). Open/closed is one user setting for all kinds
(`PartDescriptionExpanded`, default open).

## Forms

Column "Ctrl" = control: NB number box (min..max, step), CB check box, CMB combo, TB text box, RO read-out,
BTN button. Unlisted min/max = open.

### Project

Section General.

| Label | Ctrl | Notes |
|---|---|---|
| Format version | RO | |
| Random seed | NB (integer) + BTN "New seed" (N) at the right | New seed draws a fresh random seed. |
| Description | TB, multiline, wraps | Free text; an example's description is its menu tooltip. |

### Scene

Sections: Canvas, Sources from the image.

| Label | Ctrl | Enabled / visible | Notes |
|---|---|---|---|
| Mode | CMB Canvas, Image | | |
| Scene image | TB (the image's name, applied per keystroke) + BTN "..." + BTN save glyph + BTN X | image rows hidden in Canvas mode | "..." picks an image (PNG, BMP, JPG/JPEG, TIF/TIFF, GIF; All files) and embeds it, switching to Image mode. Save glyph writes the embedded bytes to a file (proposed under the image's name in its folder if that exists, else the project's; filter by the bytes' type); changes nothing in the project. X drops name, path and bytes; the canvas keeps the image's size as a blank canvas. Tooltips: "Pick an image file; it is embedded in the project", "Save the embedded image to a file", "Drop the embedded image and its file path; the canvas keeps the image's size". |
| Image file | RO path, trimmed with ellipsis, plus "(not found)" when the file is missing (a relative path is resolved against the project's folder) | hidden in Canvas mode | |
| Embedded image | RO "W x H, N KB" (or "(unreadable)") | hidden in Canvas mode | |
| Width | NB 8..16384 | disabled in Image mode | |
| Height | NB 8..16384 | disabled in Image mode | |
| Edges | CMB Open, Reflective | | |
| Damping edge | NB -8192..8192 | | negative = rim outside the canvas |
| Refractive index | NB 1..2.55 step 0.01 | | blank canvas |
| Image sources | CB full width | | |
| Image source amplitude | NB 0..1000 step 0.1 | enabled when Image sources | |
| Object sources | CB full width | | |

### Simulation

Sections: Run, Solver.

| Label | Ctrl |
|---|---|
| Steps | NB 1..10,000,000 step 100 |
| Visualize every | NB 1..1000 |
| 4th-order Laplacian | CB full width |
| Wave speed | NB 0.000001..1,000,000 step 0.01 |
| Intensity time decay | NB 1..100, drag step 0.1, 1 decimal |
| Source frequency scale | NB 0..1000 step 0.1 |
| Intensity scale | NB 1e-7..1e6 step 0.1 |

### Display

Sections: Field, Intensity (each the layer sub-form), Preview, Export (range), Video export.

Layer sub-form (Field, then Intensity):

| Label | Ctrl |
|---|---|
| Colour scheme | colour scheme picker (layer's kind); each scheme shows its gradient swatch (56 x 12) before its name, in the closed picker and in its list |
| Brightness | NB logarithmic 0.0001..100000 (same value as the picture header's box) |
| Black level | NB -10..1 step 0.01 |
| Preview | CB (show this picture; same as the transport bar's show toggle) |
| Outline in the preview | CB |
| Outline brightness | NB 0..5 step 0.05 |
| Outline in the export | CB |

Rest:

| Section | Label | Ctrl |
|---|---|---|
| Preview | Handles in the preview | CB |
| Preview | Labels in the preview | CB |
| Export | From step | NB min 0 (the timeline's in point) |
| Export | To step | NB min 0, 0 = end (the out point) |
| Export | Probe graphs in the export | CB |
| Export | Probe markers in the export | CB |
| Export | Source markers in the export | CB |
| Video export | Frame rate | NB 1..240 (also the playback rate at speed 1) |
| Video export | Quality | NB 1..100 |

Order on screen: Field, Intensity, Preview (Handles, Labels), Export (From step, To step, Probe graphs,
Probe markers, Source markers), Video export (Frame rate, Quality).

### Source (point and composite share one form)

Sections: General, Position, Wave, Timing, Array, Random offsets added per emitter.

| Label | Ctrl | Enabled |
|---|---|---|
| Name | TB | |
| Enabled | CB | |
| X | NB min 0 | |
| Y | NB min 0 | |
| Frequency | NB 0.01..255, drag 0.1, 2 decimals | |
| Amplitude | NB 0..100, drag 0.1, 2 decimals | |
| Phase | NB -180..180, drag 0.5, 2 decimals | |
| Wavelength | RO "~ N.N px", "(nominal)" appended with the 9-point stencil; tooltip explains | |
| Mode | CMB Continuous, Pulsed | |
| Pulse shape | CMB Gaussian, Smoothed square | disabled when Continuous |
| Pulse width | NB min 1 | disabled when Continuous |
| Off width | NB min 1 | disabled when Continuous |
| Detach after step | NB min -1 | |
| Composite source | CB | |
| Columns | NB min 1 | array rows enabled only when Composite |
| Rows | NB min 1 | |
| Horizontal spacing | NB | |
| Vertical spacing | NB | |
| Rotation | NB -180..180 step 5, drag 0.5, 1 decimal | |
| Frequency (random) | NB min 0 | |
| Phase (random) | NB min 0 | |
| Off width (random) | NB min 0 | |
| Position (random) | NB min 0 | |

### Settings every part shares (`PartCommonView`, badges 1..6)

| Label | Ctrl |
|---|---|
| (Description header) | toggle + kind text |
| Name | TB (section General) |
| Enabled | CB |
| X | NB min 0 (section Position) |
| Y | NB min 0 |
| Angle | NB -180..180 step 5, drag 0.5, 1 decimal |

### Glass and mirror material (`ShapeMaterialView`, lens, polygon, rectangle, ellipse, triangle, outline; badges 7..15)

Section Material.

| Label | Ctrl | Enabled |
|---|---|---|
| Mirror | CB | |
| Refractive index | NB 1..2.55 step 0.01 | disabled when Mirror |
| AR coating | NB 0..50 | disabled when Mirror |
| Coating profile | CMB Layer, Graded | disabled when Mirror |
| Coat the front | CB | disabled when Mirror |
| Coat the back | CB | disabled when Mirror |
| Coat the sides | CB | disabled when Mirror |
| Absorption | NB 0..255 step 5 | disabled when Mirror |
| Absorption ramp | NB 1..400 step 5 | disabled when Mirror |

Lens only: ticking Mirror renames a part called "Lens" to "Mirror" and back (only via the box, never on load
or undo).

### Kind-specific forms (after the shared rows)

| Kind | Section | Rows |
|---|---|---|
| Lens | Shape | Diameter NB min 2 step 10; Concave (diverging) CB (flips the sign of both radii); R1 NB step 10; R2 NB step 10; Centre thickness NB min 1; Edge thickness RO |
| Fresnel lens | Shape | the lens's rows without the Concave box; Zones NB 1..100; Zone division CMB (Equal width, Equal depth); Edge thickness RO reckons with the deepest zone |
| Polygon | Shape | Number of sides NB 3..64; Side length NB min 2 step 10 |
| Rectangle | Shape | Length NB min 1 step 10; Width NB min 1 step 10; link toggle "Linked" (square) right of both boxes, spanning the two rows |
| Ellipse | Shape | Length, Width as rectangle; Linked (circle) |
| Triangle | Shape | Type CMB (Equilateral, Right isosceles (45 deg), 30-60-90, Isosceles, Custom); Base NB min 2 step 10; Angle A NB 1..178 drag 0.5 1 decimal, enabled for Isosceles and Custom; Angle B same, enabled for Custom only; Apex angle RO |
| Outline | Points | Hint text (grey, wraps); a heading row (X, Y) over full-width rows, one per point: X NB, Y NB sharing the width, remove BTN (in Tab cycle); BTN "Add point" (appends after the last) |
| Diffuser | Shape; Roughness | Length NB min 1 step 10; Width NB min 2 step 10; Centre wavelength NB 2..10000 2 decimals; Amplitude NB 0..10000; Sigma NB 0..10000; Profile height RO; Ripples RO |
| Interference filter | Shape; Layers | the material's coating rows with the back box alone; Length NB min 1 step 10; Width NB min 1 step 10; Stack thickness RO; hint text; a heading row (Thickness, Index) over full-width rows, one per layer: ordinal, Thickness NB 0..1000 2 decimals, Index NB 1..2.55, up BTN, down BTN, remove BTN; BTN "Add layer" (a copy of the last), BTN "Remove all" |
| Simple 2D grating | Material; Lattice | Mirror, Refractive index, Absorption, Absorption ramp without coating rows; Columns NB min 1; Rows NB min 1; Column spacing NB min 2; Row spacing NB min 2; Dot diameter NB 1 to half the smaller spacing |
| Meta lens / prism / axicon | Shape; Lens or Prism or Axicon; Pattern | the material's coating rows without the sides box; Length NB min 1 step 10; Width NB min 2 step 10; Target frequency NB 1..255 2 decimals; Wavelength RO; the lens: Focal length NB min 1 step 10, Lens type CMB (Converging, Diverging), Zones RO; the prism: Deflection NB -89..89 2 decimals, Zone width RO, Zones RO; the axicon: Cone angle NB 0.1..89 2 decimals, Line of focus RO, Zone width RO, Zones RO; Pattern index NB 1..2.55; Pitch NB 1..10000 2 decimals; Height NB 1..10000 2 decimals; Full-turn height RO |
| Facetted grating | Shape; Facets | the material's coating rows without the front and sides boxes; Length, Width as the diffuser's; Pattern CMB (Sawtooth, Square wave, Sinusoidal); Pitch NB 2..10000 2 decimals; Amplitude NB 0..10000 |
| Fibre | Core | Core index NB 1..2.55 step 0.01; Core diameter NB min 2 |
| Fibre | Cladding | Cladding index NB 1..2.55; Cladding thickness NB min 0; Cladding absorption NB 0..255 step 5; Absorption band NB 1..400; AR coating NB 0..50; Coating profile CMB; Coat the front/back/sides CB |
| Fibre | Shape | Ends CMB Flat, Round |
| Fibre | Nodes | Hint text; per node: "Node" X NB, Y NB, Smooth CB, remove BTN; "In" InX NB, InY NB; "Out" OutX NB, OutY NB; BTN "Add node" (appends beyond the last, continuing the path). Smooth on lines the handles up at once; then moving one turns the other keeping its length. |
| Bragg reflector | Layers | Layer pairs NB 1..200; Thickness 1 NB min 1; Index 1 NB 1..2.55; Thickness 2; Index 2 |
| Bragg reflector | Shape | Width NB min 2 step 10 |
| GRIN lens | Material | Centre index, Edge index NB 1..2.55; AR coating NB 0..50; Coating profile; Coat front/back/sides |
| GRIN lens | Shape | Length NB min 1 step 10; Diameter NB min 2 step 10 |
| Zone plate | Design | Target frequency NB 1..255 drag 0.1 2 decimals; Wavelength RO; Focal length NB min 1 step 10; Inverse CB |
| Zone plate | Shape | Diameter NB min 2 step 10; Thickness NB min 1; Anti-reflection NB 0..400; Mount CB; Mount width NB min 1 step 10, enabled when Mount |
| Zone plate | Zones | Number of zones RO (the zone index at the rim plus one); Outermost zone RO |
| XOR / AND / OR gate | Design | Design frequency NB 1..255 drag 0.1 2 decimals; Wavelength RO |
| gate | Material | Core index, Cladding index NB 1..2.55; AR coating NB 0..50; AR on input side CB; AR on output side CB; Quarter-wave coating RO, lambda / (4 sqrt(core index)) |
| gate | Shape | Port spacing NB 2..2000 step 2 (shows the spacing the design uses, a too-small value raised; an edit stores the typed value) |
| gate | Outputs active | Upper output, Lower output: label, then a CB whose content is the output's function word for this gate kind (`Enum.GateOutput.*`: "XOR: dark with both on", "AND: 4 x as bright with both on", "OR: as bright for one as for two", "Remainder: 3 x with both on"). Upper / lower: XOR gate Xor / And, AND gate And / Xor, OR gate Or / Remainder. |
| gate | Layout | Core width, Coupler gap, Coupler length, Total length: RO |
| gate | Ports | Input A, Input B, Upper output, Lower output: RO canvas coordinates "(x, y)" |

The three gate kinds share one form. Read-outs show units (px, deg) as in the figures, formatted as under
Pane behaviour.

### Probe

Sections: General (Name, Active, Start after step), Measurement (Detector to Kind), Position, Graph, Axes.

| Label | Ctrl |
|---|---|
| Name | TB |
| Active | CB |
| Start after step | NB min 0 |
| Detector | CMB Standard, Counting, Imaging |
| Detector sensitivity | NB 0..1000000 2 decimals drag 0.5; enabled for a detector |
| Bin size | NB 1..20; enabled for an imaging detector |
| Layer | CMB Field, Intensity; disabled for a detector (Intensity) |
| Measurement | CMB, items by layer: Field -> Amplitude; Intensity -> Value, Integrated value; disabled for a detector (Integrated value) |
| Kind | CMB Point, Line; disabled for a detector (Line) |
| A x, A y | NB min 0 |
| B x, B y | NB min 0 (line probes only; disabled for a point probe, decision 0008) |
| Box size | CMB Tiny, Small, Medium, Large, Extra large, Custom |
| Box x, Box y | NB min 0 |
| Box width | NB 60..4096 step 10 (custom only per hint) |
| Box height | NB 30..4096 step 10 (custom only per hint) |
| Step axis | CMB Running, Full; enabled for a point probe |
| Window | NB 10..1,000,000 step 10 |
| Auto-scale steps | CB; enabled for a point probe |
| Auto-scale values | CB |
| Value axis | CMB Linear, Logarithmic; enabled on the intensity layer |
| Decades | NB 1..12; enabled for a logarithmic axis |
| Value minimum | NB step 0.1 |
| Value maximum | NB step 0.1 |

### Colour scheme (project scheme)

Sections: General, Stops.

| Label | Ctrl | Notes |
|---|---|---|
| Name | TB | Renaming repoints every layer that used the old name (pictures keep their colours). Tree row renames live. |
| Kind | CMB Unspecified, Field, Intensity | pickers list it for that kind only |
| Description | TB wraps, no newline | |
| Based on | CMB with gradient + name rows (shared schemes and the project's other schemes); first row of Stops | Picking copies stops, kind, category, description into this scheme; the choice stays shown until the next pick. |
| (gradient) | the scheme's gradient bar with its stop markers, above the stop rows |
| Stop rows | Position NB 0..1 step 0.01, up to 3 decimals (64 px wide); colour swatch button; hex TB "#RRGGBB" (Enter applies); remove BTN | |
| Add stop | BTN | Inserts a stop in the widest gap with the gradient's colour there. |

Loading a sampled table (many stops) slims it to its bends (picker "Load from file..."); the scheme form has
no separate reduce button.

## Automation ids

`SourceWavelength`, `CollectionCount`; per-row ids in the views (grep `AutomationId` in `Views/`).
