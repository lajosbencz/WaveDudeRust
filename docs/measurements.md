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
  the phase and time errors of [docs/physics.md](physics.md).
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

2026-11-10. [`tools/compare-csharp`](../tools/compare-csharp/) on upstream's one-of-every-kind project (`_tools/ui_check/figures.wdproj` as a
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

2026-11-12, `cargo test -p wavedude-core --test run --release -- --ignored --nocapture live_run_drag_burst_timing`:
Example16 at half resolution, 8-bit samples, on the GPU; the source moved by a pixel 120 times at 32 edits a
second, the position at frame 100; the position's frame alone wanted, and every frame (the 50 stored frames
before the position captured on the way).

| | source moved, position alone | source moved, every frame | part moved, every frame |
|---|---|---|---|
| one frame from step 0 | 25 ms | 23 ms | 58 ms |
| frames shown during the 3.8 s burst | 120, one per edit | 120, one per edit | 120, one per edit |
| the final frame after the last edit | under 1 ms | 10 ms (0.5 frame times) | 25 ms (0.4 frame times) |

A part edit recomposes the scene and gives the engine the new medium. Where the time goes
(`cargo test -p wavedude-core --test run --release -- --ignored --nocapture part_edit_cost`, Example16, 1500x900
canvas): composing the scene pixels 4.9 ms, the medium from them 2.7 ms, the GPU engine taking the new scene
7.5 ms. The sub-step medium takes its damping root and its table index once per run of equal cells; taken per
cell, the GPU engine needed 31.6 ms, a part moved took 88 ms a frame and showed 1 frame of the 120 edits.

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

Burned-in overlays as epaint meshes, 2026-11-13: every overlay on both layers,
```
wavedude-cli export crates/wavedude-core/tests/fixtures/examples/Example02_Coherent_source_gaussian_beam.wdproj \
  --out ex2.avi --steps 1000 --from 600 --to 1000 --layers both --markers --graphs --source-markers
```
200 frames of each layer, AVI, the CLI's "ms/frame exporting", alternated with the previous CPU rasteriser (commit
d377336), three runs each, load average 6 to 14:

| Overlays drawn by | ms a frame |
|---|---|
| previous CPU rasteriser (bitmaps and ab_glyph) | 3.1 to 3.4 |
| epaint meshes rasterised on the CPU (`wavedude_export::raster`) | 3.5 |

The meshes' rasteriser draws bands of rows in parallel; the source markers, which do not move, are tessellated
once; probe strokes are quads on whole pixels, without feathering. What is left over the previous rasteriser is
drawing the source markers' triangles every frame (about 0.3 ms a layer here, 16 000 triangles).

Before, 2026-11-12, the meshes were drawn on the GPU by egui-wgpu and read back: 6.0 to 6.6 ms a frame on the RTX
5070 Ti, 11.0 to 11.2 on lavapipe; uploading and reading back only the rectangles the shapes cover, 4.9 to 5.1 and
9.5 to 9.9, of which the device setup was 0.2 s an export (the run on the CPU engine leaves no device to share).

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

Preview colouring, 2026-11-13, a new frame's picture of a 2880x1080 16-bit plane (Example 21's canvas) at
1600x600 device pixels with the outline, mean of 60:
`cargo test --release -p wavedude-core --features gpu --test colour -- --ignored --nocapture preview_colouring_time`
(`VK_ICD_FILENAMES` picks lavapipe). Before, the presenter's paint and outline blend on the CPU: 0.87 ms a
picture, about 570 frames a second for both pictures, plus the 3.8 MB texture upload each. After, a run on the
GPU engine: the picture set to the next device frame, drawn, submitted and waited for: 0.066 to 0.075 ms on the
RTX 5070 Ti, about 7000 frames a second for both; 2.2 to 2.4 ms on lavapipe, where the app runs the CPU engine
and keeps the presenter. Playback stays at the 30 fps clock either way; the preview's share of a frame shrinks.

Solver quality after decision 0010 ([docs/physics.md](physics.md) has the tables): CPML
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

Photonic gate demonstrations ([examples/gates](../examples/gates/), both engines): every truth
table row within 0.1 of its expectation; the fibre XOR sends 0.005 of one
input's power to Q with both inputs on, the neat OR 0.497, the plain OR 0.994.

Engine correctness fixes found by the C# comparison ([docs/comparison](comparison/)): the
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

## The browser demo

The demo page and its package are gone ([decision 0019](decisions/0019-egui-and-the-platform-layer.md)); the numbers are the engines' in
the browser.

2026-11-12, Chrome 158 and Firefox 159 (flatpak, `dom.webgpu.enabled`, with the NVIDIA driver's flatpak GL
extension; without it Firefox's WebGPU runs on lavapipe), both on Linux. A page beside the built demo
(`demo/build.sh target/site`, served by `python3 -m http.server`) loads the package and, on Example04 at the
Standard tier, times frame 200 reached from a fresh session (400 steps and one capture), then frames 201 to 220
one at a time (two steps and one capture each), then ten renders of one layer at 800x800.

| | Chrome GPU | Firefox GPU | CPU engine, either browser |
|---|---|---|---|
| Frame 200 from step 0 | 60 to 65 ms | 75 to 97 ms | |
| A played frame, before / after the readback nudge | 2.6 / 1.6 ms | 100 / 1.7 ms | 52 to 54 ms |
| Render 800x800 | 0.5 to 0.6 ms | 0.6 ms | 0.5 ms |

Firefox maps a buffer when it polls the device, on a 100 ms timer, unless the queue is submitted to. A page
timing `mapAsync` of a 4 MiB copy (nine runs, median): plain 2.2 ms in Chrome, 100 ms in Firefox; with
`onSubmittedWorkDone` awaited as well, the same; with an empty `queue.submit([])` every turn of the event loop
while it waits, 4.1 ms in Chrome and 8 ms in Firefox. The package's wait (`crates/wavedude-wasm/src/wait.rs`)
races the readback against a 1 ms timer that submits an empty batch, so Chrome's readback is not delayed.

## The browser's frame spill

2026-11-13, Chrome for Testing 158 headless on Linux (Playwright), the app built by `tools/build-web.sh` and served by
`python3 -m http.server`. `navigator.deviceMemory` set to 0.25 and RAM cache size 10 % (a 25.6 MB budget); a 640x480
project of 1500 steps, run with F5: 750 frames of 0.61 MB, 43 in memory and 707 (0.43 GB) in the origin private file
system. The reads are the app's debug lines `spilled frame read back` (request to bytes in Rust); playback is
F7 from frame 0 for 12 s, frames shown counted by their reads.

| | WebGPU (RTX 5070 Ti) | WebGL2 (SwiftShader, CPU engine) |
|---|---|---|
| Run of 1500 steps | 12.5 s | 45.0 s |
| Read of a spilled frame, median / p95 / worst | 0.7 / 0.9 / 12 ms | 140 / 145 / 222 ms |
| Playback over spilled frames | 60 frames/s (680 in 11.3 s) | 33 frames/s (373 in 11.2 s) |

The read is a chain of five promises, each resolved in a task of the main thread: under WebGL2 the software
rasteriser's frames between them make up the latency, not the storage.

## The browser's live run

2026-11-13, Chrome for Testing 158 headless (WebGPU through Vulkan) and Firefox 159 (`dom.webgpu.enabled`) on Linux,
RTX 5070 Ti, Playwright; the app built by `tools/build-web.sh`, served by `python3 -m http.server`. A new project
(1920x1080, 2500 steps, no sources or probes), run to its end at the Low preview (625 stored frames, half resolution, 8-bit)
and in Chrome at High with the memory warning's halved resolution and 8-bit samples (1250 stored frames). Steps/s from
the app's `Run Completed` line; animation frames counted by a `requestAnimationFrame` loop from the run's start to that
line. Before: at most two captures in flight, the run yielding while the previous one was not mapped (one capture per
animation frame); after: a ring of captures by the GPU engine's staging budget (64 MiB, 2 to 16), landed in order at
each pump.

| | Before: steps/s | animation frames a stored frame | After: steps/s | animation frames a stored frame |
|---|---|---|---|---|
| Chrome, Low | 266 | 0.91 | 3500 | 0.07 |
| Firefox, Low | 200 to 240 | 1.0 to 1.2 | 2100 to 2300 | 0.10 to 0.12 |
| Chrome, High | 120 | 1.0 | 1900 | 0.06 |

Natively the threaded run waits for the oldest capture only when the ring is full: `cargo test --release -p
wavedude-core --test run -- --ignored --nocapture live_run_drag_burst_timing`, two runs each, before / after: one frame
of Example16 25 to 31 / 23 to 25 ms (source moved), 70 to 87 / 60 to 64 ms (part moved); 118 to 120 / 120 of 120 frames
shown with every frame wanted.

## The browser's live run in a worker

2026-11-14, Chrome 158 (Playwright, WebGPU through Vulkan) on Linux, RTX 5070 Ti; the app built by `tools/build-web.sh`
into a folder served by `python3 -m http.server`, before (16c0b44, the worker pumped in 16 ms slices, a pump with work
left or spill writes in flight posting itself a message) and after (the pump waits on readiness, 8 ms slices). A
temporary counter in the worker's loop logged the pumps and the self-posted messages each second. The messages between
page and worker were counted by a Playwright init script wrapping `Worker.prototype.postMessage` and listening on the
worker. Fresnel lens lighthouse (`examples/optics/01-fresnel-lens-lighthouse.wdproj`) at the High preview, run with F5
and the memory warning's 8-bit samples (2000 steps, 1000 stored frames of 1.4 MB); the time is the app's `Run Completed`
line. Waiting on the page: a new project (1920x1080, 2500 steps) run with the warning's halved resolution, the page's
thread blocked for 3 s (`while (performance.now() < end) {}` from Playwright) 0.5 s after the run started, so no frame
is acknowledged meanwhile.

| | Before | After |
|---|---|---|
| Lighthouse run | 1.6 s | 1.2 to 1.5 s |
| Pumps / self-posts a second while running | 161 000 / 161 000 | 1 000 to 1 800 / 0 |
| Pumps / self-posts a second while the page is blocked | 213 000 to 307 000 / as many | under 300 / 0 |
| Pumps while idle | 460 a second, as many self-posts | none in 36 s |
| Messages page to worker / worker to page while idle, after a run | | none / none in 13 s |

After, the run lands a capture read back only below the spill's writes in flight, the frames posted and not
acknowledged ([`crates/wavedude-core/src/run/pending.rs`](../crates/wavedude-core/src/run/pending.rs), `WRITES_IN_FLIGHT`):
with 4 the lighthouse took 2.0 to 2.2 s, with 16 (the GPU engine's largest capture ring) 1.2 to 1.5 s, with no
bound 1.0 s. The GPU's submissions are paced on `on_submitted_work_done`, two on the card at most; pacing by the steps
on the card instead, as a capture every second step cuts a submission short, took the same 2.0 to 2.2 s at a bound of 4.

## The browser's preview coloured on the page's GPU

2026-11-14, Chrome 158 (Playwright, WebGPU through Vulkan) on Linux, RTX 5070 Ti, a 144 Hz page; the app built by
`tools/build-web.sh` into two folders served by `python3 -m http.server`: before (58f6004, the presenter colouring every
shown frame on the page's thread) and after (the frames uploaded and coloured on the page's renderer device). The
startup page's High preview (16-bit samples, 16 bits per channel dithering), Example04 (3 sources 3 frequencies) run
with F5 and the memory warning's halved resolution (1250 stored frames), both pictures shown at 402x402. Main-thread
time is the change of Chrome's `TaskDuration` (CDP `Performance.getMetrics`) over 10 s of playback from Home, which
played 600 frames at the 60 fps clock in both; two passes each.

| | Before | After |
|---|---|---|
| Main-thread time over 10 s of playback | 2.66 to 2.71 s | 1.20 s |
| Per played frame | 4.4 to 4.5 ms | 2.0 ms |

The last frame's pictures, screenshotted in both, differ by at most 1 of 255 in 1.6 % of the pixels: the card's fused
multiply-adds moving the resampling at an f16 rounding boundary (`wavedude_core::render::gpu`).

## The idle window

2026-11-13, Ryzen 9 9900X, release build on Xvfb (`DISPLAY=:77`, presented through lavapipe), a fresh settings folder.
Wakeups are the main thread's voluntary context switches over 10 s without input, read from
`/proc/<pid>/task/<pid>/status` (`voluntary_ctxt_switches`); CPU is its `utime + stime` over the same 10 s.

| | 240 ms idle tick | Woken by news only |
|---|---|---|
| Startup page shown, 30 s after the start | 615 to 620 wakeups, 0.58 s CPU | 0 wakeups, 0 s CPU |
| Example01 run to its end (2500 steps), then idle | 620 to 630 wakeups | 0 wakeups |

While nothing moves the window requests no repaint; a log line, the scene image file changing, the run's state
changing kind, an open request from the OS and the OS colour scheme wake it.
