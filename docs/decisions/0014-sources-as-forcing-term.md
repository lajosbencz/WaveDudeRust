# 0014: Sources are a transparent forcing term, not WaveDude Next's blend

Status: accepted, 2026-11-11 (the user's decision). Replicating Next's blend was tried and rejected.

## Context

FDTD codes inject a source in one of three ways:

- **Hard**: the source sets its cell, `u = V`. The amplitude is exact, but the cell is a wall to every other wave:
  it scatters.
- **Soft**: the drive is added to the cell's update, `u_tt = c^2 L u + f` (Taflove & Hagness). This is the source
  term of the wave equation: linear, transparent to passing waves, but the radiated amplitude is not the drive's.
- **Transparent** (Schneider, Wagner and Broschat, JASA 103(1) 1998; Schneider, Wagner and Ramahi, IEEE TAP 46(8)
  1998): a soft source corrected by the grid's impulse response, so it radiates a prescribed field without
  scattering.

Next (and the original Wave Dude 2D, whose VB.NET source has the same line) blends the source cell once a step,
before its sub-steps, `u = 0.9 u + 0.1 V`: a 10 % hard source. Its documentation calls it "soft source injection";
0.9 is a tuning constant without a derivation. Its side effects are not modelled in either code base:

- the cell loses a tenth of its field every step, so a source scatters a passing wave (1.6 % of a pulse's peak),
  loads a cavity it drives (a resonance levels off at the source instead of growing) and couples to its neighbours
  in an array;
- its radiated amplitude depends on the time step and the sub-step count. Next's sub-steps keep the 4th-order
  stencil stable and follow the scene's lowest index; Next's own notes claim the result does not depend on them,
  which holds for propagation but not for the once-a-step source. In Example24 and Example33 one pixel where a
  fibre's edge averages a mirror pixel as index 1 (red 104 to 113) raises Next's sub-steps from 2 to 3 and its
  source amplitude by 22 to 31 %.

A physical emitter couples to the field only through a loss of its own, set by a physical parameter: the resistive
source of antenna FDTD (Luebbers and Langdon 1996), a source with an internal resistance that absorbs as a load.

## Decision

- A source is a soft source normalised by the engine's lattice Green's function at its frequency
  ([docs/physics.md](../physics.md), Sources): transparent, its Amplitude the swing of its cell in free space, independent of the
  sub-step count. This is the transparent source's idea in the frequency domain.
- Next's blend is not replicated. A replica (closed-loop solve per frequency and cell speed) brought 29 of the 33
  examples within 6 % of Next's brightness, but it copies a discretisation artefact; the comparison verifies, it
  does not calibrate.
- A lossy emitter is built from parts: an absorbing part over the source.

## Consequences

- Against Next, the brightness that fits its field over the one that fits ours is 4.1 to 14.2 across the 33
  examples at step 600. `wavedude-cli compare` reports it and it is evaluated, not a failure; the shape measures
  (field Pearson, the fits after normalisation, probe correlation, render parity) are the verification.
- Next's resonators driven at resonance level off at the source; ours grow until damping or absorbers limit them
  (Example08, 14, 15, 16 differ most over long runs). Arrays are exact superposition.
- The `.wdproj` adapter converts Next's units with one fixed factor, not a per-project fit: `Brightness` by 8 and a
  detector's `Sensitivity` by 64 (`BRIGHTNESS_GAIN`, `SENSITIVITY_GAIN`). Projects from Next render 0.5 to 2 times
  as bright as in Next.
