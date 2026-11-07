# 0006 Testing strategy

Status: accepted, 2026-11-05

- Unit tests in every crate; analytic physics tests in wavedude-core (0003).
- Image snapshot tests (`insta`) for the painter, renderer and export frames.
- Golden fixtures generated from WaveDude Next (exports of the examples) are
  compared with tolerances, never bit for bit.
- `proptest` for ranges and normalisation.
- No end-to-end UI tests; the app state is a headless, testable core with
  thin Freya views.
