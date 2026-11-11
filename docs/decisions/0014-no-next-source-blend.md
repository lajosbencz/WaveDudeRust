# 0014: Sources stay a forcing term; WaveDude Next's blend is not adopted

Status: rejected, 2026-11-11. The source model of decision 0010 stands.

## Context

The comparison against Next (`wavedude-cli compare`, tools/compare-csharp) showed the renderer and the probe-graph
layout byte-identical on the same data and the whole brightness gap in the field amplitude: the brightness that
fits Next's field over the one that fits ours is 4.1 to 14.2 across the 33 examples at step 600. Next blends the
source cell once a step before its sub-steps, `u = 0.9 u + 0.1 V`. A replica of the blend, solved per frequency and
cell speed so that our scheme's cell answers a drive and an incoming wave as Next's does, brought 29 of the 33
examples within 6 %.

## Decision

Not adopted. The blend is a relaxation of the cell towards the drive, not a source term of the wave equation:

- it removes a tenth of the cell's field every step, so a source absorbs and re-radiates a passing wave (1.6 % of a
  pulse's peak);
- its radiated amplitude depends on the time step and Next's sub-step count, which the scene's lowest index sets: in
  Example24 and Example33 one pixel where a fibre's edge averages a mirror pixel as index 1 (red 104 to 113) raises
  Next's sub-steps from 2 to 3 and its source amplitude by 22 to 31 %.

A physical source is linear and additive, transparent to passing waves, with an amplitude independent of the
discretisation. Matching Next's brightness would mean calibrating on Next's numerics; the comparison verifies, it
does not calibrate.

## Consequences

- The amplitude ratio against Next is reported by `wavedude-cli compare` and evaluated, not a failure; the shape
  measures (field Pearson, the fits after normalisation, probe correlation, render parity) are the verification.
- The `.wdproj` adapter keeps its fixed brightness conversion of 8 (`BRIGHTNESS_GAIN`), a unit conversion, not a
  per-project fit: projects from Next render 0.5 to 2 times as bright as in Next.
