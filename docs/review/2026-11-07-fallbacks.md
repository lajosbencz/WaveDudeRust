# Fallback audit, 2026-11-07

Read-only pass over the workspace for fallbacks that handle nothing: defaults for cases the model's
normalisation or a type already excludes, silent substitutions that would hide a bug, catch-all match arms
over our own enums, and duplicate guards. Guards for user input, file content, OS and hardware conditions
and first-frame layout were kept and are listed at the end of each section.

Normalisation is the anchor: every project is normalised on load ([`io/wdproj.rs:122`](../../crates/wavedude-core/src/io/wdproj.rs#L122), `io/wdr.rs:107`,
[`io/legacy.rs:264`](../../crates/wavedude-core/src/io/legacy.rs#L264)), when a document is built ([`wavedude-app-state/src/document.rs:59`](../../crates/wavedude-app-state/src/document.rs#L59)) and after every edit
command ([`wavedude-app-state/src/commands.rs:390`](../../crates/wavedude-app-state/src/commands.rs#L390)), with the bounds of [`model/ranges.rs`](../../crates/wavedude-core/src/model/ranges.rs). Settings are
validated at their boundary ([`wavedude-app-state/src/settings.rs:313-317`](../../crates/wavedude-app-state/src/settings.rs#L313-L317)). N:line below is
[`model/normalize.rs`](../../crates/wavedude-core/src/model/normalize.rs).

## Verdict

- 1 bug hidden by a fallback (the frame store's spill).
- 4 structural causes behind most of the app findings: stringly typed menu ids, forms reading the model
  back out of `serde_json::Value` with defaults, swallowed `serde_json::to_value` errors, and string keys
  that fall back to the key.
- 3 duplicated implementations of model rules in the painter (`fit_radii`, `effective_angles`,
  `normalize_index`) and one in [`gate_design.rs`](../../crates/wavedude-core/src/scene/gate_design.rs).
- About 90 single clamps, `unwrap_or`s and catch-alls, listed below.

## 1. A bug: the frame store's spill ([run/store.rs](../../crates/wavedude-core/src/run/store.rs))

`in_dir` starts the spill file at `ram_frames` when `ram_frames < capacity` (store.rs:170). If the
producer's allocation fails at a frame j below `ram_frames`, `memory_end` falls to j (store.rs:268) and
`write(j)` calls `start_spill(j)` again (store.rs:295-297). The second `self.spill.set(..)` on the
`OnceLock` fails silently (`let _ =`, store.rs:252), the fresh file is dropped, but `first_spilled` is set
to j. `file_offset` then computes `j - ram_frames` (store.rs:315): a panic in debug, a wrap in release.

Fix: create the spill lazily only, at the first frame that does not fit, and `.expect` the `set`. The
header's `first` is then right by construction. `blocks[frame].set(..)` at store.rs:293 is the same
pattern: a failed set means a frame written twice; `.expect` it.

## 2. Structural causes (wavedude-app, wavedude-app-state)

1. Menu, tree and combo ids are strings parsed with silent fallbacks ([`ui/menus.rs:534, 543, 655-704`](../../crates/wavedude-app/src/ui/menus.rs),
   [`preview/mod.rs:150-165`](../../crates/wavedude-app/src/ui/preview/mod.rs#L150-L165)): `rsplit_once(':').unwrap_or(..)`, `if let Ok(i) = parse()`, `_ => debug!`,
   unknown theme names to `System`. Every id is made by the same files, so a miss is a typo. Fix: a typed
   `MenuAction` enum carried by `MenuItem`, matched exhaustively.
2. Property forms read typed model data out of `serde_json::Value` with defaults (`ui/properties.rs:135,
   139, 143, 176, 202, 212, 225, 236`; `properties_forms.rs:345, 681, 687, 730, 748-750, 770, 782,
   806-809, 953-955, 977, 1020, 1024`). The paths are code constants and `[ui/tests.rs:89](../../crates/wavedude-app/src/ui/tests.rs#L89)-98` proves they
   resolve; at runtime a typo shows 0, false or "" and writes the wrong type back. Fix: `expect` in
   `Form::get`/`bind`, `from_value::<T>().expect` for lists and stops; `detach_after` (`:414`) maps JSON
   null to None explicitly.
3. `serde_json::to_value` of our own structs with the error swallowed ([`properties.rs:40`](../../crates/wavedude-app/src/ui/properties.rs#L40),
   [`properties_forms.rs:430, 1005, 1006`](../../crates/wavedude-app/src/ui/properties_forms.rs), [`preview/overlay.rs:291, 328, 332`](../../crates/wavedude-app/src/ui/preview/overlay.rs) which would send
   `points = null` as an edit, [`ui/app.rs:195`](../../crates/wavedude-app/src/ui/app.rs#L195)). Fix: `.expect`, as [`history.rs:95`](../../crates/wavedude-app-state/src/history.rs#L95) does.
4. Missing string keys fall back to the key or "" ([`wavedude-app-state/src/strings.rs:198, 217-230`](../../crates/wavedude-app-state/src/strings.rs)). The
   per-language fallback to English (`:193`) stays; a key missing from English is a bug the tests already
   enforce. Fix: panic with the key (debug at least); the test asserting the leak flips to `should_panic`.
5. Two policies for missing platform dirs: `main.rs:23-24` builds a temp-dir fallback for the state while
   `BootInfo` keeps the raw `Option`, so [`settings_views.rs:8`](../../crates/wavedude-app/src/ui/settings_views.rs#L8) silently skips saving the layout. Fix: pass
   the resolved `AppDirs`.

## 3. Duplicated model rules

- [`scene/shapes/lens.rs:4-46`](../../crates/wavedude-core/src/scene/shapes/lens.rs#L4-L46) copies `fit_radii`/`guard_radius`/`sag` from N:333-376 and fits the already
  fitted radii again ([`lens.rs:62-65`](../../crates/wavedude-core/src/scene/shapes/lens.rs#L62-L65)). Use `lens.r1`/`r2` as stored; export `sag` if the painter needs it.
- [`scene/shapes/triangle.rs:8-21`](../../crates/wavedude-core/src/scene/shapes/triangle.rs#L8-L21) copies `Triangle::effective_angles` (N:249), which normalisation already
  wrote back (N:270). Use the fields.
- [`scene/gate_design.rs:56-58, 193-200`](../../crates/wavedude-core/src/scene/gate_design.rs) copies `normalize_index` and re-clamps `design_frequency` and
  `port_spacing` (N:314-317). Read the fields.
- [`model/normalize.rs:363-371`](../../crates/wavedude-core/src/model/normalize.rs#L363-L371) `surface_shape`/`SurfaceShape` has no caller outside its test: delete.
- [`model/normalize.rs:428`](../../crates/wavedude-core/src/model/normalize.rs#L428) re-implements `Probe::box_pixels` (probe.rs:139) with an impossible
  `unwrap_or((400, 200))`: keep one, make `nominal()` total.
- [`cpu.rs:314-321`](../../crates/wavedude-core/src/engine/cpu.rs#L314-L321) and [`gpu/mod.rs:541-547`](../../crates/wavedude-core/src/engine/gpu/mod.rs#L541-L547) `update_alpha` are identical: share.
- [`scene/shapes/mod.rs:175-176`](../../crates/wavedude-core/src/scene/shapes/mod.rs#L175-L176) `guard_index` and its five call sites; `Band::of` (`mod.rs:130`) and
  [`gate.rs:31`](../../crates/wavedude-core/src/scene/shapes/gate.rs#L31) re-clamp coatings (N:215, N:318).

## 4. Clamps and defaults the model already guarantees

wavedude-core engine/render/scene:
[`rectangle.rs:6-7`](../../crates/wavedude-core/src/scene/shapes/rectangle.rs#L6-L7), `ellipse.rs:7-8,16-17`, [`polygon.rs:7-8`](../../crates/wavedude-core/src/scene/shapes/polygon.rs#L7-L8), [`triangle.rs:27`](../../crates/wavedude-core/src/scene/shapes/triangle.rs#L27), [`bragg.rs:16-20`](../../crates/wavedude-core/src/scene/shapes/bragg.rs#L16-L20),
[`grin.rs:16-17`](../../crates/wavedude-core/src/scene/shapes/grin.rs#L16-L17), `fiber.rs:123,128,206`, `zone_plate.rs:14,37,38,42` and `zone_at` (`:9-12`, return
`i64`), `mod.rs:297-298`, [`gate_design.rs:111`](../../crates/wavedude-core/src/scene/gate_design.rs#L111) (keep `n_c <= n_cl` only), `painter.rs:32` (enabled is
filtered in [`compose.rs:35`](../../crates/wavedude-core/src/scene/compose.rs#L35)), `painter.rs:243-245` (`inner_edge` is always Some for distance shapes),
[`pixels.rs:15-17`](../../crates/wavedude-core/src/scene/pixels.rs#L15-L17), [`scene_data.rs:22`](../../crates/wavedude-core/src/scene/scene_data.rs#L22) (upper bound 10 unreachable from a u8; fix [spec/20](../spec/20-scene-painting.md) line 10),
[`scene_data.rs:46`](../../crates/wavedude-core/src/scene/scene_data.rs#L46), [`emitters.rs:55-56, 89, 106`](../../crates/wavedude-core/src/engine/emitters.rs), [`envelope.rs:53, 65, 74, 99, 109`](../../crates/wavedude-core/src/engine/envelope.rs) (`rng_seed` as
`NonZeroU32`, widths >= 1), [`substeps.rs:37`](../../crates/wavedude-core/src/engine/substeps.rs#L37), [`cpu.rs:144`](../../crates/wavedude-core/src/engine/cpu.rs#L144) (assert, not `.max(1)`), [`cpu.rs:314`](../../crates/wavedude-core/src/engine/cpu.rs#L314),
[`gpu/mod.rs:541`](../../crates/wavedude-core/src/engine/gpu/mod.rs#L541), [`capture.rs:46, 53`](../../crates/wavedude-core/src/engine/capture.rs), [`gpu/mod.rs:958`](../../crates/wavedude-core/src/engine/gpu/mod.rs#L958), `gradient.rs:14-15, 42-48 (delete), 60`,
[`brightness.rs:37, 39`](../../crates/wavedude-core/src/render/brightness.rs), mutex poison recovery at `lut.rs:202,211`, `gate_design.rs:206,210`,
[`compose.rs:66`](../../crates/wavedude-core/src/scene/compose.rs#L66) (nothing user-supplied runs under the lock; `unwrap` like [`gpu/context.rs`](../../crates/wavedude-core/src/engine/gpu/context.rs)).

wavedude-core model/io/run:
[`run/spec.rs:56`](../../crates/wavedude-core/src/run/spec.rs#L56) `NoSteps` (steps >= 1 by N:60; [`tests/run.rs:111`](../../crates/wavedude-core/tests/run.rs#L111) feeds an unnormalised project),
[`spec.rs:96`](../../crates/wavedude-core/src/run/spec.rs#L96) and [`model/simulation.rs:48`](../../crates/wavedude-core/src/model/simulation.rs#L48) `visualize_every.max(1)`, [`spec.rs:75, 108, 178`](../../crates/wavedude-core/src/run/spec.rs)
`quality.validated()` three times (make the divisor an enum `1 | 2 | 4`; `validated` also accepts 3),
`simulation.rs:397` capacity test (asserted at `:167-171`), [`store.rs:86`](../../crates/wavedude-core/src/run/store.rs#L86), [`normalize.rs:392, 398`](../../crates/wavedude-core/src/model/normalize.rs)
(`fit_radii` after the clamps at N:213-214), [`normalize.rs:316`](../../crates/wavedude-core/src/model/normalize.rs#L316) and `:22` (re-implement `clamp_or`),
[`io/fields.rs:86-88`](../../crates/wavedude-core/src/io/fields.rs#L86-L88) and [`wdproj.rs:74`](../../crates/wavedude-core/src/io/wdproj.rs#L74) (defaults for an attribute already found), `io/wdr.rs:113`
(format 0 silently read as 1: reject it), [`io/schemes.rs:143`](../../crates/wavedude-core/src/io/schemes.rs#L143) (lerp of two u8), [`legacy.rs:272`](../../crates/wavedude-core/src/io/legacy.rs#L272).
Catch-alls over our own enums: [`parts.rs:443, 457`](../../crates/wavedude-core/src/model/parts.rs) (`material()`, `coating()`), [`wdproj.rs:632`](../../crates/wavedude-core/src/io/wdproj.rs#L632)
(`write_part`), `fields.rs:122` (`AttrType`), `simulation.rs:256` (`wait_completed` reports a driver panic
as Cancelled; `join` drops it at `:262`).
Hidden errors: [`schemes.rs:86`](../../crates/wavedude-core/src/io/schemes.rs#L86) every `read_dir` error is "no schemes" (only `NotFound` should be),
[`legacy.rs:100`](../../crates/wavedude-core/src/io/legacy.rs#L100) every I/O error is "not found".

wavedude-export, wavedude-cli:
[`range.rs:15`](../../crates/wavedude-export/src/range.rs#L15), [`csv.rs:53-54`](../../crates/wavedude-export/src/csv.rs#L53-L54), [`overlays.rs:377`](../../crates/wavedude-export/src/overlays.rs#L377), [`avi.rs:148`](../../crates/wavedude-export/src/sinks/avi.rs#L148), [`mov.rs:162`](../../crates/wavedude-export/src/sinks/mov.rs#L162), [`mp4.rs:103`](../../crates/wavedude-export/src/sinks/mp4.rs#L103) (`fps`),
[`jpeg.rs:15`](../../crates/wavedude-export/src/sinks/jpeg.rs#L15), [`mp4.rs:29`](../../crates/wavedude-export/src/sinks/mp4.rs#L29), [`mpeg1.rs:188`](../../crates/wavedude-export/src/sinks/mpeg1.rs#L188) (`quality`), [`mpeg1.rs:189`](../../crates/wavedude-export/src/sinks/mpeg1.rs#L189), [`graph.rs:97, 123, 155`](../../crates/wavedude-export/src/graph.rs) (and
the `DECADES`, `MIN_WINDOW` consts), `naming.rs:33`, [`jpeg.rs:17`](../../crates/wavedude-export/src/sinks/jpeg.rs#L17), [`mov.rs:113`](../../crates/wavedude-export/src/sinks/mov.rs#L113); silent substitutions
[`exr.rs:29`](../../crates/wavedude-export/src/sinks/exr.rs#L29), [`sequence.rs:74, 181, 194`](../../crates/wavedude-export/src/sinks/sequence.rs) (plane kind always matches), [`mpeg1.rs:147`](../../crates/wavedude-export/src/sinks/mpeg1.rs#L147) (`AC_CODES` parse
to 0), [`raster.rs:324`](../../crates/wavedude-export/src/raster.rs#L324), [`graph.rs:240-247`](../../crates/wavedude-export/src/graph.rs#L240-L247), [`overlays.rs:406`](../../crates/wavedude-export/src/overlays.rs#L406) (a missing trace draws an empty graph),
`job.rs:40` (a canvas size mismatch silently drops the glowing edges); CLI: `gates.rs:190, 418, 456,
551` (`BOX.nominal()`), `bench.rs:206`, `truth.rs:175`, `export.rs:220`, `compare.rs:184, 219, 235,
241, 293, 304`.

wavedude-app, wavedude-app-state, wavedude-widgets (single findings):
[`transport.rs:182`](../../crates/wavedude-app/src/ui/preview/transport.rs#L182), [`playback.rs:79-81`](../../crates/wavedude-app-state/src/playback.rs#L79-L81), [`status.rs:28`](../../crates/wavedude-app/src/ui/status.rs#L28), [`graph_boxes.rs:47, 56, 75, 81, 83, 91`](../../crates/wavedude-app/src/ui/preview/graph_boxes.rs),
[`properties_forms.rs:59, 122, 136, 290-291, 371`](../../crates/wavedude-app/src/ui/properties_forms.rs), [`properties.rs:90`](../../crates/wavedude-app/src/ui/properties.rs#L90) (a range-key typo gives open
bounds), [`commands.rs:536, 636`](../../crates/wavedude-app-state/src/commands.rs), [`clipboard.rs:166-171, 177-181, 218`](../../crates/wavedude-app-state/src/clipboard.rs) (an undecodable pasted image is
accepted as (0, 0): return an error), [`document.rs:113`](../../crates/wavedude-app-state/src/document.rs#L113), `examples.rs:67-71`, `lib.rs:478, 540`,
[`menus.rs:124, 220`](../../crates/wavedude-app/src/ui/menus.rs), [`export_dialog.rs:146, 176, 288, 462-487`](../../crates/wavedude-app/src/ui/export_dialog.rs), [`dialogs.rs:59`](../../crates/wavedude-app/src/ui/dialogs.rs#L59), `overlay.rs:336-338,
423, 612`, `handles.rs:405-427`, `presenter.rs:305`, `tree.rs:278`, `selection.rs:182, 302-303`,
[`gradient_editor.rs:48`](../../crates/wavedude-widgets/src/gradient_editor.rs#L48), [`message_list.rs:595`](../../crates/wavedude-widgets/src/message_list.rs#L595), `timeline.rs:52, 129`, [`graph_box.rs:110`](../../crates/wavedude-widgets/src/graph_box.rs#L110),
[`menu.rs:347`](../../crates/wavedude-widgets/src/menu.rs#L347), [`theme.rs:228`](../../crates/wavedude-widgets/src/theme.rs#L228), and the `.maybe(id.is_some(), |r| r.test_id(id.unwrap_or_default()))`
pattern in eight widgets.

## Kept

Engine: the GPU to CPU fallback (logged), compact to plain medium tables (table overflow is content),
wgpu's non-zero buffer sizes, WGSL dispatch guards, NaN halves from a diverged field, image red 1..99,
damping edge against the canvas, zero or non-finite gradients and tangents, wavelength None at scale 0,
`tau.max(1)` and `max(1, |W|)` (documented in [physics.md](../physics.md)), the all-mirror sub-step floor, Sobel and
gain saturation, zoom windows from the UI.
Model/io/run: normalisation itself, non-finite to default, legacy and `.wdproj` tolerant readers per
[spec/60](../spec/60-file-formats.md), unknown scheme names render grey, the clock before the epoch, store allocation failures and
recycled buffers, `Drop` joins, the memory warning's `saturating_sub`.
Export/CLI: poisoned `Mutex<()>` recovery, `available_parallelism`, dead workers, `NotFound` on partial
cleanup, `moov` overflow append, NAL types, whitespace glyphs, command-line overrides, division by
constant signals in diagnostics, geometry floors in the gate generator that depend on the solver.
App: platform dirs (once unified), the clock, empty log lines, user-picked paths, `GateDesign` failing on
degenerate input, store read errors, superseded scratch ids, settings file values (better clamped once in
`Settings::normalize`, which lacks `export_filter_index` and the dock layout), user text in number boxes,
the memory warning replaced while open, log-axis `y_max`, first-frame layout zeros, the Freya modifier
root context quirk, `Notice` and quick-xml catch-alls.

## Outcome (same day)

Everything above is done, except the items judged legitimate on a second look:

- The store's spill moves its first frame down instead of starting a second file; `blocks[frame].set`
  asserts. Typed `MenuAction` in the app, forms panic on a mistyped path, `to_json` expects, a key missing
  from English panics, one `AppDirs` for the whole app.
- The preview divisor stays a `u32` validated once at the settings boundary (`PreviewQuality::legal_divisor`,
  snapping 3 to 2) with debug asserts in `RunSpec`; an enum would have spread into the menus and the CLI.
- `BoxSize::nominal` stays an `Option`: Custom has no size without its probe.
- Kept as reachable after all: the `.get(..)` lookups during a held drag (an undo can remove the item or
  node under it), `jitter`'s zero early return (it decides which random numbers are drawn), the jittered
  off width's floor, the gradient editor's empty default, `GateDesign` failing on degenerate input.
- The gate generator normalises its gate before designing it, since the design reads the fields as stored.
- A pasted image that does not decode is now a `FragmentError`; `.wdr` format 0 is refused; a scheme folder
  that cannot be read reports its error; a run driver's panic is a `Failed` state, not `Cancelled`; an
  export scene of another size than the canvas is an error, not silently dropped edges.
- `wavedude-app-state` now depends on `wavedude-export` without its encoders, for the export filter count its
  settings clamp needs.

Workspace fmt, clippy with `-D warnings` and all tests pass.
