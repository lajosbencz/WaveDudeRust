# 21 - Logic gate design (XOR, AND, OR)

The layout of the three logic gate kinds (`XorGate`, `AndGate`, `OrGate`) as geometry, and how it is painted. The
layout is computed from the part's properties by a design routine that calls the mode solver of
`22-waveguide-modes.md`. Pointers: `Parts/LogicGateDesign.cs`, `Model/OpticalPart.cs` (`LogicGatePart` and the three
kinds), `Parts/PartShape.cs` (`LogicGateShape`), `Parts/GatePortFollower.cs`.

All lengths are pixels (cells), all angles of the waves radians unless marked deg. The part frame (x', y') has its
origin at the part's centre (X, Y) (the top-left corner of canvas pixel (X, Y)), x' along the gate's axis from the
inputs (-x') to the outputs (+x'), y' across it, y' < 0 being "upper" (towards smaller canvas y at Angle 0). Part frame
to canvas: see rotation in `00-overview.md`. Below, x and y mean x' and y'.

## What a gate is

Two single-mode slab waveguides (arms) mirrored about the axis: arm A at y < 0, arm B at y > 0. From left to right:
open input end, straight input stretch with the input ports, a lead, a phase-section stretch (one arm's core index
changed over part of it), a lead, an S-bend that brings the arms together, a straight directional coupler, an S-bend
apart, a straight output stretch with the output ports and (optionally) an output absorber, open output end.

The coupler is a 3 dB coupler: for coherent inputs a (arm A) and b (arm B) at the coupler entrance it sends
`(a + i b)/sqrt(2)` to the upper output (arm A) and `(i a + b)/sqrt(2)` to the lower output (arm B). The phase section
puts wave B `PhaseShift` ahead of wave A. The kinds differ only in `PhaseShift` (fixed per kind, not persisted):

| Kind | PhaseShift (deg) | Upper output (arm A) | Lower output (arm B) |
|---|---|---|---|
| XOR | +90 | Xor: `(a - b)/sqrt(2)` | And: `i(a + b)/sqrt(2)` |
| AND | -90 | And | Xor |
| OR | +30 | Or: waves meet at 120 deg | Remainder: waves meet at 60 deg |

Output powers relative to one input's power, for inputs (A, B) = 00, 10, 01, 11:

| Output | 00 | 10 | 01 | 11 |
|---|---|---|---|---|
| Xor | 0 | 1/2 | 1/2 | 0 |
| And | 0 | 1/2 | 1/2 | 2 |
| Or | 0 | 1/2 | 1/2 | 1/2 |
| Remainder | 0 | 1/2 | 1/2 | 3/2 |

These are design intents (labels in the UI), not computed by the program.

## Inputs

| Symbol | Source | Unit | Range / rule |
|---|---|---|---|
| `F` | `DesignFrequency` | level | 1..255, round (even) to 2 decimals on Normalize; default 50 |
| `s` | `Simulation.SourceFrequencyScale` | - | 0..1000, default 2; not stored on the gate |
| `n_c` | `CoreIndex` | - | NormalizeIndex: clamp 1..2.55, round (even) 2 decimals, NaN -> 1; default 2.55 |
| `n_cl` | `CladdingIndex` | - | as `n_c`; default 1.45 |
| `P_in` | `PortSpacing` | px (int) | clamp 2..2000; default 2 (= "as close as the design allows") |
| `phi` | kind | deg | +90, -90, +30 |
| `T_ar` | `Coating` | px (int) | clamp 0..50; default 6; AR band at the ends |
| | `InputCoated`, `OutputCoated` | bool | default true |
| | `UpperOutputActive`, `LowerOutputActive` | bool | default true |
| `X, Y, Angle` | common part attributes | px, px, deg | see 10 |

The gate has no width or length property: both follow from the design (unit width `2*P`, length `Lambda`, below).

Wavelength in a medium of index 1 (`Simulation.Wavelength` with speed 1 px/step):
```
cyc     = F/255 * 0.5 * s / (2*pi)                     cycles per step
lam_raw = cyc > 0 ? 1/cyc : NaN                         = 2*pi / (F/255 * 0.5 * s)
lam     = round_even(clamp(lam_raw, 6, 200), 3 decimals)   only if lam_raw is finite and > 0, else: no design
```
The design is a pure function of `(lam, n_c, n_cl, P_in, phi)`; the C# caches it per key (cache cleared beyond 64
entries), which is not observable.

## Constants

| Name | Value | Meaning |
|---|---|---|
| `PHASE_STEP` | 0.1 | index step of the phase section |
| `MAX_INDEX` | 2.55 | largest part index |
| `BEND_EXP` | 11.0 | bend radiation exponent: `(2/3) gamma^3 R / beta^2 = BEND_EXP` |
| `MIN_ABSORBER` | 0.5 | narrowest absorbing band, in wavelengths |
| `ABSORPTION` | 64 | blue level at full absorber share |
| `BEND_SHARE_LIMIT` | 0.6 | largest share of pi/4 the two S-bends may couple |
| `COUPLING_EXCESS` | 0.07 rad | measured extra coupling, taken off the straight coupler |
| `BEND_SAMPLES` | 24 | Simpson intervals per S-bend |

## Design procedure

`Omega`, `SingleBeta`, `KappaAt`, `mu(beta)` are defined in `22-waveguide-modes.md`. `(int)` truncates, `round_even`
is half-to-even, `ceil`/`floor` as usual; all arithmetic f64.

```
 1  if not (lam > 0) or not (n_c > n_cl): no design
 2  Om  = Omega(lam)
 3  NA  = sqrt(n_c^2 - n_cl^2)
    w   = max(2, (int)floor(0.9 * lam / (2*NA)))                core width (int px): 0.9 of the slab's
                                                                  second-mode cut-off lam/(2 NA)
 4  beta0 = SingleBeta(lam, Om, n_c, n_cl, w, 0)                 null -> no design
 5  gamma = sqrt(max(1e-9, mu(beta0) - Om*n_cl^2))               cladding decay constant (1/px)
 6  c     = max(2, (int)ceil(3/gamma))                           clear cladding thickness (int px), 3 decay lengths
    a_min = max(2, (int)ceil(MIN_ABSORBER * lam))                narrowest absorber band (int px)
    o     = c + a_min                                            cladding band seen by the solver (int px)
 7  lowered = n_c + PHASE_STEP > MAX_INDEX + 1e-9
    b_leads = phi >= 0
    phaseOnA = (b_leads == lowered)
    n_p   = round_even(lowered ? n_c - PHASE_STEP : n_c + PHASE_STEP, 2 decimals)
 8  beta  = SingleBeta(lam, Om, n_c, n_cl, w, o) ?? beta0
    beta_p = n_p > n_cl ? (SingleBeta(lam, Om, n_p, n_cl, w, o) ?? beta) : beta
    dbeta = |beta_p - beta|                                      if dbeta < 1e-6: no design
 9  L_reg = max(1, (int)round_even((pi/2) / dbeta))             phase-section stretch (quarter period shift)
    L_ph  = min(L_reg, (int)round_even(|phi| * pi/180 / dbeta)) phase section length
10  R     = BEND_EXP * 1.5 * beta^2 / gamma^3                    smallest S-bend radius (px)
11  gap   = max(2, (int)round_even(1/gamma))
    gap  += (w + gap) mod 2                                      makes the coupled spacing even
    loop:
      D   = w + gap                                              coupled spacing, centre to centre (even)
      P   = max(P_in, max(D + w + 2c + 2, (int)ceil(w + 2.0*(c + a_min))))
      P  += P mod 2                                              port spacing used (even)
      L_b = max(1, (int)ceil(sqrt(2*pi*R*(P - D)/2)))            S-bend length; its peak curvature
                                                                  2*pi*Delta/L_b^2 (Delta = (P-D)/2) equals 1/R
      kappa = KappaAt(D, Om, n_c, n_cl, w, o)                    coupling constant in the straight coupler (rad/px)
      B   = BendKappa(D, P, L_b)                                 coupling of one S-bend (rad), below
      if 2B <= BEND_SHARE_LIMIT * pi/4 or gap >= 2c: break
      gap += 2
12  L_c = kappa > 0 ? max(0, (int)round_even((pi/4 - COUPLING_EXCESS - 2B) / kappa)) : 0
13  L_in   = (int)round_even(lam/2)       straight input stretch, end to input ports
    L_lead = (int)round_even(lam)         each lead
    L_out  = (int)round_even(3*lam)       straight output stretch
    L_ramp = (int)round_even(2*lam)       output absorber rise
```
`BendKappa` (composite Simpson over the S-bend, `BEND_SAMPLES = 24` intervals):
```
BendKappa(D, P, L_b) = L_b/(3*24) * sum_{i=0..24} wt_i * KappaAt(P - (P - D)*S(i/24), Om, n_c, n_cl, w, o)
wt_i = 1 for i in {0, 24}; 4 for odd i; 2 for even i
S(t) = t - sin(2*pi*t)/(2*pi)                                    S-bend shape, S(0) = 0, S(1) = 1, S'(0) = S'(1) = 0
```
Derived:
```
Lambda = L_in + L_lead + L_reg + L_lead + 2*L_b + L_c + L_out     total length (int px)
A_w    = P/2 - w/2 - c                                           absorber band width (px, >= a_min by construction)
halfW  = P                                                       half the unit's outer width
```
Note: the outer radius of each arm `w/2 + c + A_w` is always exactly `P/2`.

## Stations along the axis

```
x_L   = -Lambda/2                        left (input) end
x_in  = x_L + L_in                       input ports
x_ps  = x_in + L_lead                    phase section start
x_pe  = x_ps + L_ph                      phase section end (exclusive)
x_b1  = x_in + 2*L_lead + L_reg          S-bend in starts (arms at P/2)
x_c0  = x_b1 + L_b                       coupler starts (arms at D/2)
x_c1  = x_c0 + L_c                       coupler ends, S-bend out starts
x_b2  = x_c1 + L_b  (= Lambda/2 - L_out) S-bend out ends
x_out = Lambda/2 - L_out + round_even(lam/2)   output ports (f64; round_even(lam/2) not truncated)
x_r   = Lambda/2 - L_ramp                output absorber starts to rise
x_R   = +Lambda/2                        right (output) end
```

## Arm centre lines

Arm A follows `y = -h(x)`, arm B `y = +h(x)`, with `far = P/2`, `near = D/2`:
```
x <= x_b1 or x >= x_b2:   h = far,                          h' = 0
x_c0 <= x <= x_c1:        h = near,                         h' = 0
x_b1 < x < x_c0:          t = (x - x_b1)/L_b,     h = far - (far - near)*S(t),  h' = -(far - near)*(1 - cos(2*pi*t))/L_b
x_c1 < x < x_b2:          t = 1 - (x - x_c1)/L_b, h = far - (far - near)*S(t),  h' = +(far - near)*(1 - cos(2*pi*t))/L_b
```
(Order of tests: the first line first, then the coupler, else the bends.)

Distance of a point from its arm (first order, exact on straight stretches), and the arm it belongs to:
```
delta(x, y) = | |y| - h(x) | / sqrt(1 + h'(x)^2)
arm(y)      = B if y > 0 else A                       (y = 0 belongs to A)
```

## Cross-section

Measured as `delta` from an arm's centre line:

| Zone | delta range | Index | Absorption share (side band) |
|---|---|---|---|
| core | `delta <= w/2` | `n_c`, or `n_p` in the phase section of the phase arm | 0 |
| clear cladding | `w/2 < delta <= w/2 + c` | `n_cl` | 0 |
| absorber band | `w/2 + c < delta <= P/2` | `n_cl` | `(1 - (P/2 - delta)/A_w)^2`, 0 at the inner edge, 1 at the rim |
| outside | `delta > P/2` | unchanged | 0 |

Where the arms are far apart (`h = P/2`), the inner absorber bands of A and B meet at y = 0 with share 1. In the
coupler (`h = D/2`) the region between the cores is clear cladding, and the outer edge lies at `|y| = D/2 + P/2`.

```
cross-section of arm A at a port (h = P/2), y' downwards; r_c = w/2, r_k = w/2 + c; default values in brackets
  y' = -P       -P/2-r_k     -P/2-r_c   -P/2   -P/2+r_c     -P/2+r_k       0
       [-60]     [-43]        [-33]     [-30]   [-27]        [-17]         [0]
        | absorber | clear clad | core   :   core | clear clad | absorber   |   arm B mirrored below
        share 1 -> 0            n_c (or n_p)                    0 -> share 1
```

## Outline, depth, extent

```
depth(x, y)  = min(Lambda/2 - |x|, P/2 - delta(x, y))      > 0 inside, not a Euclidean distance
Extent       = sqrt((Lambda/2)^2 + P^2)                     bounds: centre +- (Extent + 1), see 20
outline (overlay drawing only): for i = 0..64, x_i = x_L + Lambda*i/64:
  top    point i:        (x_i, -(h(x_i) + P/2))
  bottom point 129 - i:  (x_i, +(h(x_i) + P/2))            closed polygon of 130 points
```
`DepthIsDistance` is false (no settled runs), `InnerEdgeDistance` is 0 everywhere (`HasInnerEdges` true), the part
coating band (`Coating` of 20) is 0: so every pixel whose centre has `depth >= -0.6` is supersampled 4 x 4 (see the
refractive painting in 20).

## Index painted (red)

For a sample point inside (`depth >= 0`; outside it contributes the pixel's existing index):
```
n0 = delta > w/2                                            -> n_cl
     else (arm(y) is the phase arm) and x_ps <= x < x_pe     -> n_p        phase arm = A if phaseOnA else B
     else                                                   -> n_c
coated = (x < 0 ? InputCoated : OutputCoated) and T_ar > 0 and (Lambda/2 - |x|) < T_ar
n = coated ? round_even(sqrt(n0), 2 decimals) : n0
```
Pixel red: `clamp(round_even(mean of 16 samples * 100), 1, 255)` per 20. The end band covers core, cladding and
absorber zones alike (a quarter-wave AR band would be `lam/(4*sqrt(n_c))`, shown in the form; the default 6 px is the
measured least reflecting value).

## Absorbers painted (blue)

Painted before the index, at pixel centres whose depth (stored as f32) is `>= 0`; no supersampling:
```
inside = P/2 - delta(x, y)
if inside < 0 or |x| > Lambda/2: share = 0
else:
  side   = clamp(1 - inside/A_w, 0, 1)^2
  output = output_active(arm(y)) ? 0 : clamp((x - x_r)/L_ramp, 0, 1)^2
  share  = max(side, output)
blue = max(blue_old, round_even(64 * share))
output_active(A) = UpperOutputActive, output_active(B) = LowerOutputActive
```
The painter's generic ramp (`AbsorptionRamp = ceil(A_w)`, shortened to the deepest centre) is computed but not used
by this share. Green is never touched.

## Ports

```
port(x, y) = (floor(cx), floor(cy)) with (cx, cy) = ToCanvas(x, y)
Input A = port(x_in, -P/2)   Input B = port(x_in, +P/2)
Upper output = port(x_out, -P/2)   Lower output = port(x_out, +P/2)
```
No design -> no ports and nothing painted. Sources are ordinary project sources; one standing within 1 px on either
axis of an input port is moved onto the port's new position after a gate edit (UI behaviour, `GatePortFollower`).

## Wavelength dependence

Every length scales with the wavelength, through three channels: directly (`L_in`, `L_lead`, `L_out`, `L_ramp`,
`a_min`, the core cut-off in `w`), through the mode's confinement `gamma` (cladding `c`, bend radius `R`, start gap),
and through the solver's `beta`, `kappa`, `dbeta` (phase stretch, coupler length). The solver uses the 4th-order
stencil with the sub-step count of a scene whose fastest pixel has index 1 (`22-waveguide-modes.md`), so the design
holds for `Simulation.FourthOrderLaplacian = true` (the default) on a scene with max c2 = 1. The 9-point stencil, a
background index > 1 (fewer sub-steps), or a source frequency other than `F` miss the design (no warning is given).

Designs for other inputs (Python port of the C# procedure, `P_in` as given, `phi = 90`):

| lam | n_c | n_cl | P_in | w | c | gap | P | L_b | L_c | L_reg | L_ph | Lambda | n_p | phase arm |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 12 | 1.6 | 1.45 | 64 | 7 | 12 | 7 | 64 | 309 | 81 | 35 | 35 | 800 | 1.70 | B |
| 16 | 1.6 | 1.45 | 64 | 10 | 15 | 8 | 64 | 326 | 93 | 46 | 46 | 879 | 1.70 | B |
| 32 | 1.6 | 1.45 | 64 | 21 | 30 | 15 | 120 | 608 | 190 | 91 | 91 | 1673 | 1.70 | B |
| 16.022 | 2.0 | 1.45 | 2 | 5 | 8 | 5 | 40 | 107 | 63 | 46 | 46 | 411 | 2.10 | B |
| 32.044 | 2.55 | 1.45 | 2 | 6 | 10 | 6 | 60 | 124 | 32 | 88 | 88 | 544 | 2.45 | A |

(16.022 and 32.044 are levels 100 and 50 at scale 2. These satisfy the library_check assertions: core width rising
with lam, even spacings, `L_c > 0`, default length < 650, level 100 / core 2.0 length < 500, default phase on A at
2.45, core 2.0 phase on B at 2.10.)

## Sketches

Top view of the default design (Example 18), x' and y' in px, not to scale. `=` core, `#` phase section,
`o` port, `|` open ends, `~` output absorber zone (only when that output is switched off):
```
x':  -272 -256 -224        -136  -104          20   52           176  192 208      272
       |    o    |###########|     |   bend in   | cpl |  bend out  |   o   |~~~~~~~~|
y'-30  |====o====###########================\                  /=========o===~~~~~~~~|   arm A (upper output)
       |                                     \                /                      |
y' -6  |                                      \==============/                       |   coupler, D = 12
y' +6  |                                      /==============\                       |
       |                                     /                \                      |
y'+30  |====o=============================/                    \=========o===~~~~~~~~|   arm B (lower output)
       |  input                                                         output       |
       |  ports           <- L_reg = 88 ->                              ports        |
outer edge: y' = -(h + 30) and +(h + 30): +-60 at the ports, +-36 in the coupler
```
Per kind, only the phase section differs (stretch x' -224..-136, 88 px, in all three):
```
XOR  arm A: [#################] n_p = 2.45 over 88 px (x' -224..-136)   B leads by +90   upper = Xor, lower = And
     arm B: [                 ]
AND  arm A: [                 ]                                          A leads by 90    upper = And, lower = Xor
     arm B: [#################] n_p = 2.45 over 88 px (x' -224..-136)
OR   arm A: [######           ] n_p = 2.45 over 29 px (x' -224..-195)   B leads by +30   upper = Or,  lower = Remainder
     arm B: [                 ]
```

## Worked example: Example 18 (`Examples/Example18_XOR_AND_OR_logic.wdproj`)

All three gates use the defaults (`F` 50, `n_c` 2.55, `n_cl` 1.45, `P_in` 2, `T_ar` 6), Angle 0, scale `s` = 2,
4th-order stencil, blank canvas of index 1 (1400 x 401, damping edge 32). Values from a Python port of the C#
formulas (f64; integers exact, floats to the digits shown):

| Step | Quantity | Value |
|---|---|---|
| | `lam_raw`, `lam` | 32.0442450666, 32.044 |
| 2 | `Om` (S = 3) | 0.0384336543 |
| 3 | `NA`, `w` | 2.0976177, floor(0.9*32.044/4.19524) = floor(6.874) = 6 |
| 4 | `beta0` (band 2 lam) | 0.4169185783 rad/px |
| 5 | `mu(beta0)`, `gamma` | 0.1737636460, 0.3048883 /px (1/gamma = 3.28 px) |
| 6 | `c`, `a_min`, `o` | 10, 17, 27 |
| 7 | lowered, `n_p` | true (2.65 > 2.55), 2.45 |
| 8 | `beta`, `beta_p`, `dbeta` | 0.4169185776, 0.3990808368, 0.0178377409 |
| 9 | `L_reg`, `L_ph` | round(88.06) = 88; XOR/AND 88, OR round(29.35) = 29 |
| 10 | `R` | 101.196 px |
| 11 | loop 1: gap 4, D 10, P 60, L_b 127, kappa 0.0159001, 2B 0.83264 > 0.47124 | gap += 2 |
| 11 | loop 2: gap 6, D 12, P 60, L_b 124, kappa 0.0085385, 2B 0.44281 <= 0.47124 | stop |
| 12 | `L_c` | round((0.785398 - 0.07 - 0.442811)/0.0085385) = round(31.92) = 32 |
| 13 | `L_in`, `L_lead`, `L_out`, `L_ramp` | 16, 32, 96, 64 |
| | `Lambda`, `A_w`, unit | 16+32+88+32+248+32+96 = 544, 17, 120 x 544 |
| | phase arm | XOR: A (phaseOnA = (true == true)); AND: B; OR: A |

Stations in x' and in canvas x (canvas x = X + x', Angle 0):

| Station | x' | XOR (X 396) | AND (X 936) | OR (X 935) |
|---|---|---|---|---|
| left end `x_L` | -272 | 124 | 664 | 663 |
| input ports `x_in` | -256 | 140 | 680 | 679 |
| phase start `x_ps` | -224 | 172 | 712 | 711 |
| phase end `x_pe` | -136 (OR -195) | 260 | 800 | 740 |
| bend in `x_b1` | -104 | 292 | 832 | 831 |
| coupler `x_c0` .. `x_c1` | 20 .. 52 | 416 .. 448 | 956 .. 988 | 955 .. 987 |
| bend out end `x_b2` | 176 | 572 | 1112 | 1111 |
| output ports `x_out` | 192 | 588 | 1128 | 1127 |
| absorber rise `x_r` | 208 | 604 | 1144 | 1143 |
| right end `x_R` | 272 | 668 | 1208 | 1207 |

Ports (canvas): XOR (Y 163): A (140, 133), B (140, 193), upper (588, 133), lower (588, 193). AND (Y 103): A (680, 73),
B (680, 133), upper (1128, 73), lower (1128, 133). OR (Y 223): A (679, 193), B (679, 253), upper (1127, 193),
lower (1127, 253). The XOR's upper output row 133 is the AND's input B row, its lower output row 193 the OR's input A
row; the units overlap by 4 to 5 columns (XOR right end 668 against 664 and 663), where the later part in list order
paints over the earlier one.

Arm centre rows (canvas y = Y + y'): XOR A 133 at the ports, 157 in the coupler; B 193 and 169. Outer edges at the
ports y 103 and 223, in the coupler 127 and 199.

Indices: core 2.55 (red 255), phase section 2.45 (red 245), cladding and absorber band 1.45 (red 145). Ends: the XOR
has `InputCoated = OutputCoated = false` (no bands); AND and OR have `InputCoated = false` and an output band where
`272 - x' < 6`, i.e. x' > 266: core `round(sqrt(2.55), 2)` = 1.60 (red 160), cladding `round(sqrt(1.45), 2)` = 1.20
(red 120).

Side absorber (all outputs active, no output absorber): `A_w` = 17, `inside = 30 - delta`; share > 0 for
`delta > 13`:

| delta (px) | inside | share | blue |
|---|---|---|---|
| 13.5 | 16.5 | 0.00087 | 0 |
| 17 | 13 | 0.0554 | 4 |
| 21.5 | 8.5 | 0.25 | 16 |
| 25 | 5 | 0.498 | 32 |
| 29.5 | 0.5 | 0.942 | 60 |
