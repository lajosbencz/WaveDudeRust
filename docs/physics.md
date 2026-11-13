# Physics and numerics reference

What the engine solves, what the literature says about its errors, and what the
tests therefore measure. The original's quirks are not a target (see
[decisions/0003](decisions/0003-gpu-wgpu-and-cpu-parity.md)); its measured figures are the tolerances to beat.

## Equation

Scalar 2D wave equation on a uniform grid with unit spacing and unit step
(dx = dy = dt = 1):

    u_tt = c(x,y)^2 (u_xx + u_yy) - gamma(x,y) u_t + s(x,y,t)

- c = 1/n, n the refractive index painted into the scene (red channel).
- gamma the damping painted into the scene (blue channel); the rim outside the
  canvas carries no damping, it is a PML (Absorbing boundaries).
- s the sources, a forcing term added at their cells, normalised so that a source
  of Amplitude A swings its cell by A in free space (Sources).
- Intensity I is an exponential moving average of u^2 (the time average of the
  energy density up to a constant), so the picture shows |amplitude|^2 like a
  detector would.

## Time stepping

Both schemes advance `u+ = 2u - u- + B u` per sub-step of `dt = 1/n` (n from
the CFL rule below), with `A = dt^2 c^2 L`:

    leapfrog (9-point):                  B = A
    modified equation (4th order):       B = A + A (A u) / 12

Leapfrog is second order in time, explicit, time-reversible and
non-dissipative (Taflove & Hagness, ch. 2 and 4; LeVeque, ch. 10). The
modified equation (Dablain 1986) cancels leapfrog's leading temporal error
`dt^4 u_tttt / 12` through `u_tttt = (c^2 L)^2 u`, so time is 4th order like
space. The engine evaluates it in two passes over the grid: `w = c2 L u`, then
`u+ = u + ((u - u-) d + (w + c2 L w / 12) h)`, one extra Laplacian and one
scratch buffer `w` ([engine/cpu.rs](../crates/wavedude-core/src/engine/cpu.rs), [engine/stencil.rs](../crates/wavedude-core/src/engine/stencil.rs)); d and h are the
damping's (below).

Damping. A cell of damping factor d per sub-step (`d^(1/n)` of the scene's d
per step) runs the centred form of `u_tt + gamma u_t = c^2 L u`:

    (u+ - 2u + u-) + g (u+ - u-) = B u,   d = (1 - g) / (1 + g)
    u+ = u + d (u - u-) + h B u,          h = (1 + d) / 2

For a mode of B with factor `-lambda` the amplification satisfies
`z^2 - (1 + d - h lambda) z + d = 0`: `|z| = sqrt(d)` per sub-step, as before,
and it is stable for `lambda <= 4` whatever d in [0, 1], the undamped bound.
The original's form `u+ = u + d (u - u-) + B u` (h = 1) is stable only for
`lambda <= 2 (1 + d)`: at d = 0, the outermost ring of an inside damping
band, that is `lambda <= 2`, which the modified equation exceeds at two
sub-steps (`lambda = s - s^2/12` reaches 2.33 at s = 3.16) and which Mur's
reflection of the sub-step-alternating mode pushed over the edge: Example18
diverged from step 160. At d = 1, h = 1 exactly and nothing changes.

For a mode of A with eigenvalue `-s` a sub-step turns `e^{-i theta}` into

    leapfrog:              4 sin^2(theta/2) = s
    modified equation:     4 sin^2(theta/2) = s - s^2/12

## Spatial stencils

A Laplacian is `sum over axes of D(u) + cross D3x D3y u`, D a symmetric
second-derivative stencil along one axis (weights a0..a3 at offsets 0..3) and
D3 the 3-point one. Its symbol, the positive `-L(e^{ik.x}) / e^{ik.x}`, is

    symbol(kx, ky) = p(kx) + p(ky) - cross q(kx) q(ky)
    p(k) = -(a0 + 2 sum_m a_m cos(m k)),   q(k) = 2 - 2 cos k

9-point isotropic (second order, the leading anisotropy of the error removed;
Patra & Karttunen 2006): D = D3, cross = 1/6,

    L9 = [ 4 (u(i+-1,j) + u(i,j+-1)) + (u(i+-1,j+-1)) - 20 u(i,j) ] / 6

4th order: a 7-point-per-axis stencil with dispersion-relation-preserving
weights (Tam & Webb 1993), cross = 0. The three moment conditions of 4th order
(`a0 + 2 sum a_m = 0`, `sum a_m m^2 = 1`, `sum a_m m^4 = 0`) leave one free
weight, which minimises the squared relative phase error
`int_0^{pi/2} ((p(k) - k^2) / 2k^2)^2 dk` (4 cells a wavelength and longer):

    a3 = 0.015351082518610951
    a1 = 4/3 + 15 a3  = 1.5635995711124977
    a2 = -1/12 - 6 a3 = -0.17543982844499903
    a0 = -5/2 - 20 a3 = -2.8070216503722194

The Taylor 4th-order weights (-1/12, 4/3, -5/2, a3 = 0) are the original's;
the DRP set trades their accuracy at long waves for a 6.5 times smaller error
at 6 cells a wavelength (table under Numerical dispersion).

The grid is padded by 3 rings (the stencil's reach), which stay 0 (the wall)
unless Mur sets them, so the full stencil runs on every grid cell with no
narrower fallback near the edge, and the operator is symmetric.

Every stencil is evaluated as weighted sums of differences from the centre,
`K sum (u_k - u)`: for a smooth field each difference is exact in f32
(Sterbenz), so the round-off scales with the result, not with the field, and
the weights sum to 0 exactly. Summing the taps first left the 9-point's energy
drifting 1.7e-5 over 10,000 steps, a bias, not a walk.

Mirror pixels (red 0) are Dirichlet cells: c2 = 0, so a mirror cell never
moves from 0 (sources on it drive nothing). They are thinner than a 7-point
stencil's reach, so a tap whose path crosses a mirror cell is dropped (it reads
0, as the mirror does), by a cut byte per cell (which of the cells 1 and 2 away
along each axis are mirrors; [engine/pml.rs](../crates/wavedude-core/src/engine/pml.rs), `cuts`); without it a wave leaked
through a 2-pixel mirror. The 9-point's diagonal taps would couple the two cells
beside a mirror line that is only 8-connected (the painter's centre test makes
one of any mirror thinner than about 1.41 px at 45 degrees) straight across it:
a diagonal tap is dropped when either axis neighbour it passes is a mirror
(`diagonal_cuts`). Both rules are symmetric between the two cells of a tap, so
the operator stays symmetric. A region the mirrors close off then runs a
principal submatrix of the Laplacian, negative semi-definite as the whole is
(Stability).

The original's coefficients ([spec/30-engine.md](spec/30-engine.md)) differ: its 9-point kernel is
0.066 / 0.184 / -1, a Laplacian scaled by 0.316 (phase speed 0.562) with heavier diagonals,
its 4th order the Taylor one. Not carried over ([decisions/0003](decisions/0003-gpu-wgpu-and-cpu-parity.md), 0010): both
stencils here are unscaled Laplacians, so the wave speed is c under either.

## Stability (CFL)

A scheme is stable while `s = S^2 symbol` stays under its limit for every mode,
S = c dt/dx the Courant number: s <= 4 for leapfrog; for the modified equation
`s - s^2/12` must lie in [0, 4], which holds up to s = 12 (there the two roots
of the amplification factor meet). With the symbol's largest value M, taken at
(pi, pi) for both stencils (16/3 for the 9-point, 12.6316 for the DRP), the
Courant limit is `sqrt(limit / M)`:

    5-point, leapfrog:            1/sqrt(2)           = 0.7071
    9-point, leapfrog:            sqrt(4 / (16/3))    = 0.8660
    Taylor 4th order, leapfrog:   sqrt(4 / (32/3))    = 0.6124
    DRP 7-point, leapfrog:        sqrt(4 / 12.6316)   = 0.5627
    DRP 7-point, modified eq.:    sqrt(12 / 12.6316)  = 0.9747

A step is split into n equal sub-steps,
`n = max(1, ceil(c_max / (limit * 0.95)))` ([engine/substeps.rs](../crates/wavedude-core/src/engine/substeps.rs)), c2 scaled by
`1/n^2` and the damping factor taken as `d^(1/n)`. At index 1 both stencils
take 2 sub-steps; from index 1.08 the DRP takes 1, the 9-point from 1.22. The
test drives one sub-step at 0.985 and 1.015 of each limit: bounded below,
diverging above, which pins the limit itself, and one sub-step fewer than the
rule at c = 1 diverges (the test for the test).

Mirror cells (c2 = 0) never limit stability; the smallest n does. They must
not move: until 2026-11-06 a mirror was a live cell of c2 = 1e-6 whose taps
across were cut, and dropping the DRP's negative tap a2 on its rows made the
operator indefinite (largest eigenvalue of `C^(1/2) L_cut C^(1/2)` +4.3e-7 on a
22 x 22 grid with a 2 px wall): a mirror cavity grew by about 6.6e-4 a step,
from |u| 0.5 to 7.9 at step 25,000 and 1.4e13 at 60,000. With c2 = 0 the cut
operator of each closed region is a principal submatrix of L, so its
eigenvalues interlace L's (largest measured 4e-15); open mirror shapes (a dot,
a segment, a diagonal line, dashes) stay negative definite as well. Tests
([tests/physics.rs](../crates/wavedude-core/tests/physics.rs) `mirror_cavity_stays_bounded`, [tests/gpu.rs](../crates/wavedude-core/tests/gpu.rs) `mirrors_hold`):
a 32 x 32 mirror cavity after a burst stays at |u| 0.55 (9-point) and 0.66
(DRP) over steps 25,000 to 30,000 against 0.78 and 0.91 in the first 1000 on
the CPU, likewise on the GPU; the reviewer's 96 x 96 cavity stays between 0.23
and 0.79 over 60,000 steps on both stencils.

A cut tap keeps the centre's own weight (the tap reads 0), so for the 9-point,
whose taps all have positive weights, any cut leaves `-<u, L u>` a sum of
non-negative terms; the DRP's negative a2 makes that argument fail, which is
why its mirror cells must be fixed.

## Numerical dispersion

Substituting a plane wave e^{i(k.x - omega t)}: with `s = (c/n)^2 symbol(k)`,
the discrete frequency per step is `omega = n theta(s)`, theta from the time
scheme's relation above, and the phase velocity `omega / |k|`.

Phase velocity v/c at index 1 (2 sub-steps), space and time together
(wavedude-cli `dispersion`):

| cells/wavelength | 9-point axis | 9-point diagonal | DRP axis | DRP diagonal |
|---|---|---|---|---|
| 4 | -7.98 % | -7.55 % | -0.286 % | +0.039 % |
| 5 | -5.05 % | -4.87 % | +0.056 % | +0.046 % |
| 6 | -3.48 % | -3.40 % | +0.083 % | +0.031 % |
| 8 | -1.95 % | -1.92 % | +0.047 % | +0.013 % |
| 10 | -1.24 % | -1.23 % | +0.023 % | +0.006 % |
| 16 | -0.48 % | -0.48 % | +0.004 % | +0.001 % |
| 20 | -0.31 % | -0.31 % | +0.002 % | +0.000 % |

The DRP stencil alone (exact in time) along an axis: -0.228 % at 4 cells,
+0.079 % at 5, +0.094 % at 6, +0.050 % at 8, +0.025 % at 10, +4.5e-5 at 16,
+1.9e-5 at 20; the Taylor 4th order -2.75 %, -1.21 %, -0.61 %, -0.20 %,
-0.084 %. Before this change the 4th order (Taylor, leapfrog) ran +0.55 % on
the axis and +1.01 % on the diagonal at 6 cells. The modified equation's own
error is about s^2/720 of the frequency, 1.4e-5 at 10 cells a wavelength.

Tests ([tests/physics.rs](../crates/wavedude-core/tests/physics.rs)):
- A standing wave of 10 and 20 cells a wavelength in a reflective box, started
  as the exact mode of the operator with its wall (the product of the axis
  operators' eigenvectors, by inverse iteration: the wall's zeros are not a
  sine's odd extension for taps reaching 2 or 3 cells). Its frequency must
  match the discrete relation of that mode to 1e-5 (measured 1.5e-8 to 4.6e-7),
  and the temporal dispersion (discrete against `c sqrt(symbol)`, exact in
  time) must stay under 1e-4 at 10 cells for the modified equation: measured
  1.45e-5 (10 cells) and 1.0e-6 (20); leapfrog's is 4.1e-3 and 1.1e-3. The
  free plane wave's total error at 10 cells is +2.3e-4 for the DRP. In the box
  the DRP mode runs 1.0013 (10 cells): its wall shifts the eigenvalue.
- The DRP table above, with the 6-cell error under 0.3 %.

## Energy

Both schemes are `u+ - 2u + u- = B u` with B symmetric up to c2 (the stencil
runs on every cell with zeros beyond the wall). At a uniform c2,

    E(n + 1/2) = ( |u(n+1) - u(n)|^2 - <u(n+1), B u(n)> ) / (2 c2)

is conserved exactly (`E(n+1/2) - E(n-1/2) = <B u, u+ - u-> - <u+, B u> +
<u, B u-> = 0`) and positive while the scheme is stable. With mirrors the same
holds over the cells that move: a mirror cell is a fixed 0 (c2 = 0), and B over
the others is symmetric (the cuts drop both directions of a tap) and, region by
region, a principal submatrix of the free operator (Stability). A mirror cell
that moved broke this: it was the cut operator's indefinite part. Test: a free Gaussian
pulse in a 72 x 64 reflective box without damping at index 2 (one sub-step),
E evaluated in f64 from the f32 fields every 20 steps over 10,000 steps; the
largest relative departure must stay under 1e-6. Measured 2.2e-7 (9-point)
and 2.4e-7 (DRP, modified equation).

## Sources

A source is a forcing term of the wave equation at its cell, `u_tt = c^2 L u + f`
(Taflove & Hagness, the "soft" source: the cell stays transparent to passing
waves). Several sources on one cell add.

Position. A source's position is in cells: a whole number names a cell (its
centre), a fraction lies between cells. A source at a fractional position drives
the 2 x 2 cells around it with bilinear shares, its taps ([engine/emitters.rs](../crates/wavedude-core/src/engine/emitters.rs); a
tap off the canvas is dropped, one on a mirror cell drives nothing), the discrete
form of a point source between grid points, and its Amplitude is the swing of the
field sampled at the position with the same shares. The response of the spread
source at its own position is `G = sum_ij w_i w_j G(c_i - c_j)` over its taps,
the Green's function at the lattice offsets (0, 0), (1, 0), (0, 1) and (1, 1)
([engine/green.rs](../crates/wavedude-core/src/engine/green.rs): the kx integral gains `(r + xi)^m` per root, the ky integrand
`cos(l ky)`), and the forcing is normalised by |G| as a cell's is by |G0| below. A
source at a whole-number position is the one-cell source, bit for bit. The need:
a fibre core of an even number of cells has its axis on a cell boundary, where a
one-cell source sits half a cell off the axis and excites the odd modes visibly
(the hook in the gate examples' cups before this). The `.wdproj` adapter writes positions as
integers, so the gate generator puts two half-amplitude sources in the two
cells beside an axis.

Amplitude. A source's Amplitude A is the displacement amplitude of the wave it
radiates, at its own cell, in free space (index 1, an infinite grid), at its
frequency. A forcing `f0 e^{-i omega t}` at one cell of the infinite grid answers
there with `f0 G0(omega)`, G0 the lattice Green's function of the engine's discrete
operator at the source cell, so the forcing is the rate of change of the impulse

    p(t) = -A / |G0(omega)| E(t) cos(omega t + phi) / omega
    f(t) = dp/dt = A / |G0(omega)| (E sin(omega t + phi) - E' cos(omega t + phi) / omega)

with omega the source's angular frequency per step (from its frequency level), phi
its phase and E(t) its envelope, the turn-on times the pulse envelope times the
detach fade (below). While E = 1, `f = A / |G0| sin(omega t + phi)`. Image sources
are the same at their level's omega and phase 0, with the scene's image source
scale as A. G0 depends
on the stencil, the time scheme, the sub-step count and omega; a source in another
medium than index 1 swings its cell by the ratio of the two media's |G0| (at index
1.25 about 1.5 times A).

Turn-on ([decisions/0022](decisions/0022-smooth-source-turn-on.md), [engine/envelope.rs](../crates/wavedude-core/src/engine/envelope.rs)). A forcing
switched on abruptly, `f = H(t) sin(omega t + phi)`, delivers a net impulse
`A cos(phi) / (|G0| omega)` on top of its oscillation; in 2D an impulse P leaves
the zero-frequency wake `P / (2 pi sqrt(t^2 - r^2))` behind the front (the 2D
Green's function), a pedestal that decays as 1/t and adds over the sources of an
array (Example02's field mean was 0.31 to 0.40 of its RMS). Driving the impulse
p instead of the force makes the net impulse of any drive `p(end) - p(start)`, 0
whenever E starts and ends at 0, whatever its phase, frequency or envelope. E rises
from t = 0 as a raised cosine over half a period of the source,
`E = (1 - cos(pi t / T)) / 2`, `T = pi / omega`, continuous with its slope; the
detach fade falls as the mirrored raised cosine over 30 steps. A force ramped
without the `E'` term keeps a pedestal that depends on phi (a 16-step ramp at
32 steps a period removes it at phase 0 and leaves 7 % of the local amplitude 60
cells away at phase 90 degrees); only a ramp of several periods would make it small.

G0 ([engine/green.rs](../crates/wavedude-core/src/engine/green.rs)). Per sub-step of `dt = 1/n` with `x = dt^2 c^2 sigma` (sigma
the stencil's symbol) and `W = 4 sin^2(omega dt / 2)`, a mode answers to the
forcing as `U = f0 / (c^2 sigma - n^2 W)` under leapfrog and as
`U = dt^2 f0 (1 - (omega dt)^2/12)(1 - x/12) / (x - x^2/12 - W)` under the modified
equation, which splits over the roots `x_i = 6 -+ sqrt(36 - 12 W)` into two
resolvents. So both need `R(z) = (2 pi)^-2 int over [-pi, pi]^2 of dk / (c^2 sigma - z)`,
taken outgoing (W + i eta, eta = 1e-9). R is an iterated integral: for fixed ky,
`c^2 sigma - z` is a polynomial in `cos kx` of the stencil's reach in degree, and
the kx integral is exact by partial fractions over its roots,
`(2 pi)^-1 int dk / (cos k - xi) = 1 / r`, `r = -+sqrt(xi^2 - 1)`, `|r + xi| < 1`. The
ky integral over [0, pi] is split where an edge of the kx band (sigma at kx = 0 and
at pi, both monotone in ky) meets Re z, where it has an inverse square root
singularity; each piece is mapped by `ky = a + (b - a)(3u^2 - 2u^3)`, which removes
it, cut into pieces halving 30 times towards both ends, and integrated by 16-point
Gauss-Legendre. Errors: R agrees with a direct 2048^2 sum over the Brillouin zone
at a shift the sum resolves to 1e-9; |G0| moves by less than 1e-5 when eta is
halved and the nodes doubled; Im R tends to the continuum's 1/4 at low frequency.
|G0| is computed once per sub-step count and frequency and cached.

Each sub-step of `dt = 1/n` adds `F = dt^2 f(t_k)` where the scheme takes `A u`,
at the time `t_k = t + k/n` of its current field ([engine/forcing.rs](../crates/wavedude-core/src/engine/forcing.rs)), f the difference of
p over the sub-step scaled to the steady drive,
`f = A / |G0| (Em sin(theta) - dE cos(theta) / (2 tan(omega dt / 2)))` with Em and dE
the mean and the difference of E at `t_k -+ dt/2`: exactly `sin(theta)` while E = 1,
and the sub-steps' impulses telescope, so a drive that ends has delivered none up to
round-off. Leapfrog
`u+ = u + d (u - u-) + h (A u + F)`, the modified equation `w = A u + F` before
its second pass, so the neighbours' `c2 L w / 12` carries `dt^4 c^2 L f / 12`. The
modified equation's own source term `dt^4 f_tt / 12` is taken as
`-(omega dt)^2 F / 12` (exact for a continuous source; the envelope's own
derivatives are neglected, an error of the order of its rate of change), so
`F = dt^2 f (1 - (omega dt)^2 / 12)`; the source is 4th order in time for a
continuous drive. The radiation is therefore independent of the sub-step count
by consistency, with nothing calibrated. A source on a mirror cell drives
nothing: the cell has no wave coupling and a force would only push it off (a
free mass driven from rest drifts by `omega A cos(phi)` a step).

In a lossless resonator driven at a resonance the field grows without bound, as
the equation says; the original's blend `u = 0.9 u + 0.1 v` at its cell damped
the cell and limited it.

Tests ([tests/physics.rs](../crates/wavedude-core/tests/physics.rs)):

- Amplitude: a continuous source of amplitude 0.5 at levels 20, 50, 100 and 200
  (scale 2) on a 512 x 512 canvas with a 32 px CPML rim swings its cell by 0.5
  within 0.1 % on both stencils at two sub-steps (index 1), and at one sub-step
  (index 1.25, which one sub-step needs) by the predicted ratio of the media's
  |G0| within 0.1 %.
- A continuous level-100 source on a 256 x 256 open canvas of index 1.25: the RMS
  field over the canvas after 600 steps at 1 to 4 sub-steps is 7.171e-2 to
  7.198e-2 on the 9-point stencil (0.4 %) and 8.379e-2 to 8.419e-2 on the DRP
  (0.5 %).
- No pedestal: a level-50 source (scale 2) at phases 0 and 1 rad and an image
  source, 256 x 256 canvas of index 1 with a 32 px CPML rim: the canvas field's
  mean over its RMS, averaged over steps 337 to 400, stays within 0.5 % (0.08 %
  measured, both stencils; the GPU engine the same, [tests/gpu.rs](../crates/wavedude-core/tests/gpu.rs)); the
  abrupt drive gave 6.3 % at phase 0. The sub-step values of a detaching 1-cycle
  pulse and of a detaching continuous source sum to 0 within f32 round-off
  ([engine/forcing.rs](../crates/wavedude-core/src/engine/forcing.rs)).
- Transparency: a wave packet crossing the cell of a source of amplitude 0, or
  of a pulsed source silent until it detaches, is the same bit for bit as
  without it (a value of 0 or a detached one adds nothing); crossing a weak
  continuous source, the field minus the source's own field is the packet's
  within 7e-7 of its peak (superposition, f32 round-off).
- A point source radiates the wavelength of the stencil's discrete relation
  within 1 % (0.6 % measured, both stencils).
- A source at a fractional position, (0.5, 0), (0.5, 0.5) and (0.25, 0.75),
  levels 50 and 100 at two sub-steps: the field sampled at the position with the
  taps' shares swings by the amplitude within 0.05 % on both stencils.
- A source on a cell boundary (y = c - 0.5): the rows above and below the
  boundary agree bit for bit after 300 steps on both stencils; the one-cell source
  half a cell off it differs by more than the field's peak.

The original blended the cell once per step, `u = 0.9 u + 0.1 v`, before the
sub-steps, so its radiated amplitude grew with the sub-step count (field
energy x0.48, x1.00, x1.67, x3.35 at n = 2, 3, 4, 6 against n = 3) and with any
part that raised it.

## Absorbing boundaries

Three edge kinds:

- Reflective: u = 0 beyond the canvas (the padding rings), a wall.
- Open without a rim: Mur's first-order condition on the first padding ring;
  the second and third, which only the DRP stencil reads, continue the field
  linearly from the edge cell through the first (Open edges, below).
- A rim (damping edge below 0): the rim cells outside the canvas are a CPML
  (Roden & Gedney 2000), backed by the wall u = 0, for either edge kind. The
  rim carries the nearest canvas cell's c2 and damping, so a guide or an
  absorber runs on into it.

The CPML in second-order form with memory variables for the stretched
derivatives (Komatitsch & Martin 2007; Pasalic & McGarry 2010). Along x,
`d/dx~ = (1/s) d/dx` with `s = kappa + sigma / (alpha + i omega)`; per
sub-step dt:

    b = exp(-(sigma/kappa + alpha) dt),   a = sigma (b - 1) / (sigma kappa + kappa^2 alpha)
    psi <- b psi + a D1 f
    Q    = (1/kappa) D2 f + (1/kappa)' D1 f + D1 psi
    xi  <- b xi + a Q
    O_x f = Q / kappa + xi

D2 the stencil's axis second derivative, D1 the 4th-order central first
derivative, (1/kappa)' the profile's analytic slope; inside the canvas
sigma = 0 and kappa = 1, so O = D2. The 9-point's mixed term must be
stretched too (left plain, the layer blew up): with `z~ = u + chi O_y u`
(chi = 1/6 for the 9-point, 0 for the DRP), `lap~ u = O_x z~ + O_y u`, and
with `delta_y = O_y u - D2y u` the correction added on the rim after each
pass over the grid, times c2, is

    (O_x z~ - D2x z~) + chi D2x delta_y + delta_y

in four stages ([engine/pml.rs](../crates/wavedude-core/src/engine/pml.rs); on the GPU four kernels over the rim): psi of y,
delta_y and xi of y, psi of x on z~, then the x part and the sum. The modified
equation applies the operator twice per sub-step and keeps a second set of
psi and xi. Storage: x strips of 2 rim columns over the full height, y strips
of 2 rim rows over the full width.

Profile at relative depth r = depth/thickness (scene_data.rs, PmlParams):

    sigma = sigma_max r^m,   sigma_max = -(m + 1) ln(R) c / (2 thickness)
    kappa = 1 + (kappa_max - 1) r^m
    alpha = alpha_max (1 - r)

m = 3, R = 1e-6, kappa_max = 2, alpha_max = 0.05 per step, c the fastest
speed in the rim. kappa_max 5 (the decision's upper end) reflected short
waves at only -39.7 dB with 20 cells; a sweep of m, kappa_max and alpha_max
chose these.

Reflection, CPU engine, DRP: a Gaussian wave packet launched at the edge on a
240 x 240 canvas, the field near the edge after the packet has gone, against
a reference run on a canvas wide enough not to reflect; the ratio of the two
fields' energies in dB ([tests/physics.rs](../crates/wavedude-core/tests/physics.rs), `cpml_reflection`). Long: 27 cells
a wavelength; short: 8.

| packet, incidence | 16 cells | 20 cells | 32 cells | Mur ring alone |
|---|---|---|---|---|
| long, normal | -74.1 dB | -84.1 dB | -103.5 dB | -39.9 dB |
| long, 45 degrees | -75.1 dB | -83.7 dB | -104.3 dB | -17.6 dB |
| short, normal | -81.5 dB | -89.4 dB | -105.7 dB | -30.3 dB |
| short, 45 degrees | -84.2 dB | -92.0 dB | -102.7 dB | -17.6 dB |

The test requires the long packet under -60 dB at 20 cells and the short
under -40 dB at normal and 45 degrees. The default rim stays 32 cells.

Known limits of the layer, all small against the table:

- In damped cells the scheme is second order in time: the modified equation's
  `A^2/12` term is taken without the damping (gamma) and without the PML's
  memory terms, so the rim runs a 2nd-order update where it absorbs.
- D1(psi) is not applied on the two canvas cells next to a strip (psi lives on
  the strips only); psi there is of order sigma_max / rim^3, negligible.
- D1(D1 .) has a null at k = pi, so the layer does not damp the Nyquist mode
  through psi; only kappa and the wall act on it. The stencils' Nyquist mode is
  not radiated by the sources (their spectrum lies far below it).

The original's sponge (loss rising with the square of the depth to a half per
step) is gone. By the original's in-canvas peak measure (a long-wave pulse,
the largest |u| after it has left against the largest before) the 32 px rim
now returns 0.0060 % (9-point) and 0.0066 % (DRP), against 0.24 and 0.25 % for
the sponge; Mur's ring alone 0.0012 and 0.0007 %.

### Open edges

Mur's condition is the outgoing factor of the damped equation,
`u_t + gamma/2 u - c u_n = 0` (n the inward normal; the factorisation drops
`gamma^2/4`), centred between the edge cell and the first ring and between the
two time levels. With `S = sqrt(c2)` per sub-step at the edge cell and `g` from
its d:

    (1 + S + g/2) ring+ = (1 + S - g/2) edge - (1 - S + g/2) edge+ + (1 - S - g/2) ring

At d = 1 it is Mur's `ring+ = edge + k (edge+ - ring)`, `k = (S - 1)/(S + 1)`.
S is the long-wave phase speed of both schemes (both Laplacians are consistent
and both time schemes are exact for long waves), so no stencil-specific speed
enters: a normally incident long wave leaves without reflection on either
stencil, and the reflection rises with the wave number as for every first-order
condition (open edges alone 0.0012 % and 0.0007 % of a long-wave pulse).

The second and third rings are `2 ring1 - edge` and `3 ring1 - 2 edge` at the new
time. Chaining Mur ring by ring, as the original did, let a mode grow where two
open edges meet on the DRP stencil above a Courant number of about 0.75 per
sub-step (a background index of 1.25 at one sub-step, Courant 0.8, diverged by
step 100 at a corner, with or without damping); a one-dimensional model and a
model with a fixed wave number along the edge stay bounded, a two-edge corner
model does not, and with the linear rings it stays bounded up to the CFL limit
(0.93).

The corner rings (the padding cells beyond both edges) take the diagonal grid
cell as their edge cell and Mur's normal-incidence formula for unit distance,
though the cell lies sqrt(2) away; only the 9-point's diagonal tap at the grid's
corner cell reads them, so the error stays at the four corners.

The modified equation's `w = c2 L u` is 0 on the rings, so its second pass,
`c2 L w / 12`, sees a wall at the edge. Continuing `w` into the rings as `u` is
was measured and rejected (2026-11-06, DRP, Mur's ring alone, table above):
Mur's condition on `w` (its edge value one sub-step before kept per chain,
rings 2 and 3 linear) gave -39.5 dB (long, normal; -39.9 without), -28.5 dB
(short, normal; -30.3) and the same -17.5 dB at 45 degrees, and let the
two-edge corner grow at one sub-step (index 1.25, Courant 0.8): 2.4e10 by
step 5000 with no damping band. A linear continuation of `w` from the two
cells inside gave -39.4 and -28.5 dB and diverged likewise. The wall in the
correction term is a 4th-order-in-time term and costs nothing measurable;
the residual reflection is Mur's own first-order error.

Test ([tests/physics.rs](../crates/wavedude-core/tests/physics.rs), [tests/gpu.rs](../crates/wavedude-core/tests/gpu.rs)): a 64 x 48 open canvas of index
1.25 with an inside damping band of 6 (its outer ring d = 0) and without one, a
continuous source, 1 to 4 sub-steps on both stencils, 5000 steps: bounded;
Example18 over 3000 steps likewise on both engines.

## Intensity

The intensity is a two-stage exponential moving average of u^2, advanced once
per step after its last sub-step ([engine/cpu.rs](../crates/wavedude-core/src/engine/cpu.rs) `accumulate`, step.wgsl
`accumulate`), with alpha the retention weight:

    s1 <- alpha s1 + (1 - alpha) u^2
    s2 <- alpha s2 + (1 - alpha) s1
    I   = (s1 + s2) / 2 * IntensityScale,     alpha = exp(-1 / tau)
    tau = max(1, TimeDecay * 100 / L) steps

L the lowest frequency level in use (image levels and object emitters; 100
without sources). The blend is made where I is read (captures, probes,
`prepare_frame`). At a driven cell the stages move with the forced value, so a
source adds no lag of its own. The 16-bit store keeps sqrt(I) ([spec/40](spec/40-frames-store-render.md)) for the
half format's range; that is a storage choice, not physics.

For a steady sinusoid of amplitude A, u^2 has the mean A^2/2 and a ripple at
2 omega. The mean: switched on at t = 0, I approaches A^2/2 as
`1 - (1 + t / 2 tau) e^(-t / tau)`, so after 5 tau I is 2.4 % short (sqrt(I)
1.2 %), after 6 tau 1.0 % (0.5 %), after 8 tau 0.15 %. The ripple: each stage
passes `r = (1 - alpha) / |1 - alpha e^(-2 i omega)|` of it, the blend about
`(r + r^2) / 2` of the mean either way. At the defaults (TimeDecay 10, frequency
scale 2) a level-100 source has a period of 16 steps and tau = 10: r = 0.13,
sqrt(I) swings by about +-3.6 %; at scale 1 (32 steps) by +-7 %. TimeDecay 40
brings it under +-0.9 % at scale 2.

tau follows the level, not omega: omega = 0.5 L scale / 255 per step, so tau is
`TimeDecay * scale / 32` periods (0.62 at the defaults) and the frequency scale
changes how much ripple a project shows. Deriving tau from omega (a fixed
number of periods) was considered and not done: TimeDecay keeps the original's
meaning, saved projects look as they did, and the CLI's truth table derives its
settle time from the same rule (wavedude-cli truth.rs); a project wanting a steady
picture raises TimeDecay.

Test ([tests/physics.rs](../crates/wavedude-core/tests/physics.rs) `intensity_settles_on_the_mean_square`): a continuous
source of amplitude 0.5 at level 100, scale 2, TimeDecay 40 (tau = 40 steps) on
a 192 x 192 canvas with a 32 px rim; sqrt(I) at the source cell, averaged over a
period, against the cell's fitted swing over sqrt(2): -1.34 % at 5 tau (the
source's start-up adds to the filter's lag), -0.57 % at 6 tau, -0.10 % at 8 tau,
-0.02 % at 10 tau on both stencils; required within 1 % from 6 tau on.

## Precision

Field in f32 on both engines. Round-off per step is ~1e-7 relative; over 10^4
steps and a 2000-cell canvas the accumulated error stays below the stencil's
dispersion error by orders of magnitude, so f32 is the right storage and f64
buys nothing visible (the energy test above bounds the drift). CPU and GPU
agree to a few ulp per step and drift apart over a run through reassociation
and FMA; the test tolerance between them is relative 1e-4 on the field after
1000 steps and on sqrt(I) 1e-3 (measured 2.0e-6 and 4.6e-7 on the 9-point,
1.6e-6 and 5.2e-7 on the DRP, with a 16-cell CPML). The original demanded
bit-for-bit between its engines, which forbade the GPU's FMA; this project
does not.

## References

- Taflove, Hagness: Computational Electrodynamics, 3rd ed., Artech House 2005.
- LeVeque: Finite Difference Methods for Ordinary and Partial Differential
  Equations, SIAM 2007, ch. 10.
- Patra, Karttunen: Stencils with isotropic discretization error for
  differential operators, Numer. Methods PDE 22 (2006).
- Berenger: A perfectly matched layer for the absorption of electromagnetic
  waves, J. Comput. Phys. 114 (1994).
- Roden, Gedney: Convolution PML (CPML), Microw. Opt. Technol. Lett. 27 (2000).
- Komatitsch, Martin: An unsplit convolutional perfectly matched layer
  improved at grazing incidence for the seismic wave equation, Geophysics 72
  (2007).
- Pasalic, McGarry: Convolutional perfectly matched layer for isotropic and
  anisotropic acoustic wave equations, SEG Expanded Abstracts (2010).
- Tam, Webb: Dispersion-relation-preserving finite difference schemes for
  computational acoustics, J. Comput. Phys. 107 (1993).
- Dablain: The application of high-order differencing to the scalar wave
  equation, Geophysics 51 (1986).
- Mur: Absorbing boundary conditions for the finite-difference approximation
  of the time-domain electromagnetic-field equations, IEEE Trans. EMC 23 (1981).

## Measured (2026-11-06, after [decisions/0010](decisions/0010-solver-upgrades.md) items 1 to 4; [tests/physics.rs](../crates/wavedude-core/tests/physics.rs) and [tests/gpu.rs](../crates/wavedude-core/tests/gpu.rs))

- Energy over 10,000 steps: 2.2e-7 (9-point), 2.4e-7 (DRP) relative.
- Sub-steps at c = 1: 2 for both stencils.
- Standing wave in a box: 9-point 0.98729 at 10 cells a wavelength and
  0.99661 at 20; DRP 1.00135 and 1.00150 (its wall's shift); each within
  1.5e-8 to 4.6e-7 of its mode's relation.
- Point source at level 100 (nominal 16.02 px): 15.85 px on the 9-point
  (relation 15.94), 15.94 on the DRP (relation 16.02).
- Slab guide (core 11 cells of index 1.6, cladding 1.45, 24-cell vacuum
  wavelength) fed by a point source: the phase slope along the core 360 to
  560 cells out matches the mode solver's beta ([scene/modes.rs](../crates/wavedude-core/src/scene/modes.rs), for the
  engine's stencil, time scheme and sub-steps) within 5.8e-5 (9-point) and
  1.0e-4 (DRP).
- Mirror cavity (2-pixel walls, mirror cells c2 = 0): after 300 steps the
  field outside is 0 on both stencils (3.7e-7 and 5.1e-7 of the inside while
  mirror cells moved); bounded over 30,000 steps (Stability).
- A one-pixel 8-connected diagonal mirror line: 0 of the field's square sum
  crosses it in 600 steps on both stencils and engines (10 % without the
  9-point's diagonal cuts, a leapfrog model of the same box).
- Fresnel: a packet of 40 cells a wavelength at normal incidence on a slab of
  index 1.5 whose face lies at x = 200.3 (the boundary pixel the effective
  medium of its 0.7 glass share) reflects 0.19897 (9-point) and 0.19825 (DRP)
  of its amplitude against Fresnel's 0.2 ([tests/physics.rs](../crates/wavedude-core/tests/physics.rs)
  `slab_reflects_the_fresnel_share`, within 2 % required). At 24 cells a
  wavelength 1.6 % and 2.6 % low: the boundary pixel acts as a thin layer of
  intermediate index, an effect of order (k d)^2.
- CPML: the table under Absorbing boundaries.
- Step time, 1920 x 1080 with a 32 px rim, intensity tracked, release build,
  24-thread Ryzen 9 9900X and an RTX 5070 Ti (wavedude-cli `bench --gpu`), ms a
  step without captures, before and after:

| engine | stencil | before | after |
|---|---|---|---|
| CPU | 9-point | 1.065 | 1.48 |
| CPU | 4th order | 1.105 | 3.30 |
| GPU | 9-point | 0.091 | 0.125 |
| GPU | 4th order | 0.100 | 0.235 |

  The 4th order now takes two passes of a 13-point stencil per sub-step; the
  CPML costs about 0.5 ms (9-point) and 1.0 ms (DRP) of the CPU's time, the
  grid passes alone 0.93 and 2.10 ms without a rim. [docs/measurements.md](measurements.md)
  compares both engines with the C#'s on the same machine.
