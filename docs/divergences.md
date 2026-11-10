# Divergences from WaveDude Next

What this program does differently from WaveDude Next on purpose, at a glance; each line points to the decision
that holds the reasoning and the details. Behaviour not listed here is meant to match (spec/, ui/).

| Area | WaveDude Next | WaveDudeRust | Where |
|---|---|---|---|
| Live preview | A scratch runner on a second engine computes the frame looked at; a separate run stores every frame; a 120 ms debounce and 1.5 s of quiet (dropped upstream 2026-11-09) pace them; an edit leaves a "stale" run | One worker, one engine, one store converge on a goal: the frame looked at first, the rest when wanted; no timers; an edit replaces the frames and the pictures keep the last one | decision 0012, ui/60, spec/30 |
| Probe edit | The frame shown refreshes its graph; other frames' graphs stay stale until the next run | The frames stay, the recording is made anew for the whole run when every frame is wanted | decision 0012 |
| Undo steps | End at input boundaries (upstream since 2026-11-09) | End after 0.5 s of quiet with no mouse button held; a held key is one step | ui/60, decision 0012 |
| Solver | 9-point and 4th order stencils as scaled Laplacians, sources on the grid | True Laplacians, CPML absorber, dispersion-relation-preserving weights, sub-steps for stability; the physics differs by design | decision 0010, physics.md, 0003 |
| Display brightness | Tuned to its source, which radiates 4.8 to 9.7 times less | Own units; the `.wdproj` adapter converts by a fixed factor of 8 in both directions | decision 0004, spec/60, spec/05 |
| Video | Windows Media Foundation H.264 with a hardware encoder | Cisco's OpenH264 downloaded on consent, quality rate control, no hardware encoder; AVI, MOV, MPEG-1 built in | decision 0005 |
| Frame store | Memory then a file, frames appended in order | The same tiers, frames written by index in any order, cleared in place between generations | spec/40 |
| GPU | OpenCL, a transfer queue of its own for captures | wgpu compute, one queue; captures run after the steps (measurements.md) | decision 0003 |
| Docking | Floating windows, a five-way drop cluster | Four side lists around the centre; drops on edges, tabs and ends; no floating windows | decision 0008 (11) |
| Drag and drop | Tree rows drag into other windows and editors | Within the tree and onto a picture; the clipboard text does the rest | decision 0008 (12) |
| Edit menu | Acts on a focused text box | Acts on objects; the keys go to the box | decision 0008 (13) |
| Windows integrations | Taskbar progress, jump list, title-bar menu click-through | Not ported; file associations come from the packaging | decision 0008 (14) |
| Manual figures | Numbered badges | Screenshots of this program, controls named in reading order | decision 0008 (15) |
| Counting detector | A frame's detections are shared evenly over the steps since the last collection, drawn from the frame's mean rate | One count a step from that step's integrated intensity, since every step's readings are at hand; the same Poisson process, so the run's total agrees | spec/30 (Probes) |
| Updates and releases | Installer with EV signature | GitHub releases with attestation, an in-app updater signed with its own key | decision 0011 |
| Examples | A File > Examples submenu of its 24 projects, the description as tooltip | A File > Examples... dialog grouped by topic in five tabs with 44 projects (Next's 24 and our own 20), title and description per row, a search | decision 0008, ui/70 |

Adding a divergence: a line here, the details in a decision (a new one or a dated section of an existing one),
the behaviour in the spec or ui page it changes.
