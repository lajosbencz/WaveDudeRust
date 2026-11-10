# 70 - Guarantees and performance targets

Port of `_tools/library_check` (about 500 checks in `Program.cs`, about 60 in `ExportChecks.cs`). Each item is a
property the Rust core must prove in its test suite. Items marked (G) need golden data from the C# app (see 80).

## Determinism and parity

- [ ] Same seed and project -> bit-identical field after 300 steps (CPU, run twice).
- [ ] CPU engine bit-identical to the C# CPU engine after N steps on golden scenes, both stencils, open and reflective,
      rim and band, with image sources and object sources (G-01).
- [ ] SIMD lanes (if used) bit-identical to the scalar path (field and intensity after 100 steps of a painted scene
      with two sources; 9-point and 4th order).
- [ ] GPU vs CPU within `1e-3 * max(1, max|cpu|)` after N steps; captured index bytes identical (optional GPU).
- [x] Probe sampling: the live run's series and profiles equal the batch run's bit for bit, after a wrap to the
      frames before the position too (tests/run.rs).
- [ ] Emitters: one for a single source; disabled source none; `Amplitude/100`, phase in radians; randomized composite
      gives non-zero seeds; another project seed gives other emitter seeds; decimal frequency/amplitude/phase reach the
      emitter as typed; level 120.5 differs from 120; a level of 0 becomes the least level (0.01).
- [ ] Pcg32 output sequence for (seed, stream) pairs equals the C# one (G-03).

## Scene and engine

- [ ] BGRA scene: green pixel -> one image source at its position and level; `CountSources` agrees.
- [ ] Red 1..99 is index 1 (clamp), red 0 is a mirror.
- [ ] Damping band of 6 on an 8x8+ canvas: corner d = 0, inside d = 1; rim of 32 on 103x67 gives a 167x131 grid.
- [ ] New project: Open edges, rim 32, version 4, Scene element omitted from the file.
- [ ] Version-3 file without OverlayInExport is off, without a zone plate's Thickness/Coating is 64/32; version 4 on, 32/0.
- [ ] Version-2 file with explicit -32 keeps it; version-3 file without the attribute is -32; version-1 is Reflective.
- [ ] Reflection (long-wave pulse): open edges alone < 0.2 % back; reflective box without damping returns it all;
      open + 32 px rim, 4th order < 0.5 %; 64 px quadratic rim < 0.3 %.
- [ ] Padding rows untouched for reflective edges; intensity accumulates; time counts steps.
- [ ] Sub-steps: c2 = 1 at 4th order -> 3; 9-point -> 1.
- [ ] `TryReplaceScene`: a replaced engine steps like a fresh one (both engines).
- [ ] Moving a source changes the field.
- [ ] A cavity of mirror parts keeps the wave after 300 steps (max |u| inside >> outside).

## Store, frames, quality

- [ ] Empty store reports its layout; `SizeOf` matches; write beyond capacity rejected; `CopyLayer` returns the f16 of
      every interior cell.
- [ ] Tiered store: N frames, budget for k -> k in memory, N - k in the file; every frame reads back from its tier;
      file deleted on dispose; a run within the budget creates no file.
- [ ] Budget: the outgoing run's memory counts as reclaimed; the percentage still caps it; the new store waits for the
      release before its first allocation.
- [ ] `FramesInBudget`: 129 full-HD frames of both planes per GB; eight times as many half-size index frames
      (1035 per GB).
- [ ] Shapes: 64x48 halves to 32x24, thirds to 22x16; an index sample is one byte.
- [ ] 16-bit intensity holds f16(sqrt I) by default, f16(I) when the shape says so; the field alike.
- [ ] Index shape: every index equals the palette index of the f16 a 16-bit frame of the same floats stores (field and
      intensity root).
- [ ] Reduced run (half resolution, index, frame divisor 2) of 100 timeline frames stores 50 frames of 32x24;
      timeline frames 98 and 99 both show stored frame 49.
- [ ] An export of pictures from an index store fails (needs 16-bit); a snapshot of another format is refused.
- [ ] `StoreBytesAt(q)` equals the store a run at q allocates.
- [ ] First-start tiers table (40) for every combination: >= 30 GiB High; else < 15 GiB, seek penalty, no GPU or
      <= 4 threads Low; unknown drive counts as no penalty; else Middle.

## Rendering

- [ ] Field index, grey scheme: 0 -> 127; 0.5 (b = 1) -> 190.5 -> palette truncates to 190; clamp -> 0 and 254.
- [ ] Intensity as I, grey: <= 0 -> 0; sqrt(0.25) -> 127; clamp -> 254.
- [ ] Palette of a stop scheme hits the stops at indices 0, 127, 254.
- [ ] Black level 0.5 lifts a 0 channel to 127 in the palette (truncate) and 128 in the half table (round away).
- [ ] The 16-bit table rounds to the 8-bit one within half a level per channel.
- [ ] Inferno dark start: the half table has every blue level 3..33 where the palette's first 16 entries skip some.
- [ ] Screen renderer at the samples' size, no dither: identical to the half-table picture; a crest beside a trough
      shrinks to the colour of zero.
- [ ] Glowing edges: the two columns beside a step light up (Sobel * 0.25 clamped at 255), flat area and outer ring
      black; lanes (if used) byte-identical to scalar on noise and on a painted scene.
- [ ] Brightness fit: f16 intensity as I takes its square root to the same brightness; result clamped to 1e5; empty or
      all-zero -> none.
- [ ] Schemes: >= 20 built-ins, XML, names without underscores, capitalised, file name = name; XML round trip keeps
      name, kind, 9 stops; saving reproduces the shipped file; author JSON reads.

## Model and files

- [ ] Every persisted number has a `[Range]` that `Normalize()` enforces (both bounds), except named dependent bounds;
      a persisted number clamped without a declared range fails the check (port as: a table of ranges tested against
      normalise).
- [ ] Angle normalisation: 540 -> 180, -180 -> 180, -190 -> 170, 190 -> -170.
- [ ] Project file embeds the image (base64); relative path resolution needs the folder; the path is no simulation
      change, the bytes are; RemoveImage leaves a blank default scene.
- [ ] One element per part kind, attributes only where they differ from defaults; parts between sources and probes;
      fibre nodes sparse (handles only where set, Smooth only when off); loaded parts keep every attribute.
- [ ] DTD: root children in written order, version required; child and text elements declared; the file with its DTD
      loads back with everything; every example as saved validates; misspelt attribute, wrong enum value and
      misplaced element are reported by a validator.
- [ ] Malformed project: the error names the problem (e.g. duplicate attribute), the logged quotation is the file; a
      missing file: the quotation says why.
- [ ] Import (legacy JSON, G-08): Example 12 name and description, no notes; image path kept absolute; simulation
      15000 steps every 2, 9-point, c 1, decay 10; display previews on, 60 fps, quality 70; probe 1 a line
      (1713, 2)-(1719, 1073), box (1, 426); round trip through XML; Example 5 canvas mode with missing image noted;
      pulsed 5/20 cycles, detach after 2000; use_probes honoured; Example 11 lattice without position enabled at
      (100, 100) with a note; a newer format version is refused.
- [ ] Clipboard fragments (if ported): parse back into items; unknown element refuses the whole fragment.
- [ ] History (if ported): one snapshot per step; undo restores the file and the image bytes.

## Parts and painter

- [ ] Disabled part leaves the image untouched; an enabled part paints a copy (input not modified).
- [ ] Biconvex lens D 100, R +-300, T 40: 40 px on the axis (red 150), 100 rows, edge thickness with sag 4 per side;
      turned 90 deg it is 40 px along y.
- [ ] Coatings: graded band identical on both surfaces and the rim; bare sides; front+sides coated, back bare; a
      circle with bare sides has the band at leftmost/rightmost points only.
- [ ] Mirror lens: red 0 by the centre test only.
- [ ] Radius fit: 100x20 lens with semicircular surfaces -> R +-130; a 2000x40 lens gets radii 25010; ShapeOf flat
      threshold on 104 px between R 2704 and 2705.
- [ ] Hexagon flat faces at +-x' (26 px out), vertices at +-y' (30 px); rectangle 60x20 at x 70..129, y 50..69;
      circle of 60 has 63 vertices all 30 px out; equilateral triangle base 60 height 52, apex on +x'; 30-60-90 height
      34.6; custom 100 + 100 becomes 100 + 79.
- [ ] Outline without points paints nothing. GRIN 100x100: 1.8 on axis to 1.5 at the edge, 1 outside.
- [ ] Fibre: straight path one chord; flat end exact; 3 px coating on both ends and the tube; bare end/tube variants; a
      bare core absorbs nothing; a bent fibre painted along the curve.
- [ ] Later part paints over an earlier one.
- [ ] Absorption: none without absorption; profile 0 at outline to 1 at ramp end (`round_even(200*t^2)` for
      Absorption 200, ramp 20); a mirror ignores absorption; Normalize clamps to 255 and ramp >= 1.
- [ ] Logic gates: mode solver beta matches the engine's guide; no absorption along cores/coupler/outputs; both outputs
      off lets almost nothing out; port follower (A, B) tracks move/turn/frequency. (G-06)
- [ ] Zone plate: frequency scale halves the wavelength at twice the scale; no mount -> nothing beyond the diameter.
- [ ] Painter runs: pixel-run painting byte-identical to pixel-by-pixel over every example and every kind at 4 angles.
- [ ] Painter output byte-identical to the C# painter on every example project (G-02).

## Probes and graphs

- [ ] CPU point probe: one sample per step; taking samples clears; integrated line probe: nothing before its start
      step, non-negative sums after.
- [ ] Run: state Completed, finished raised once; frame count published matches the store; last frame has field at the
      source; pause reported as requested.
- [ ] Graph data at frame 20 / step 40: 41 samples.
- [ ] Box sizes nominal; box clamped into a full-HD canvas; custom wider than the canvas shrinks to its width with
      aspect; a box moved away detaches.

## Export

- [ ] PNG unfiltered: filter byte 0 on every row, decodes pixel for pixel; Paeth variant decodes identically.
- [ ] JPEG: SOI/EOI; SOF says 4:4:4 (all components 1x1); decodes (ffmpeg or an image crate) close to the source.
- [ ] MPEG-1: quantiser 1 at q 100, 31 at q 1, 6 at q 70, 12 at q 50; one sequence header, 7 picture headers, one end
      code for 7 frames.
- [ ] AVI: RIFF AVI, LIST hdrl, avih; a small segment limit splits frames over AVIX segments; ffmpeg decodes all
      frames identical.
- [ ] MOV Animation: smaller than raw, ffmpeg decodes identical frames; MOV PNG: moov leading, first sample decodes to
      the first frame exactly.
- [ ] EXR: `name_0001.exr` is version 2 scanline, channel Y half, uncompressed, halves bit for bit.
- [ ] CSV: point header `step;integrated value` (or amplitude/value); a line probe of 3 pixels over 4 px with VE 2,
      frame stride 1 gives exactly `frame;step;0;2;4`, `0;0;1;2;3`, `1;2;4;5.5;6`; file name
      `ball_A_B_ focus.csv` from project `ball` and probe `A/B: focus`; a blank probe name gives `ball_probe.csv`.
- [ ] Naming: a sequence chosen as `dir/chosenname.png` goes to `dir/chosenname/`; second file holds second frame; a
      still is the frame; a still is never a video.
- [ ] Markers: black stroke around the white one, ends with the arm; a cross at the corner is clipped; labels "3B", "2".
- [ ] Export job: 10-frame run, 6 PNGs numbered by timeline frame `field_0002..field_0007`; AVI holds 6 frames; cancel
      -> Cancelled; a frame beyond the store fails after the existing ones.

## Performance

The engine is compared with the C# library's engines on the same machine (`tools/compare-csharp/run.sh bench`
beside `wavedude-cli bench --gpu`); the figures and what makes a step differ are in docs/measurements.md. The C#'s
published figures were taken on other hardware and are not targets.

Constraints: the store's writer is bandwidth-bound; do not parallelise the per-frame copy into the store (measured
slower with GPU DMA); page pre-touch of fresh blocks on up to 4 cores helps.
