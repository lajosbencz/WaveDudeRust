# 05 - Deliberate deviations from the original

The spec files describe WaveDude Next as it is. This file lists where the Rust
implementation departs from it on purpose (the user's decisions, recorded in
docs/decisions). Where a spec file and this file disagree, this file wins.

## Engine (30, 70)

- Stencils are unscaled Laplacians (decision 0003): isotropic 9-point
  (diagonals 1/6, axes 2/3, centre -10/3) and the 4th order, a 7-point-per-axis
  stencil with dispersion-relation-preserving weights run 4th order in time by
  the modified equation (decision 0010; the original's Taylor -1/12, 4/3, -5/2
  per axis is not used). The original's 0.066 / 0.184 / -1 kernel is not used.
- A mirror pixel (red 0) is a fixed zero (c2 = 0), not a cell of index 1000,
  and a stencil tap across a mirror is dropped, the 9-point's diagonals
  included (docs/physics.md, Spatial stencils and Stability).
- Sub-steps are derived per stencil from the CFL bound with the scene's
  largest c2 (docs/physics.md), for both stencils. The original's rule
  (4th order only, `ceil(sqrt(maxC2 * 32/3 / 2))`) is not used.
- Bit-for-bit equality with the C# engines is not a requirement, nor is a ban
  on fused multiply-add. The guarantees in 70 that say "bit-identical to the
  C# engine" (G-01) become "within documented tolerance" and the analytic
  tests of docs/physics.md are the correctness proof.
- The CPU and GPU engines of this project agree within a documented
  tolerance (relative 1e-4 on the field after 1000 steps, 1e-3 on sqrt(I)).
- The GPU engine is wgpu, not OpenCL; its work-group shape is its own.
- Summation order, f32 literal values and the "round through f32" of the
  object source frequency are not load-bearing. Keep f32 for the field.
- The importer of Wave Dude 2D projects and of `.wdproj` files scales the
  `Simulation.SourceFrequencyScale` by the original 9-point kernel's phase
  speed, sqrt(0.316) = 0.562 (its long-wave expansion is (axis + 2 diagonal)
  = 0.316 times the Laplacian), for projects that used the 9-point stencil so imported pictures keep
  their wavelengths, and logs that it did.
- A source's position is a decimal in cells (the original's an integer pixel): a
  whole number is the original's cell, a fraction spreads the drive over the 2 x 2
  cells around it with bilinear shares, normalised by the spread source's own
  response, so a source can sit on a fibre axis that lies on a cell boundary
  (docs/physics.md, Sources). The `.wdproj` adapter writes a whole number without
  a point, as the original does, and a fraction with one, which the original's
  reader does not take.
- Sources are a forcing term (docs/physics.md, Sources): each sub-step adds
  `dt^2 f(t_k)` with `f = A / |G0(omega)| e(t) sin(omega t + phi)` where the scheme
  takes `A u`, G0 the free-space lattice Green's function of the engine's
  operator at the source cell, so a source is transparent to passing waves, its
  radiation does not depend on the sub-step count, and Amplitude A is the
  displacement amplitude a continuous source gives its own cell in free space
  (index 1). The original's `SourceOpacity = 0.9` blend once per step
  (30-engine.md, Sources) is not used: its radiated amplitude grew with the
  sub-step count. A source on a mirror cell drives nothing.
- Damping is centred: `u+ = u + d (u - u-) + (1 + d)/2 B u` instead of
  `u+ = u + d (u - u-) + B u`; same decay per sub-step, stable for every d in
  [0, 1] (the original's form diverges at d near 0 at two sub-steps of the
  modified equation, Example18's damping band).
- Mur's condition on open edges without a rim includes the edge cell's damping
  and sets only the first padding ring; the second and third continue the
  field linearly (the original's ring-by-ring chain diverged at the corners on
  the DRP stencil at Courant numbers above about 0.75).

## Rounding rules (00)

The round-half-even and truncation notes exist to reproduce the C# bytes. The
Rust code uses the natural, documented rounding for each place and the tests
compare against fixtures with tolerances, except where a value is an index or
a byte that the UI or a file shows (palette indices, painted red levels,
frame indices), where the spec's rule is kept so pictures match.

## Files (60)

- Native format is `.wdr` (decision 0004); `.wdproj` is read and written as a
  compatibility format, its DOCTYPE declaring the same things, not byte for
  byte (0009).
- A `.wdproj` with Linked=true and Length != Width loads with Length = Width
  whatever the attribute order (0009).
- A Wave Dude 2D JSON colour scheme with `colors` and no `stops` becomes
  evenly spaced stops (0009).
- Settings are the app's; the core does not read them.

## Export (50)

- MP4 through openh264 and the `mp4` crate (0005), not Media Foundation.
- PNG, JPEG and OpenEXR through crates (`png`, `jpeg-encoder` at 4:4:4,
  `exr`), not in-house encoders; the output must decode to the same pixels
  and declare the same structure (4:4:4, one HALF channel "Y"). MPEG-1, AVI
  and MOV have no maintained crates and stay in-house.
- Lossy outputs are measured by PSNR against the rendered frame (0009), not
  against the C# bytes.
- Probe CSV: shortest round-trip float formatting as Rust prints it, `\n`.
- The overlay text in exports uses an embedded open font, not Segoe UI.

## Scene painting (20)

- The settled-run optimisation is not required; the painting rules are.
- The 16x supersampling rule, the coating band, the face rule and the
  absorption profiles are kept as specified: they define the pictures.

## Model (10)

- The model is format-agnostic (serde); the XML attribute names of 10 are the
  `.wdproj` adapter's concern.
- `Simulation.WaveSpeed` is kept for wavelength estimates only, as in the
  original, with default 1.
- Pcg32, StableHash and xorshift32 are kept exactly, so a project's composite
  sources come out at the same positions as in the original (cheap, and it
  keeps imported scenes identical).

## Solver upgrades (decision 0010, 2026-11-06)

Where 30-engine.md and 70-guarantees.md describe the quadratic rim, the Mur
ring as the absorber of a rim, the Taylor 4th-order weights, leapfrog's
second order in time, the 9-point fallback ring and the 16-sample mean of n at
a boundary pixel, this project does instead: CPML in the rim (m = 3,
R = 1e-6, kappa_max 2) with a wall behind it, Mur only for Open edges without
a rim; the DRP-optimised 7-point stencil with the modified-equation 4th-order
time scheme for `Stencil::FourthOrder`; the full stencil everywhere with
mirror cuts that keep the operator symmetric; the area-weighted average of n^2
at a boundary pixel (exact coverage of a pixel cut by a straight edge). The
formulas, coefficients and measurements are in docs/physics.md.

The source definition of docs/physics.md (Sources) radiates 5 to 9 times the
original's amplitude, so the display brightness defaults are field 1 and
intensity 1.5 instead of 30 (10-model.md, DisplayLayer); projects from the
original's formats are fitted once on opening in the app.
