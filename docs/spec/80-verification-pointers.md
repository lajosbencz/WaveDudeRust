# 80 - Verification pointers and golden fixtures

Source: `graealex/WaveDudeNext` at `08f9e63`. Paths relative to its root; the library is `WaveDudeNext/`.

## C# files per spec file

| Spec | C# files |
|---|---|
| 00 | `CLAUDE.md` (architecture, numbers), `OpticalParts.md`, `MANUAL.md` |
| 10 | `Model/Project.cs`, `Scene.cs`, `Simulation.cs`, `Display.cs`, `Source.cs`, `Probe.cs`, `OpticalPart.cs`, `INamed.cs`, `Pcg32.cs`, `Engine/Emitter.cs`, `Rendering/ColorScheme.cs`, `Runs/PreviewQuality.cs`, `WaveDudeNext.WPF/Settings.cs` |
| 20 | `Parts/PartPainter.cs`, `Parts/PartShape.cs`, `Parts/LogicGateDesign.cs`, `Parts/WaveguideModes.cs`, `Parts/GatePortFollower.cs`, `Model/OpticalPart.cs` (geometry), `Rendering/GlowingEdges.cs`, `Engine/SceneData.cs`, `Runs/ScenePixels.cs`, `WaveDudeNext.WPF/MainWindow.Live.cs` (`LoadSceneImage`, composition cache) |
| 21 | `Parts/LogicGateDesign.cs`, `Parts/PartShape.cs` (`LogicGateShape`), `Model/OpticalPart.cs` (`LogicGatePart`, kinds), `Parts/GatePortFollower.cs`, `_tools/library_check/Program.cs` (gate checks) |
| 22 | `Parts/WaveguideModes.cs`, `Engine/WaveSimulator.cs` (`ComputeSubSteps`) |
| 30 | `Engine/WaveSimulator.cs`, `Engine/GpuWaveSimulator.cs` (kernels as strings), `Engine/EngineOptions.cs`, `Engine/IWaveEngine.cs`, `Engine/WaveEngineFactory.cs`, `Probes/ProbeSampler.cs`, `ProbeCollector.cs`, `ProbeSeries.cs`, `ProbeTrace.cs`, `Runs/ScratchRunner.cs` |
| 40 | `Runs/RunSpec.cs`, `SimulationRun.cs`, `FrameStore.cs`, `FrameMemory.cs`, `FrameCache.cs`, `StridedFrameSource.cs`, `PreviewDefaults.cs`, `MachineFacts.cs`, `StorageDevice.cs`, `Engine/FrameShape.cs` (FrameConversion), `Engine/FrameSnapshot.cs`, `Rendering/ColorLut.cs`, `FrameRenderer.cs`, `ScreenRenderer.cs`, `BrightnessFit.cs`, `SampleKind.cs` |
| 50 | `Export/*.cs`, `Probes/ProbeGraph.cs` (box sizes, axes), `WaveDudeNext.WPF/MainWindow.Export.cs` (range, naming, run selection, decorations) |
| 60 | `Model/Project.cs` (Save/Load/migration), `ProjectSchema.cs` (DTD), `LegacyProject.cs`, `UnreadableFileException.cs`, `Rendering/ColorSchemeStore.cs`, `Runs/FrameStore.cs` (spill header) |
| 70 | `_tools/library_check/Program.cs`, `_tools/library_check/ExportChecks.cs` |

Additional readings: `Examples/*.wdproj` (24 projects), `_tools/ui_check/figures.wdproj` (+ `figures_scene.png`),
`_tools/legacy_examples/` (Wave Dude 2D projects for the importer), `WaveDudeNext/ColorSchemes/*.wdscheme`.

## Golden fixtures to generate from the C# app

Generate with a small console harness referencing the C# library (pattern: `_tools/library_check`), on Windows or any
.NET 10 host (CPU engine; the GPU is not needed). Store under `WaveDudeRust/tests/golden/` with a manifest
(commit hash, parameters). Binary floats little-endian.

| ID | Fixture | Content | Used by |
|---|---|---|---|
| G-01 | Engine traces | For each case: scene pixels (PNG), options, emitters; field `u` and intensity (f32 raw) of the canvas after steps 1, 2, 10, 100, 300. Cases: blank 64x48 9-point reflective; same 4th order open rim 8; band 6; image sources; continuous, Gaussian-pulsed, smoothed-square pulsed, random-gap composite, detaching source; index 2.55 region and a mirror region. | 30, 70 |
| G-02 | Painted scenes | For every example project and `figures.wdproj`: the composed BGRA scene pixels (PNG, lossless) after the painter. Plus each part kind alone on 200x200 at angles 0, 17, 45, 90 deg, coatings Layer/Graded. | 20, 70 |
| G-03 | Pcg32 / hashes | `StableHash` of 20 names (ASCII, non-ASCII, empty); first 16 `NextUInt32` for 5 (seed, stream) pairs; `Next(-5, 5)` sequences; xorshift32 of 10 seeds. | 10 |
| G-04 | Emitters | For each example project: the emitter list (x, y, level, amplitude, phase, on, off, base, amount, detach, seed). | 10, 30 |
| G-05 | Frame conversion | For a G-01 state: planes for divisors 1, 2, 4 (non-multiple canvas 67x45 too), Half and Index, root and I. | 40 |
| G-06 | Logic gate geometry | For each gate kind at 3 design frequencies, 2 port spacings, 2 angles: the painted 400x300 scene and `Ports()`. | 20 |
| G-07 | LUTs | Palette (255 BGRA) and half table (65536 BGRA) for Default field, Default intensity, Inferno, Grey at black levels -0.05, 0, 0.5 and brightness 1, 30, kinds Field/Intensity/IntensityRoot; deep table for one case; Bayer thresholds for 10..16 bits. | 40 |
| G-08 | Legacy import | `_tools/legacy_examples/*` imported and saved as `.wdproj` (DTD included), plus the notes list. | 60 |
| G-09 | Project round trip | Every example and `figures.wdproj` loaded and re-saved (with DTD): the bytes. The DTD text alone as a separate file. | 60 |
| G-10 | Brightness fit | 10 sample sets (field, intensity, root; with NaN, zeros, negatives) and their fits. | 40 |
| G-11 | Screen renderer | A 1920x1080 f16 plane resampled to 914x514, 640x360, 3000x1700 (enlarge), plain and dithered 10/16 bits. | 40 |
| G-12 | Exports | Example 16 short run (200 steps): PNG still, PNG sequence of 5 frames, JPEG q 85, MPEG-1, AVI, MOV rle, MOV png, EXR (I and root), CSV of every probe; with probe markers, graph boxes, source markers on and off. | 50 |
| G-13 | Glowing edges | Edge picture for 3 scenes (noise 257x131, Example 16, figures). | 20 |
| G-14 | Lens fit | `FitRadii` input/output for 30 (D, R1, R2, T) tuples including the library_check cases. | 10 |
| G-15 | Fibre flattening | Flattened point lists for 10 node/handle sets. | 20 |

Comparison rules: G-01 bit-exact for the CPU engine (tolerance only for a GPU path); G-02, G-05, G-07, G-10, G-11,
G-13 byte-exact; G-12 decoded pixels exact for lossless formats, container structure per 50, lossy formats within a
PSNR bound `TODO(verify)` (choose, e.g. >= 45 dB vs the C# output).

## Notes for the fixture harness

- Force the CPU engine (`WaveEngineFactory` with GPU off) and Release build (library always optimised).
- Pin culture to invariant; record `Vector<float>.Count` (lanes are bit-identical by design, record anyway).
- Use fixed project seeds; a new project's seed is random.
- Example projects load without DTD; saving adds one (G-09 records the saved form).
