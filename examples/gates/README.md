# Photonic gates

These projects recreate the demonstrations of the Huygens Optics video "Creating Logic using Waves: Photonic
Gates" (2026-11-05), in the video's order. Inputs A (upper) and B (lower) feed light into two fibres; Q (upper)
is the logic output, R (lower) the residual. Every project names what it shows, with its designed numbers, in its
description.

All projects use a frequency level of 50 (a wavelength of 32.04 px in vacuum, 17.80 px in the core). The fibres
are cores of index 1.8 in a medium of index 1.4, open edges with a 32 px absorbing rim. The layout comes from the
program's mode solver for the 4th-order stencil the engine runs: a 12 px single-mode core, an evanescent decay
length of 5.92 px, and for the coupled pair a 15 px gap with a coupling length of 1056.2 px.

| File | What it shows |
|---|---|
| [`01-single-fiber.wdproj`](01-single-fiber.wdproj) | One thin single-mode fibre. The point probe Inside reads a constant intensity along the fibre; the line probe Across shows the evanescent tail beyond the interfaces. |
| [`02-coupled-fibers-full-transfer.wdproj`](02-coupled-fibers-full-transfer.wdproj) | A second fibre B 15 px away: over one coupling length (1056 px) the light of A crosses over to B almost completely. |
| [`03-coupled-fibers-close.wdproj`](03-coupled-fibers-close.wdproj) | The same pair with half the gap (7 px): the coupling length drops to 272 px and the light goes back and forth almost four times. |
| [`04-crossover.wdproj`](04-crossover.wdproj) | The fibres approach, run side by side for one full transfer and bend apart: A alone exits at R, B alone at Q. |
| [`05-or-plain.wdproj`](05-or-plain.wdproj) | Half the coupling (a 3 dB coupler): A alone or B alone gives half at Q and half at R; both on give twice that level at Q. |
| [`06-or-neat.wdproj`](06-or-neat.wdproj) | The same coupler with B delayed by 30 degrees: Q shows the same level for A, B and both. |
| [`07-xor.wdproj`](07-xor.wdproj) | B delayed by 90 degrees: with both on, all light goes to R and Q goes dark. |
| [`08-xor-gate-part.wdproj`](08-xor-gate-part.wdproj) | The program's XOR gate part, its phase section built into arm B instead of a source delay. |
| [`09-or-gate-part.wdproj`](09-or-gate-part.wdproj) | The OR gate part. |
| [`10-and-gate-part.wdproj`](10-and-gate-part.wdproj) | The AND gate part: Q is A and B, R is A xor B. |

### Sources, mirror cup and probes

As in the video, each source sits in its fibre's core inside a mirror block, so no light leaves around the
source. An input lies on its fibre's axis, which for a core of an even number of cells is a cell boundary: it is two sources
named `A` and `A lower` (`B`, `B lower`) of half the amplitude, one in each of the two cells beside the axis, so the
field leaves the cup symmetrically. In `01` the block is a mirror rectangle 24 px deep and 48 px tall (2 by 4 core widths) that the fibre
passes through, the fibre starting 6 px from its back and the source in the middle of the block. In `02` to `07`
one E-shaped mirror cup holds both inputs: a mirror rectangle across both fibres, then a slot of the medium
(index 1.4, no absorption) for each fibre, 6 px wider than the core where the gap allows (2 px in `03`), then the
fibres, which are painted last and so win inside the slots.

Two line probes on the intensity read the light: Input across both fibres just after the cup (A at the top end
of the line, B at the bottom), Output across both outputs near the right end (Q at the top, R at the bottom). Each
line reaches 30 px (half a core and four decay lengths) beyond the outer cores, so its graph shows both peaks and
the evanescent tails; each line is its own mirror image about the pair's axis, so its upper half reads arm A or Q
and its lower half arm B or R. The Input graph sits at the lower left, the Output graph at the lower right. `01`
keeps the video's two probes, Inside (a point in the core) and Across (a line).

In `04` to `07` the fibres start 75 px apart, bend towards each other over 228 px, run parallel and bend apart
again, Q up and R down, back to 75 px; the bends end one lead (32 px) before the canvas's right edge and the
fibres run on into the absorbing rim, where the light leaves. The bends couple too (0.068 rad each), so the
straight run is shorter than the coupling length suggests: 965 px for the crossover, 437 px for the
3 dB coupler.

The gate parts (`08` to `10`) are designed for a blank canvas of index 1, so their projects use one: a 12 px core
of 1.8 in a cladding of 1.4, a 10 px gap, an 85 px coupler between two 260 px S-bends, and a 90 px phase section
of index 1.9 (30 px in the OR gate), the ports 82 px apart. Their inputs start at a mirror wall over the canvas's
left edge. Past each output end lies a 58 px square absorber pad of index 1 (absorption 255, rising over 29 px
from its outline), where the light of Q and R is dumped as in the video.

The projects were written by `wavedude-cli gates --out examples/gates --gpu`. The brightness of each example is the
app's Auto fit on its final frame, with the sources as saved.

## Truth tables

Each table comes from four runs with the sources off and on, as measured by `wavedude-cli truth-table <project>` on
the CPU engine. A value is the intensity summed over one half of a line probe and averaged over the last 256
steps: in A and in B over the upper and lower half of the Input line, Q and R over the upper and lower half of
the Output line. Q and R are relative to their sum with A alone, in A and in B relative to theirs. The expected
values assume a lossless coupler. A row passes when Q and R each lie within 0.1 of the expectation.

Plain OR ([`05-or-plain.wdproj`](05-or-plain.wdproj)): Q doubles with both inputs on.

| A | B | in A | in B | Q | R | Q expected | R expected |
|---|---|---|---|---|---|---|---|
| 0 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 |
| 1 | 0 | 0.995 | 0.005 | 0.469 | 0.531 | 0.500 | 0.500 |
| 0 | 1 | 0.005 | 0.995 | 0.531 | 0.469 | 0.500 | 0.500 |
| 1 | 1 | 1.004 | 1.004 | 0.992 | 0.992 | 1.000 | 1.000 |

Neat OR ([`06-or-neat.wdproj`](06-or-neat.wdproj)): B at -30 degrees, Q at the same level in the three true rows, a quarter of the light
at Q with both on.

| A | B | in A | in B | Q | R | Q expected | R expected |
|---|---|---|---|---|---|---|---|
| 0 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 |
| 1 | 0 | 0.995 | 0.005 | 0.469 | 0.531 | 0.500 | 0.500 |
| 0 | 1 | 0.005 | 0.995 | 0.531 | 0.469 | 0.500 | 0.500 |
| 1 | 1 | 1.004 | 1.002 | 0.496 | 1.490 | 0.500 | 1.500 |

XOR ([`07-xor.wdproj`](07-xor.wdproj)): B at -90 degrees, Q dark with both on.

| A | B | in A | in B | Q | R | Q expected | R expected |
|---|---|---|---|---|---|---|---|
| 0 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 |
| 1 | 0 | 0.995 | 0.005 | 0.469 | 0.531 | 0.500 | 0.500 |
| 0 | 1 | 0.005 | 0.995 | 0.531 | 0.469 | 0.500 | 0.500 |
| 1 | 1 | 1.002 | 0.998 | 0.005 | 1.994 | 0.000 | 2.000 |

XOR gate part ([`08-xor-gate-part.wdproj`](08-xor-gate-part.wdproj)): Q is A xor B, R is A and B.

| A | B | in A | in B | Q | R | Q expected | R expected |
|---|---|---|---|---|---|---|---|
| 0 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 |
| 1 | 0 | 0.999 | 0.001 | 0.484 | 0.516 | 0.500 | 0.500 |
| 0 | 1 | 0.001 | 0.914 | 0.479 | 0.449 | 0.500 | 0.500 |
| 1 | 1 | 1.005 | 0.919 | 0.003 | 1.924 | 0.000 | 2.000 |

OR gate part ([`09-or-gate-part.wdproj`](09-or-gate-part.wdproj)): Q is A or B, R the remainder.

| A | B | in A | in B | Q | R | Q expected | R expected |
|---|---|---|---|---|---|---|---|
| 0 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 |
| 1 | 0 | 0.999 | 0.001 | 0.484 | 0.516 | 0.500 | 0.500 |
| 0 | 1 | 0.001 | 0.938 | 0.478 | 0.450 | 0.500 | 0.500 |
| 1 | 1 | 0.998 | 0.936 | 0.464 | 1.464 | 0.500 | 1.500 |

AND gate part ([`10-and-gate-part.wdproj`](10-and-gate-part.wdproj)): Q is A and B, R is A xor B.

| A | B | in A | in B | Q | R | Q expected | R expected |
|---|---|---|---|---|---|---|---|
| 0 | 0 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 | 0.000 |
| 1 | 0 | 0.999 | 0.001 | 0.484 | 0.516 | 0.500 | 0.500 |
| 0 | 1 | 0.001 | 1.092 | 0.556 | 0.522 | 0.500 | 0.500 |
| 1 | 1 | 1.005 | 1.098 | 2.075 | 0.003 | 2.000 | 0.000 |

All rows pass. The inputs are clean: each source puts 0.5 % or less of its light into the other half of the Input
line, and both on read the sum of the single inputs. The fibre couplers send slightly more than half the light
across (0.531 instead of 0.500): the pair couples about 0.03 rad more than the solver predicts. In the gate parts,
the arm with the phase section reads lower already at the Input line (0.92 to 0.94 for B in the XOR and OR gates,
where the section is on arm B), and B alone reaches Q + R of 0.92 and 0.93 there. In the AND gate the section is
on arm A, the arm the table is normalised by, so B alone reads 1.09 at the input and Q + R of 1.09.

## Figures

The `figures` folder holds the scenes of `01`, `02` and `06` (red the index, black the mirror) and the intensity at
the last frame of `02`, `06`, `07` and `08`, with the glowing edges, the probe markers and their graphs, with the
sources as the projects are saved: A alone in `02`, A and B in the others.
