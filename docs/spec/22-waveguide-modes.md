# 22 - Waveguide mode solver

The guided modes of a straight slab waveguide as the engine discretises it. Pointer: `Parts/WaveguideModes.cs`; the
callers are in `Parts/LogicGateDesign.cs`. Units: pixels (cells), steps, radians; all arithmetic f64.

## Where it is used

Only by the logic gate design ([`21-logic-gate-design.md`](21-logic-gate-design.md)): the single-guide propagation constant `beta` (core width
check, cladding decay `gamma`, bend radius), the phase section's `beta_p`, and the coupling constant `kappa` of two
guides (straight coupler and S-bends). Nothing else calls it: not the fibre (its core and cladding are painted from
the part's own diameters, no mode is computed), not the zone plate, not sources, not the engine. A Rust port needs it
for the gates alone.

## Model: the engine's own discrete equations

The engine (30, 4th-order stencil, damping `d = 1`) advances per sub-step of length `1/S`:
```
u_next = 2u - u_prev + (c2 / S^2) * Lap(u),     c2 = 1/n^2,   Lap = L_x + L_y
L_x u = -(u[x-2] + u[x+2])/12 + 4/3*(u[x-1] + u[x+1]) - 5/2*u[x]        (same along y)
```
A guided mode `u(x, y_j, t) = phi_j * exp(i*(beta*x - omega*t))` (x along the guide, y_j the transverse rows) gives:
```
L_x exp(i beta x) = -mu(beta) * exp(i beta x)
mu(beta)  = cos(2 beta)/6 - 8 cos(beta)/3 + 5/2          longitudinal stencil term; mu(0) = 0, mu(pi) = 16/3, rising
time:  exp(-i omega/S) - 2 + exp(i omega/S) = -4 sin^2(omega/(2S))
=>     (L_y phi)_j + Om * n_j^2 * phi_j = mu * phi_j       (the mode equation)
Om     = 4 S^2 sin^2(omega / (2S))                        leapfrog frequency term
```
This is the characteristic equation the program solves: an eigenvalue problem `A phi = mu phi` with
```
A = L_y + diag(Om * n_j^2)        j = 0..N-1, phi = 0 outside the array (Dirichlet)
(L_y phi)_j = -(phi_{j-2} + phi_{j+2})/12 + 4/3*(phi_{j-1} + phi_{j+1}) - 5/2*phi_j
```
`A` is symmetric pentadiagonal. The fundamental mode of a symmetry (even or odd about the array's middle) is the
eigenvector of the largest eigenvalue `mu` in that symmetry class; then `beta = mu^-1(mu)` in (0, pi).

Informative only (not computed): for `S -> inf` and small `beta`, `Om -> omega^2`, `mu(beta) -> beta^2` and
`L_y -> d^2/dy^2`, which is the continuous scalar slab equation `phi'' + (omega^2 n^2 - beta^2) phi = 0` whose even
fundamental mode satisfies `tan(k_t w/2) = g/k_t` (`k_t = sqrt(omega^2 n_c^2 - beta^2)`,
`g = sqrt(beta^2 - omega^2 n_cl^2)`); its second mode is cut off at `w = lam/(2 NA)`, the rule the gate uses for its
core width.

## Omega

```
omega = 2*pi / lam                                  rad/step, lam = wavelength in px at index 1
S     = ComputeSubSteps(maxC2 = 1)                  (30): max(1, (int)round_even(ceil(sqrt(1.0 * (f64)(32f/3f) / 2.0))))
                                                    = ceil(sqrt(5.3333335)) = 3
Om    = 4 * S^2 * sin(omega/(2S))^2
```
`S` is that of a scene whose fastest pixel has index 1 (the default blank canvas). `32f/3f` is the f32 constant
(10.666667f) widened to f64.

## Index profile: Rows

The transverse profile is sampled as the painter paints a horizontal band structure:
```
Rows(first, count, profile):  for j in 0..count-1, row j spans [first + j, first + j + 1):
  n_j = round_even( (1/4) * sum_{k=0..3} profile(first + j + (k + 0.5)/4), 2 decimals )
```
Every caller uses `first = -half`, `count = 2*half`, so the array is symmetric about y = 0 (row j mirrors row
N-1-j for a symmetric profile).

Profiles of the callers (`w` core width int, `cl` cladding band int, `lam` wavelength):

| Caller | half | profile(y) |
|---|---|---|
| `SingleBeta(lam, Om, n_core, n_clad, w, cl)` | `ceil(w/2 + reach + 8)`, `reach = cl > 0 ? cl : 2*lam` | `n_core` if `|y| <= w/2`; `n_clad` if `|y| <= w/2 + reach`; else 1.0 |
| `KappaAt(dist, Om, n_core, n_clad, w, cl)` | `ceil(dist/2 + w/2 + cl + 8)` | with `e = min(|y - dist/2|, |y + dist/2|)`: `n_core` if `e <= w/2`; `n_clad` if `e <= w/2 + cl`; else 1.0 |

```
SingleBeta = Beta(rows, Om, even)
KappaAt    = (be, bo) both non-null ? max(0, (be - bo)/2) : 0
             be = Beta(rows, Om, even), bo = Beta(rows, Om, odd)
```
The absorber's damping is left out of the profiles (the field there is about e^-3 of the core's).

## Numeric method: Beta(index[0..N-1], Om, odd)

Shifted inverse iteration with a banded Cholesky factor.
```
p_j   = Om * n_j^2
top   = max(0, max_j p_j)
sigma = top + 1e-9
M     = sigma*I - A          positive definite (L_y is negative semi-definite), so the largest eigenvalue of A
                             in a symmetry class is the dominant eigenvalue of M^-1 in it
```
Cholesky `M = L L^T`, L lower triangular with bands `l0` (diagonal), `l1`, `l2` (first, second sub-diagonal); M has
diagonal `sigma + 5/2 - p_j`, first off-diagonal `-4/3`, second `+1/12`:
```
for j in 0..N-1:
  if j >= 2: l2[j] = (1/12) / l0[j-2]
  if j >= 1: l1[j] = (-4/3 - (j >= 2 ? l2[j]*l1[j-1] : 0)) / l0[j-1]
  l0[j] = sqrt(sigma + 2.5 - p_j - l1[j]^2 - l2[j]^2)            (l1[0] = l2[0] = l2[1] = 0)
```
Start vector and iteration:
```
x_j = odd ? t : 1 - t^2,   t = (j + 0.5)/N - 0.5
mu  = NaN
for it in 0..399:
  symmetrise: for j in 0..N/2-1 (integer N/2): a = x_j, b = x_{N-1-j}
              even: x_j = (a + b)/2, x_{N-1-j} = x_j;   odd: x_j = (a - b)/2, x_{N-1-j} = -x_j
              if N odd and odd: x_{N/2} = 0
  normalise:  x = x / sqrt(sum x_j^2)
  next = x^T A x  (Rayleigh quotient; A applied with phi = 0 outside 0..N-1, as
                  -(x_{j-2} + x_{j+2})/12 + 4*(x_{j-1} + x_{j+1})/3 - 2.5*x_j + p_j*x_j, summed j = 0..N-1)
  if |next - mu| < 1e-15: mu = next; stop
  mu = next
  solve L L^T x' = x:  forward  y_j = (x_j - l1[j]*y_{j-1} - l2[j]*y_{j-2}) / l0[j]   (j ascending)
                       backward x_j = (y_j - l1[j+1]*x_{j+1} - l2[j+2]*x_{j+2}) / l0[j]   (j descending)
                       (terms with an index outside 0..N-1 omitted; in place)
edge = max(p_0, p_{N-1})
return mu > edge ? BetaOf(mu) : null          (NaN fails the test)
```
- Tolerance 1e-15 absolute on `mu`; at most 400 iterations. Reaching the limit is not an error: the last Rayleigh
  quotient is used (seen for the odd mode of a single guide, below).
- The convergence factor per iteration is `(sigma - mu_1)/(sigma - mu_2)` of the two largest eigenvalues of the class.
- Guidedness is tested against the edge rows (index 1.0 in every caller), not the cladding: a mode held only by the
  finite cladding band counts as guided.
- Symmetry is about the middle of the array; a non-symmetric profile gives no meaningful odd mode (not used).

`BetaOf(mu)`: bisection, 100 iterations, on `[0, pi]`:
```
lo = 0, hi = pi
repeat 100: mid = (lo + hi)/2; if mu(mid) < target: lo = mid else hi = mid
return (lo + hi)/2
```
`mu` is monotonic on (0, pi) from 0 to 16/3; a target outside clamps to about 0 or pi.

## Quantities the gate derives

| Quantity | Formula | Unit |
|---|---|---|
| `beta` | `SingleBeta(...)` | rad/px |
| `gamma` | `sqrt(max(1e-9, mu(beta) - Om*n_cl^2))` (decay `exp(-gamma |y|)` in the cladding) | 1/px |
| `dbeta` | `|beta(n_p) - beta(n_c)|` (phase section) | rad/px |
| `kappa(dist)` | `(beta_even - beta_odd)/2` of two guides `dist` apart centre to centre | rad/px |
| bend coupling | Simpson integral of `kappa` along the S-bend (21) | rad |

## Precision for a port

f64 throughout; deterministic up to the last ulps of `sin`/`cos`/`sqrt` in the platform's libm. A port must reproduce
the gate's integers (widths, lengths, spacings) exactly and `beta`, `kappa` to about 1e-10. Checks in the C# harness
(`library_check`): the solver's `beta` for rows `Rows(-57, 114, |y| <= 4 -> 1.6, |y| <= 53 -> 1.45, else 1.0)` at
`lam = 16` matches the `beta` measured on a simulated guide within 2e-3.

## Worked example (the default gate, Example 18)

Computed with a Python port of the formulas above (run with `python3 -I`):

`lam = 32.044`, `omega = 2*pi/lam = 0.1960799309`, `S = 3`, `Om = 36*sin(0.0326799885)^2 = 0.0384336543`
(continuous `omega^2 = 0.0384473393`).

Single guide, `SingleBeta(32.044, Om, 2.55, 1.45, w = 6, cl = 0)`: `reach = 2*lam = 64.088`,
`half = ceil(3 + 64.088 + 8) = 76`, N = 152 rows:

| Rows j | y span | n_j |
|---|---|---|
| 0..8 | -76 .. -67 | 1.00 |
| 9..72 | -67 .. -3 | 1.45 |
| 73..78 | -3 .. 3 | 2.55 |
| 79..142 | 3 .. 67 | 1.45 |
| 143..151 | 67 .. 76 | 1.00 |

(No partial rows: the core edge |y| = 3 and the band edge 67.088 fall between sample points.)

| Result | Value |
|---|---|
| `p_edge = Om*1.0^2`, `top = Om*2.55^2` | 0.0384337, 0.2499148 |
| even mode, iterations to `|dmu| < 1e-15` | 22 |
| `mu` | 0.17376364604 (> p_edge: guided) |
| `beta = BetaOf(mu)` | 0.41691857833 rad/px |
| effective index `beta/omega` | 2.12627 |
| `gamma = sqrt(mu - Om*1.45^2)` | 0.30488832 /px (1/gamma = 3.28 px; `c = ceil(3/gamma) = 10`) |
| odd mode | hits the 400-iteration limit; `beta_odd = 0.2812`, `mu = 0.0791` < `Om*1.45^2 = 0.0808` (held by the finite band only; unused) |

With the solver's band `cl = o = 27` (`half = ceil(3 + 27 + 8) = 38`, N = 76): `beta = 0.41691857764`; with the
phase core 2.45: `beta_p = 0.39908083676`; `dbeta = 0.01783774088`.

Coupled guides `KappaAt(dist, Om, 2.55, 1.45, 6, 27)`:

| dist | N | beta_even (it) | beta_odd (it) | kappa (rad/px) |
|---|---|---|---|---|
| 10 | 86 | | | 0.0159001222 |
| 12 | 88 | 0.4247124722 (19) | 0.4076354734 (25) | 0.0085384994 |
| 20 | 96 | | | 0.0007358294 |
| 60 | 136 | 0.4169185817 (21) | 0.4169185743 (21) | 3.7e-9 |

Harness fixture: `Rows(-57, 114, ...)` at `lam = 16`: `Om = 0.1539924953`, `beta = 0.5985080080`.
