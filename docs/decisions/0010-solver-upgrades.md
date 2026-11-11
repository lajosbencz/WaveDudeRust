# 0010 Solver upgrades beyond the original

Status: accepted, 2026-11-06 (the user's decisions)

The original's scheme is textbook explicit FDTD: leapfrog, second order in
time, Taylor stencils, Mur plus a graded sponge at the edge, index averaged
over 16 point samples at a material boundary. The user asked for the modern,
correct technique from the literature, replacing the original where needed.

1. **CPML absorbing boundary** (Roden and Gedney 2000) replaces the graded
   sponge as the rim's absorber: auxiliary convolution fields on the border
   cells with a polynomial conductivity profile, a hard wall behind it. Mur
   stays available as the "Open" edge without a rim, and the reflective wall
   as is.
2. **Fourth order in time** by the modified-equation method (Dablain 1986):
   one further Laplacian application per step cancels the leapfrog's leading
   temporal error, so time and space are both fourth order.
3. **Dispersion-relation-preserving stencil coefficients** (Tam and Webb 1993)
   for the 7-point-per-axis Laplacian replace the Taylor 4th-order weights:
   the one free weight minimises the squared relative phase error over wave
   numbers 0 to pi/2 (4 cells a wavelength and longer; [engine/stencil.rs](../../crates/wavedude-core/src/engine/stencil.rs)). The
   isotropic 9-point stays as the cheap stencil.
4. **A symmetric operator everywhere**: the full stencil runs through the
   whole grid including the PML; the outermost ring is the wall. Energy is
   conserved to round-off without damping.
5. **Sub-cell material smoothing** (Farjadpour et al. 2006, Meep): a boundary
   cell takes the area-weighted average of n^2 from the shape's signed
   distance, the correct effective medium for the scalar wave equation,
   instead of the mean of n over 16 samples.

The analytic physics tests of [docs/physics.md](../physics.md) are extended for each: PML
reflection coefficient below -60 dB at 20 cells, temporal dispersion measured
against the 4th-order relation, the DRP phase error table, energy to round-off,
and a slab's reflection against Fresnel at a non-integer interface position
([tests/physics.rs](../../crates/wavedude-core/tests/physics.rs) `slab_reflects_the_fresnel_share`).
The gate design's mode solver follows the discretisation and the generated
gates of [examples/gates](../../examples/gates/) are re-measured.
