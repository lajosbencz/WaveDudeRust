# 0037: The Cupertino widget style

Status: accepted, 2026-11-26 (the user's decision).

## Context

The app built with Slint's default Fluent style. Slint draws the menu bar itself on Linux and the web, with the
style's metrics: Fluent's is at least 40 px high with 10 px between titles. Slint 1.19 has no compact or density
setting, no spacing property on `MenuBar` and no app-supplied styles; the style is the only lever.

## Decision

- `frontends/waveduderust_slint/build.rs` compiles the UI with the `cupertino` style: a 24 px menu bar, and its
  look for every std widget (buttons, fields, spin and combo boxes, scrollbars, tabs, check boxes).
- The app's own colours (`ui/theme.slint`, Next's) stay; the std widgets follow the style's palette, light or dark.

## Consequences

- Layout matches Next loosely, as [0033](0033-ui-match-scope.md) allows; the widgets no longer look like Windows'.
- Windows and macOS keep their native menu bars (`muda`); the style changes only the widgets there.
