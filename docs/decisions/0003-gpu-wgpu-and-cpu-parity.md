# 0003 wgpu compute engine plus a CPU engine at parity

Status: accepted, 2026-11-05

The original drives OpenCL by hand-written P/Invoke. OpenCL is deprecated on
macOS and absent on many Linux installs.

Decision: one GPU engine on wgpu (WGSL kernels, f32 storage) and one CPU engine
(rayon over rows, the compiler's auto-vectorisation). f16 storage behind
`shader-f16` and explicit wide lanes were considered and not built: the
measured step costs (docs/measurements.md) are bound by the stencil's reads,
which f16 would halve, so they stay an option, not a promise.
Both expose the same engine trait and the same features: stencils, sub-steps,
sources, probes, captures, scene replacement. The CPU engine is the reference
for tests and the fallback without a usable adapter.

Bit-for-bit equality with the C# engines is not a goal. Correctness is proven
analytically (see docs/physics.md): CFL stability, phase velocity against the
dispersion relation per stencil, energy of a free pulse, reflection
coefficient of each boundary, with documented tolerances that match or exceed
the original's measured figures.

## Stencils are true Laplacians (added 2026-11-06, the user's decision)

The original's "9-point" kernel (0.066 diagonals, 0.184 axes, -1 centre) is a
Laplacian scaled by 0.316 (axis + 2 diagonal weights) with a diagonal weight of
0.36 of the axes; its "4th order" kernel is the textbook one at full scale. So a
wave runs 0.562 times as fast under the first as under the second and the wavelength changes
with the stencil switch. Not carried over: both of this project's stencils
approximate the unscaled Laplacian, the isotropic 9-point (1/6, 2/3, -10/3)
and the 4th order (-1/12, 4/3, -5), so an index means one speed under either,
and each derives its own sub-step count from the CFL bound (docs/physics.md).
No "classic" kernel. The importer divides an imported 9-point project's
frequency scale by 0.562 so its pictures keep their wavelengths (measured
against the C# library: 14.9 px at level 60 on both sides, docs/comparison)
and logs that it did.
