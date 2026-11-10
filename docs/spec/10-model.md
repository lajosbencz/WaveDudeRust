# 10 - Model

The project model: every persisted property with its XML name, type, default, range, unit and normalisation.
Persistence details (element order, omission rules, DTD, migration) are in `60-file-formats.md`.

Rules for all model objects:
- Every persisted number has a default and a clamp in `Normalize()`; a `[Range]` beside the default states the same
  bounds unless the bound depends on another value ("dependent" below).
- `Normalize()` runs after load, after import, before a run, and after every edit. Non-finite numbers become the
  default unless stated.
- Rust: the bounds are the constants of `model/ranges.rs`, which `normalize` and the painter's limits use; a
  non-finite number's fallback is the type's `Default` value. The `.wdr` names are the fields' `snake_case` and the
  enum members' `PascalCase` names (60-file-formats.md).
- Positions are integer canvas pixels (origin top-left, y down). Angles in degrees, CCW on screen, normalised into
  (-180, 180] (see `00-overview.md`).
- Every change is an observable event carrying (object, property, old value, new value); a simulation edit (scene,
  solver, seed, sources, parts, active-probe set) invalidates a run; a display or probe-presentation edit does not.

## Project (`Model/Project.cs`, root element `Project`)

| XML | Type | Default | Range / notes |
|---|---|---|---|
| `Version` (attr) | int | 3 | written as current version; see migration in 60 |
| `Seed` (attr) | int | new project: `random in [1, int.MaxValue)` | any int; seeds all Pcg32 streams |
| `Description` (element) | string | "" | free text |
| `Scene` | Scene | defaults | omitted when all defaults |
| `Simulation` | Simulation | defaults | omitted when all defaults |
| `Display` | Display | defaults | always written |
| `Source` * | Source | - | list order = emitter order |
| parts * | one element per kind (see Parts) | - | list order = paint order |
| `Probe` * | Probe | - | list order = probe numbering for markers |
| `ColorScheme` * | ColorScheme | - | project-local schemes |

Names: sources, parts, probes and schemes are `INamed`. New item names: `"<stem> <n>"` with the lowest free n >= 1,
compared case-insensitively across items of the same list. A copy: `"<name> copy"`, then `"<name> copy 2"`, `3`, ...
Name stems: Source `"Source"`, Probe `"Probe"`, parts see table below.

New items land at the centre of the shown canvas: `(W/2, H/2)`, integer division.
A new line probe gets `B = A + (100, 0)`.

## Scene (`Model/Scene.cs`)

| XML | Type | Default | Range | Notes |
|---|---|---|---|---|
| `Mode` | enum `Canvas`/`Image` | Canvas | - | Image: pixels from the embedded image |
| `Image` | string | "" | - | file name of the image (display) |
| `ImagePath` | string | "" | - | where it came from; stored relative if inside the project folder |
| `ImageData` (element) | base64 bytes | null | - | the embedded image file bytes (PNG/JPEG/BMP ...); written only if non-null |
| `Width` | int | 1920 | 8..16384 | canvas width (Canvas mode; Image mode takes the image's size) |
| `Height` | int | 1080 | 8..16384 | canvas height |
| `DampingEdge` | int | -32 | -8192..8192 | < 0: rim of `-DampingEdge` cells outside the canvas; > 0: band inside; 0: none |
| `Edges` | enum `Open`/`Reflective` | Open | - | grid border condition |
| `RefractiveIndex` | double | 1.0 | 1..2.55 | background index of a blank canvas |
| `ImageSources` | bool | false | - | green pixels emit |
| `ImageSourceScale` | double | 1 | 0..1000 | amplitude of image sources |
| `ObjectSources` | bool | true | - | project sources emit |

Derived: `HasRim = DampingEdge < 0`; `Rim = max(0, -DampingEdge)`.
Removing the image clears Image, ImagePath, ImageData and sets Mode = Canvas, keeping the image's size as W x H.
If the embedded image cannot be decoded or is missing in Image mode: `TODO(verify)` (UI shows "(not found)" for a
missing path file; the embedded bytes are authoritative).

## Simulation (`Model/Simulation.cs`)

| XML | Type | Default | Range | Unit / notes |
|---|---|---|---|---|
| `Steps` | int | 2500 | 1..10000000 | steps per run |
| `VisualizeEvery` | int | 2 | 1..1000 | steps per timeline frame |
| `FourthOrderLaplacian` | bool | true | - | false: 9-point stencil |
| `WaveSpeed` | double (px/step) | 1 | 1e-6..1e6 | nominal speed; only used for wavelength estimates in the editor, never by the engine |
| `TimeDecay` | double | 10 | 1..100 | intensity EMA time constant factor |
| `SourceFrequencyScale` | double | 2 | 0..1000 | omega = level/255*0.5*scale |
| `IntensityScale` | double | 1 | 1e-7..1e6 | multiplies the intensity output |

Wavelength of a level in pixels (for display and part design):
```
Wavelength(level, c, scale) = c / (level/255 * 0.5 * scale / (2*pi))
```
Level 100, c = 1, scale 2: 16.0 px. The editor's estimate uses `c = WaveSpeed`; the gate and zone plate designs use
`c = 1` with their `DesignFrequency`.

## Display (`Model/Display.cs`)

| XML | Type | Default | Range / notes |
|---|---|---|---|
| `Field` | DisplayLayer | field defaults | element |
| `Intensity` | DisplayLayer | intensity defaults | element |
| `Video` | Video | defaults | element, omitted at defaults |
| `ExportFrom` | int (steps) | 0 | >= 0 |
| `ExportTo` | int (steps) | 0 | 0 = to the end; if `To != 0 && To < From` then `To = 0` |
| `ProbeGraphsInExport` | bool | true | |
| `ProbeMarkersInExport` | bool | true | |
| `SourceMarkersInExport` | bool | false | |
| `HandlesInPreview` | bool | true | UI only |
| `LabelsInPreview` | bool | true | UI only |

### DisplayLayer

| XML | Type | Field default | Intensity default | Range / notes |
|---|---|---|---|---|
| `ColorScheme` | string | "Default field" | "Default intensity" | empty -> the layer's default name |
| `Brightness` | double | 1 (Rust; original 30) | 1.5 (Rust; original 30) | 1e-4..1e5; changed with the source model (decision 0010): `.wdproj` keeps 30 as the omitted value, below |
| `BlackLevel` | double | -0.05 | 0 | -10..1; field always written (`[DefaultValue(0)]` vs initial -0.05) |
| `Preview` | bool | true | true | layer captured/shown |
| `Overlay` | bool | true | true | glowing edges on the preview picture |
| `OverlayGain` | double | 3 | 3 | 0..5 |
| `OverlayInExport` | bool | true | true | add glowing edges into exported frames (off until file version 4) |

Brightness (Rust, decision 0010): the source model of docs/physics.md (Sources) swings a source cell of Amplitude
100 % by 1 in free space, the field falling as 1/sqrt(r); its intensity, the EMA of u^2, is about 1/2 there. The
palette's top is |u| * b = 1 (field) and sqrt(I) * b = 1 (intensity), so field 1 puts the source's swing at the top
and intensity 1.5 puts sqrt(1/2) * 1.5 = 1.06 just past it. The original's 30 was made for its blended source,
which radiates 5 to 9 times less. The `.wdproj` adapter reads a missing `Brightness` (and a missing layer element) as
30 and writes the attribute whenever the value differs from 30; `.wdr` carries the model's values. A project read
from `.wdproj` without this program's `Generator`, Wave Dude 2D json or an example has its brightness fitted once in
the app (Auto's fit on both layers, at the first frame with content), outside the undo history and without marking the
project dirty. With 8-bit frames one 16-bit frame of that position is computed for the fit alone and never shown; it is
asked again only when it was abandoned and nothing else computes.

### Video

| XML | Type | Default | Range |
|---|---|---|---|
| `Fps` | int | 60 | 1..240 |
| `Quality` | int | 85 | 1..100 |

## Source (`Model/Source.cs`)

| XML | Type | Default | Range / notes |
|---|---|---|---|
| `Name` | string | "Source n" | |
| `Enabled` | bool | true | |
| `X`, `Y` | int | centre | always written; not clamped (outside emitters are dropped by the engine) `TODO(verify)` |
| `Frequency` | double (level) | 100 | 0.01..255; NaN -> 100 |
| `Amplitude` | double (%) | 100 | 0..100 |
| `Phase` | double (deg) | 0 | -180..180 |
| `Mode` | enum `Continuous`/`Pulsed` | Continuous | |
| `PulseShape` | enum `Gaussian`/`SmoothedSquare` | Gaussian | |
| `PulseWidth` | int (cycles) | 10 | >= 1 |
| `OffWidth` | int (cycles) | 10 | >= 1 |
| `DetachAfter` | int (steps) | -1 | < 0: never; else the source fades out over 30 steps after this step |
| `Array` | SourceArray | null | present = composite source |

### SourceArray

| XML | Type | Default | Range |
|---|---|---|---|
| `SizeX`, `SizeY` | int | 1 | >= 1 |
| `SpacingX`, `SpacingY` | int (px) | 10 | any int (no clamp) |
| `Angle` | double (deg) | 0 | (-180, 180] |
| `RandomFrequency` | int (levels) | 0 | >= 0 |
| `RandomPhase` | int (deg) | 0 | >= 0 |
| `RandomOffWidth` | int (cycles) | 0 | >= 0; per-pulse random gap (runtime xorshift) |
| `RandomPosition` | int (px) | 0 | >= 0 |

### Element generation (never persisted)

Run after `Normalize()`. `rng = Pcg32((ulong)(uint)Project.Seed, StableHash(Source.Name))`.
`Jitter(rng, a) = a <= 0 ? 0 : rng.Next(-a, a)` (inclusive, see Pcg32 below).

```
if no Array element:  one element (X, Y, Frequency, Phase, OffWidth), no draws
else (an Array, even 1 x 1, goes through the loop with jitter):
  a = Angle * pi/180
  for row in 0..SizeY-1:
    for col in 0..SizeX-1:
      dx = (col - (SizeX-1)/2.0) * SpacingX
      dy = (row - (SizeY-1)/2.0) * SpacingY
      x  = X + round_even(dx*cos(a) + dy*sin(a))
      y  = Y + round_even(dy*cos(a) - dx*sin(a))
      x += Jitter(rng, RandomPosition)
      y += Jitter(rng, RandomPosition)
      level = clamp(Frequency + Jitter(rng, RandomFrequency), 0.01, 255)
      phase = clamp(Phase + Jitter(rng, RandomPhase), -180, 180)
      off   = max(1, OffWidth + Jitter(rng, RandomOffWidth))
```
The draw order per element (pos x, pos y, freq, phase, off) is fixed; every Jitter call with a <= 0 draws nothing.

### Emitter (engine input, `Engine/Emitter.cs`)

Per element of an enabled source, only when `Scene.ObjectSources`:

| Field | Value |
|---|---|
| X, Y | element position (canvas px) |
| Level | element level |
| Amplitude | `Source.Amplitude / 100` |
| PhaseRadians | `element phase * pi / 180` |
| Pulsed, Shape | from Source |
| OnWidthCycles | `PulseWidth` |
| OffWidthCycles | element off width |
| OffWidthBase | `Source.OffWidth` |
| RandomizeOffWidthAmount | `Array.RandomOffWidth` (0 without array) |
| DetachAfterStep | `DetachAfter` |
| RngSeed | `Pcg32(seed, StableHash(Name) ^ 0x47415053).NextUInt32()` once per element in element order; 0 -> 1 |

Engine constant `DetachRampSteps = 30`.

## Pcg32 and hashes (`Pcg32.cs`)

```
ctor(seed: u64, stream: u64):
  inc = (stream << 1) | 1; state = 0
  NextUInt32(); state = state + seed (wrapping); NextUInt32()
NextUInt32():
  old = state
  state = old * 6364136223846793005 + inc            (wrapping u64)
  xs  = (u32)(((old >> 18) ^ old) >> 27)
  rot = (u32)(old >> 59)
  return (xs >> rot) | (xs << ((-rot) & 31))
Next(min, max) inclusive:
  range = (u64)(max - min + 1); limit = 2^32 - (2^32 % range)
  loop v = NextUInt32() until v < limit
  return min + (int)(v % range)
NextDouble() = (NextUInt32() >> 8) / 2^24
StableHash(s): h = 2166136261u32; for each UTF-16 code unit c: h = (h ^ c) * 16777619 (wrapping); return h
Xorshift32(x): x ^= x << 13; x ^= x >> 17; x ^= x << 5
```
Stream argument for sources is `StableHash(Name)` widened to u64; a diffuser's roughness draws its phases from
`Pcg32(seed, StableHash(Name) ^ 0x44494646)`, one `NextDouble` per possible component (13) in order (`model/profiled.rs`),
so the seed is part of the scene while a diffuser is enabled (`SceneKey`).

## Probe (`Model/Probe.cs`)

| XML | Type | Default | Range / notes |
|---|---|---|---|
| `Name` | string | "Probe n" | |
| `Active` | bool | true | inactive probes are not sampled |
| `Layer` | enum `Field`/`Intensity` | Field | |
| `Measurement` | enum `Amplitude`/`Value`/`IntegratedValue` | Amplitude | Field forces Amplitude; Intensity + Amplitude -> Value |
| `Kind` | enum `Point`/`Line` | Point | |
| `AX`, `AY` | int | centre | always written; clamped into the canvas when sampled |
| `BX`, `BY` | int | 0 | line end; written only for Line |
| `BoxSize` | enum `Tiny`,`Small`,`Medium`,`Large`,`ExtraLarge`,`Custom` | Medium | graph box |
| `BoxX`, `BoxY` | int | 0 | box top-left; a box at A follows the probe |
| `BoxWidth` | int | 400 | 60..4096; written only for Custom |
| `BoxHeight` | int | 200 | 30..4096; written only for Custom |
| `ScaleMode` | enum `Running`/`Full` | Running | graph time axis: last `Window` steps or whole run |
| `AutoScaleX`, `AutoScaleY` | bool | true | |
| `YMin`, `YMax` | double | -1, 1 | `YMax <= YMin` -> -1, 1 |
| `YScale` | enum `Linear`/`Logarithmic` | Linear | Field forces Linear |
| `Decades` | int | 6 | 1..12 |
| `Window` | int (steps) | 500 | 10..1000000 |
| `StartAfter` | int (steps) | 0 | >= 0; integrated value starts here |

Box sizes (px): Tiny 200x100, Small 300x150, Medium 400x200, Large 500x250, ExtraLarge 700x350, Custom = BoxWidth x
BoxHeight. Fit into the canvas (`BoxInCanvas`): if larger than the canvas in either axis, scale both by
`min(W/w, H/h)` (aspect kept), round (even); then clamp the position so the box lies inside.

## Optical parts (`Model/OpticalPart.cs`, `Parts/*`)

The XML element name is the kind. Common attributes on every part:

| XML | Type | Default | Range / notes |
|---|---|---|---|
| `Name` | string | "<stem> n" | |
| `Enabled` | bool | true | disabled parts are not painted |
| `X`, `Y` | int | centre | >= 0, always written; the centre is the top-left corner of pixel (X, Y) |
| `Angle` | double (deg) | 0 | (-180, 180] |

Shared constants: refractive index range 1..2.55, rounded to 0.01 (round even), NaN -> 1; coating 0..50 px
(`MaxCoating`); `Absorption` 0..255 (blue level at full share); `AbsorptionRamp` 1..400 px.

`CoatedPart` adds (lens, shapes, GRIN, fibre):

| XML | Type | Default | Notes |
|---|---|---|---|
| `Coating` | double (px) | 0 | 0..50; AR band depth |
| `CoatingProfile` | enum `Layer`/`Graded` | Layer | |
| `FrontCoated`, `BackCoated`, `SidesCoated` | bool | true | faces by outward normal (see 20) |

### Kinds

| Element | Stem | Base | Kind-specific attributes (default, range) |
|---|---|---|---|
| `Lens` | Lens | Coated | `Mirror` false; `RefractiveIndex` 1.5; `Absorption` 0; `AbsorptionRamp` 40; `Diameter` 200 (>= 2); `R1` 300 (0 = flat); `R2` -300; `Thickness` 40 (>= 1) |
| `FresnelLens` | Fresnel lens | Coated | the lens's attributes; `Zones` 4 (1..100); `Division` EqualWidth/EqualDepth. A new one is a plate of `Thickness` 20, `Diameter` 500, radii 1500 |
| `Polygon` | Polygon | Coated shape | `Mirror`, `RefractiveIndex` 1.5, `Absorption`, `AbsorptionRamp` 40; `Sides` 6 (3..64); `Side` 60 (>= 2) |
| `Rectangle` | Rectangle | Coated shape | shape props; `Length` 60; `Width` 60; `Linked` false |
| `Ellipse` | Ellipse | Coated shape | shape props; `Length` 60; `Width` 60; `Linked` false |
| `Triangle` | Triangle | Coated shape | shape props; `Base` 60 (>= 2); `Type` Equilateral; `AngleA` 60, `AngleB` 60 (1..178) |
| `Outline` | Outline | Coated shape | shape props; `Point` children (int X, Y, part frame) |
| `Diffuser` | Diffuser | Coated shape | shape props; `Length` 50 (>= 1); `Width` 500 (>= 2); `CenterWavelength` 16 (2..10000); `Amplitude` 1 (0..10000); `Sigma` 6 (0..10000). A new one coats its back alone |
| `InterferenceFilter` | Interference filter | Coated shape | shape props (`BackCoated` alone counts); `Length` 50 (>= 1); `Width` 500 (>= 1); `Layer` children (`Thickness` 2 (0..1000, 0.01), `RefractiveIndex` 1.5), the outermost first. A new one is a quarter-wave stack for 16 px: five pairs 2.00 at 2.00 px then 1.50 at 2.67 px and one more 2.00 |
| `SimpleGrating` | Grating | shape material (no coating) | `Mirror` (a new one is one), `RefractiveIndex`, `Absorption`, `AbsorptionRamp`; `Columns` 1 (>= 1); `Rows` 31 (>= 1); `ColumnSpacing`, `RowSpacing` 16 (>= 2); `Diameter` 2 (1 to half the smaller spacing, clamped in `normalize`) |
| `MetaLens`, `MetaPrism`, `MetaAxicon` | Meta lens, Meta prism, Meta axicon | Coated shape (`FrontCoated` the ridge tops, `BackCoated` the plate's back; sides not offered) | shape props; `Length` 50 (>= 1); `Width` 500 (>= 2); `DesignFrequency` 50 (1..255); `Pitch` 4 (1..10000); `Height` 32 (1..10000); `PatternIndex` 2 (1..2.55); the lens `FocalLength` 500 (>= 1), `LensType` Converging/Diverging; the prism `Deflection` 20 (-89..89); the axicon `ConeAngle` 10 (0.1..89). Rust: one `MetaSurface` kind with a `design` (`model/meta.rs`) |
| `FacettedGrating` | Facetted grating | Coated shape | shape props (the coating's `BackCoated` alone counts); `Length` 50; `Width` 500; `Pattern` Sawtooth/SquareWave/Sinusoidal; `Pitch` 32 (2..10000); `Amplitude` 4 (0..10000) |
| `Fiber` | Fiber | Coated | `CoreIndex` 1.5; `CoreDiameter` 20 (>= 2); `CladdingIndex` 1.45; `CladdingThickness` 10 (>= 0); `Absorption` 0; `AbsorptionRamp` 10; `Ends` Flat/Round (Flat); `Node` children |
| `BraggReflector` | Bragg reflector | part | `Pairs` 10 (1..200); `Thickness1` 10; `Thickness2` 10; `Index1` 1.5; `Index2` 2.0; `Width` 200 |
| `GrinLens` | GRIN lens | Coated | `Length` 100; `Diameter` 100; `CentreIndex` 1.8; `EdgeIndex` 1.5 |
| `XorGate` | XOR gate | gate | outputs Xor (+90 deg) / And |
| `AndGate` | AND gate | gate | outputs And (-90 deg) / Xor |
| `OrGate` | OR gate | gate | outputs Or (+30 deg) / Remainder |
| `ZonePlate` | Zone plate | part | `Mount` false; `MountWidth` 200; `Diameter` 800; `Thickness` 32; `Coating` 0 (0..400, absorption rise; 64 and 32 until file version 4); `DesignFrequency` 50; `FocalLength` 600; `Inverse` false |

Size attributes are ints. Lower bounds: Bragg `Thickness1`/`Thickness2` >= 1, `Width` >= 2; GRIN `Length` >= 1,
`Diameter` >= 2; zone plate `MountWidth` >= 1, `Diameter` >= 2, `Thickness` >= 1, `DesignFrequency` 1..255,
`FocalLength` >= 1; rectangle/ellipse `Length`, `Width` >= 1; fibre `CladdingThickness` >= 0. Upper bounds
int.MaxValue unless stated. Verify every kind against `[Range]` in `Model/OpticalPart.cs`.

Rectangle/Ellipse `Linked`: while true, setting Length also sets Width and vice versa; switching Linked on sets
Width = Length. On load the attribute order decides (`TODO(verify)`: whether a loaded Linked=true with Length != Width
keeps both).

Triangle presets (Normalize writes the preset angles): Equilateral (60, 60), RightIsosceles (45, 45),
ThirtySixtyNinety (90, 30), Isosceles (`a = min(AngleA, 89)`, b = a), Custom (a, `min(AngleB, 179 - a)`).

Fibre `Node` (element): `X`, `Y` (int, part frame px, always written), `InX`, `InY`, `OutX`, `OutY` (int, 0, relative to the node),
`Smooth` (true: dragging one handle mirrors the other; UI only). Default path: nodes (-100, 0, OutX = 67) and
(100, 0, InX = -67).

Logic gate attributes (all three kinds):

| XML | Default | Range |
|---|---|---|
| `DesignFrequency` | 50 | 1..255, round (even) to 2 decimals |
| `Coating` | 6 | 0..50 |
| `InputCoated`, `OutputCoated` | true | |
| `UpperOutputActive`, `LowerOutputActive` | true | |
| `CoreIndex` | 2.55 | 1..2.55 |
| `CladdingIndex` | 1.45 | 1..2.55 |
| `PortSpacing` | 2 | 2..2000, raised to the design's minimum |

The gate's frequency scale is not persisted; it is taken from `Simulation.SourceFrequencyScale` when designing.

### Lens radius fit (`LensPart.FitRadii`, applied in Normalize)

```
half = max(2, D)/2
for r in (R1, R2): if !finite(r) or r == 0: flat; elif |r| < half: r = sign(r)*half
Sag(r, h) = |r| - sqrt(r^2 - min(|r|, h)^2)          (flat -> 0)
sag1 = Sag(R1, half); sag2 = Sag(R2, half)
convex = (R1 > 0 ? sag1 : 0) + (R2 < 0 ? sag2 : 0)
budget = max(1, T) + (R1 < 0 ? sag1 : 0) + (R2 > 0 ? sag2 : 0)
if convex > budget + 1e-9:
  scale = budget / convex
  for each convex radius: s = sag*scale; r = sign(r) * ceil((half^2 + s^2) / (2*s))
```
A Fresnel lens fits the deepest zone's sag instead (`lens_geometry::fit_radii_zoned`): for zones of equal width the
outer zone's, `Sag(r, half) - Sag(r, half (N-1)/N)`; for equal depths `Sag(r, half) / N`; the refit radius is found by
the sagitta for equal depths and by bisection for equal widths. Zone boundary k lies at `half k/N` (equal width) or
where the sag reaches `k/N` of the rim's, `sqrt(r^2 - (|r| - k S/N)^2)` (equal depth).
Shape class for icons and naming: a surface is flat when its sag at the rim < 0.5 px.
`Mirror` toggled by the user renames "Lens n" <-> "Mirror n" (`MirrorName`); loading never renames.

## Colour scheme (`Rendering/ColorScheme.cs`)

| XML | Type | Notes |
|---|---|---|
| `Name` | string | lookup key, case-insensitive |
| `Kind` | enum `Unspecified`/`Field`/`Intensity` | which layer it is meant for |
| `Description` | string | |
| `Stop` * | `At` (double 0..1), `Color` "#RRGGBB" | file order kept; rendering uses a stable sort by `At`; no stops renders as Grey |

Lookup by name: project schemes first, then built-in (embedded) and user schemes
(`%LOCALAPPDATA%\WaveDudeNext\ColorSchemes\*.wdscheme`). Legacy names with underscores (e.g. `Default_field`) resolve
to the spaced name. A missing scheme renders as `Grey` (stops 0: #000000, 1: #FEFEFE).

Built-in schemes (23): Blue to yellow, Bright rainbow, Copper, Default field, Default intensity, Flare inverted,
Gray scales inverted, Gray scales, Green blue, Green pink, Greens, Hot orange, Ice and fire, Inferno, Magma,
Orange hot, Orange symmetry, Plasma, Purple rain, Rainbow, Simply red, Turkey brown, Yellow purple. Copy the files
from `WaveDudeNext/ColorSchemes/*.wdscheme` verbatim.

Default field stops (9, at i/8): #FDFDFD, #3020F4, #160BD2, #1A18AA, #000000, #770202, #BF1515,
#FB3B3B, #FFFFFF. Default intensity (9, i/8): #000000, #0E4494, #21ACDE, #20E0D3, #21F63D, #F2F94D, #EAB348,
#F04D1D, #FF0000.

Also readable: the original author's JSON scheme format (keys `name`, `origin`, `category`, `scheme_type`,
`bits_per_channel` 8, `entry_count` 255, `colors`, `stops`). `TODO(verify)`: mapping of `colors` without stops.

## Preview quality (app setting, affects runs; `Runs/PreviewQuality.cs`)

`PreviewQuality(ResolutionDivisor in {1,2,4}, FrameDivisor in {1,2,4}, Format in {Half, Index}, IntensityRoot = true)`.
`Full = (1, 1, Half, true)`. First-start tiers in `40-frames-store-render.md`.

## Settings file (app, not project; `WaveDudeNext.WPF/Settings.cs`)

`%LOCALAPPDATA%\WaveDudeNext\Settings.xml`, root `Settings`, sparse (defaults omitted). Non-UI relevant entries:

| XML | Default | Meaning |
|---|---|---|
| `UseGpu` | true | engine choice, CPU fallback |
| `FrameMemoryPercent` | 0 | 0 = Auto; else share of physical RAM for the frame store |
| `PreviewResolution` | 1 | resolution divisor |
| `PreviewFrames` | 1 | frame divisor |
| `PreviewSampleBytes` | 1 | 1 = 8-bit index, 2 = f16 |
| `PreviewDitherBits` | 0 | 0, 10, 12, 14, 16 |
| `AutoRerun` | true | re-run 1.5 s after the last simulation edit |
| `WarnWhenRamInsufficient` | true | |
| `ExportHint` * | - | per project: last export paths (Project, Run, Still, Probe), max 20 projects |
UI entries (window, layout, theme, language, zoom, recent files) belong to the UI spec.
