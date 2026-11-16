# 0030: Undo keeps a user-set number of steps

Status: accepted, 2026-11-16. Amends [`docs/ui/60-live-editing-and-runs.md`](../ui/60-live-editing-and-runs.md),
Undo and redo (unlimited undo).

## Context

The history kept every step of a session: one clone of the project per step, the scene image's bytes shared between
the clones. A clone holds the model without the image, of the order of its `.wdproj` text (the examples: 0.7 to
26 KB), so a long session grows without bound, by its model size per step.

## Decision

- The setting `UndoSteps` (Settings > General, "Undo steps", a number box) caps the steps Undo can go back: default
  1000, 10..100000; a stored value out of range clamps with a warning in the log. No settings version bump: a missing
  key reads as the default.
- Recording a step beyond the cap drops the oldest steps. The current step is the newest then (a new step ends the
  redo branch first), so it is never dropped.
- A lower cap takes effect as the next step is recorded, not at once: the number box applies while it is scrubbed or
  stepped, and a passing low value would otherwise drop steps for good.
- The project is clean only while the current step is the saved one; once the saved step is dropped, no undo makes it
  clean, a save does.

## Consequences

- At the default and the examples' sizes the history stays under some tens of MB.
- Undo after 1000 steps stops at the oldest kept step.
