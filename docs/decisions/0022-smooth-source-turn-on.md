# 0022: A source drives its impulse, turned on smoothly

Status: accepted, 2026-11-13 (the user's decision). Amends [0014](0014-sources-as-forcing-term.md): sources stay a
transparent forcing term; how a drive starts and ends changes.

## Context

A source added `f = A / |G0| e(t) sin(omega t + phi)` from t = 0 with e = 1 at once. The force is switched on
abruptly, so its time integral, the impulse given to the field, keeps the offset `A cos(phi) / (|G0| omega)` on top
of its oscillation. In 2D an impulse leaves a zero-frequency wake behind the front that decays as 1/t; over the
sources of an array it adds up. Against WaveDude Next at steps 300 and 600 (`tools/compare-csharp/sweep.sh`), the
field's mean was 0.31 and 0.40 of its RMS in Example02 (Next: 0.04, -0.01), 0.23 and 0.30 in Example22, 0.12 in
Example29; the 64 px block means reached 0.5 to 0.6 of the RMS and washed out the coherent-source pictures.

Next blends the source cell towards its value once a step, `u = 0.9 u + 0.1 V`. Its single sources leave a similar
wake (a free-space source at phase 0, 60 cells away: mean 0.072 of the amplitude in Next, 0.090 here); in its
arrays the blend, which takes a tenth of every source cell's field each step, drains it. That loss is what 0014
rejects.

## Decision

- A source drives its impulse `p = -A / |G0| E(t) cos(omega t + phi) / omega`; the forcing is `f = dp/dt`,
  `A / |G0| sin(omega t + phi)` while E = 1, so the steady state and 0014's normalisation are unchanged. A drive
  whose envelope starts and ends at 0 delivers no net impulse, whatever its phase, frequency or pulse shape.
- Each sub-step takes the difference of p across it, scaled to the steady drive: the sub-steps' impulses
  telescope, so the discrete drive leaves none either, up to round-off ([docs/physics.md](../physics.md), Sources).
- E is the turn-on times the pulse envelope times the detach fade. The turn-on is a raised cosine over half a
  period of the source, `(1 - cos(pi t / T)) / 2`, `T = pi / omega`; the detach fade keeps its 30 steps and falls
  as the mirrored raised cosine instead of linearly. Both are continuous with their slope. Every drive starts at
  t = 0: an edit resets the engine.
- The table of values is computed once on the CPU for both engines ([engine/forcing.rs](../../crates/wavedude-core/src/engine/forcing.rs)); the
  shaders add it as before.

The length. With the impulse driven, the pedestal is gone at any turn-on length; the length shapes only the
start. Next's cell rises as `1 - 0.9^n`: 50 % after 6.6 steps, 63 % after 9.5. Half a period at the coherent
examples' frequency (level 50 at scale 2, 32 steps a period) is 16 steps: 50 % after 8, 63 % after 9.4. A length
in periods keeps the start the same in the source's own time at every frequency (the `E'` term peaks at
`1 / (4 k)` of the drive for k periods); Next's rise is fixed in steps. Field Pearson against Next, summed over
Examples 02, 11, 21, 22, 29 at steps 300 and 600, is flat in the length: 8.82 for an abrupt start, 8.85 to 8.86
for 6 to 16 steps or a quarter to half a period, 8.83 for a whole period, against 8.40 before; half a period is the
highest.

Rejected: ramping the force without the `E'` term. The pedestal it leaves depends on the phase: a 16-step ramp
removes it at phase 0 and leaves 7 % of the local amplitude at phase 90 degrees 60 cells away, which an abrupt
start does not have; only ramps of several periods make it small at every phase, far slower than Next's start.

## Consequences

- The field's mean is near 0 behind the front: Example02 -0.000 and 0.002 of its RMS at steps 300 and 600,
  Example22 -0.002 and 0.005, Example29 0.000; field Pearson against Next 0.822 to 0.960 and 0.657 to 0.721
  (Example02), 0.807 to 0.956 and 0.659 to 0.750 (Example22), 0.765 to 0.815 and 0.762 to 0.779 (Example29).
- Single sources lose the wake Next keeps: Example11 0.976 to 0.964 and 0.972 to 0.968, Example21 0.991 to
  0.968 and 0.989 to 0.983. The start of a source at phase 0 differs most from Next's, which rises from the
  abrupt `sin` with its pedestal ([divergences.md](../divergences.md)).
- Without the pedestal the field pictures are darker against Next's: the field luma ratio of Example02 went from
  1.17 and 1.45 to 0.47 and 0.70; the brightness gap of 0014 shows where the pedestal had hidden it.
- Pulses keep their envelopes; their forcing gains the `E'` term, so a short Gaussian pulse leaves no wake either.
- Past Nyquist (`omega dt >= pi` per sub-step) a drive is aliased; its turn-on keeps only the envelope's mean, as
  the `dE` term's `tan(omega dt / 2)` nears 0 at multiples of `2 pi`.
