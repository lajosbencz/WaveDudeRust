# 00 - Overview

Spec of the non-UI behaviour of WaveDude Next (C#, .NET 10 library `WaveDudeNext/`), the source of truth for the
Rust core and export crates. UI behaviour is specified elsewhere ([`docs/ui`](../ui/)).

Source tree read: `graealex/WaveDudeNext` at commit `08f9e63` (version 0.1.3). C# names in backticks are pointers for
verification only (see [`80-verification-pointers.md`](80-verification-pointers.md)).

## What the program is

A 2D scalar wave-equation simulator. The user builds a scene (a blank canvas or an RGB image whose channels encode
refractive index, image sources and damping), places object sources, optical parts (lenses, mirrors, shapes, fibres,
Bragg stacks, GRIN lenses, logic gates, zone plates) and probes, runs a finite-difference simulation on CPU or GPU,
scrubs the recorded frames of two layers (field and intensity), and exports videos, image sequences, OpenEXR data or
probe CSV. Successor of Jeroen Vleggaar's Wave Dude 2D (Python/VB), whose numerics follow 0x23's WaveSimulator2D.
License GPL-3.0-or-later.

## Pipeline

```
Project (model, .wdproj)
  |  Scene image decode (or blank canvas) + PartPainter (enabled parts, list order)
  v
Scene pixels (BGRA8, canvas W x H)          red = index, green = image sources, blue = damping
  |  SceneData.FromBgra / SceneData.Empty (+ damping band or rim)
  v
SceneData (grid = canvas + 2*Rim per axis): c2[], d[], image-source list
  |  + EngineOptions (from Simulation, Scene) + Emitters (from Sources, Pcg32) + ProbeSampler (from Probes)
  v
Engine (CPU or OpenCL): AdvanceOneStep per step; SampleProbes per step; CaptureFrame every CaptureEvery steps
  |
  v
FrameStore: per stored frame one plane per layer (field, intensity), FrameShape (divisor, f16 or u8 index)
  |                                   \
  v                                    v
Renderer (ColorLut, brightness)       ExportJob: render -> outline -> source markers -> probe markers/boxes -> sink
  |                                    (OpenEXR sinks take the raw f16 planes, undecorated)
  v
Preview (8-bit: palette at store size; 16-bit: ScreenRenderer at device size, optional Bayer dither)
```

Side paths:
- The live run: one engine and one store converging on the project as it is now and the frame looked at; the
  frame looked at comes first, the rest when wanted ([spec/30](30-engine.md) The live run).
- Probe collector: point probes one sample per step, line probes one profile per timeline frame.

## Crate mapping (proposed)

| Concern | Spec file | Crate |
|---|---|---|
| Model, file formats, importer, Pcg32 | 10, 60 | core |
| Scene pixels, part painter, glowing edges | 20 | core |
| Logic gate design, waveguide mode solver | 21, 22 | core |
| Engine (CPU reference, optional GPU), probes | 30 | core |
| Run, frame store, preview quality, renderer, colour LUTs, brightness fit | 40 | core |
| Encoders and sinks, export job, CSV | 50 | export |

## Glossary

| Term | Meaning |
|---|---|
| canvas | The visible simulation area, `W x H` pixels = cells. Origin top-left, x right, y down. Positions in the project are integer canvas pixels. |
| cell | One grid point; dx = dt = 1, so a cell is a canvas pixel and a step is the time unit. |
| rim | Absorbing frame outside the canvas, `Rim = max(0, -DampingEdge)` cells per side; simulated, never shown or stored. |
| damping band | Absorbing frame inside the canvas for `DampingEdge > 0`; visible. |
| grid | Simulated area: `(W + 2*Rim) x (H + 2*Rim)` cells. Canvas cell (x, y) is grid cell (x + Rim, y + Rim). |
| padding / ring | One extra cell around the grid in the engine's buffers; 0 for reflective edges, Mur open boundary for open edges. |
| stride | Row pitch of the padded buffers: `Stride = W + 2 + 2*Rim`. Canvas (x, y) is at `(y + 1 + Rim)*Stride + x + 1 + Rim`. |
| step | One time step (`AdvanceOneStep`): sources blended, then `subSteps` leapfrog sub-steps. |
| sub-step | 4th-order stencil only: n smaller leapfrog steps per step for stability (`c2/n^2`, `d^(1/n)`). |
| VisualizeEvery | Steps per timeline frame (project setting, default 2). |
| frame | Timeline frame k = the state after the (0-based) step with index `k*VisualizeEvery` has completed, i.e. after `k*VisualizeEvery + 1` steps. `FrameCount = ceil(Steps / VisualizeEvery)`. |
| stored frame | With a frame divisor F only every F-th timeline frame is captured; timeline frame k shows stored frame `k / F` (integer division). |
| layer | `Field` (signed displacement u) or `Intensity` (two-stage EMA of u^2, blended, scaled). |
| sample kind | How a stored 16-bit intensity plane is meant: `Field`, `Intensity` (I itself), `IntensityRoot` (sqrt I, the preview's default). |
| FrameShape | Geometry and format of captured planes: canvas size, resolution divisor (1/2/4), format (`Half` = f16, `Index` = u8 palette index), per-layer brightness baked into index frames, `IntensityRoot` flag. |
| preview quality | App setting `(ResolutionDivisor, FrameDivisor, Format, IntensityRoot)`; never changes the simulation, only what is captured and stored. Exports always use `Full = (1, 1, Half, root)`. |
| source / object source | A project `Source` (point or composite array) expanded into `Emitter`s. |
| image source | A scene pixel with green > 0 emitting at frequency level = green, when `Scene.ImageSources` is on and the scene is an image. |
| level | Frequency on Jeroen's 0..255 scale; angular frequency `omega = level/255 * 0.5 * SourceFrequencyScale` rad/step. |
| part | An `OpticalPart` painted into the scene pixels before a run. |
| probe | Point or line measurement of field amplitude, intensity value or integrated intensity. |
| live run | The run behind the preview: the project as it is now, the frame looked at first, the rest when wanted. |
| palette | 255 colours (index 0..254) taken from a colour scheme gradient at `i/254`. |
| half table | 65536 colours, one per f16 bit pattern, gradient evaluated at the value's own position. |
| glowing edges | Sobel edge picture of the scene pixels, overlaid on preview, optionally added into exports. |
| spill file | Temp file holding the frames beyond the RAM budget. |

## Cross-cutting conventions (load-bearing for bit-exactness)

- **Rounding**: C# `Math.Round(x)` and `Math.Round(x, digits)` without a mode are round-half-to-even. Rust `f64::round`
  is half-away-from-zero; use `round_ties_even` wherever this spec says "round (even)". Places that use
  `MidpointRounding.AwayFromZero` are marked "round (away)".
- **Casts**: C# `(int)x` on a double/float truncates toward zero; `(byte)` of an in-range int is the low byte.
- **Float widths**: each formula states f32 or f64. The CPU engine's field arithmetic is f32; source values are
  computed in f64 then cast to f32. No fused multiply-add anywhere in the engine (GPU uses `FP_CONTRACT OFF`).
- **Angles**: degrees, counter-clockwise on the screen (y down). Rotation of a part-frame offset (x', y') to canvas:
  `X + x'*cos(a) + y'*sin(a), Y + y'*cos(a) - x'*sin(a)`; inverse (canvas to frame):
  `x' = ox*cos - oy*sin, y' = ox*sin + oy*cos` with `(ox, oy) = canvas - centre`. Normalised into (-180, 180]:
  `a - 360*ceil((a - 180)/360)` when outside; non-finite -> 0.
- **Pixel geometry**: pixel (x, y) spans [x, x+1) x [y, y+1); its centre is (x + 0.5, y + 0.5). A part's centre
  (X, Y) is the top-left corner of pixel (X, Y), so even sizes cover whole pixels.
- **Invariant culture** for all numbers written to files and logs (decimal point).
- **Randomness**: only through `Pcg32` seeded from `Project.Seed` (and xorshift32 per emitter for pulse gaps);
  generated data (emitter elements) is never persisted.
- **Logs**: Debug for decisions, Info and up reach the status bar; numbers invariant. Every engine build logs grid,
  canvas, rim, edges, stride, stencil/sub-steps. A malformed project/scheme/settings file is quoted in the error's
  log entry (first 64 K chars). Not normative for the Rust core beyond "log enough to diagnose".
