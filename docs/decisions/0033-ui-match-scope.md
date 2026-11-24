# 0033: What of Next's UI is matched

Status: accepted, 2026-11-24 (the user's decision).

## Decision

- Matched exactly: what a simulation computes, what an edit changes in the project, what a command or a control
  does (menus, shortcuts, tree, preview handles, transport, export, settings that change a run or the preview).
- Matched in general: layout, look, widget details. Pixel sizes and paddings, tooltip wording, access letters,
  automation ids, glyph codepoints, timings and tab-order indices follow Slint's and our own defaults.
- Dropped: the dock system (dragging panes, tab groups, the "Window position" menu, a saved layout; the layout is
  fixed and View > Panes shows or hides a pane), the updater, UI zoom, touch-first behaviour.
- Kept: the four languages with live switching.

## Consequences

- [docs/parity.md](../parity.md) rows of the dropped and the general kind read `divergence` with this decision.
