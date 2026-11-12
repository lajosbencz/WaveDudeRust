# 0007 Workspace layout

Status: accepted, 2026-11-05

One repository, one Cargo workspace, edition 2024:

- `wavedude-core`: model, scene painter, engines, frame store, probes, renderer.
- `wavedude-model-derive`: the `Fields` and `Word` derives of the model types (decision 0013).
- `wavedude-export`: PNG/JPEG sequences, MPEG-1, AVI, MOV, MP4, OpenEXR, CSV.
- `wavedude-widgets`: Freya widgets WaveDude needs.
- `wavedude-app-state`: the application state without a UI toolkit (document, history, selection, commands, live runs,
  settings, strings, log); no Freya, no rfd, testable headless.
- `wavedude-app`: the desktop application, the Freya views over `wavedude-app-state`.
- `wavedude-cli`: headless checks, benches, scripted exports (CI without a display).
- `wavedude-testing`: finding and clicking in the headless Freya tests of widgets and app.
- `wavedude-examples`: the example projects of the Examples dialog and the web demo (decision 0017).
- `wavedude-wasm`: the engine for the browser (decision 0017).

Docs under [`docs/`](../): [`spec/`](../spec/) (core), `ui/` (UI behaviour), [`decisions/`](./).
