# Comparison with the C# WaveDudeNext

Two runs. The sweep of all 33 example projects (2026-11-12, next section) is the current picture. The sections
after it are the deep dive of 2026-11-06 into six projects, with their reports in the folders here; their
numbers predate later changes (Example01's gain was 7.6 then and is 7.8 in the sweep), their explanations stand.

## Sweep of the 33 examples (2026-11-12)

[`tools/compare-csharp/sweep.sh`](../../tools/compare-csharp/sweep.sh): both CPU engines, 600 steps, fields dumped every 100, WaveDudeNext at 05a0348
(2026-11-09), the Rust engine at the commit that added this section. Each program paints its own scene and runs
its own sub-step rule. Reproduce with `tools/compare-csharp/sweep.sh <out-dir>`; the reports are not kept here.

What agrees:

- **Rendering.** Next's own field and intensity planes through our renderer at the file's brightness: 0 pixels
  differ in all 198 dumped frames. The probe-graph layouts are the same. Every picture difference comes from the
  simulation, none from the display.
- **Where the light goes.** The time-averaged intensity correlates at 0.85 to 1.00 at step 600, 0.97 or more in
  25 of 33.
- **Probes that see a wave.** 17 of the 19 comparable probes correlate at 0.85 to 1.00.
- **Painted scenes.** Identical in 15 of 33; elsewhere up to 1.96 % of the pixels differ, all along index edges
  (sub-cell smoothing, decision 0010; the mirror rule of [spec/05-deviations.md](../spec/05-deviations.md)).

What does not:

- **Brightness.** Next's field takes 4.1 to 14.2 times our brightness (median 7.8) for the same picture. The
  adapter's fixed conversion of 8 leaves projects from Next 0.37 to 2.09 times as bright as there (luma, median
  1.0). The cause is the source model (decision 0014): Next's blended source radiates an amplitude that depends on
  the frequency, the medium, nearby sources, returning waves and its sub-step count.
- **The instantaneous field.** At step 600 the field Pearson is below 0.8 in 12 of 33 and below 0.7 in 8 (02, 06,
  09, 17, 20, 22, 28, 33), while the intensity of the same examples stays at 0.85 to 0.99: the patterns agree, the
  phases drift apart over time (Example20: 0.985 at step 100, 0.639 at 600, intensity 0.971). Example09 differs
  by design (the true Laplacian's speed, below). For the rest the causes are the two source models' phase and the
  different dispersion of the DRP and Taylor stencils; the drift has not been split between them per example, and
  this comparison cannot say which side is closer to the physics. The analytic tests (Correctness measures) can.
- **Probes that compare nothing.** 23 of 42 probes record nothing in Next within 600 steps (the wave has not
  arrived), so they test nothing at this run length. Example28's spectrum line (0.25) is at the arriving front:
  Next's series is 2e4 times weaker there than ours, so ours arrives earlier or stronger; not isolated.
  Example32's photon counter line correlates at 0.53 at the expected unit ratio (62 against 64); not isolated.
- **Detectors.** The tool compares a probe's mean intensity, not the detections; the Sensitivity conversion of
  the adapter is not checked against Next's counts here.
- **Example24 and Example33.** One painted pixel where a fibre's edge averages a mirror pixel as index 1 raises
  Next's sub-steps from 2 to 3, and its source amplitude with them by 22 to 31 % (decision 0014).

| example | painted scene differs | field Pearson step 100 / 600 | intensity Pearson 600 | brightness fit C# / Rust | luma Rust / C# | probe Pearson |
|---|---|---|---|---|---|---|
| 01 | identical | 0.980 / 0.955 | 0.999 | 7.8 | 0.99 | 1.000 |
| 02 | identical | 0.841 / 0.657 | 0.985 | 7.6 | 1.45 | none (1 silent in Next) |
| 03 | identical | 0.730 / 0.790 | 0.967 | 11.6 | 1.75 | none (2 silent in Next) |
| 04 | identical | 0.983 / 0.968 | 0.986 | 7.8 | 0.99 | 1.000 |
| 05 | identical | 0.908 / 0.815 | 0.956 | 8.2 | 1.00 | 0.854, 0.980 |
| 06 | identical | 0.878 / 0.691 | 0.961 | 9.2 | 1.24 | 0.941, 1.000 |
| 07 | identical | 0.000 / 0.971 | 0.972 | 7.7 | 1.00 | none (2 silent in Next) |
| 08 | 0.23 % | 0.999 / 0.997 | 1.000 | 4.1 | 0.37 | 0.997, 0.998 |
| 09 | identical | -0.206 / 0.209 | 0.873 | 5.9 | 0.92 | no probes |
| 10 | 0.08 % | 0.733 / 0.803 | 0.995 | 7.9 | 1.01 | no probes |
| 11 | identical | 0.980 / 0.972 | 0.967 | 7.9 | 0.98 | none (1 silent in Next) |
| 12 | 0.12 % | 0.816 / 0.794 | 0.977 | 14.2 | 1.18 | no probes |
| 13 | 1.96 % | 0.949 / 0.921 | 0.998 | 6.8 | 0.85 | 0.998 (1 silent in Next) |
| 14 | 0.15 % | 0.997 / 0.984 | 0.988 | 6.0 | 0.70 | none (2 silent in Next) |
| 15 | 0.17 % | 0.756 / 0.805 | 0.983 | 13.4 | 1.11 | no probes |
| 16 | identical | 0.970 / 0.982 | 0.989 | 4.8 | 0.59 | 0.977, 0.961 |
| 17 | < 0.01 % | 0.833 / 0.357 | 0.930 | 9.3 | 1.16 | none (2 silent in Next) |
| 18 | 0.61 % | 0.992 / 0.991 | 0.998 | 6.5 | 0.75 | 0.999 |
| 19 | identical | 0.948 / 0.805 | 0.996 | 9.7 | 1.22 | 1.000 |
| 20 | 0.06 % | 0.985 / 0.639 | 0.971 | 10.9 | 1.18 | none (2 silent in Next) |
| 21 | identical | 0.994 / 0.989 | 0.988 | 6.5 | 0.75 | 1.000 (1 silent in Next) |
| 22 | identical | 0.837 / 0.659 | 0.985 | 7.5 | 1.55 | 0.996 (1 silent in Next) |
| 23 | 0.36 % | 0.966 / 0.956 | 0.991 | 8.6 | 1.13 | none (2 silent in Next) |
| 24 | 0.41 % | 0.979 / 0.872 | 0.998 | 6.9 | 0.97 | 1.000 (1 silent in Next) |
| 25 | 0.14 % | 0.916 / 0.716 | 0.943 | 10.0 | 1.27 | no probes |
| 26 | 0.15 % | 0.751 / 0.898 | 0.998 | 8.7 | 1.09 | none (1 silent in Next) |
| 27 | 1.17 % | 0.959 / 0.941 | 0.983 | 7.4 | 0.84 | no probes |
| 28 | 0.05 % | 0.561 / 0.652 | 0.993 | 11.0 | 1.47 | 0.247 |
| 29 | identical | 0.753 / 0.762 | 0.996 | 10.9 | 2.09 | no probes |
| 30 | 0.33 % | 0.940 / 0.895 | 0.977 | 5.1 | 0.56 | no probes |
| 31 | 1.16 % | 0.992 / 0.963 | 0.999 | 6.6 | 0.77 | none (2 silent in Next) |
| 32 | identical | 0.979 / 0.969 | 0.975 | 7.8 | 0.99 | 0.533 |
| 33 | 0.21 % | 0.919 / 0.533 | 0.846 | 10.4 | 1.35 | none (2 silent in Next) |

## Deep dive of 2026-11-06: method

- **Same inputs.** The projects are the Rust repository's copies of the C# `Examples` files (byte for byte
  the same); 07-xor is [`examples/gates/07-xor.wdproj`](../../examples/gates/07-xor.wdproj). Each program
  paints its own scene from the project's parts, and the two paintings are compared pixel by pixel.
- **Same step schedule.** 600 steps with the fields dumped every 100 (07-xor: 2592 steps every 432, its own
  run length), probes sampled every step.
- **Each engine at its own settings.** The C# engine runs its own sub-step rule (3 sub-steps at index 1 on
  the 4th order, 1 on its 9-point kernel), the Rust engine its CFL rule (2 at index 1 on both stencils, 1 in
  07-xor where the C# takes 2). Nothing is forced to match.
- **Tools.** [`tools/compare-csharp`](../../tools/compare-csharp/) dumps the C# scene, fields, intensities and probe series;
  `wavedude-cli compare` runs the Rust engine and writes `report.md` with the pictures (C# | Rust | Rust / gain -
  C#) into each folder below.

Measures in the tables:

- **Field Pearson**: correlation of the two fields over the canvas at the same step.
- **Shape error after gain**: RMS of (Rust / g - C#) over the RMS of C#, g the least-squares gain.
- **Gain**: g itself, informational only (see "Source amplitude" below).
- **Probe correlation**: Pearson of the probe series over the second half of the run.
- **Fronts and wavelengths**: the farthest pixel right of a source above 1 % of the row's peak, and the
  zero-crossing wavelength along the source's row.

## Per-scene results

| scene | what it shows | painted scene | field Pearson (first / last dump) | shape error after gain (first / last) | gain Rust / C# | probe correlation | fronts, wavelengths | report |
|---|---|---|---|---|---|---|---|---|
| Example01 | pulsed source, tilted mirror, line probe; rim 64 | identical | 0.980 / 0.969 | 20 % / 26 % | 7.6 | 1.000 | fronts within 3 px (598 / 597 at step 600); 16.0 / 16.0 px at step 300 | [report](Example01_Source_Mirror_Probe/report.md) |
| Example09 | four sources at levels 30 to 240, **9-point** stencil; rim 32 | identical | about 0 (other speed, by design) | - | - | no probes | wavelengths 29.9 / 29.8, 14.9 / 14.9, 7.4 / 7.4 px at levels 30, 60, 120; 3.7 / 3.3 px at 240; fronts 337 / 598 px, ratio 1.77 | [report](Example09_Laplacian_dispersion/report.md) |
| Example16 | composite sources in two absorbing boxes; open, no rim | identical | 0.970 / 0.982 | 25 % / 19 % | 4.8 | 0.976 (intensity), 0.961 (field) | fronts within 1 px | [report](Example16_Wave_in_2_boxes/report.md) |
| Example18 | XOR, AND and OR gate parts; damping band of 32 inside the canvas, open, no rim | 0.61 % of pixels, all within 2 px of an index edge, up to 8 levels | 0.992 / 0.992 | 13 % / 13 % | 6.4 | wave has not reached Probe 1 | wavelengths within 0.1 px where one source owns the row | [report](Example18_XOR_AND_OR_logic/report.md) |
| Example21 | ball lens of index 2.55 with a graded coating; rim 90 | identical | 0.994 / 0.989 | 11 % / 15 % | 6.3 | 1.000 (Probe 2; Probe 1 sees no wave by step 600 in either) | fronts 598 / 597 px, wavelength 39.9 / 39.9 px (theory 40.1) | [report](Example21_Ball_lens/report.md) |
| 07-xor | fibre coupler XOR, both inputs on; rim 32 | 0.57 % of pixels, all within 2 px of an index edge, up to 5 levels | 0.965 / 0.948 | 27 % / 34 % | 9.4 | 0.998 (output), 0.870 (input) | wavelengths within 1 px; fronts within 5 to 43 px in the guides (dispersion, see below) | [report](gates-07-xor/report.md) |

**The shape error against the C# is not a correctness measure.** It is bounded from below by the two source
models (below): the C# relaxes its source cell towards the drive once per step, the Rust adds a forcing per
sub-step normalised by the lattice Green's function, and the two radiate the same wave at a different phase.
Shifted in time to the best match, the Rust field agrees with the C# at a Pearson of 0.987 to 0.999 and a
shape error of 5 to 17 % (table below), which is what is left of the solvers' differences. Neither number
says which program is right; the analytic tests below do.

Time offset that best matches the two fields (Rust step against the C# step; scratch probe, the fields of
`wavedude-cli compare` at neighbouring steps):

| scene | C# step | best Rust step | Pearson there | shape error there | same with 30 px around each source left out |
|---|---|---|---|---|---|
| Example01 | 600 | 601 | 0.987 | 17 % | same |
| Example16 | 600 | 602 | 0.992 | 13 % | same |
| Example18 | 600 | 601 | 0.996 | 9 % | 0.997, 8 % |
| Example21 | 100 / 600 | 101 / 601 | 0.999 / 0.995 | 5 % / 11 % | same |
| 07-xor | 432 / 2592 | 433 / 2594 | 0.996 / 0.995 | 9 % / 10 % | same |

Leaving out the source's neighbourhood changes nothing, so the difference is not a near-field effect but a
phase offset of the radiated wave of one to two steps (Example21: 0.9 steps at step 100, 0.8 at 600, constant)
and a slowly growing one in 07-xor (1.3 steps at 432, 1.7 at 2592), where the dispersion differs most (C#
leapfrog at 2 sub-steps against the Rust's modified equation at 1, in fibre guides).

## Where they agree

- **Scene geometry.** Rectangles, mirrors, absorbers and the ellipse with its graded coating paint byte for
  byte the same. Fibres and gate parts differ only within 2 px of an index edge (100 % of the differing
  pixels), by 1 to 8 red levels: the Rust painter takes the area-weighted mean of n^2 at a pixel cut by an
  edge (decision 0010), the C# the 16-sample mean of n.
- **Speeds and wavelengths on the 4th-order projects.** Fronts within 1 to 3 px after 600 steps, wavelengths
  within 0.1 px wherever one source owns its row (Example21: 39.9 / 39.9 px against 40.1 from the frequency
  level).
- **Wave pattern.** Intensity Pearson 0.96 to 1.00 at every dump on all 4th-order scenes (0.999 in 07-xor):
  the interference patterns, the lens focus and the guided modes are the same. The field differs by the phase
  offset above.
- **Probes.** 1.000 for the line probes of Example01 and Example21, 0.998 for 07-xor's output, 0.96 to 0.98
  for Example16's two; the input probe of 07-xor (0.870) sits next to the sources, where the two source models
  differ most.
- **Gate logic.** 07-xor with both inputs on: the C# dump puts 0.2 % of the output into Q (previous run; the C#
  side is unchanged), the Rust truth table 0.25 % (`wavedude-cli truth-table examples/gates/07-xor.wdproj`: 4 of 4
  rows within tolerance; one input alone 0.468 / 0.532).
- **9-point wavelengths.** Example09 as loaded (the importer divides the frequency scale by 0.562): 29.9 /
  29.8 px at level 30, 14.9 / 14.9 px at level 60, 7.4 / 7.4 px at level 120, at every dump.

## Where they differ, and why

Each difference follows from a documented decision; none is unexplained.

1. **Source amplitude (gain 4.8 to 9.7).** The Rust source is an additive forcing normalised by the free-space
   lattice Green's function, so a source's Amplitude is the swing of its own cell in free space at index 1
   ([docs/physics.md](../physics.md), Sources). The C# blends the cell, `u = 0.9 u + 0.1 v`, once per step before its
   sub-steps, so its cell swings far less than the Amplitude and its radiation grows with its sub-step count.
   The gain is therefore a units difference, not an error: it depends on the frequency, the envelope and the
   C#'s sub-step count (9.4 in 07-xor at the C#'s 2 sub-steps against 6.3 to 7.6 at its 3, the ratio of the
   C#'s own energies at n = 2 and 3). Projects imported from the C# are brighter in the Rust app at the same
   brightness setting; the Rust brightness no longer depends on what the scene does to the sub-step count. The
   two source models also put a different phase on the radiated wave, the one-step offset above.
   Over the 33 examples (2026-11-11, 600 steps) the brightness fit ratio C# / Rust ranges 4.1 to 14.2; in
   Example24 and Example33 one painted pixel of index 1.04 to 1.13 (a fibre's edge over a mirror pixel) sets the
   C#'s sub-steps to 3 instead of 2 and its amplitude 22 to 31 % higher. Decision 0014 records why the blend
   is not replicated.
2. **The true Laplacian speed (Example09).** The C#'s 9-point kernel (0.184 axis, 0.066 diagonal) runs at
   sqrt(0.316) = 0.562 px per step for long waves; the Rust's unscaled Laplacian at 1 (decision 0003). With the
   importer's frequency scale / 0.562 the wavelengths match, so the wave crosses the canvas 1.78 times as fast
   per step: fronts 337 / 598 px at step 600 (ratio 1.77, the 1 px lag being the threshold measure), and the
   step-for-step fields are uncorrelated by construction. Level 240 cannot match: at 3.6 px a wavelength both
   kernels are dominated by their dispersion; the Rust run gives 3.3 px and its front travels at the group
   speed, 384 px at step 600 (0.64 of the long-wave speed). Both runs show the grid's X at that level.
3. **The CPML rim.** The Rust rim is a CPML (decision 0010; -84 to -104 dB in [docs/physics.md](../physics.md)) where the C#
   has its quadratic sponge (0.09 % of a long-wave pulse in the C#'s notes). No edge reflection shows in the difference pictures
   of Example01, Example09, Example21 or 07-xor (rims 64, 32, 90, 32): the residual is the front ring and the
   phase offset, nothing along the edges.
4. **The DRP dispersion.** The 4th-order projects run the DRP stencil with the modified-equation time scheme in
   the Rust, the Taylor weights with leapfrog in the C#. At the wavelengths here (16 to 40 px) the phase
   speeds differ slightly ([docs/physics.md](../physics.md), Numerical dispersion), which shows as the slow growth of the shape error over time (Example21: 11 %
   to 15 % at equal steps, 5 % to 11 % at the best offset) and the growing offset in 07-xor's guides, whose
   front columns differ by 5 to 43 px at steps 432 to 1296 (a threshold measure on a slow guided front).
5. **The additive source's near field against the blended one.** The blended C# cell is a damped oscillator
   pinned towards the drive; the Rust cell is transparent and swings by its Amplitude. Leaving 30 px around the
   sources out changes the correlations by under 0.001, so this is confined to a few cells; it shows in the
   07-xor input probe (0.870), which lies close to its sources.
6. **The sub-cell smoothing at edges.** The 0.57 % and 0.61 % of painted pixels in 07-xor and Example18 differ
   by 1 to 8 red levels along index edges (exact coverage against 16 samples). Their effect is inside the
   wavelength agreement above (within 0.1 px in Example18, within 1 px in 07-xor's guides).

## Example18: stable now

The previous run diverged from step 160 at the top edge (x 652 to 674, y 0), growing 40 times every 10 steps,
in the damping band of 32 at an open edge without a rim at 2 sub-steps. With Mur including the edge cell's
damping and the second and third rings continued linearly ([spec/05-deviations.md](../spec/05-deviations.md)), the field stays bounded
over the whole run: gain 6.40 to 6.42 and energy ratio 41.5 to 42.0 at every dump from 100 to 600, Pearson
0.991 to 0.994, the intensity at 0.998 by step 600. [docs/physics.md](../physics.md) records Example18 bounded over 3000 steps
on both Rust engines, and a corner model with an inside damping band bounded over 5000 steps at 1 to 4
sub-steps.

## Correctness measures

What shows the Rust engine is right is not its agreement with the C#, but the analytic tests of
[docs/physics.md](../physics.md) ([tests/physics.rs](../../crates/wavedude-core/tests/physics.rs), [tests/gpu.rs](../../crates/wavedude-core/tests/gpu.rs)):

- **Transparency**: a wave packet crossing a silent source is the same bit for bit.
- **Superposition**: crossing a weak continuous source, the field minus the source's own is the packet's
  within 7e-7 of its peak.
- **Sub-step independence**: the RMS field of a continuous source at 1 to 4 sub-steps within 0.4 % (9-point)
  and 0.5 % (DRP); the source's cell swings by its Amplitude within 0.1 %.
- **Energy**: 2.2e-7 (9-point) and 2.4e-7 (DRP) relative drift over 10,000 steps.
- **Dispersion against the analytic relation**: standing waves within 1.5e-8 to 4.6e-7 of the discrete
  relation; a point source radiates the relation's wavelength within 1 %.
- **PML reflection**: -74 to -106 dB for long and short packets at normal and 45 degrees incidence.
- **Slab guide against the mode solver**: the phase slope along the core within 5.8e-5 (9-point) and 1.0e-4
  (DRP) of the solver's beta.

## Verdict per scene

- **Example01**: no sign of a mistake. Identical scene, same fronts and wavelengths, probe 1.000; the
  difference is the source model's phase (one step) and amplitude.
- **Example09**: no sign of a mistake. The import factor now keeps the wavelengths (14.9 px at level 60 on
  both sides, as expected); the 1.77 front ratio is the deliberate true-Laplacian speed. The behaviour at level
  240 is each kernel's dispersion near 4 px a wavelength.
- **Example16**: no sign of a mistake. Fronts within 1 px; the 19 to 25 % shape error falls to 13 % at a
  two-step offset, the composite sources' 58 emitters each carrying the source model's phase.
- **Example18**: fixed. Bounded over the run, correlated at 0.99, wavelengths within 0.1 px. The Source 1
  front column at steps 200 and 300 (C# 536 and 546, Rust 116 and 168) is the 1 % threshold on a row that other
  waves cross too (the report's own caveat), not a field difference (Pearson 0.991 and 0.992 at those steps).
- **Example21**: no sign of a mistake. Fronts and wavelength match the theory; the shape error at the best
  offset grows 5 to 11 % over 600 steps, the DRP's documented dispersion difference.
- **07-xor**: no sign of a mistake. Same gate logic, output probe 0.998, intensity 0.999; the largest shape
  error of the set (34 %) is 10 % at a 2-step offset, the rest the source phase and the different dispersion
  at the C#'s 2 and the Rust's 1 sub-step.

## The 9-point runs of `wavedude-cli compare`

A 9-point project is compared twice: as loaded (the importer divides the frequency scale by the original
kernel's phase speed 0.5621, so Example09's wavelengths come out 29.9 / 14.9 / 7.4 px against the C#'s
29.9 / 14.9 / 7.4 at levels 30 / 60 / 120) and unscaled (the original's frequency scale, so the waves are
1/0.562 longer). The fronts travel 1.77 times as far per step in Rust, the deliberate change of decision
0003. The time-offset table above came from a scratch probe; `compare` reports equal steps only.

## Reproducing

```sh
WAVEDUDE_NEXT_DIR=<checkout of graealex/WaveDudeNext> \
  tools/compare-csharp/run.sh <project.wdproj> <dump-dir> --steps 600 --every 100 --probe-steps
cargo run -p wavedude-cli --release -- compare <project.wdproj> <dump-dir> --out docs/comparison/<name> --steps 600 --every 100
cp examples/gates/07-xor.wdproj <dir>/07-xor.wdproj   # then --steps 2592 --every 432
```

The projects are `crates/wavedude-core/tests/fixtures/examples/<name>.wdproj`. The C# side's requirements are in
[`tools/compare-csharp/README.md`](../../tools/compare-csharp/README.md).
