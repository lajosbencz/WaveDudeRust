# 30 - Engine

Finite-difference solver of the damped scalar wave equation `u_tt = c^2 * lap(u)` with dx = dt = 1 (leapfrog),
soft sources, Mur open boundary or reflective walls, and a two-stage EMA intensity. The CPU engine
(`Engine/WaveSimulator.cs`) is the reference; the GPU engine (`Engine/GpuWaveSimulator.cs`, OpenCL) matches it within
tolerance. All field arithmetic is f32 unless stated. No FMA contraction.

## Inputs

| Input | From |
|---|---|
| `SceneData` | grid size `GW x GH`, `c2[]`, `d[]`, image sources (x, y, level), rim, open-edges flag (see 20) |
| `FourthOrder` | `Simulation.FourthOrderLaplacian` |
| `SourceFrequencyScale` | `Simulation` |
| `ImageSourceScale` | `Scene` (image source amplitude) |
| `TimeDecay`, `IntensityScale` | `Simulation` |
| `TrackIntensity` | intensity layer previewed, or any active non-Amplitude probe, or an export needs it |
| Emitters | from sources (see 10); positions in canvas px, shifted by +Rim into the grid |
| Probe sampler | probe cell indices (see Probes) |

## Buffers and indexing

```
Stride = GW + 2                   (= W + 2 + 2*Rim)
rows   = GH + 2
cell (gx, gy) of the grid -> index (gy + 1)*Stride + gx + 1
canvas (x, y)             -> grid (x + Rim, y + Rim)
```
Buffers: `u_prev`, `u`, `u_next` (or two swapped), intensity stages `s1`, `s2`, per-cell `c2`, `d` (padded copies).
The padding ring is 0 for reflective edges and set by the Mur condition for open edges.
Emitters whose position falls outside the canvas are dropped at SetEmitters.

## Sub-steps

```
maxC2 = max over the grid c2 (f32; 0 if none positive)
subSteps = FourthOrder && maxC2 > 0 ? max(1, (int)ceil(sqrt((f64)maxC2 * (f64)(32f/3f) / 2.0))) : 1
if subSteps > 1 (n = subSteps):
  d_cell = (f32)pow(d_cell, 1.0/n)  per cell
  mult   = 1f / (n*n)
else mult = 1f
```
c2 = 1 (index 1) gives n = 3. Re-derived when the scene is replaced.

## Laplacian

9-point (`FourthOrder = false`), every grid cell:
```
lap = 0.066f*(u[i-S-1] + u[i-S+1] + u[i+S-1] + u[i+S+1])
    + 0.184f*(u[i-S] + u[i+S] + u[i-1] + u[i+1])
    - u[i]
```
4th order (`FourthOrder = true`), cells with `1 <= gx <= GW-2` and `1 <= gy <= GH-2`:
```
lap = (-1f/12)*(u[i-2] + u[i+2] + u[i-2S] + u[i+2S])
    + (4f/3)*(u[i-1] + u[i+1] + u[i-S] + u[i+S])
    + (-5f)*u[i]
```
The outermost grid ring (gy = 0, gy = GH-1, and gx = 0, gx = GW-1 of every row) uses the 9-point stencil.
Summation order is exactly as written (parenthesised sums left to right, then the products added left to right);
constants are f32 literals `0.066f`, `0.184f`, `4f/3f`, `-1f/12f`, `-5f`. Every cell reads only the current `u`, so
cells may be processed in any order or in parallel.

## Update

```
next[i] = center + (center - prev[i]) * d[i] + lap * c2[i] * mult        (left to right, f32)
```
`next` is written over `prev[i]`; the two buffers then swap roles (two field buffers, not three).
```
```

## Step (AdvanceOneStep)

```
t = time                         (f64, steps done so far)
ApplySources(t)
for k in 0..subSteps-1:
  UpdateField (intensity only on the last sub-step): writes next over prev
  if open edges: ApplyOpenEdges(old = u, next)
  swap u and prev
time += 1
```

## Sources

Constants: `SourceOpacity = 0.9`, `DetachRampSteps = 30`.

Image sources (green pixels):
```
omega[level] = level/255.0 * 0.5 * SourceFrequencyScale            (f64 table, 256 entries)
value[level] = (f32)(sin(omega[level]*t) * ImageSourceScale)        (once per used level per step)
u[cell] = u[cell]*0.9f + value[level] * (1f - 0.9f)                 (f32; 1f - 0.9f is 0.100000024f)
```
Object emitters:
```
f = (f64)(f32)(Level/255.0 * 0.5 * SourceFrequencyScale)          (rounded through f32, used as f64)
fade = 1
if DetachAfterStep >= 0:
  since = t - DetachAfterStep                                      (f64)
  if since >= 30: skip emitter
  if since > 0: fade = (f32)(1 - since/30)
value = (f32)(sin(PhaseRadians + f*t) * Amplitude * envelope) * fade     (f64 inside, then f32 * f32)
u[cell] = u[cell]*0.9f + value*(1f - 0.9f)
```
Image sources are applied first (in collection order), then object emitters (in list order); a cell shared by several
emitters gets each blend in turn. CPU computes `sin(...)` in f64; GPU in f32.

Envelope (continuous: 1):
```
on = OnWidthCycles ; off = OffWidthCycles ; period = on + off
if period <= 0: envelope = 1
(on, off, period are ints; pos, cycleClock f64)
cycleClock = f*t / (2*pi)                        (phase not included)
non-random gap (RandomizeOffWidthAmount <= 0): starts at the burst
  if period <= 0: pos = 0, isFirst = true
  adj = cycleClock + off
  isFirst = floor(adj/period) == 0
  pos = adj - period*floor(adj/period)
random gap (amount a > 0): starts in the off span
  state per emitter, reset by Reset/SetEmitters: anchor = 0.0, curOff = OffWidthCycles, rng = RngSeed
  off = curOff ; period = on + off ; if period <= 0: pos = 0, isFirst = true
  since = cycleClock - anchor ; guard = 0
  while since >= period and guard < CycleAdvanceGuard:
     anchor += period ; since -= period
     rng = xorshift32(rng)
     off = max(1, OffWidthBase + (int)(rng % (u32)(2a+1)) - a) ; curOff = off
     period = on + off ; guard += 1
  if since < 0: since = 0
  isFirst = anchor == 0.0
  pos = since
onPos = pos - off
Gaussian:      envelope = onPos < 0 ? 0 : exp(-0.5*z^2),  z = (onPos - on/2) / max(on/6, 1e-6)
SmoothedSquare:
  if onPos < -off/2 and !isFirst: onPos += period
  envelope = (0.5 + atan(onPos/0.35)/pi) * (0.5 - atan((onPos - on)/0.35)/pi)
```
Random-gap state advances with simulated time and is reset by `Reset`. `CycleAdvanceGuard = 1000`.
The GPU kernel uses the same xorshift sequence.

## Intensity

```
lowest = min(lowest image level used in 1..255, lowest object emitter Level > 0); 100 if neither exists
tau    = max(1.0, TimeDecay*100/lowest)          (f64)
alpha  = (f32)exp(-1/tau)                         (recomputed by SetEmitters)
on the last sub-step, per cell:
  s1 = s1*alpha + next*next*(1f - alpha)
  s2 = s2*alpha + s1*(1f - alpha)                 (the updated s1)
  out = (s1*(1f - 0.5f) + s2*0.5f) * (f32)IntensityScale
```
All f32. The CPU writes `out` per cell on that sub-step; the GPU keeps s1, s2 and blends with the same expression
where it is read (captures, probes).

## Open edges (Mur first order)

After every sub-step, on the padding ring:
```
top/bottom, x in 0..GW+1:  ix = clamp(x, 1, GW)
  pair (ring = x,               inner = S + ix)
  pair (ring = (GH+1)*S + x,    inner = GH*S + ix)
left/right, y in 1..GH:
  pair (ring = y*S,             inner = y*S + 1)
  pair (ring = y*S + GW + 1,    inner = y*S + GW)
for each pair:
  s = sqrtf(c2[inner]*mult) * 0.56213877f
  k = (s - 1)/(s + 1)
  next[ring] = old[inner] + k*(next[inner] - old[ring])
```
`old` is the field before this sub-step. Reflective edges: the ring stays 0 (Dirichlet wall).

Measured reflection (long-wave pulse): open ring alone 0.05..0.09 %; reflective box 72 %. At wavelengths of a few
cells the Mur ring reflects almost everything; the default quadratic rim (32 px) absorbs alone. Rim factor table in 20.

## Engine operations

| Operation | Behaviour |
|---|---|
| Initialize(scene, options) | allocates, uploads c2, d, sources; logs grid, canvas, rim, edges, stride, stencil, sub-steps, work groups |
| Reset | field, intensity, time, pulse states to 0 |
| SetEmitters(list) | replaces object emitters; recomputes alpha |
| SetProbes(sampler) | sets the probe cell lists |
| TryReplaceScene(scene) | same grid size, edges and image-source count: swaps c2/d/source tables in place, re-derives sub-steps; else returns false (caller rebuilds) |
| AdvanceOneStep | as above |
| SampleProbes | reads probe cells after the step (field or blended intensity), at once |
| QueueProbes / DrainProbes | queues that reading per step; drain hands the queued readings out in order. The GPU engine copies each step's readings into a batch buffer and reads a batch (32 steps) back at once, so recording probes keeps the step batching |
| CaptureFrame(shape, snapshot) | converts field and/or intensity of the canvas (not the rim) into planes of `FrameShape` (see 40) |
| PrepareFrame | host-side full float copy of field and blended intensity (tests) |

One driver thread per engine; no internal locking.

## GPU engine (optional in Rust)

- Scene as compact maps: u8 index into a c2 table (<= 256 distinct values), u16 index into a damping table (<= 4096
  distinct values, else rebuild); tables hold the CPU's f32 values.
- Work groups 32 x 8, global size rounded up to whole groups; kernels: `wave_step`, `apply_sources`,
  `apply_object_sources`, `open_edges`, `capture_half`, `capture_index`.
- Intensity stores s1 and s2; blend at read time.
- `capture_index` rounds f32 to half with bit-level round-to-nearest-even before indexing, so CPU and GPU index frames
  agree. Build option `-cl-fp32-correctly-rounded-divide-sqrt` when the device supports it.
- Subnormals: the card may flush; host blend differs by 1 ulp at 1e-45 in about 1/2000 values.
- Parity: library_check compares CPU vs GPU within `1e-3 * max(1, max|cpu|)` after N steps. `TODO(verify)`: CLAUDE.md
  claims bit-for-bit captured planes; treat bit-exactness as a stretch goal, the tolerance as the requirement.

## Probes (`Probes/ProbeSampler.cs`, `ProbeCollector.cs`)

- Positions clamped into the canvas, then shifted by `Rim*Stride + Rim`.
- Point probe: one cell. Line probe: Bresenham from A to B, both ends inclusive, A first.
- Layer Field: sample `u`. Layer Intensity: sample the blended intensity.
- `UseIntensity = Measurement != Amplitude`.

Collection during a run:
| Probe | Recorded |
|---|---|
| Point, Amplitude/Value | one value per step, after each step (series index i = value after step i+1) |
| Point, IntegratedValue | running sum of the intensity, held at 0 while `step < StartAfter`; sample j of a batch belongs to step `batchStart + j + 1` |
| Line, Amplitude/Value | the live profile at each timeline frame (when `i % VisualizeEvery == 0`) |
| Line, IntegratedValue | per-pixel f64 totals, added each step once `round(time) >= StartAfter`; profile per timeline frame |
| Counting detector | one value per step: the events of the probe's Poisson process (`engine/poisson.rs`, seed `project.seed`, stream `StableHash(Name) << 24`) up to the line's integrated intensity summed over its pixels times `Sensitivity`, less those counted before (a series, as a point probe's; 0 before `StartAfter`) |
| Imaging detector | a profile per timeline frame: at every pixel the events of that pixel's process (stream `(StableHash(Name) << 24) ^ pixel`) up to its integrated intensity times `Sensitivity`, so the image only grows; the detections of a bin of `BinSize` pixels added up and shown at each of them, the last bin shorter |

A Poisson process is unit-rate on the half-line, walked from 0 with exponential gaps `-ln(1 - NextDouble)`: the count
at a position is Poisson with that mean, and asking step by step or once gives the same count. A span of more than
100000 expected events in one call is jumped with a normal approximation (Box-Muller) of its count.

Line length (graph x axis) = Euclidean length of the clamped A-B; sample k of n at distance
`n > 1 ? k*length/(n-1) : 0` (as in the CSV header).

## The live run

One worker owns one engine and one frame store and converges on a goal: the run request (a copy of the
project, the quality, whether the intensity is tracked), the timeline position, how much is wanted (the
position's frame, or every frame), a pause, a fit request. The worker builds the spec from the request, so
the scene is composed on its thread, never on the UI's; the UI derives what it shows meanwhile (the schedule,
the store's size, the probes) from the request's project. The project's image bytes are shared, so the copy
is cheap. Between frames it takes the next
target: the first stored frame at or after the position that is missing or whose probe traces are not recorded
(every frame wanted: then the probes' last steps while the engine stands at the traces' end, then the frames
before the position still missing, then the probes' last steps from wherever the engine stands). It steps to it, reset to 0 when the engine passed it; `k*CaptureEvery + 1`
steps for stored frame k, probes queued every step and drained every 256 steps and at the end; with every frame
wanted the missing stored frames passed on the way are captured too, so one pass makes the run; a capture is
pipelined with the steps that follow it. A goal change is read every 16 steps: a unit of work for another spec is left; one for the goal's spec goes
on when it is the goal's frame, else while every frame is wanted and the goal's frame is there; a pause with
another spec leaves the target so the spec is adopted. A fit frame is made in a state of its own (`Fitting`),
which leaves the run's frames and its completion as they are. A spec with other
frames (scene, options, emitters, seed, schedule, quality, brightness when baked into indices) starts a
generation: the store is cleared, its buffers kept, the frame written last kept readable with the traces it was
recorded with (when the probes measure the same); the engine is kept
across scene edits of the same grid (`TryReplaceScene`), rebuilt otherwise. A spec with other probe measurements
alone clears the traces and keeps the frames. Specs share the scene medium while the scene, its parts and edges
are unchanged (`SceneCache`, shared with the outline painter), so comparing two specs' scenes is a pointer
compare. A request that cannot be built (an image scene without its image) fails the run, which the status
bar reports. A fit request yields the
goal's frame once more in f16 samples, served once.
