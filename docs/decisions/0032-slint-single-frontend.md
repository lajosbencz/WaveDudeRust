# 0032: One frontend on Slint, rebuilt from the core

Status: accepted, 2026-11-24 (the user's decision). Supersedes [0019](0019-egui-and-the-platform-layer.md) and the
frontend and app crates of [0031](0031-modular-core-and-frontends.md).

## Context

- WaveDude Next's maintainer found the egui UI unmaintainable by humans and suggested Slint
  ([wavedudenext/WaveDudeNext#7](https://github.com/wavedudenext/WaveDudeNext/issues/7)).
- A spike on Slint 1.19 (`unstable-wgpu-30`) shared one wgpu device between the engine and the renderer and showed
  the field texture without CPU readback, natively and in the browser on WebGPU, live during a run. Release binary
  24.5 MB against the Material frontend's 39.1 MB. `.slint` files reload at runtime with `slint/live-preview`.

## Decision

- One frontend, `frontends/waveduderust_slint` (binary `waveduderust`), native and browser. No parallel frontends.
- Kept: `waveduderust`, `waveduderust_macros`, `waveduderust_cli`. The CLI's host services (settings and their
  store, folders, the OpenH264 download, user colour schemes) move to `waveduderust_platform`, shared with the app.
- Removed: `waveduderust_app`, `waveduderust_egui_platform`, `waveduderust_egui_preview`, `waveduderust_examples`,
  `waveduderust_icon_subset`, `waveduderust_workers`, the classic and Material frontends. Their code is reference at
  tag `v0.4.0`; [docs/rebuild.md](../rebuild.md) maps it.
- The UI is declared in `.slint` files; Rust binds callbacks, properties and models. Look, layout and shortcuts follow
  `docs/ui/`; documented divergences stand.
- `just hot` runs the app with Slint's live preview.
