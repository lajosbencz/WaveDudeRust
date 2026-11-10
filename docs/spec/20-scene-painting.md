# 20 - Scene pixels and part painting

How the scene's BGRA8 pixels are built from the project, and what each channel means to the engine.
Reference for the painting rules and their measurements: `OpticalParts.md` in the C# repo (read it before porting).

## Scene pixel channels

| Channel | Meaning |
|---|---|
| red | refractive index: `n = max(red/100, 1)` (red 100 = 1.00, 255 = 2.55; red 1..99 read as 1); **red 0 = mirror** (a fixed wall, u = 0; the original used n = 1000) |
| green | image source: green > 0 emits at frequency level = green (only when `Scene.ImageSources` and an image scene) |
| blue | absorber: damping factor per step `d = 1 - blue/255` |
| alpha | ignored by the engine; painted pixels and blank canvases are 255 |

Index resolution is 0.01 (one red level). The maximum index of an image pixel is 2.55.

## Composition order

```
if Scene.Mode == Image and image decodes:  pixels = decoded image (BGRA8, its own W x H)
else:                                      pixels = none (blank canvas of Scene.Width x Scene.Height)
if no enabled part:   result = pixels as is (blank -> engine uses SceneData.Empty with Scene.RefractiveIndex)
else:
  if blank: pixels = W x H, red = round_even(RefractiveIndex*100), green = blue = 0, alpha = 255
  else:     pixels = copy of the image
  for part in Project.Parts in list order, enabled only: Paint(part, pixels)
```
The result is cached by (image identity, parts version, size, background index); identical inputs give the same buffer.
Glowing edges (below) are computed from the composed pixels.

## Per-part painting

Parts are painted as normalised (spec/10 bounds): the formulas below take each value as stored, with no
clamps of their own.

### Bounds

`Bounds = centre +- (Extent + 1)` in canvas coordinates, floor of the min, ceil of the max, clipped to the canvas.
Extent is the part's farthest point from its centre (rotated hull). Only pixels inside are visited.

### Depth function

Each kind provides, in the part frame (x', y') (see rotation in 00):
- `depth(p)`: > 0 inside, < 0 outside, 0 on the outline. For kinds with `DepthIsDistance` it is a signed Euclidean
  distance to the outline (px); for the lens and the logic gates it is not a distance.
- `IndexAt(p)`: null outside; 0 for a mirror; else the index with coating band applied.
- `BodyIndexAt(p)`: the body index without coating (used for settled runs).
- `InnerEdgeDistance(p)`: distance to an inner index boundary (fibre core, Bragg layers), else +inf; the gates report 0.
- `InnerEdge(p)` (Rust, decision 0010): the distance to the nearest inner edge on p's side and the body index across
  it, for the smoothed path below; kinds without it fall back to 16 samples.
- Faces (for coatings): classified by the outward normal's x' component `nx`:
  `nx < -cos(45 deg)` Front, `nx > cos(45 deg)` Back, else Sides. A fibre: start face Front, end face Back, tube Sides.
  Inside the band the nearest face decides.

Order per part: absorption is painted first (if `Absorption > 0`), then the index (if the kind paints an index).

### Mirror parts (`Mirror = true`)

Centre sample only: `depth(centre) >= 0` -> red = 0 (no supersampling). Green and blue untouched: a mirror ignores
its `Absorption` (library_check "a mirror ignores its absorption").

### Refractive parts

Rust rule (decision 0010, item 5): a pixel an edge crosses takes the effective medium of the scalar wave
equation, the area-weighted permittivity `n_eff = sqrt(sum(share_k * n_k^2))` (Farjadpour et al. 2006),
instead of the original's mean of n over 16 point samples.
```
reach = DepthIsDistance ? 0.71 : 0.6         (sqrt(1/2) with a margin; SampleReach)
for each pixel (x, y) in bounds:
  c = (x + 0.5, y + 0.5) in part frame
  e = depth(c)
  if e < -reach: skip
  existing = red == 0 ? 1.0 : red/100.0
  inner = InnerEdgeDistance(c)
  if e > reach and inner > reach and (Coating == 0 or |e - Coating| > reach):
      v = IndexAt(c)                          (centre sample stands for the pixel)
  else if DepthIsDistance:
      v = Smoothed(c)                         (below; none when f = 0: skip pixel)
  else:                                       (lens, logic gates)
      16 samples at (x + 0.125 + sx*0.25, y + 0.125 + sy*0.25), sx, sy in 0..3
      each sample: IndexAt(s) if inside else existing
      if no sample inside: skip pixel
      v = sqrt(mean of the 16 n^2)
  red = clamp(round_even(v*100), 1, 255)
```
Mirror index 0 inside IndexAt makes the pixel red 0 only via the mirror path above.

Smoothed, for kinds whose depth is a signed distance:
```
Coverage(e, nx, ny): share of the unit pixel on the inner side of a straight edge e px from its centre,
  (nx, ny) the edge's unit normal in canvas axes; a = max(|nx|, |ny|), b = min(|nx|, |ny|):
  |e| >= (a + b)/2:  1 if e > 0 else 0
  |e| <= (a - b)/2:  0.5 + e/a                         (clamp(0.5 + e, 0, 1) for an axis-aligned edge)
  else:              t = ((a + b)/2 - |e|)^2 / (2ab);  e > 0 ? 1 - t : t
normal = forward differences of the field (step 1e-3 px along canvas x and y), normalised;
  only taken where |e| < 0.71 (else any normal gives 0 or 1)
f = Coverage(e, normal of depth)                       share inside the outline
coated = Coating > 0 and e < Coating + reach and the face at c is coated
Part(b), the permittivity of the part's share over a body of index b:
  not coated:  f * b^2
  Layer:       g = Coverage(e - Coating, same normal)   share deeper than the band
               (f - g) * round_even(sqrt(b), 2)^2 + g * b^2
  Graded:      f * BandIndex(max(e, 0), b, existing)^2  (the band's index at the centre; the profile is continuous)
near = BodyIndexAt(c)
if inner <= reach:
  (d, far) = InnerEdge(c)                    distance on c's side (>= 0), body index across the edge
  if the kind has no InnerEdge: the 16-sample path above
  h = Coverage(d, normal of d)
  eps = h * Part(near) + (1 - h) * Part(far)
else:
  eps = Part(near)
v = sqrt(eps + (1 - f) * existing^2)
```
`BodyIndexAt` is asked at a centre just outside the outline too: the GRIN lens holds its edge index beyond its
sides, the Bragg reflector its end layer's index beyond its ends; the fibre and the polygon kinds are defined there.
`InnerEdge`: fibre `(|r - core/2|, the other of core and cladding)`; Bragg the nearest layer boundary inside the
stack (`k*period + T1` for every pair, `k*period` from the second; the stack's ends are the outline), the other
layer's index. Where the outline and an inner edge cross the same pixel the two shares multiply, exact for
perpendicular edges (a Bragg layer meeting the side).

Accuracy: Coverage is exact for a straight edge at any angle. A curved edge of radius R differs from its tangent by
the circular segment between them within the pixel, at most about `1/(12 R)` of the pixel for an axis-aligned
tangent and `0.24/R` at 45 deg; a corner of the outline inside the pixel (a polygon vertex, a fibre's flat end
meeting the tube) is not accounted for. The painted permittivity area `sum((n_eff^2 - 1)/(n^2 - 1))` of a hexagon,
an ellipse and a triangle at 0, 17 and 45 deg is within 0.7 px of the polygon's area (the 0.01 index rounding).

### Settled runs (optimisation that is byte-exact)

The original's optimisation; the Rust painter paints pixel by pixel. When `DepthIsDistance`, a pixel centre at
distance e from the nearest edge settles the next
`n = (int)(e - reach - SkipAllowance - 1e-6)` pixels of its row (if n >= 1): inside runs take `BodyIndexAt`
(coating band excluded by the condition above), outside runs are skipped. The result must equal pixel-by-pixel
painting byte for byte (library_check "painter runs"). A Rust port may skip this optimisation if it paints pixel by
pixel with the same rules.

### Coating band

```
t = depth / Coating, for depth < Coating on a coated face
Layer:   index = round_even(sqrt(body), 2 decimals)
Graded:  index = outside + (body - outside) * (1 - cos(pi*t))/2      (outside = existing index under the pixel)
```
`GradedProfile(t) = (1 - cos(pi t))/2`. Faces not coated keep the body index up to the outline.

### Absorption

```
ramp = min(AbsorptionRamp, deepest centre depth of the part)
t = clamp(depth/ramp, 0, 1)
share = t^2                           (AbsorptionProfile; kinds may override, see below)
blue = max(old blue, round_even(Absorption * share))
```

## Kinds

| Kind | Depth / geometry (part frame) | Index | DepthIsDistance |
|---|---|---|---|
| Lens | `min(SurfaceDepth(v1=-T/2, R1, -1), SurfaceDepth(v2=+T/2, R2, +1), D/2 - |y|)` | RefractiveIndex | no |
| Polygon | regular n-gon, circumradius `R = Side/(2 sin(pi/n))`, vertex k at angle `pi + (2k+1)pi/n` | RefractiveIndex | yes |
| Rectangle | vertices `(-L/2,-W/2), (L/2,-W/2), (L/2,W/2), (-L/2,W/2)` | RefractiveIndex | yes |
| Ellipse | polygon of n vertices `(a cos(2 pi k/n), b sin(2 pi k/n))`, a = L/2, b = W/2, `n = clamp(round_even(Ramanujan circumference / 3), 24, 180)` | RefractiveIndex | yes |
| Triangle | vertices `(-h/2, -B/2), (-h/2, B/2), (h/2, apexY)` | RefractiveIndex | yes |
| Outline | polygon through the `Point`s (part frame); < 3 points paints nothing | RefractiveIndex | yes |
| Fiber | tube around a flattened Bezier path, see below | core / cladding | yes |
| BraggReflector | rectangle `min(L/2 - |x|, W/2 - |y|)`, `L = Pairs*(T1+T2)` | layers along x | yes |
| GrinLens | rectangle Length x Diameter | `nc - (nc - ne)*(y/(D/2))^2` | yes |
| Zone plate | slab of Thickness along x, Diameter along y | none (absorption only) | yes |
| Logic gates | waveguide arms, see below | core / cladding | no |

Polygon-type shapes: even-odd fill, signed distance to the nearest edge.

Lens surface depth:
```
SurfaceDepth(p, v, r, side):
  flat (r == 0): return (x - v) * -side
  centre = v + r ; along = (x - centre) * -side ; dist = |p - (centre, 0)|
  convex (r*side < 0): return along > 0 ? |r| : |r| - dist
  concave:             return along < 0 ? -inf : dist - |r|
```
Ramanujan circumference: `pi*(3(a+b) - sqrt((3a+b)(a+3b)))`.

Triangle (angles a = AngleA, b = AngleB in radians, half = Base/2):
```
h     = 2*half*sin(a)*sin(b)/sin(a+b)
apexY = -half + h*cos(a)/sin(a)
```

### Fibre

- Path: per node pair, control points `node, node + Out, next + In, next`. If both handles lie within 0.05 px of the
  chord: one segment; else `steps = clamp(ceil(sqrt(0.75*maxSecondDiff/0.05)), 1, 512)`, points at `t = k/steps`
  (maxSecondDiff = max |second difference| of the control polygon). Consecutive duplicate points dropped; < 2
  points paints nothing.
- Tube radius `core/2 + clad`. `Ends = Flat`: cut by planes normal to the start tangent (first node's Out handle, else
  the first chord) and the end tangent (last node's `-In`, else the last chord). `Round`: capsule ends.
- Depth = max over chords (looked up through a 32-px cell grid, margin 2; cells without chords give -2).
- Body index: core if path distance <= core/2, else cladding. Cladding thickness 0: bare core (cladding = core index).
- Inner edges when clad > 0 (the core boundary). SkipAllowance covers the wedge jump at a bent flat end.
- Absorption hump (mode stripper): `band = min(AbsorptionRamp, clad/2)`, `inner = outerR - 2*band`; share rises as t^2
  from `inner` to `inner + band` and falls as t^2 to the outer radius. Resolved: exact profile under "Absorption
  profiles" below.

### Bragg reflector

Layers along x from `-L/2` with `period = T1 + T2`: `[k*period, k*period + T1)` Index1, then Index2. Inner edges at the
layer boundaries. No coating.

### GRIN lens

Index `nc - (nc - ne)*(y/(D/2))^2`. Faces: the end faces (x = +-L/2) are Front/Back by the sign of x, else Sides.

### Zone plate

```
lambda = Wavelength(DesignFrequency, 1, SourceFrequencyScale)
a = lambda^2/4 ; b = lambda*f
ZoneAt(r) = floor((-b + sqrt(b^2 + 4*a*r^2)) / (2*a))
blocked = (ZoneAt(|y|) % 2 == 1) != Inverse
```
Absorption only, block absorption 96 (blue level). Share 0 in open zones; in blocked zones `t^2` with
`t = (T/2 - |x|) / min(Coating, T/2)` clamped to 0..1. `Mount`: an absorbing frame of `MountWidth` beyond the
diameter. Resolved: the mount's share is a block's, see "Absorption profiles" below.

### Logic gates

Waveguide arms at angle +-atan(slope) about the x' axis; arm distance `| |y| - offset | * cos(atan(slope))`. End coating
`round_even(sqrt(n), 2)` within `Coating` of a coated end; side absorbers. The geometry (arm offsets, slope, lengths,
port spacing minimum) comes from a design routine `LogicGateDesign.For(wavelength, core, cladding, spacing, phase)`
plus `WaveguideModes` (slab-waveguide mode solver). Resolved: the design is specified in `21-logic-gate-design.md`,
the solver in `22-waveguide-modes.md`, the gate's absorbers under "Absorption profiles" below.

## Glowing edges (`Rendering/GlowingEdges.cs`)

Sobel magnitude per colour channel of the composed scene pixels (BGRA8):
```
for y in 1..H-2, x in 1..W-2, channel c in B, G, R:
  sum(x)  = p[y-1][x] + 2*p[y][x] + p[y+1][x]
  diff(x) = p[y+1][x] - p[y-1][x]
  gx = sum(x+1) - sum(x-1)
  gy = diff(x-1) + 2*diff(x) + diff(x+1)
  out = floor(min(255, sqrt(gx^2 + gy^2) * 0.25))          (sqrt in f64)
outer ring: 0; alpha 255; image smaller than 3x3: all black
```
Additive blend into a rendered frame: per channel `min(255, frame + (int)(edge*gain))`; gain <= 0 changes nothing.
In exports only when `DisplayLayer.OverlayInExport` and composed scene pixels exist with the run's canvas size
(an image scene, or a canvas with enabled parts); a blank canvas without parts has no outline.

## SceneData (engine input, `Engine/SceneData.cs`)

From the composed pixels (W x H) and Scene settings, on a grid `GW = W + 2*Rim`, `GH = H + 2*Rim`:
```
per canvas pixel:
  n  = red == 0 ? mirror (u held at 0) : max(red/100.0, 1)
  c2 = (f32)((1/n)^2)
  d  = (f32)(1 - blue/255.0)
  green > 0 and useSources: image source (x, y, level = green), row-major order
blank canvas (no pixels):
  speed = (f32)(1/RefractiveIndex); c2 = speed*speed; d = 1
```
`Simulation.WaveSpeed` is not an engine input (editor wavelength estimates only).

Rim (Rim > 0):
```
grow c2 and d to GW x GH by repeating the nearest canvas edge cell
for i in 0..Rim-1 (ring i counted from the outer ring inward):
  into   = (Rim - i)/Rim
  factor = (f32)(1 - 0.5*into^2)
  every cell of ring i: d = factor                (overwrites)
image sources shift by (+Rim, +Rim)
```
Damping band inside (Rim = 0, DampingEdge > 0):
```
border = min(DampingEdge, min(W, H)/2)
ring i (0 = outermost canvas ring), i < border: d = (f32)sqrt(i/border)     (overwrites)
```
Both overwrite `d` (blue-channel damping in those rings is lost). Ring i covers `x in i..GW-1-i` on rows i and
GH-1-i, and `y in i..GH-1-i` on columns i and GW-1-i. `border <= 0` does nothing.

## Absorption profiles

Every absorber a part paints goes into the blue channel; the engine turns blue into the per-step damping factor
`d = 1 - blue/255` (per sub-step `d^(1/S)`, see 30). Common rules (`PartPainter.PaintAbsorption`):

```
only when the shape's Absorption > 0, painted before the index
for each pixel (x, y) in bounds, centre (x + 0.5, y + 0.5):
  depth = (f32) depth(centre)                     stored as f32; only its sign and the default share read it
  if depth >= 0:
    share = Share(centre, depth, ramp)            per kind, 0..1
    blue  = max(blue_old, round_even(Absorption * share))
deepest = max(0, max over pixel centres of depth (f64))
ramp    = min(AbsorptionRamp_shape, deepest)
```
No supersampling; green and red untouched (red is painted afterwards by the index pass, where the kind paints an
index). A part never lowers the blue already there. A mirror part has `Absorption = 0`.

### (a) Default profile: lens, polygon, rectangle, ellipse, triangle, outline

```
Absorption_shape     = Mirror ? 0 : Absorption
AbsorptionRamp_shape = AbsorptionRamp
Share = ramp > 0 ? clamp(depth/ramp, 0, 1)^2 : 1
```
`depth` is the shape's depth (signed Euclidean distance to the outline for the polygon kinds; the lens's surface
depth, not a distance). The ramp shortened to the deepest centre lets a part thinner than twice its ramp reach full
strength in its middle. Bragg reflector and GRIN lens paint no absorption.

Example (Example 18, "Absorber 1": Rectangle at (90, 161), Length 62, Width 172, Absorption 255, AbsorptionRamp 77):
pixel centres have `x' = x + 0.5 - 90`, `depth = min(31 - |x'|, 86 - |y'|)`, `deepest = 30.5`, so `ramp = 30.5`. A
centre at `|x'| = 20.5` (depth 10.5): share `(10.5/30.5)^2 = 0.1185`, blue `round(30.22) = 30`; at `|x'| = 0.5`:
blue 255.

### (b) Fibre cladding hump (mode stripper)

Quantities (part frame, px):
```
R_c    = CoreDiameter/2                           core radius
R_o    = R_c + CladdingThickness                  outer radius
band   = min(AbsorptionRamp, CladdingThickness/2)
inner  = R_o - 2*band                             (>= R_c)
Absorption_shape = CladdingThickness > 0 ? Absorption : 0
```
Share by the distance `r` from the flattened path (distance to the nearest chord listed for the point's 32 px grid
cell; +inf where no chord is listed), independent of the painter's depth and ramp:
```
if band <= 0 or r <= inner or r >= R_o: share = 0
t     = (r - inner)/band                          0..2
share = clamp(t <= 1 ? t : 2 - t, 0, 1)^2
```
A symmetric hump of total width `2*band` in the outer part of the cladding: 0 at `r = inner`, rising as `t^2` to the
peak 1 at `r = R_o - band`, falling as `(2 - t)^2` to 0 at `r = R_o`. Pixels are painted only where the tube's depth
is `>= 0`, so with flat ends the hump stops at the end planes; with round ends `r` is the distance to the end point and
the hump follows the caps. A band wider than half the cladding is squeezed to `CladdingThickness/2`.

Example (Example 23, "Single-mode fibre": CoreDiameter 6, CladdingThickness 31, Absorption 252, AbsorptionRamp
default 10): `R_c = 3`, `R_o = 34`, `band = 10`, `inner = 14`, peak at `r = 24`.

| r (px) | t | share | blue |
|---|---|---|---|
| <= 14 | - | 0 | 0 |
| 16.5 | 0.25 | 0.0625 | 16 |
| 19 | 0.5 | 0.25 | 63 |
| 24 | 1 | 1 | 252 |
| 29 | 1.5 | 0.25 | 63 |
| 33.5 | 1.95 | 0.0025 | 1 |
| >= 34 | - | 0 | 0 (outside) |

"Wide multi-mode fibre" (CoreDiameter 52, CladdingThickness 51, AbsorptionRamp 15): `R_c = 26`, `R_o = 77`,
`band = 15`, hump from 47 to 77, peak at 62.

### (c) Zone plate blocks and mount

```
halfT  = Thickness/2
halfD  = Diameter/2
outerH = halfD + (Mount ? MountWidth : 0)
ramp_z = min(Coating, halfT)
lam    = Wavelength(DesignFrequency, 1, SourceFrequencyScale)    (no shape when not finite or <= 0)
depth  = min(halfT - |x'|, outerH - |y'|)                       a rectangle's signed distance
Absorption_shape = 96 (BlockAbsorption)
```
Share at a centre (x', y'), independent of the painter's ramp:
```
across = |y'|
if across > outerH:                          share = 0
if across <= halfD and not IsBlock(across):  share = 0          open zone
otherwise (a block, or the mount band halfD < across <= outerH):
  share = ramp_z > 0 ? clamp((halfT - |x'|)/ramp_z, 0, 1)^2 : 1
IsBlock(r) = (ZoneAt(r) mod 2 == 1) != Inverse
ZoneAt(r)  = floor((-b + sqrt(b^2 + 4*a*r^2)) / (2*a)),  a = lam^2/4, b = lam*FocalLength
```
The rise runs from the two faces across the axis (x' = +-halfT) inward over `ramp_z`; the sides of a block toward the
open zones, the mount's inner edge at `halfD` and its outer edge at `outerH` are sharp. The mount is a block of the
plate's thickness and the same profile, on both sides from `halfD` to `outerH`, regardless of the zones. No index is
painted (red unchanged).

Examples:
- A plate of Thickness 64, Coating 32, Mount off (the defaults until file version 4): `halfT = 32`, `ramp_z = 32`; in a block, `|x'| = 16`: share
  0.25, blue 24; `|x'| = 0.5`: `(31.5/32)^2 = 0.969`, blue 93.
- Example 11 (Diameter 600, Thickness 16, Coating 0, DesignFrequency 160, FocalLength 500, Mount on, MountWidth 107,
  scale 2): `lam = 2*pi*255/160 = 10.01383`, `halfT = 8`, `ramp_z = 0` (share 1 everywhere it is not 0, blue 96),
  `halfD = 300`, `outerH = 407`. Zone radii `r_n = sqrt(n*lam*f + (n*lam/2)^2)`: 70.94, 100.57, 123.48, 142.93,
  160.19, ...; 17 zones within 300. Rows with `|y'| < 70.94` are open (zone 0), `70.94 <= |y'| < 100.57` blocked,
  alternating; `300 < |y'| <= 407` is the mount (blue 96 over the full 16 px thickness).

### (d) Logic gate absorbers

Side band in the cladding's outer part and, for a switched-off output, the output ramp; strength 64. Exact formulas
and an example in `21-logic-gate-design.md` ("Absorbers painted").

### (e) Damping band and rim (scene, not a part)

`Scene.DampingEdge` is not painted into blue; `SceneData` overwrites `d` directly (formulas in "SceneData" above).
As a function of the cell's ring index `i = min(gx, gy, GW-1-gx, GH-1-gy)` (distance in whole cells from the grid's
outer ring):
```
rim (DampingEdge < 0, Rim = -DampingEdge), i < Rim:        d = (f32)(1 - 0.5*((Rim - i)/Rim)^2)
band (DampingEdge > 0, Rim = 0), border = min(DampingEdge, min(W, H)/2) (integer division), i < border:
                                                            d = (f32)sqrt(i/border)
```
Equivalent blue `255*(1 - d)` (not quantised); any blue in those rings is overwritten.
