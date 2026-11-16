# 0028: No manual in the app; Help opens the project page

Status: accepted, 2026-11-16. Amends [0006](0006-testing.md) (the figures' rendering),
[0008](0008-ui-behaviour-answers.md) (15, the figures) and [0019](0019-egui-and-the-platform-layer.md)
(egui_commonmark for the manual, the figures on lavapipe).

## Context

The app carried WaveDude Next's manual, rewritten for this program: `manual.md` (90 KB) with 42 PNG figures
(2.77 MB) cropped from the window by a figure generator test on Mesa's lavapipe, shown in a window of its own
(Help > Manual, F1). The browser build's wasm was 22.0 MB raw, 7.5 MB with gzip; the figures barely compress, so
they were about a third of the download. Every UI change also asked for its figures to be taken again and its text
to follow.

## Decision

- The manual, its figures, its window and the figure generator are removed; the manual's text is kept nowhere in
  this repository. WaveDude Next's `MANUAL.md` stays the reference where one is needed.
- Help > Project page on GitHub (F1) opens <https://github.com/lajosbencz/WaveDudeRust> (the package's
  `repository`): in the system browser natively, in a new tab in the browser build.

## Why

Release size: the figures were the largest single asset of the browser build. The project page documents the
program next to its releases and issues and needs no rebuild to change.

## Consequences

- The browser wasm went from 22.0 MB raw / 7.5 MB gzip to 19.0 MB / 5.7 MB (`tools/build-web.sh`, `gzip -9`).
- Links open through `platform::open` (`wavedude-app-state`): natively the system's opener (`xdg-open`, `open`,
  `explorer`), in the browser a new tab; no new dependency.
- egui_commonmark stays for the About dialog's license text; egui_extras' image loader for the About icon.
- The docs no longer ask for a manual update with a user-visible change.
