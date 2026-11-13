# Divergences from WaveDude Next

What this program does differently from WaveDude Next on purpose, at a glance; each line points to the decision
that holds the reasoning and the details. Behaviour not listed here is meant to match (spec/, ui/).

## When a divergence is warranted

- The literature has a more accurate or better-bounded technique (absorbers, stencils, time integration, material
  smoothing): the decision cites it, [`physics.md`](physics.md) derives it, a test states the tolerance.
- Next's behaviour is a bug or a quirk of its implementation (a timer, a race, a stale state): not ported.
- A Windows-only or WPF-only integration has no cross-platform equivalent: dropped, or replaced by what the
  packaging gives.
- The toolkit or a platform cannot do it: the decision says what replaces it.
- A measured gain with the same results, recorded in [`measurements.md`](measurements.md).

Not warranted: taste, convenience, calibrating to Next's output, dropping a feature Next's users rely on, or a
file written in a way Next cannot read. The process: an issue, a decision (new or a dated section of an existing
one), the row below, the spec updated, the manual when users see it.

## Divergences

| Area | WaveDude Next | WaveDudeRust | Where |
|---|---|---|---|
| Live preview | A scratch runner on a second engine computes the frame looked at; a separate run stores every frame; a 120 ms debounce and 1.5 s of quiet (dropped upstream 2026-11-09) pace them; an edit leaves a "stale" run | One worker, one engine, one store converge on a goal: the frame looked at first, the rest when wanted; no timers; an edit replaces the frames and the pictures keep the last one | decision 0012, [ui/60](ui/60-live-editing-and-runs.md), [spec/30](spec/30-engine.md) |
| Probe edit | The frame shown refreshes its graph; other frames' graphs stay stale until the next run | The frames stay, the recording is made anew for the whole run when every frame is wanted | decision 0012 |
| Undo steps | End at input boundaries (upstream since 2026-11-09) | End after 0.5 s of quiet with no mouse button held; a held key is one step | [ui/60](ui/60-live-editing-and-runs.md), decision 0012 |
| Solver | 9-point and 4th order stencils as scaled Laplacians, sources on the grid | True Laplacians, CPML absorber, dispersion-relation-preserving weights, sub-steps for stability; the physics differs by design | decision 0010, [physics.md](physics.md), 0003 |
| Source start | The source cell is blended towards `sin` from the first step; a single source leaves a slowly decaying zero-frequency wake, an array drains it through the blend | The source drives its impulse, turned on over half a period: no zero-frequency wake from any source; the first periods of a source at phase 0 differ | decision 0022, [physics.md](physics.md) |
| Display brightness | Tuned to its source, which radiates 4.8 to 9.7 times less | Own units; the `.wdproj` adapter converts by a fixed factor of 8 in both directions | decision 0004, [spec/60](spec/60-file-formats.md), [spec/05](spec/05-deviations.md) |
| Video | Windows Media Foundation H.264 with a hardware encoder | Cisco's OpenH264 downloaded on consent, quality rate control, no hardware encoder; AVI, MOV, MPEG-1 built in | decision 0005 |
| Frame store | Memory then a file, frames appended in order | The same tiers, frames written by index in any order, cleared in place between generations | [spec/40](spec/40-frames-store-render.md) |
| GPU | OpenCL, a transfer queue of its own for captures | wgpu compute, one queue; captures run after the steps ([measurements.md](measurements.md)) | decision 0003 |
| Docking | Floating windows, a five-way drop cluster | Four side lists around the centre; drops on edges, tabs and ends; no floating windows | decision 0008 (11) |
| Drag and drop | Tree rows drag into other windows and editors | Within the tree and onto a picture; the clipboard text does the rest | decision 0008 (12) |
| File drop | A project or colour scheme dropped on the window opens | An image dropped on the window also becomes the scene image; the last file of a frame wins | [ui/10](ui/10-menus-and-shortcuts.md) |
| Leaving the browser page | Not applicable | The browser build asks before the page is left while the project has unsaved edits | [ui/10](ui/10-menus-and-shortcuts.md) |
| User colour schemes | Files placed in a folder by hand | Added to and removed from the user's schemes in the tree, the scheme form and the scheme picker; kept in the config folder or the browser's storage | [spec/10](spec/10-model.md), [ui/20](ui/20-project-tree.md) |
| Browser | None | The web build has no exports (parked), no updater, no recent files, no file watch | decisions 0019, 0020, 0021 |
| Edit menu | Acts on a focused text box | Acts on objects; the keys go to the box | decision 0008 (13) |
| Windows integrations | Taskbar progress, jump list, title-bar menu click-through | Not ported; file associations come from the packaging | decision 0008 (14) |
| Manual figures | Numbered badges | Screenshots of this program, controls named in reading order | decision 0008 (15) |
| Counting detector | A frame's detections are shared evenly over the steps since the last collection, drawn from the frame's mean rate | One count a step from that step's integrated intensity, since every step's readings are at hand; the same Poisson process, so the run's total agrees | [spec/30](spec/30-engine.md) (Probes) |
| Probe graph range | A fitted range tops at the data's maximum, so a precursor the picture renders black fills the graph | A fitted range tops no lower than the probe layer's display full scale (amplitude, intensity); the comparison CLI keeps Next's fit | [ui/40](ui/40-preview-pane.md) |
| Updates and releases | Installer with EV signature | GitHub releases with attestation, an in-app updater signed with its own key | decision 0011 |
| Examples | A File > Examples submenu of its 33 projects, the description as tooltip | A File > Examples... dialog grouped by topic in six tabs with 53 projects (Next's 33 and our own 20), title and description per row, a search | decision 0008, [ui/70](ui/70-dialogs-and-windows.md) |
