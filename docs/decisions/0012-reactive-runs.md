# 0012: The live run converges on a goal; no scratch frames, no timers

Date: 2026-11-09. Status: accepted.

## Context

The live preview had two runners mirroring WaveDude Next: a `ScratchRunner` computing "frame k as the project
is now" on a second engine, and the `SimulationRun` storing every frame, with the controller pacing them by a
120 ms debounce, 1.5 s of quiet before the re-run, and pausing the run while a scratch frame computed. The
abandon-on-newer-request rule starved drags of heavy frames; the debounce and quiet were constants nobody could
derive; the two engines and the coordination were half of [`live.rs`](../../crates/wavedude-app-state/src/live.rs). WaveDude Next dropped its timers on
2026-11-09 and kept the two runners.

## Decision

One worker, one engine, one store. The controller publishes a goal (a run request: the project copy, quality
and tracking the worker builds the spec from; position; `Wanted::Position` or `Wanted::All`; paused; a fit
request) and reads back status, frames and traces. Composing the scene for a spec happens on the worker, so a
part drag costs the UI thread a project copy per pointer move and nothing more. The worker takes targets by
state: the frame looked at first, then the rest when every frame is wanted, the probes' tail before any frame
before the position still missing. With every frame wanted the pass to the frame looked at captures the frames
it passes, so one pass from step 0 makes the whole run in timeline order after the frame looked at; with the
position alone wanted the seek captures that frame alone. Measured, the captures on the way cost a drag nothing
([docs/measurements.md](../measurements.md), The live run).

Rules settled here:
- Work is done for the goal's spec alone: a unit for another spec is left at the next check, 16 steps on. An
  edit means one run; a drag computes nothing to completion until the hand rests for a frame time, and the
  pictures hold the last state made meanwhile. (Finishing the frame in flight was tried: with a frame slower
  than the drag it showed a result of a state already gone, then ran again.) No debounce, no quiet period.
- A spec with other frames starts a generation in the same store (buffers kept); the frame written last stays
  readable so a drag shows each frame made, and its traces stay with it so the graphs hold with the picture
  instead of drawing the new recording as it comes. A spec with other probe measurements keeps the frames and records
  the traces anew; names and graph boxes are not measurements.
- "Re-run automatically" means every frame is wanted after an edit; off means the position's frame alone. Run,
  Play and an export want every frame. Stop wants the position's frame alone. Turning the setting on asks for
  nothing by itself. The memory warning asks before every frame is wanted, as before.
- Undo steps keep their quiet-time rule (a held key is one step); the Next input-boundary rule was not adopted.
- A paused run still makes the frame looked at. The memory warning asks about the project as it is when
  answered, not when asked. A probe's name is a display change. Showing the intensity is a simulation change
  (the run tracks it). A device switch rebuilds the engine without losing frames or traces. An export holds
  the project: edits wait for it, New/Open/Close are refused.

## Consequences

- `run/scratch.rs` is gone; [`run/worker.rs`](../../crates/wavedude-core/src/run/worker.rs) replaces it; `SimulationRun` stays the batch run for exports and
  the CLI. The store writes by index, tracks presence and clears in place.
- A probe edit with every frame wanted re-records the whole run (stepping without captures); before, only the
  frame shown was refreshed and the other frames' graphs were stale.
- The status bar shows the current pass's steps. There is no "edited since": the frames of an edited project
  are gone, not stale.
- Measured ([docs/measurements.md](../measurements.md), The live run): Example16 at half resolution on the GPU, 120 edits at 32 a
  second: a 25 ms frame per edit, the final frame under a millisecond after the last edit, the same with the
  frames before the position captured on the way; a frame slower than the drag lands once the hand rests.
