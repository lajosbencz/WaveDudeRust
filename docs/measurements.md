# Measurements

Release builds on the user's workstation (Ryzen 9 9900X, 24 threads, RTX 5070
Ti 16 GB, Linux, Wayland; wgpu over Vulkan, the C# engine's OpenCL on NVIDIA's
driver).

## Engine against WaveDude Next, same machine

2026-11-07. `wavedude-cli bench --gpu --steps 2000` against the C# library's own
engines through `tools/compare-csharp/run.sh bench 1920 1080 2000`: a 1920x1080
canvas, a 32-cell rim outside, open edges, one source at the centre, intensity
tracked; with captures, both planes as f16 every 2 steps, pipelined. Best of
three alternating runs (runs differ by up to 10 %), ms a step:

| Engine, stencil | C# | Rust | Rust / C# |
|---|---|---|---|
| CPU 9-point | 0.83 | 1.52 | 1.8 |
| CPU 9-point, captures every 2 | 1.66 | 1.90 | 1.1 |
| CPU 4th order | 1.73 | 3.36 | 1.9 |
| CPU 4th order, captures every 2 | 2.64 | 3.77 | 1.4 |
| GPU 9-point | 0.037 | 0.108 | 2.9 |
| GPU 9-point, captures every 2 | 0.075 | 0.253 | 3.4 |
| GPU 4th order | 0.079 | 0.229 | 2.9 |
| GPU 4th order, captures every 2 | 0.101 | 0.330 | 3.3 |

A step is not the same work in both:

- 9-point: the C#'s kernel is a Laplacian scaled by 0.316, so its wave moves
  0.562 px a step at one sub-step; ours is unscaled (decision 0003), the wave
  moves 1 px a step, and the CFL bound (0.866) needs two sub-steps. Per pixel
  of wave travel the CPU engines are level (C# 1.47 ms, Rust 1.52) and the GPU
  is 1.6 times slower (C# 0.066 ms, Rust 0.108).
- 4th order: both move the wave 1 px a step. The C# runs the Taylor stencil
  in three leapfrog sub-steps (three grid passes), ours the DRP stencil with
  the modified equation in two sub-steps of two passes each (four passes), for
  the phase and time errors of docs/physics.md.
- Ours runs a CPML on the rim, the C# a damping ramp folded into the grid pass.
  On the GPU the CPML is about a third of the 9-point step, and it is work,
  not pass overhead: fusing its four passes into two (each thread recomputing
  its neighbours' memory variables) left the step where it was (0.121 ms in
  that run), and leaving the CPML's arithmetic out of that variant took it to
  0.084 ms. The fusion was not kept.
- GPU captures: the C#'s 8 MB readback runs on a transfer queue of its own,
  beside the steps (0.08 ms a capture over its two steps); wgpu has one queue,
  so ours runs after the steps (0.29 ms a capture), plus a 0.16 ms copy out of
  the mapped buffer on the host, which overlaps the card's next steps. On the
  CPU our capture is the faster one (0.76 ms against 1.66).

### Re-measured 2026-11-08

`wavedude-cli bench --gpu --steps 2000` after the UI and format work: GPU 9-point 0.112 ms, 4th order 0.211 ms,
with captures 0.243 and 0.334 ms; CPU 1.54 / 3.35 ms, with captures 1.89 / 3.76 ms. Unchanged within the run-to-run
spread. The gap to the C# is the work described above (two sub-steps, four passes, the CPML, one queue), not
overhead; nothing cheaper than those was found, so the figures stand.

### Example22 end to end, 2026-11-11

Both programs on the same RTX 5070 Ti (Next under wine, OpenCL; ours wgpu), default preview settings (full
resolution, every frame, 8-bit samples), the cache sized to hold the whole run (11.0 GB, nothing on disk), F5 from
frame 0 over 3800 steps and 1900 frames:

| | Next 05a0348 | WaveDudeRust 2a5f4e8 |
|---|---|---|
| Run Completed | 1.7 s | 3.5 s |
| Grid | 2880 x 1080 canvas | 3008 x 1208 with the 64 px CPML rim, 2 sub-steps a step |

The ratio is the sub-steps and the rim, as above. With the default 30 % cache the same run spilled 7.2 GB to disk
and took 6.9 s.

## The new parts against WaveDude Next 05a0348

2026-11-10. `tools/compare-csharp` on upstream's one-of-every-kind project (`_tools/ui_check/figures.wdproj` as a
canvas scene, 800x500, 600 steps) and on four 640x400 projects of a 31-element line source before a 240 px part,
1200 steps, `wavedude-cli compare` with the Rust dumps it now writes beside the report.

Painted scene of every kind: 1.05 % of the pixels differ, all in red, 99.3 % of them within 2 px of an index edge,
none by more than 16 levels (0.16 in index), the blue and green channels equal everywhere; the rectangle and the
simple grating are bit for bit the same. The edge pixels are the effective medium against the C# 16-sample
supersampling (decision 0010): a ridge under a quarter pixel wide vanishes there and keeps a faint index here.

| project | field Pearson at the end | intensity Pearson | what was measured | C# | Rust |
|---|---|---|---|---|---|
| meta lens, F 200 | 0.954 | 0.987 | intensity peak on the axis (x), half-maximum beam width at x = 350 / 450 | 355, 19 / 37 px | 370, 19 / 34 px |
| meta prism, 20 deg | 0.940 | 0.976 | brightest row at x = 450 / 550; centroid of the beam over x 450..600 | 178 / 74; 119 | 179 / 73; 122 |
| facetted grating, pitch 32 | 0.978 | 0.994 | intensity across x = 500: Pearson, brightest row | 0.996; 129 | same; 131 |
| diffuser | 0.975 | 0.994 | intensity across x = 500: Pearson, brightest row | 0.998; 267 | same; 265 |

The field gain stays the known 5.5 to 6.2 of the source model (decision 0010); the C# engine took 3 sub-steps to
our 2. Both engines focus the meta lens well short of its nominal 456 px, the near field of a 240 px aperture at
a 32 px wavelength.

## The live run

2026-11-09, `cargo test -p wavedude-core --test run --release -- --ignored --nocapture live_run_drag_burst_timing`:
Example16 at half resolution, 8-bit samples, on the GPU; the source moved by a pixel 120 times at 32 edits a
second, the position at frame 100; the position's frame alone wanted, and every frame (the 50 stored frames
before the position captured on the way).

| | source moved, position alone | source moved, every frame | part moved, every frame |
|---|---|---|---|
| one frame from step 0 | 25 ms | 23 ms | 88 ms |
| frames shown during the 3.8 s burst | 120, one per edit | 120, one per edit | 1 |
| the final frame after the last edit | under 1 ms | 21 ms (0.9 frame times) | 86 ms (1.0 frame times) |

A part edit recomposes the scene and gives the engine the new medium; at 32 edits a second no frame of a mid-drag
state completes, the picture follows one frame time after the hand rests. Where the time goes
(`part_edit_cost`, Example16, 1500x900 canvas): composing the scene pixels 5.9 ms, the medium from them 2.7 ms,
the GPU engine taking the new scene 31.6 ms (the medium's layout built on the CPU and uploaded); the next target
for a lighter part drag is that upload.

Before the engine was kept across edits (rebuilt per edit, 45 to 55 ms): one frame 64 to 70 ms, 59 frames
shown during the burst.

The captures on the way are hidden behind the steps: a single pass fills the timeline in order at no cost to
the frame under the cursor. Work is done for the current project alone: a frame of a mid-drag state lands only
when it is done before the next edit, as here; a slower frame is left and the picture follows once the hand
rests, one frame time later.

Before (scratch frames abandoned for a newer request within 16 steps, a 120 ms debounce): the same during the
drag, the debounce on top once the hand rested.

## Other operations

2026-11-06 unless noted.

| Operation | Rust |
|---|---|
| GPU step, plain f32 maps, 9-point / 4th order | 0.113 / 0.120 ms |
| GPU capture of both planes, halves / indices | 0.45 to 0.51 / 0.25 to 0.29 ms |
| CPU capture of both planes as halves | 0.95 to 0.99 ms (5.3 before the row-parallel pass) |
| GPU two steps + capture, overlapped | 0.43 to 0.45 ms |
| Store write of a full-HD f16 frame, both planes | a move into memory (0.86 to 0.94 ms before the capture wrote into the store's buffer) |
| Half table build | 0.19 ms |
| Render 2880x1080, 16-bit / 8-bit | 0.17 / 0.13 ms |
| Screen path 1920x1080 to 914x514, plain / dithered | 0.44 / 0.50 ms |
| Painter: 400x330 outline | 1.65 ms |
| Painter: 400 px lens / hexagon | 0.23 / 0.15 ms |
| Painter: S-bent fibre across the canvas | 2.6 ms |
| Freya: 1080p frame drawn through Skia with 200 lines | 0.35 ms avg, 1.9 max |
| JPEG 1080p 4:4:4 q90, one thread / sequence sink with workers | 15.6 / 1.8 ms a frame |
| MP4 1080p (openh264) | 5.1 ms a frame |
| MPEG-1 1080p (slices on rayon) | 3.5 ms a frame |

Two-phase capture into the store's buffers (review C-1, C-2): a run begins
each capture into a buffer the frame store hands out (its prepared memory frame,
or a recycled one for the spill file) and collects it after the next has begun,
so the GPU's conversion and readback overlap the steps between captures, and a
frame in memory is neither allocated per capture nor copied by the writer.
`wavedude-cli bench --gpu` (1920x1080, rim 32, both planes as f16 every 2 steps,
200 steps), ms a capture over the steps alone; before: a synchronous capture
into new planes per frame; after: the run's pipelined capture with two reused
buffers. Two runs each, machine quiet.

| Engine, stencil | Before | After |
|---|---|---|
| CPU 9-point | 1.14 to 1.81 | 0.83 to 0.98 |
| CPU 4th order | 1.28 | 0.81 to 0.98 |
| GPU 9-point | 0.52 to 0.53 | 0.23 to 0.24 |
| GPU 4th order | 0.47 to 0.49 | 0.20 to 0.21 |

GPU two steps plus a capture: 0.402 to 0.406 ms before, 0.255 to 0.258 after
(9-point); 0.478 to 0.485 before, 0.343 to 0.345 after (4th order). The store
write of a memory frame is now a move; the copy it made (0.86 to 0.94 ms a
full-HD frame) is gone.

Export pipeline (review X-5 to X-8), `wavedude-cli export` of Example02 (1920x1080),
`--steps 1000 --from 600 --to 1000 --markers`, 200 field frames, CPU engine,
release build, the job's time per frame. Before: the run first, then the
export, every sink on the job's thread (MOV samples encoded there too). After:
the export reads the run as it goes; the figure is the job's own time, the
time it waited for the run taken out. The after figures were taken while other
builds and tests loaded the machine (load average 45 to 58 on 24 threads), so
the run itself took 15 to 25 s instead of about 7 and the totals are not
comparable; the export's own cost is.

| Format | Before, ms a frame | After, ms a frame |
|---|---|---|
| MP4 | 11.8 to 12.1 | 3.0 to 3.3 |
| PNG sequence | 4.6 to 4.7 | 3.2 to 3.3 |
| MOV with PNG samples | 35.5 to 35.8 | 3.0 to 3.4 |
| AVI | 2.4 to 4.4 | 2.7 to 2.8 |
| MPEG-1 | 6.4 to 7.9 | 2.7 to 3.0 |
| JPEG sequence | 2.0 to 3.5 | 2.7 to 3.1 |

After, every format costs the job about the render and copy of a frame: the
encoders run on a sink thread (MP4, MPEG-1, AVI) or the job's encoder pool
(image sequences, MOV samples written in order), and MOV PNG no longer deflates
on the job's thread.

Export quality, PSNR against the rendered frame: MPEG-1 41.8 dB at 200x120,
44.6 at 640x360, 47.9 at 1280x720; MP4 40.2 at 640x360, 44.4 at 1280x720
(tiny pictures fall under 40 dB through the 300 kbit/s floor). AVI, MOV
Animation and MOV PNG decode identical to the source through ffmpeg.

CPU to GPU agreement after 1000 steps on a 128x96 scene with every source
kind: field 4.3e-6 to 5.9e-6 relative, sqrt(I) about 1e-6; probes 8.9e-7 on a
peak of 0.23; captured index planes at most 2 of 12288 samples one level apart,
half planes up to 8 ulps near zero. The driver fuses multiply-adds, so cells are
not bit-identical by design (decision 0003).

Preview playback of Example 16 (1500x900, 16-bit samples through the screen
path, both pictures, every handle and graph box drawn, release build): 100
frames in 3.33 s at the 30 fps clock, the presenter 1.3 ms a frame on average
and 2.5 at worst, the wait at the picture 3 ms on average and 8 at worst, the
worst gap between two presented frames 54 ms (the 16 ms state tick quantises
the 33 ms frames to 32 or 48 ms).

Solver quality after decision 0010 (docs/physics.md has the tables): CPML
reflection -74 to -84 dB at 16 to 20 cells and about -104 dB at 32, at normal
and 45 degree incidence, long and short packets (the Mur ring alone: -17 to
-42 dB); DRP phase error at 6 cells per wavelength 0.08 percent on the axis
and 0.03 diagonal (Taylor 4th order with leapfrog: 0.55 and 1.01); temporal
error of the modified equation 1.5e-5 at 10 cells (leapfrog 4.1e-3); energy
of a free pulse within 2.4e-7 over 10,000 steps; a slab guide's measured
propagation constant within 1e-4 of the mode solver; CPU to GPU field within
2e-6 after 1000 steps; a slab of index 1.5 with its face inside a pixel
reflects 0.199 (9-point) and 0.198 (DRP) of the amplitude against Fresnel's
0.2; a mirror cavity bounded over 60,000 steps (mirror cells c2 = 0; it grew
to 1.4e13 on the DRP while mirror cells moved); nothing crosses a one-pixel
diagonal mirror line (10 % without the 9-point's diagonal cuts).

Photonic gate demonstrations (examples/gates, both engines): every truth
table row within 0.1 of its expectation; the fibre XOR sends 0.005 of one
input's power to Q with both inputs on, the neat OR 0.497, the plain OR 0.994.

Engine correctness fixes found by the C# comparison (docs/comparison): the
Mur edge inside an in-canvas damping band diverged (Example 18) and is stable
now at any damping and sub-step count through a centred damped scheme; the
9-point import factor is 0.562 on the frequency scale; the source is an
additive soft forcing normalised by the lattice Green's function, transparent
to passing waves bit for bit and the same at any sub-step count.

Open performance items:
- Wake the state tick on the playback clock's own channel instead of the
  16 ms loop, so a frame is never a tick late (the 54 ms worst gap).
- GPU probes cost a round trip per step; batch them into a ring buffer read
  back per heartbeat.
- The painter's settled runs (one pixel centre settling the next pixels of
  its row) are not implemented.
