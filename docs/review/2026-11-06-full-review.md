# Full review, 2026-11-06

Scope: the whole workspace, reviewed for physical correctness, idiomatic and
DRY Rust, modularity, future-proofing and performance. Four reviewers read
one slice each (physics; wavedude-core code; wavedude-export and wavedude-cli;
wavedude-widgets and wavedude-app). Claims below were verified by reading the code
and, where it says so, by running it. Baseline: clippy clean at default
lints, 433 tests passing, fmt clean.

Severity: BUG = wrong result or crash; INACC = inaccuracy that is neither
documented nor configurable (the user's rule); DESIGN / DRY / IDIOM / PERF /
FUTURE / NOTE as named.

## Outcome (same day)

Every finding below was addressed, with these exceptions, each left on
purpose and documented where it belongs:

- P-3: continuing w into the Mur rings was built, measured and reverted.
  It was no better (-39.5 against -39.9 dB) and diverged at a corner on the
  DRP stencil. w stays 0 on the rings; [physics.md](../physics.md) (Open edges) has the
  numbers.
- P-4: tau is not derived from omega; that would change the meaning of
  TimeDecay. The rule and its ripple are documented and tested instead.
- A-17: no render buffer pool; the presenter renders a frame in 1.3 ms
  against a 33 ms budget, so there is nothing to gain yet.
- C-16: `ProbeTrace.steps_sampled` stays u64; the rest is u32.
- X-12: the in-house MP4 muxer is noted, not built.

Measured after the fixes: the 4th-order engine stays bounded over 60k
steps in a mirror cavity (was 1e13); diagonal mirror leak 0 (was 2.5 %);
full-HD capture 0.20 ms on the GPU (was 0.48) and 0.9 ms on the CPU (was
1.3); MP4 export 3.2 ms a frame (was 12), MOV PNG 3.2 (was 36); idle app
CPU 1.15 % (was 1.9 %); playback unchanged at 30 fps, 1.3 ms a frame. The
state layer is now the `wavedude-app-state` crate with no UI dependency. The
workspace is clean under fmt, clippy with warnings denied, and all tests,
which now finish in about 90 s.

## Verdict

The numerics are derived and implemented carefully: the Dablain time scheme,
the DRP coefficients and their CFL limits, the lattice-Green source
normalisation, the CPML recursion and the n^2 sub-cell average all match the
literature and were checked independently. The architecture is sound: an
Arc-shared immutable RunSpec, a narrow WaveEngine trait, a UI-free state
layer behind a Backend seam, app-agnostic widgets, bit-correct in-house
containers.

Three things need fixing before the project is handed to anyone:

1. Mirrors make the 4th-order engine unstable (BUG, reproduced).
2. The engine trait hides the GPU's asynchronous capture, so real runs never
   use it and allocate plus copy a frame per capture.
3. Reactivity in the app: one global revision plus a 4 ms poll that clones
   the scene image and re-renders everything at the frame rate.

After those, the duplicated orchestration (export and engine construction in
the CLI and the app), the fourfold field naming in the `.wdproj` adapter and
the missing `.wdr` migration path are the maintenance risks.

## Priority list

P0, correctness

- [x] P-1 Mirror cells as true Dirichlet cells on CPU and GPU; stability test
      of 30k+ steps with mirrors on both stencils.
- [x] P-2 9-point: diagonal tap cuts across 8-connected mirror lines.
- [x] X-2 CLI PNG writer never calls `finish()`; BufWriters not flushed.
- [x] X-3 Partial container files on early error or Ctrl+C; no cancellation.
- [x] A-4 Preview, scheme picker and export resolve user colour schemes
      differently (grey in the preview, coloured in the export).

P1, performance and API

- [x] C-1 Two-phase capture on the WaveEngine trait.
- [x] C-2 Capture into caller-provided buffers; recycle snapshots.
- [x] A-1 Scene watch keyed on a revision, not a clone of the scene.
- [x] A-2 Split the global revision into document, selection, playback, run,
      settings.
- [x] A-3 Keep `image_data` out of the properties form JSON.
- [x] C-7 Engines hold `Arc<SceneData>`.
- [x] C-8 GPU device loss handling; no `expect` on mapping.

P2, structure

- [x] X-1 Export orchestration into wavedude-export (`ExportPlan`, `build_job`).
- [x] X-4 One engine factory in wavedude-core, one `EngineArgs` in the CLI.
- [x] C-5 `.wdr` migration through `serde_json::Value`; explicit renames;
      tolerate unknown part kinds.
- [x] C-4 One declarative field table for the `.wdproj` reader, writer, DTD
      and range keys.
- [x] A-9/A-10 Property forms keyed on `PartKind`, paths derived; a test that
      every form row resolves.
- [x] A-19 Move `wavedude-app/src/state` into its own crate.

P3, documentation debt

- [x] P-3, P-4, P-7, P-9: w on the Mur padding, the intensity section, the
      CPML limits, the doc slips and the two promised tests.

## Physics (P)

P-1 BUG. [engine/pml.rs:142-179](../../crates/wavedude-core/src/engine/pml.rs#L142-L179), [scene/scene_data.rs:23](../../crates/wavedude-core/src/scene/scene_data.rs#L23). A mirror is a live
cell with c2 = 1e-6 that keeps its full taps to both sides; only the taps
crossing it are cut. Dropping a negative-weight tap (a2 = -0.175) lowers the
diagonal of -L_cut, and the cut operator becomes indefinite: the largest
eigenvalue of C^(1/2) L_cut C^(1/2) on a 22x22 grid with a 2 px wall is
+4.3e-7, a growing mode of about 6.6e-4 per step. Reproduced in CpuEngine on
the scene of `mirror_cavity_keeps_the_wave_inside` with reflective edges and
a 3-cycle burst: 9-point bounded at max|u| 0.2 to 0.45 over 60k steps; DRP
0.52 at 20k, 7.9 at 25k, 4.4e2 at 30k, 1.4e13 at 60k. The test stops at 300
steps and cannot see it. [physics.md:135](../physics.md) ("mirror cells never limit
stability") is false. Fix: c2 = 0 on mirror cells (or hold u = 0) on both
engines and keep the cuts; each side is then a principal submatrix of L,
negative semi-definite by interlacing (measured largest eigenvalue 4e-15).

P-2 INACC. cpu.rs:305-334, step.wgsl:15-21, [physics.md:105](../physics.md). The 9-point's
diagonal taps couple (i+1,j) and (i,j+1) straight across an 8-connected
mirror line, which the painter's centre test produces from any mirror
thinner than about 1.41 px at 45 degrees. Leapfrog model, 600 steps: 2.5 %
of the energy crosses an 8-connected line, 4e-10 a 4-connected one. Fix:
diagonal cut bits for the 9-point (drop a diagonal tap when either
orthogonal neighbour it passes is a mirror), or paint mirrors 4-connected.

P-3 INACC. cpu.rs:104, 337-347, step.wgsl:40-55. On open edges without a
rim, w = c2 L u is zero on the padding rings while u there is
Mur-extrapolated, so at the edge the correction term sees a wall. Fix:
continue w into the rings as u is (Mur on ring 1, linear on 2 and 3), or
document and measure Mur's reflection with and without.

P-4 INACC. [physics.md:375-379](../physics.md) against cpu.rs:247-248, 417-419, 491-497. The
docs give a one-stage EMA; the code runs two stages with alpha as the
retention weight, shows 0.5 s1 + 0.5 s2, and sets tau = TimeDecay * 100 /
lowest level, which ignores source_frequency_scale (at level 100, scale 2,
tau is about 1.25 periods, leaving a few percent of 2-omega ripple). The
test "converges to A^2/2 after 5 time constants" named in the docs does not
exist. [spec/30-engine.md](../spec/30-engine.md) describes the code correctly. Fix: rewrite the
section, add the test, consider deriving tau from omega.

P-5 INACC. painter.rs:161, 169-171. `existing` reads red 0 as n = 1 and any
pixel with coverage f > 0 is overwritten, so a refractive part that touches
a mirror opens holes in it; red 1..99 is read as red/100 (n < 1) while the
engine clamps to 1. Fix: keep the mirror unless the part covers most of the
pixel; `existing = max(1, red/100)`.

P-6 INACC. [scene/modes.rs:32-41](../../crates/wavedude-core/src/scene/modes.rs#L32-L41). The mode solver averages n over 4 samples
per row; the painter averages n^2 with exact coverage. Boundary rows of
odd-width cores can round to a different red level than painted. Fix:
average n^2 in `rows`.

P-7 IMPROVEMENT. pml.rs. Undocumented limits: damped regions are only 2nd
order in time (the A^2/12 term ignores gamma); D1(psi) is not applied on the
two canvas cells next to a strip (psi there is about sigma_max/rim^3,
negligible); D1(D1 .) has a null at k = pi, so the layer does not damp the
Nyquist mode through psi. The CPML itself is correct: Roden-Gedney b and a,
nested psi/xi recursion, stretched 9-point cross term, second memory set for
the second operator application, cuts inside the PML, profiles and
sigma_max.

P-8 NOTE. cpu.rs:73-76. Mur corner rings use the diagonal cell as "inner"
with a unit-distance formula; only the 9-point's diagonal tap reads them.

P-9 NOTE, doc slips. modes.rs module doc says A's eigenvalue is -s/S^2 (it
is -s; the Om formula after it is right). [05-deviations.md:9-11](../spec/05-deviations.md) still lists
the Taylor weights as the 4th order. Decision 0010 says the DRP is optimised
over 4 to 10 cells per wavelength; the code minimises over k in [0, pi/2].
Decision 0010 promises a Fresnel slab test at a non-integer interface; it
does not exist.

Verified correct: the time scheme `next = u + (u - prev) d + h (w + c2 L w
/ 12)`, w = c2 L u, the right A^2 for u_tt = c^2 Lap u, self-adjoint in the
1/c2 inner product, centred damping derived correctly, CPU and GPU in the
same order; DRP weights satisfy the moment conditions, max symbols 16/3 and
12.6316, modified-equation limit s < 12, giving 0.8660 and 0.97468; the
Green's function is that of the discrete operator and time scheme including
temporal dispersion; the n^2 area average is the TM case of Farjadpour
2006, right for this equation; Mur at d = 1 reduces to Mur 1981.

## wavedude-core (C)

C-1 PERF/DESIGN. [engine/mod.rs:40](../../crates/wavedude-core/src/engine/mod.rs#L40), [run/simulation.rs:322](../../crates/wavedude-core/src/run/simulation.rs#L322), [gpu/mod.rs:435](../../crates/wavedude-core/src/engine/gpu/mod.rs#L435),
965. The trait has only a synchronous `capture_frame`; the GPU's pipelined
`begin_capture`/`finish_capture` are inherent and used only by tests. Every
production capture stalls the stepping thread. Fix: a two-phase capture on
the trait (an opaque token keeps it object-safe); the CPU implements `begin`
eagerly.

C-2 PERF. capture.rs:98, [gpu/mod.rs:503-507](../../crates/wavedude-core/src/engine/gpu/mod.rs#L503-L507), store.rs:247-252. Every frame
allocates fresh planes that the writer copies into a store block and drops:
one allocation and one extra 8 MB copy per full-HD frame; the scratch frame
does the same. Fix: `capture_into(&mut self, shape, out: &mut Snapshot)` and
recycle snapshots through a return channel, or hand the store's block to the
capture.

C-3 DRY, drift risk. capture.rs:14-28 and [render/lut.rs:175-190](../../crates/wavedude-core/src/render/lut.rs#L175-L190) both define
`field_index`, `intensity_root_index`, `intensity_index` (f16 vs f32, NaN
ordering differs); capture.wgsl is a third copy. `capture::plane` and
`block_means` duplicate `plane_rows` and are only used by tests. Fix: one
set in `capture` taking f16, lut calls it; `plane` behind `#[cfg(test)]`.

C-4 DRY. [io/wdproj.rs](../../crates/wavedude-core/src/io/wdproj.rs) names every attribute four times: read (125-325),
write (469-757), `decls()` DTD (879-1080) and the `"Type.Prop"` keys of
[model/ranges.rs](../../crates/wavedude-core/src/model/ranges.rs); defaults restated on read and write. Fix: one declarative
table per type generating reader, writer and DTD, or at least derive
`decls()` from the writer on a default-plus-every-optional project.

C-5 FUTURE. io/wdr.rs:27-84. `.wdr` deserialises straight into `Project`:
no place to migrate when `format < FORMAT`; an unknown `PartKind` fails the
whole load; serialised names are the Rust field names, so a rename breaks
files silently. Fix: parse to `serde_json::Value`, `migrate(v, from)` per
version, then `from_value`; explicit `rename` on persisted structs; read
parts as `Vec<Value>` and skip unknown kinds with a note.

C-6 DRY. cpu.rs:541-548 and [gpu/mod.rs:834-840](../../crates/wavedude-core/src/engine/gpu/mod.rs#L834-L840) copy the `try_replace_scene`
predicate; cpu.rs:525 and [gpu/mod.rs:802](../../crates/wavedude-core/src/engine/gpu/mod.rs#L802) the emitter filter; the intensity
blend `(s1*0.5+s2*0.5)*scale` appears four times. Fix:
`SceneData::same_grid`, `Emitter::inside`, one `blend`.

C-7 PERF/API. [engine/types.rs](../../crates/wavedude-core/src/engine/types.rs), scratch.rs:200. `RunSpec.scene` is an Arc,
but both constructors take `SceneData` by value and `try_replace_scene`
clones: every build deep-copies c2 and d (tens of MB at full HD). A failing
`GpuEngine::new` consumes the scene, so wavedude-app (live.rs:865) clones it
first for the CPU fallback. Fix: engines hold `Arc<SceneData>`.

C-8 FUTURE. [gpu/context.rs:27-33](../../crates/wavedude-core/src/engine/gpu/context.rs#L27-L33), [gpu/mod.rs:1245-1258](../../crates/wavedude-core/src/engine/gpu/mod.rs#L1245-L1258). Device loss and
mapping failure are `expect` panics; the context is a process-wide
`OnceLock`, so a lost device is never recreated; `GpuError` carries a
String. Fix: `set_device_lost_callback` sets a flag; context in
`RwLock<Option<Arc<GpuContext>>>` rebuilt on the flag; stepping and capture
return `Result`.

C-9 DESIGN. simulation.rs:278, 31. `step_all -> Result<bool, String>`,
`RunState::Failed(String)`, `ScratchEvent::Failed { message: String }` lose
the typed errors. Fix: `enum RunError { Store, Engine, Panic }` with
thiserror, `Failed(Arc<RunError>)`.

C-10 DRY. parts.rs:6-9, [shapes/mod.rs:31](../../crates/wavedude-core/src/scene/shapes/mod.rs#L31), ranges.rs:42, 78-79: `MAX_COATING`,
`INDEX_MIN`, `INDEX_MAX`, `MAX_ABSORPTION_RAMP` repeat `ranges`; normalize.rs
hardcodes fallbacks that duplicate `Default` (Source 100.0 at :102, Scene
1.0 at :50). Fix: consts from `ranges`, `let d = Source::default()`.

C-11 PERF. store.rs:296-300. `with_layer` allocates a plane per read of a
spilled frame; `copy_layer` copies even from RAM. Fix: `with_layer_into`.

C-12 PERF. compose.rs:46-58. `SceneKey::of` hashes the whole image and
serialises every part to JSON per call. Fix: derive Hash on parts with a
float-bits newtype; image hash computed once when `image_data` changes.

C-13 NOTE. Cargo.toml: `wide` and `anyhow` are unused in wavedude-core.

C-14 PERF. cpu.rs:150-152. `s1`, `s2` allocated full-grid even without
intensity tracking, and the passes zip over them. Fix: allocate only when
tracking; two pass closures.

C-15 IDIOM. types.rs:232, probes.rs:90-95. `Vec<Vec<f32>>` readings and
profiles: one allocation per profile per frame. Fix: flat Vec plus offsets.

C-16 NOTE. `ProbeCells.cells` exposes the padded layout; the GPU packs
`c | 1<<31` ([gpu/mod.rs:821](../../crates/wavedude-core/src/engine/gpu/mod.rs#L821)) with nothing tying it to the size ranges; step
types churn (`u64`, `u32`, `AtomicU64` read as u32). Fix: `u32::try_from`
or a debug assert; one step type.

C-17 IDIOM. spec.rs:63 builds a throwaway `Scene { ..Default }` as a
parameter bag; wdr.rs:47 clones the project with the image twice to drop
it. Fix: a small `SceneEdges` struct; serialise a borrowed view or
`#[serde(skip)]` on `image_data`.

Done well: immutable Arc-shared RunSpec; WGSL constants generated from the
Rust `StencilCoefficients` with [tests/gpu.rs](../../crates/wavedude-core/tests/gpu.rs) cross-checking the engines;
`plane_rows` with `for_each_init` scratch rows fusing mean, root and
conversion; the CPU stencil on row slices; FrameStore's single-writer
many-reader design with testable `Budget`; thiserror at the boundary;
bounded colour-table caches; a clean `gpu` feature gate.

## wavedude-export and wavedude-cli (X)

X-1 DRY/DESIGN. wavedude-cli/export.rs:120-193 and wavedude-app export_dialog.rs:
361-421 both orchestrate an export (range, schemes, looks, paths, sinks,
overlays, job) and already differ: the app always composes the scene and
filters overlays on `wants_halves`; the CLI composes only when needed and
uses the default SchemeStore. Fix: `ExportPlan` and `build_job` in
wavedude-export.

X-2 BUG. wavedude-cli/png_out.rs:9-13, 20-24. `write_header()?.write_image_data()`
never calls `finish()`; IEND and the flush happen in Drop where errors are
lost, so a full disk gives a truncated PNG and exit 0. The app's `write_csv`
(export_dialog.rs:356) never flushes its BufWriter either.

X-3 BUG. wavedude-cli/export.rs:180-191 and the avi/mov/mp4/mpeg1 sinks. Sinks
open in a loop with `?`; a later failure drops an earlier sink unfinished
(only ImageSink has Drop), leaving zeroed headers or no moov. Nothing deletes
partial outputs on Failed or Cancelled; the CLI passes a constant false
cancel flag, so Ctrl+C kills the process mid-container. Fix: open all sinks
first, finish and remove on error, a ctrlc handler on the cancel flag, the
report listing paths written.

X-4 DESIGN/DRY. run.rs:65-80, bench.rs:150-165, wavedude-app live.rs:863. The
engine factory exists three times; `wavedude-cli --gpu` fails at runtime unless
wavedude-cli's own `gpu` feature is on; `run` and `probes` hardcode CPU while
export, bench, truth and gates take the flag. Fix: `engine::build(scene,
opts, prefer_gpu)` in wavedude-core; one `#[derive(Args)] EngineArgs` flattened
into every running subcommand.

X-5 PERF. job.rs:375-399, mov.rs:596. Copy, render, overlays and encode run
serially on one thread; MP4, MovPng (deflate on the job thread) and AVI
block the next frame's render, while ImageSink spawns up to 16 threads per
sink on top of rayon. Fix: a generic `Threaded<S: FrameSink>` with a bounded
channel of two recycled buffers; parallel MovPng encode written in order;
one encoder pool shared across targets.

X-6 PERF, per-frame copies. mp4.rs:148 `[y,cb,cr].concat()` copies 3 MB per
1080p frame, `encoded.to_vec()` and SPS/PPS `to_vec` every frame (use
`YUVSlices` and `write_vec`, parse SPS/PPS only until the track exists);
sequence.rs:265, 273 and exr.rs:271 clone the halves twice; png.rs:226 and
mov.rs:544 allocate per frame; overlays.rs:213-221 redraws static probe
strokes and labels every frame (pre-render them as `sources` already is).

X-7 PERF. avi.rs:173, 288, mov.rs:598. `stream_position()` on a BufWriter
flushes and seeks, two or three times per frame. Fix: a `pos` counter, seek
only in `close_segment`/`finish`.

X-8 DESIGN, FrameSource. lib.rs:127-143, 222-238. The `Arc<S>` impl is
unused (both callers pass `&*store`); `job.rs:380` fails with FrameMissing
instead of waiting and the CLI waits for the whole run first, so export
never overlaps the readback (add `wait_for(frame)`); `with_layer` on a
spilled frame allocates then copies (see C-11); the doc says f16
little-endian but the code is native-endian.

X-9 DRY. wavedude-cli/export.rs:20-64 mirrors `ExportFormat` and `ExportLayers`
in clap enums. Fix: `cfg_attr(feature = "clap", derive(ValueEnum))` on the
wavedude-export enums.

X-10 DRY. The wait-and-match on RunState repeats in export.rs:145, run.rs:106
and 186 (ignores Cancelled), truth.rs:166. Fix: `SimulationRun::
wait_completed()`. run.rs:171 builds `Colouring` per frame per layer.

X-11 FUTURE, CI. `rust-version = "1.87"` but wavedude-app uses let-chains
(1.88). CI never builds `wavedude-export --no-default-features` nor `wavedude-cli
--features gpu`; no MSRV job.

X-12 FUTURE, deps. Two `png` versions (0.17 in the CLI, 0.18 in export). The
`mp4` crate has had no release since 2023; the in-house MOV writer already
writes moov/co64, so an in-house avc1/avcC muxer is small. `OpenH264API::
from_source` compiles Cisco's source: Cisco's patent licence covers only
their binaries, which matters for distribution.

X-13 IDIOM. sequence.rs:215 `expect` on an I/O path; mp4.rs:79, 94 `check`
runs twice.

X-14 NOTE, containers. AVI (avih/strh/strf, indx, ix00, idx1, segments), MOV
(mvhd 100, tkhd 84, stsd, co64, 64-bit mdat, free-atom reserve) and MPEG-1
(pack and system header, PES, VLC tables, escapes, mismatch quantise) are
correct at the bit level. Two gaps: MPEG-1 SCR equals PTS (mpeg1.rs:322,
357), so later packets of a large picture arrive after its PTS (offset PTS
by about 9000 ticks); AVI `frame_bytes() as u32` (avi.rs:152) wraps for 4
GiB frames (reject in `new`).

Done well: small object-safe `FrameSink` with idempotent `finish`, writers
generic over `Write + Seek` and testable in memory; the job reuses one
halves and one RGBA buffer; containers and the MPEG-1 encoder are correct
and parallel per slice; `ExportFormat` is the one source of extensions and
capabilities; the CLI's tests decode what they export.

## wavedude-widgets and wavedude-app (A)

A-1 PERF. [preview/mod.rs:248-261](../../crates/wavedude-app/src/ui/preview/mod.rs#L248-L261), 281-285, polled every 4 ms (:537).
`SceneInputs::of` clones the whole `Scene`, image bytes included, and all
parts 250 times a second and compares them: with a 5 MB image about 1.25
GB/s of copying while idle. Fix: key the watch on scene and parts revision
counters bumped by `apply()`.

A-2 PERF/DESIGN. shared.rs:18-23, 75-89, state/mod.rs:292-303. One global
revision, bumped by `tick()` whenever the playback position changes, so at
the frame rate the menu memo rebuilds the MenuModel, the tree and properties
memos re-run, `PictureCell` rebuilds every handle (overlay.rs:616) and
`observable()` formats `run_status()` twice. Fix: separate document,
selection, playback, run and settings revisions with matching read helpers.

A-3 PERF. properties.rs:35-41, memo at :339. `object_json(Row::Scene)`
serialises `image_data` as a JSON array of numbers (about 32 bytes per
image byte, a 5 MB PNG becomes about 160 MB), repeated per playback frame
while Scene is selected, then cloned twice more (:342, :359). Fix: remove
`image_data` as commands.rs:599 does; `Snapshot.json` as `Rc<Value>`.

A-4 BUG. [preview/mod.rs:184](../../crates/wavedude-app/src/ui/preview/mod.rs#L184) vs export_dialog.rs:383-386 and
properties_forms.rs:69-75. The preview resolves schemes through
`SchemeStore::default()` (no user schemes), the export loads the user
folder: a user scheme is grey in the preview and coloured in the export;
the picker lists only project and built-in schemes. Fix: one SchemeStore
loaded into AppState at boot.

A-5 DESIGN. picture.rs:231-238. `CanvasImage::render` sets state and calls
the parent's `on_device_size` during render, which triggers another render.
Fix: report from `on_sized`.

A-6 PERF. picture.rs:78-94 with pictures.rs:91-115. `OverlayHook` and
`HitTestHook` compare by `Rc::ptr_eq`, but `PictureCell` makes new closures
per render, so `CanvasImage` never compares equal and re-renders with its
parent every frame. Fix: closures in `use_hook`, model through a State.

A-7 PERF. message_list.rs:159-161, 404. Every render clones up to 5000
`LogLine`s and recompiles the search regex, including renders from a hover.
Fix: memoise on (log revision, filter), keep indices.

A-8 DEAD CODE. playback.rs:19-63 `PlaybackClock` (used by its own test only;
playback runs off the 16 ms tick loop, app.rs:285); properties_forms.rs:1198
`target_of` (duplicates properties.rs:349-356); dialogs.rs:128
`import_project`; live.rs:312 `has_run`; number_box.rs:112
`is_logarithmic`; color_button.rs:44 `from_color`.

A-9 DRY. properties_forms.rs:561-742 (`part`, 182 lines), `fiber` :827,
`gate` :908, `scheme` :1083. Each kind defines the same closure under a
different name; every row names type, member, snake path and range key
(four spellings of one thing); `decimals` (:429) and `decimals_row` (:743)
are near-duplicates. Fix: a scoped builder that derives path and range key
from the member; one function per kind.

A-10 FUTURE. properties_forms.rs:563-573, 600, 739, commands.rs:4-6. Forms
branch on serde variant names and JSON paths; a rename empties the form
silently or fails at runtime with `NoField`, and no test catches it. Fix:
match on `PartKind`; a test that builds every form and resolves every row's
path through `Form::get`.

A-11 FUTURE. strings.rs:54-62. `Language::system()` reads POSIX variables
only, so Windows and macOS start in English. Fix: `sys-locale` with the
variables as fallback.

A-12 IDIOM/PERF. strings.rs:151-180. `get<'a>(&self, key: &'a str) -> &'a
str` ties the result to the key although the tables are `'static` (hence
dozens of `.to_owned()`); `label`, `hint`, `enum_word`, `kind` format a key
per lookup; `format` substitutes placeholders sequentially, so an argument
containing `{1}` is replaced too. Fix: `Cow<'static, str>`, single-pass
substitution.

A-13 FUTURE. settings.rs:86, 279-285. No version field; `std::fs::write` is
not atomic, a crash mid-write truncates the file and the next start
overwrites it with defaults. Fix: `version: u32`, write to a temp file and
rename.

A-14 DRY. pictures.rs:136-148 vs picture.rs:166-170, 360-369. `CanvasImage`
has a busy dim and spinner, `PictureCell` draws its own; pictures.rs:166-172
re-implements `layer_key`.

A-15 PERF. history.rs:62-64. `snapshot` clones the whole project including
the image, then sets `image_data = None`.

A-16 NOTE. presenter.rs:49, 259, mod.rs:324, 352. Change detection keys on
`Arc::as_ptr` as usize without holding the Arc; an address reused after a
free can skip a frame or show a stale outline. Fix: hold the Arc or use an
id.

A-17 NOTE. presenter.rs:247-251, mod.rs:336. A fresh RGBA Vec per frame
(the ImageHandle is unavoidable with Freya). A small pool via
`Bytes::try_into_mut`; measure first.

A-18 NOTE. app.rs:30, mod.rs:51. The 4 ms preview poll and the 16 ms tick
loop always run, about 310 wakeups a second idle; the tick loop has an idle
divisor, the poll does not.

A-19 FUTURE. `state/**` is Freya-free but lives in the UI crate, so it
cannot be built or reused without Freya and rfd. Fix: a `wavedude-app-state`
crate.

Checked and fine: the four language tables have identical key sets (a test
enforces it) and every key the code looks up exists; no nested
`fill_minimum` pitfall remains after the menu fix; the only `unwrap`s on
user paths are mutex poison and serde of the app's own types.

Done well: widgets carry no app knowledge (generic ids, hooks, labels, pure
parts unit-tested, gallery and headless tests); the Backend trait is a
narrow, mockable seam with a tested fake; `Command::Set` by serde path with
old/new trace logging avoids per-property command code and handles the
image; the presenter takes only the newest request and keeps worst-case
stats; playback uses a fractional carry so jitter does not drift the rate.
