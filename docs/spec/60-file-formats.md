# 60 - File formats

## Project file `.wdproj` (`Model/Project.cs`, `Model/ProjectSchema.cs`)

XML 1.0, UTF-8 without BOM, `<?xml version="1.0" encoding="utf-8"?>`, tab indentation, no namespaces
(`xmlns:xsi`/`xsd` suppressed). Root `Project`. Simple values are attributes; lists are repeated child elements.
Sparse: an attribute equal to its `[DefaultValue]` is omitted (exceptions below). Numbers in invariant culture,
f64 in shortest round-trip form, bools `true`/`false`, enums by member name. This program writes the root's
`Generator="WaveDude Rust <version>"`; a file carrying it holds this program's display brightness and is not fitted
on opening (05-deviations.md, decision 0004).

### Element order

```
Project @Version @Seed @Generator?
  Description?           (element text; omitted when empty)
  Scene?                 (omitted when every property is at its default and no image)
    ImageData?           (base64 of the image file bytes)
  Simulation?            (omitted when at defaults)
  Display
    Field                (DisplayLayer)
    Intensity            (DisplayLayer)
    Video?               (omitted when at defaults)
  Source*
    Array?
  <part>*                (Lens | FresnelLens | Polygon | Rectangle | Ellipse | Triangle | Outline | Diffuser |
                          InterferenceFilter | FacettedGrating | Fiber | BraggReflector | GrinLens | XorGate |
                          AndGate | OrGate | ZonePlate, in list order)
    Point*               (Outline)
    Layer*               (InterferenceFilter; @Thickness @RefractiveIndex, sparse against 2 and 1.5)
    Node*                (Fiber)
  Probe*
  ColorScheme*
    Stop*
```

Attribute order within an element = declaration order in 10-model tables.

`Source/@X @Y` are decimals in this project (05-deviations.md): a whole number is written without a point.

Always-written values (no default omission): `Source/@X @Y`, part `@X @Y`, `Probe/@AX @AY`, `Outline/Point/@X @Y`,
`Fiber/Node/@X @Y`, `Field/@ColorScheme`, `Field/@BlackLevel` (its `[DefaultValue]` is 0 while the initial value is
-0.05, so -0.05 is written), `Intensity/@ColorScheme`.
Conditional: `Probe/@BX @BY` only for `Kind="Line"`; `Probe/@BoxWidth @BoxHeight` only for `BoxSize="Custom"`.

### DOCTYPE

`Save` writes `<!DOCTYPE Project [ ... ]>` after the XML declaration. The internal subset is generated from the
serialized model:
- one `<!ELEMENT name (children)>` per element name, children in written order with `?`/`*` as above, `EMPTY` for
  leaves, `(#PCDATA)` for text elements;
- one `<!ATTLIST name attr type presence>` line per attribute; bools as `(true|false)`, enums as `(A|B|...)`, every
  other type `CDATA`;
- no DTD defaults; `#REQUIRED` for value-type attributes that are always written, else `#IMPLIED`;
- no comments (a switch exists, off).

`Load` ignores any DOCTYPE (no validation, no entity expansion). Files without a DOCTYPE (the shipped examples) load
the same. History snapshots and clipboard fragments carry no DOCTYPE. Requirement: a saved file validates against its
own DTD; a misspelt attribute, a wrong enum value and a misplaced element are reported by a DTD validator.
Fixture: the DTD text of a saved `figures.wdproj` (F-02 in 80) is the reference; the Rust writer must emit the same
declarations (byte equality optional, `TODO(verify)` decide).

### Load and migration

```
parse (DTD ignored)
if Version < 2: Scene.Edges = Reflective
if Version < 3 and the Scene element has no DampingEdge attribute: Scene.DampingEdge = 0
if Version < 4: a Field/Intensity element without OverlayInExport = false; a ZonePlate without Thickness = 64,
                without Coating = 32 (the defaults until 2026-11-09)
if Version > 4: warning, load anyway
Version = 4
Normalize()
```
The Rust adapter reads each element onto the base its file's version means (`FileVersion` in `io/wdproj.rs`) and
writes against the current defaults, so a migrated value is written out.
A missing `Scene` element means a default Scene, then the migration above applies (a version-1 file without Scene gets
Reflective and DampingEdge 0; a version-3 file without Scene gets Open and -32).
Unknown elements and attributes are ignored. A malformed file fails with the parser's line and position; the error log
entry quotes the file's text (first 64 K characters, plus the reported line if beyond).

`ImagePath` is written relative to the project's folder when the image lies inside it, else absolute. The embedded
`ImageData` is authoritative; on load the app follows the file on disk if it exists and differs `TODO(verify)` (UI
behaviour: "the scene image is embedded and follows its file on disk").

### Annotated example

Derived from `_tools/ui_check/figures.wdproj` (one object of every kind, defaults omitted), extended with non-default
values to show every conditional rule. Comments are annotations, not part of real files.

```xml
<?xml version="1.0" encoding="utf-8"?>
<!-- Save also writes <!DOCTYPE Project [ ... ]> here; omitted for brevity -->
<Project Version="4" Seed="1810971758">
	<Description>One object of every kind.</Description>
	<!-- Scene: written because Mode/size differ from the defaults. Missing attributes take defaults:
	     DampingEdge -32 (rim of 32 px outside), Edges Open, RefractiveIndex 1, ImageSources false,
	     ImageSourceScale 1, ObjectSources true -->
	<Scene Mode="Image" Image="figures_scene.png" ImagePath="figures_scene.png" Width="800" Height="500">
		<ImageData>iVBORw0KGgo...</ImageData>   <!-- base64 PNG, 800 x 500 -->
	</Scene>
	<!-- Simulation omitted would mean: Steps 2500, VisualizeEvery 2, FourthOrderLaplacian true, WaveSpeed 1,
	     TimeDecay 10, SourceFrequencyScale 2, IntensityScale 1 -->
	<Simulation Steps="4000" VisualizeEvery="4" />
	<Display ExportFrom="400" ExportTo="2000">               <!-- in/out points in steps -->
		<Field ColorScheme="Default field" BlackLevel="-0.05" />   <!-- both always written -->
		<Intensity ColorScheme="Default intensity" Brightness="12" OverlayInExport="false" />
		<Video Fps="30" Quality="90" />                       <!-- omitted at 60 / 85 -->
	</Display>
	<!-- point source, level 20 (wavelength 80 px at scale 2, c = 1) -->
	<Source Name="Source 1" X="120" Y="160" Frequency="20" />
	<!-- composite: 1 x 7 elements, 16 px apart along y; pulsed Gaussian, 30 cycles on, 600 off -->
	<Source Name="Source 2" X="120" Y="340" Mode="Pulsed" PulseWidth="30" OffWidth="600">
		<Array SizeY="7" SpacingY="16" />
	</Source>
	<!-- parts, painted in this order; X/Y always written (centre = top-left corner of pixel X,Y) -->
	<Lens Name="Lens 1" X="300" Y="120" />                    <!-- D 200, R1 300, R2 -300, T 40, n 1.5 -->
	<Polygon Name="Polygon 1" X="420" Y="120" />              <!-- hexagon, side 60 -->
	<Rectangle Name="Rectangle 1" X="300" Y="250" Angle="30" Coating="4" CoatingProfile="Graded" />
	<Ellipse Name="Ellipse 1" X="420" Y="250" />
	<Triangle Name="Triangle 1" X="300" Y="380" />            <!-- equilateral, base 60 -->
	<Outline Name="Outline 1" X="420" Y="380">                <!-- points in the part frame -->
		<Point X="-40" Y="-30" />
		<Point X="40" Y="-25" />
		<Point X="30" Y="35" />
		<Point X="-35" Y="25" />
	</Outline>
	<Fiber Name="Fiber 1" X="560" Y="420">                    <!-- core 20 @1.5, cladding 10 @1.45 -->
		<Node X="-100" Y="0" OutX="67" />                      <!-- handles relative to the node -->
		<Node X="100" Y="0" InX="-67" />
	</Fiber>
	<BraggReflector Name="Bragg reflector 1" X="660" Y="80" /> <!-- 10 pairs, 10+10 px, 1.5/2.0 -->
	<GrinLens Name="GRIN lens 1" X="660" Y="420" />           <!-- 100 x 100, 1.8 centre, 1.5 edge -->
	<XorGate Name="XOR gate 1" X="540" Y="80" />
	<AndGate Name="AND gate 1" X="540" Y="250" Enabled="false" />  <!-- not painted -->
	<OrGate Name="OR gate 1" X="540" Y="330" />
	<ZonePlate Name="Zone plate 1" X="700" Y="250" />
	<!-- point probe; box at A (follows the probe), Medium 400 x 200 -->
	<Probe Name="Probe 1" AX="500" AY="160" BoxX="500" BoxY="160" />
	<!-- line probe: BX/BY written only for Kind="Line"; Custom box writes BoxWidth/BoxHeight -->
	<Probe Name="Probe 2" Layer="Intensity" Measurement="Value" Kind="Line" AX="200" AY="60" BX="600" BY="60"
	       BoxSize="Custom" BoxX="200" BoxY="60" BoxWidth="320" BoxHeight="120" YScale="Logarithmic" />
	<!-- project-local scheme; looked up before built-in and user schemes -->
	<ColorScheme Name="Scheme 1">
		<Stop At="0" Color="#000000" />
		<Stop At="0.5" Color="#808080" />
		<Stop At="1" Color="#FFFFFF" />
	</ColorScheme>
</Project>
```
Note: real files put each element's attributes on one line.

### Rust adapter

One attribute table per model type (`io/wdproj.rs` on `io/fields.rs`) names each attribute once with its codec, its
presence (sparse against the type's base, always, or conditional) and the model type that owns its range key
(`<owner>.<attribute>` in `model/ranges.rs`). The reader starts from the base and sets each attribute present, the
writer compares with the same base, and the DTD's `ATTLIST` lines come from the same tables (text attributes `CDATA`
and never `#REQUIRED`). A part element's attributes are the part's, then the coating's and the material's when the
kind has them, then the kind's own.

Quirks kept in the adapter, for WaveDude Next's reader:

- A coating thickness (`Coating`, the gates' too) is an integer there; the model's decimal is written rounded.
- A source position is an integer there; a source between pixels is written with its decimals and a warning,
  since rounding would move it.
- `Brightness` is written in this program's units (05-deviations.md): WaveDude Next shows such a file darker,
  and fits nothing on opening.
- `Rescaled` on `Simulation` is declared in the DTD and read (a file that already holds this program's frequency
  scale), never written: a written file carries the original's scale so WaveDude Next reads it right.

## Native project `.wdr` (Rust)

A zip container: `project.json` (deflated) holding `{"format": N, "project": {...}}`, and for a scene with an image
`scene.<ext>` (stored) with the embedded bytes verbatim (`png`, `jpg`, `bmp`, `gif`, `tif`, `webp`, else `bin`). The
JSON is sparse: every value equal to its type's default is left out, object by object (`io/sparse.rs`, decision
0004), the image path relative to the project's folder when inside it, never the image bytes. Names are the model's serde names, pinned by `rename_all` (fields `snake_case`, enum
members `PascalCase`); a part is `{name, enabled, x, y, angle, "kind": {"<Kind>": {...}}}`. A snapshot test of every
key and string value of one object of every kind (`tests/io.rs`) fails when a rename would change the format.

Load:

```
parse project.json as JSON (a syntax error names its line and column)
format > FORMAT: refused ("made by a newer version")
format 0 or none: refused (no version writes it)
migrate(project, format): one step per format from the file's to FORMAT (FORMAT = 2: format 1 left out
    overlay_in_export for off and a zone plate's thickness and coating for 64 and 32)
parts whose kind this build does not know are left out, each with a note on the load result (logged as a warning)
deserialise into the model; missing fields take their defaults
image bytes from scene.<ext>; Normalize()
```

## Colour scheme file `.wdscheme` (`Rendering/ColorScheme.cs`, `ColorSchemeStore.cs`)

```xml
<?xml version="1.0" encoding="utf-8"?>
<ColorScheme Name="Default field" Kind="Field" Description="...">
	<Stop At="0" Color="#FDFDFD" />
	...
	<Stop At="1" Color="#FFFFFF" />
</ColorScheme>
```
`Kind` omitted = Unspecified. Same element inside a `.wdproj`. User schemes in
`%LOCALAPPDATA%\WaveDudeNext\ColorSchemes`; built-ins embedded (23 files in `WaveDudeNext/ColorSchemes`).
Also importable: the original author's JSON scheme (`name`, `origin`, `category`, `scheme_type`,
`bits_per_channel` 8, `entry_count` 255, `colors`, `stops`) `TODO(verify)` mapping rules in `ColorSchemeStore.cs`.

## Legacy import: Wave Dude 2D `configuration.json` (`Model/LegacyProject.cs`)

Root must be a JSON object. `format_version` (missing = 1) > 1 is refused ("made by a newer version"). Tolerant
readers: a bool only from a JSON bool; an int from a number (rounded, even) or numeric text; a double from a number or
numeric text; missing or wrong type -> the fallback in the tables (fallbacks differ from model defaults on purpose).
Root keys: `project_name` (suggested file name), `description` -> Description; sections `scene`, `simulation`
(simulation and display keys), `object_sources` (array), `probes` (array). The project gets a fresh seed; `Normalize()`
runs at the end. Notes (non-fatal) are collected for the user.

Scene (`scene`):
| Key | Fallback | Target |
|---|---|---|
| `mode` == "image" | "user_defined" | Mode Image, else Canvas |
| `user_width`, `user_height` | 1920, 1080 | Width, Height |
| - | - | Edges = Reflective |
| `damping_edge_enabled`, `damping_edge_size_px` | false, 64 | DampingEdge = enabled ? abs(size) : 0 |
| `use_sources` | false | ImageSources |
| `image_source_scale` | 1 | ImageSourceScale |
| `use_object_sources` | false | ObjectSources |
| `background_refractive_index` | 1 | RefractiveIndex |
| `image_file` | - | relative path with `/`; embedded if the file exists (Image = file name, ImagePath = absolute), else path kept and a note logged |

Simulation and display:
| Key | Fallback | Target |
|---|---|---|
| `steps` | 2500 | Steps |
| `visualize_every` | 2 | VisualizeEvery |
| `use_4th_order_laplacian` | false | FourthOrderLaplacian |
| `wave_speed_c` | 0.563 | WaveSpeed |
| `time_decay_factor` | 10 | TimeDecay |
| `source_frequency_scale` | 2 | SourceFrequencyScale |
| `intensity_scale` | 1 | IntensityScale |
| `preview_field`, `preview_intensity` | true | Field/Intensity.Preview |
| `field_color_scheme`, `intensity_color_scheme` | - | ColorScheme (underscore names resolve) |
| `field_brightness`, `intensity_brightness` | 30 | Brightness |
| `field_black_level` | -0.05 | Field.BlackLevel |
| `intensity_black_level` | 0 | Intensity.BlackLevel |
| `video_fps` | 60 | Video.Fps |
| `video_quality` | 85 | Video.Quality |
| `use_gpu` | - | ignored |

Sources (`object_sources`):
| Key | Fallback | Target |
|---|---|---|
| `name` | "Source n" | Name |
| `x`, `y` | 0 | X, Y |
| `source_on` | true | Enabled |
| `has_position` | true | false only logs a note (the source stays enabled at x, y) |
| `detach_source`, `detach_after_steps` | false, 1000 | DetachAfter = detach ? steps : -1 |
| `frequency`, `amplitude`, `phase_deg` | 100, 100, 0 | Frequency, Amplitude, Phase |
| `mode` == "pulsed" | "continuous" | Mode Pulsed |
| `pulse_shape` == "smoothed_square" | "gaussian" | PulseShape SmoothedSquare, else Gaussian |
| `pulse_width_cycles`, `off_state_cycles` | 10, 10 | PulseWidth, OffWidth |
| `main_type` == "composite" | "single" | Array present |
| `array_size_x`, `array_size_y`, `spacing_x`, `spacing_y` | 1, 1, 10, 10 | Array sizes, spacings |
| `composite_mode` == "randomize" (fallback "inherit") and bool `randomize_<x>` | - | Random* = int `randomize_<x>_amount`, else 0; x in frequency, phase, off_width, position |
Elements are regenerated from the seed; stored element lists are not read.

Probes (`probes`):
| Key | Fallback | Target |
|---|---|---|
| - | "Probe n" (always; names are not read) | Name |
| `active` (true) and scene `use_probes` (true) | - | Active; a note when use_probes is false |
| `place_on` == "intensity" | "field" | Layer |
| `probe_type` == "line" | "point" | Kind |
| `ax`, `ay`, `bx`, `by` | 100, 100, 300, 100 | AX, AY, BX, BY |
| `box_size` tiny/small/large/extra_large | Medium | BoxSize |
| `scale_mode` == "full" | Running | ScaleMode |
| `auto_scale_x`, `auto_scale_y` | true, true | AutoScaleX/Y |
| `y_min`, `y_max` | -1, 1 | YMin, YMax |
| `running_window` | 500 | Window |
| `start_after_step` if `use_start_after_step` | 0 | StartAfter |
| `measurement` "intensity" / "integrated_intensity" / other | "amplitude" | Value / IntegratedValue / Amplitude |
| `box_x`, `box_y` | 450, 20 | BoxX, BoxY (set last, so A does not drag the box) |

Fixtures: `_tools/legacy_examples` in the C# repo (G-08 in 80).

## Settings file

`%LOCALAPPDATA%\WaveDudeNext\Settings.xml`, root `Settings`, XmlSerializer, sparse. Entries in 10-model. A malformed
file is logged with its text and replaced by defaults on the next save. Rust: platform config dir; format free.

## Frame store spill file

Internal; layout in 40.
