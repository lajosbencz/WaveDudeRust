# 0007 Workspace layout

Status: accepted, 2026-11-05

One repository, one Cargo workspace, edition 2024:

- `wave-core`: model, scene painter, engines, frame store, probes, renderer.
- `wave-export`: PNG/JPEG sequences, MPEG-1, AVI, MOV, MP4, OpenEXR, CSV.
- `wave-widgets`: Freya widgets WaveDude needs.
- `wave-app-state`: the application state without a UI toolkit (document, history, selection, commands, live runs,
  settings, strings, log); no Freya, no rfd, testable headless.
- `wave-app`: the desktop application, the Freya views over `wave-app-state`.
- `wave-cli`: headless checks, benches, scripted exports (CI without a display).

Docs under `docs/`: `spec/` (core), `ui/` (UI behaviour), `decisions/`.
