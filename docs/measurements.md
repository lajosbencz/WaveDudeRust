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
