# 0036: Docking returns

Status: accepted, 2026-11-26 (the user's decision). Supersedes the dock point of [0033](0033-ui-match-scope.md)'s
"Dropped"; replaces the four side lists of [0008](0008-ui-behaviour-answers.md) item 11.

## Context

0033 dropped Next's dock system: the panes sat in a fixed layout and View > Panes showed or hid them. The user wants
the panes docked again, as Next docks them, built the way Slint builds things rather than after Next's WPF code.

## Decision

- The panes dock as Next's do ([docs/ui/00](../ui/00-overview.md), Panes and default arrangement): a split tree of
  tab groups with weighted splits around the fixed preview, splitters, a drag of a title bar (the group) or a tab (the
  pane) onto an outer edge, beside a group or into it, the title bar's "Window position" menu (Move to Left, Right,
  Top, Bottom, Close), and the arrangement saved and restored.
- The port rules of docs/ui/00 stand: the pointer picks the target without indicators, and the arrangement is saved
  as it changes, once the pointer lets go, on both targets.
- The dock is the reusable crate [`slint_dock`](../../crates/slint_dock), to become a standalone library: a headless
  Rust `Dock` (the tree, its geometry, drop targets, moves, show and close, an `Arrangement` serde-able behind the
  `serde` feature) and a Slint library `ui/dock.slint` (`DockArea`) that draws the chrome from its models. It holds no
  WaveDude wording and depends on no WaveDude crate; the app passes its texts and colours in.
- The app ([`docking.rs`](../../frontends/waveduderust_slint/src/docking.rs)) registers the panes, places each pane's
  content at its computed rectangle, and keeps the arrangement's JSON in the settings' `Layout`, a string the platform
  crate does not read.

## Consequences

- Slint cannot instantiate a pane's content from a model, so every pane stays an element of the app's window, placed
  over its group; the library draws title bars, tab strips, splitters and the drag's preview.
- Slint draws its context menus without mnemonics: the "Window position" menu has no access keys, as the app's other
  context menus ([divergences.md](../divergences.md), Row menu).
- A saved layout naming panes the app no longer registers loses them; panes it lacks are hidden until shown. An
  unreadable `Layout` is logged and the default arrangement used.
