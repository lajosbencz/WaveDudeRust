# 40 - Runs, frames, store, rendering

## Run schedule (`Runs/RunSpec.cs`, `Runs/SimulationRun.cs`)

`RunSpec` is an immutable snapshot of the project plus scene pixels plus preview quality at run start.

```
VE            = Simulation.VisualizeEvery
FrameCount    = ceil(Steps / VE)
FrameDivisor  = quality.FrameDivisor (1, 2, 4)
CaptureEvery  = VE * FrameDivisor
StoredFrames  = ceil(FrameCount / FrameDivisor)
StoredIndexOf(timeline frame k) = k / FrameDivisor          (integer division)

for i in 0..Steps-1:
  engine.AdvanceOneStep()
  if probes: engine.SampleProbes()                 (point probes, integrated line totals)
  if i % VE == 0: collect line profiles, flush point series
  if i % CaptureEvery == 0: capture frame (Step = i) into the store (it holds StoredFrames)
```
Frame k holds the state after step index `k*VE` (that is after `k*VE + 1` steps). Steps of a run are numbered by their
0-based index in exports and in/out points (`ExportFrom`, `ExportTo` are step numbers).

Layers captured: Field always; Intensity when the engine tracks it (`TrackIntensity`: intensity layer previewed, any
active non-Amplitude probe, or an export of the intensity).

Run state progression: Created -> Running -> (Paused) -> Completed | Cancelled | Failed (Rust: the typed error, the
frame store's, the engine's, a thread that could not start, or a panic with its message). A completed run's frames are
immutable. A simulation edit replaces the frames (Rust: the live run's store is cleared and refilled, [spec/30](30-engine.md) The live
run); an export needs the frames of the project as it is.

Rust's frame store takes frames by index in any order and tracks which are present (a per-frame flag and the
contiguous prefix); a cleared store keeps its buffers for the next generation and the frame written last.

Export run rule: an export needs a run with `Full` quality (divisor 1, every frame, f16 samples, current, complete);
otherwise a fresh full-quality run is made first. `.exr` of I needs a run with `IntensityRoot = false`.

## FrameShape (`Engine/FrameShape.cs`)

| Field | Meaning |
|---|---|
| CanvasWidth, CanvasHeight | canvas px |
| Divisor | 1, 2, 4 |
| Format | `Half` (0, f16 per sample) or `Index` (1, u8 palette index) |
| FieldBrightness, IntensityBrightness | baked into Index frames (from Display at run creation) |
| IntensityRoot | true: an f16 intensity plane holds sqrt(I); false: I |

```
Width  = ceil(CanvasWidth / Divisor)
Height = ceil(CanvasHeight / Divisor)
PlaneBytes = Width * Height * (Format == Half ? 2 : 1)
```
Rows are top to bottom, no padding.

## Frame conversion (`Engine/FrameConversion.cs`)

Input: the engine's f32 field `u` and intensity `I` of the canvas (rim excluded).
```
block (bx, by) covers canvas x in [bx*div, min((bx+1)*div, W)), same for y   (clipped at right/bottom)
field value     = mean of u over the block                              (f32)
intensity value = mean of I over the block (energy mean)               (f32)
Half format:
  field      -> (f16)value                   (round to nearest even)
  intensity  -> IntensityRoot ? (f16)sqrtf(mean) : (f16)mean
Index format (b = the layer's brightness, f32):
  FieldIndex(v)          = isNaN(h) ? 127 : (int)(clamp(h*b, -1, 1)*127 + 127)        with h = (f32)(f16)v
  IntensityRootIndex(r)  = h > 0 ? (int)(min(h*b, 1)*254) : 0                          with h = (f32)(f16)sqrtf(I)
  IntensityIndex(I) (IntensityRoot = false) = I > 0 ? (int)(min(sqrtf(I)*b, 1)*254) : 0
```
The index of an Index frame equals the palette index of the f16 value a Half frame would store.
Block mean: f32 sum in row-major order over the block's cells, divided by the clipped block's cell count
(`rows * cols`, not div^2). Divisor 1 copies the cell value.

## Frame store (`Runs/FrameStore.cs`, `Runs/FrameMemory.cs`)

Capacity = StoredFrames. Per frame one plane per stored layer in order Field, Intensity.

Memory budget:
```
available = min(free physical memory, available commit)        (Rust: the OS's available memory, sysinfo)
budget    = max(0, available + reclaimed - 2 GiB)
if FrameMemoryPercent > 0: budget = min(physical * pct/100, budget)
reclaimed = bytes of the outgoing run's store (its release is awaited before allocating)
FrameBytes = layers * PlaneBytes
RamFrames  = min(Capacity, budget / FrameBytes)
StoreBytesAt(q) = layers * PlaneBytes(div, format) * ceil(FrameCount / q.FrameDivisor)
```
Frames `0..RamFrames-1` live in native memory; the rest in a spill file in `%LOCALAPPDATA%\WaveDudeNext\cache`
(delete on close; Rust: an unlinked file in the OS cache dir's `frames` folder, so a killed process leaves nothing;
named `.wdfs` leftovers of earlier builds are removed when the worker starts). No memory mapping.
In the browser (Rust only) the budget is the frames' memory, min(device memory, 2 GiB) (1 GiB when the browser does
not tell), or its FrameMemoryPercent share when > 0; the frames past it spill to the origin private file system, one
file per frame, written and read asynchronously (decision 0020). A spilled frame stays in memory while its write is
in flight (the run waits while 4 are) and while it is the frame written last; a spilled frame being read back keeps
the picture shown before until it is there; during playback the frames it reaches next are read back ahead.
Spilling is refused, failing the run at its start, when the frames past memory exceed the free storage estimated
when the page loaded, or the browser has no such file system; a write refused for quota fails the run.

Spill file header (64 bytes, little-endian i32 each):

| Offset | Field |
|---|---|
| 0 | magic `0x53464457` ("WDFS") |
| 4 | version 3 |
| 8 | Width (stored) |
| 12 | Height (stored) |
| 16 | layers flags (Field = 1, Intensity = 2) |
| 20 | VisualizeEvery |
| 24 | Capacity |
| 28 | first frame in file |
| 32 | format (Half 0, Index 1) |
| 36 | divisor |
| 40 | canvas width |
| 44 | canvas height |
| 48 | intensity root (1/0) |
| 52..63 | zero |

Then frames `first..Capacity-1`, each `layers * PlaneBytes`, planes in layer order. Plane offset:
`64 + (frame - first)*FrameBytes + layerIndex*PlaneBytes`. The file is internal; compatibility is not required.

Readers: any thread may read a frame `< Count` while the single writer appends. `CopyLayer(frame, layer)` returns the
plane bytes. Rust: `read_layer` copies a plane into the caller's buffer, `with_layer_into` lends it (a frame in memory
shared with the reader, so no lock is held while it renders; a spilled one read into a caller's buffer reused across
calls, refused when a clear ran meanwhile).

Memory warning (`RunSpec.StoreBytesAt`): a preview run whose store exceeds the budget asks first (UI); levers are the
RAM cache to Auto and three quality steps (resolution / 2, frames / 2, 8-bit).

## Preview quality defaults (`Runs/PreviewDefaults.cs`)

| Tier | Condition (first match) | Quality (res, frames, format) | Dither bits |
|---|---|---|---|
| High | physical RAM >= 30 GiB | (1, 1, Half) | 16 |
| Low | RAM < 15 GiB, or cache drive with seek penalty, or no OpenCL GPU, or <= 4 hardware threads | (2, 2, Index) | 0 |
| Middle | otherwise | (1, 1, Index) | 0 |

Evaluated once when no settings file exists. Fixed defaults otherwise: (1, 1, Index), dither 0.

## Colour lookup (`Rendering/ColorLut.cs`, `ColorScheme.cs`)

Gradient evaluation `Evaluate(stops, pos)` in f64:
```
if pos <= first.At: first colour ; if pos >= last.At: last colour
else find segment [a, b] with a.At <= pos < b.At (binary search)
  t = (pos - a.At)/(b.At - a.At) ; channel = a + (b - a)*t       (f64, per channel 0..255)
```
Black level per channel (bl = DisplayLayer.BlackLevel):
```
Level(c) = clamp(c/255*(1 - bl) + bl, 0, 1)
```

Palette (8-bit frames), 255 entries:
```
raw[i] = Interpolate(stops, 255)[i]   = round_away(Evaluate(stops, i/254))  per channel
pal[i] = (byte)(Level(raw[i]) * 255)                                     (truncate)
```
Half table (f16 frames), 65536 entries indexed by the f16 bit pattern, per (scheme, black level, brightness b, kind):
```
v = (f64)(f16 bits)
pos = Field:          isNaN(v) ? 0.5 : (clamp(v*b, -1, 1) + 1)/2
      Intensity:      (v <= 0 or NaN) ? 0 : min(sqrt(v)*b, 1)
      IntensityRoot:  (v > 0) ? clamp(v*b, 0, 1) : 0                 (NaN -> 0)
colour = round_away(Level(Evaluate(stops, pos)) * 255) per channel
```
Deep table (dithering): `round_away(Level(...) * 65280)` per channel, packed `R<<32 | G<<16 | B` in a u64.
Missing scheme: `Grey` (stops 0 #000000, 1 #FEFEFE), black level 0.

## Frame renderer (`Rendering/FrameRenderer.cs`)

Stored resolution, BGRA8 output (alpha 255):
- Index frame: `pal[min(index, 254)]`.
- Half frame: `halfTable[bits]`.

## Screen renderer (`Rendering/ScreenRenderer.cs`, 16-bit preview only)

Resamples the f16 plane in value space to a target size (tw, th), then colours via the half table.
```
per axis, scale s = src/tgt:
  src == tgt: copy
  shrink (s > 1): target t covers [t*s, (t+1)*s); weight of source j = overlap length / s
  enlarge (s < 1): u = (t + 0.5)*s - 0.5, clamped to [0, src-1]; linear between floor(u) and floor(u)+1
rows first (horizontal pass), then columns; f32 arithmetic
IntensityRoot planes: square before resampling, sqrt after (energy blend)
result -> f16 (nearest even) -> half table
```
Dithering (preview only, `PreviewDitherBits` in {10, 12, 14, 16}):
```
e = bits - 8 ; side = 2^(e/2)        (10 -> 2, 12 -> 4, 14 -> 8, 16 -> 16)
Bayer(1) = [[0]]; Bayer(2n): for m = Bayer(n)[y][x]:
  [y][x] = 4m, [y][x+n] = 4m+2, [y+n][x] = 4m+3, [y+n][x+n] = 4m+1
threshold[y][x] = (2*Bayer[y][x] + 1) << (8 - e)
deep channel level L (0..65280): out = (L >> 8) + (((L & 0xFF)*2 >= threshold[py & (side-1)][px & (side-1)]) ? 1 : 0)
```
(px, py) are the output pixel's coordinates; the matrix is row-major `[y][x]`; the full low byte is compared.
Per channel: R from bits 32..47, G from 16..31, B from 0..15.

## Colouring on the GPU (Rust only)

Where a plane lives on the GPU, a render pass colours it there (`render::gpu`): the screen renderer and the frame
renderer above, then the outline blend, as on the CPU, from the CPU's own tables (half table; deep table and Bayer
thresholds; palette; each axis's first samples and weights; the outline's additions `min(255, trunc(e * gain))` per
pixel), uploaded when their inputs change. The plane is read as stored (packed words: two halves or four indices).
A half plane is coloured per picture pixel: the axes' sums rows first in f32 (squared and rooted for an intensity
root, the root correctly rounded), the bit-exact f16 rounding of the capture, then the lookup and dithering. An index
plane's stored samples take the palette plus the outline at the stored size and are scaled bilinearly between
sample centres, as a texture sampler scales the CPU's picture. A card that fuses the multiply-adds of the
resampling may pick a neighbouring table entry at an f16 rounding boundary; `tests/colour.rs` states the tolerance.

## Brightness fit (`Rendering/BrightnessFit.cs`)

Auto brightness for a frame (from f16 or f32 samples):
```
magnitude = Field: |v| ; Intensity: sqrt(max(0, v)) ; IntensityRoot: max(0, v)      (finite samples only)
n = count ; if n == 0: none
rank = min(n - 1, floor(0.995*(n - 1)))
top = rank-th smallest magnitude (selection)
if top <= 0: none
b = clamp(1/top, 1e-4, 1e5)
digits = 2 - floor(log10(b))
b = digits >= 0 ? round_even(b, digits) : round_even(b / 10^-digits) * 10^-digits
```
An Index frame is first recomputed as f16 by the live run (a fit frame at the preview's resolution).
