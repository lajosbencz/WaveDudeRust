# WaveDudeRust rewrite specifications

This project is a complete rewrite of [WaveDudeNext](https://github.com/graealex/WaveDudeNext) in Rust as a cross-platform (Linux, macOS, Windows) app and library.

## Goal

The end goal is for the original authors to adopt, so our rewrite must be easy to read, build and maintain: we must follow idiomatic rules and best practices.

## Credit chain

- Jeroen Vleggaar (Wave Dude 2D)
- Alexander Graef (WaveDude Next)

## License

> GPL-3.0-or-later

We must have a clear license chain for all our components.
Use modern toolings for lineage tracing.

## Ground rules

- Original repo is read-only, run it with wine
- Re-implement from scratch: only read the original to know what it does (features, formats, protocols, behaviour, docs), not how it does it. Do not port the code, quirks or bugs
- Our physics engine must be correct per the literature, not per the original. Bit-for-bit match of output is not a goal and we shouldn't calibrate for it. Any deliberate inaccuracy must be documented, and configurable if sensible. Tolerances must be stated and at least as good as the original
- Where the literature has a better technique than the original, document the decision and use the solution from the literature (absorbing boundaries, stencils, time integration, material smoothing)
- Idiomatic Rust, composition over inheritance, DRY, no silent fallbacks, types carry invariant
- Terse comments, only for facts the code cannot say, no need for historic lore, always reflect current state unless in a dedicated (change)log

## Extra features

- Make sure we can zoom in with mousewheel, but do not allow zooming out. Middle mouse pans.

## Stack

- One Cargo workspace: core, export, cli, widgets, app state, app
- UI: Freya 0.5
- Render: wgpu + CPU fallback
- Project files: support the WaveDudeNext formats; `.wdproj` is the one project format
- Export: PNG/JPEG/EXR sequences, MPEG-1, AVI, QuickTime, H.264 MP4 via openh264

## Plan

1. Design: Map the original features, model, file formats and guarantees into `docs/spec` without reading its implementation
2. Build: Core library create from that spec, with unit-level analytic physics tests and snapshot tests (dispersion, reflection, absorption, refraction, diffusion, energy conservation)
3. Compare: The original C# program (headless harness) to find mistakes on either side. Fix ours, report theirs, never calibrate
4. Design: Map the WPF UI layout and behaviour into `docs/ui`
5. Build: Missing widgets for which Freya doesn't give a ready solution
6. Build: Desktop and terminal applications

Steps 1-4 may run in parallel.
Decisions go to `docs/decisions` as ADRs, measurements to `docs/measurements.md`.
If something is ambiguous/missing, ask the user instead of assuming.

## Finish line

- `cargo fmt --check` green
- `cargo clippy --workspace --all-targets -D warnings` green
- `cargo test --workspace` green
- GPU tests skip without an adapter
- Original example projects open and run, the photonic logic gates from Huygens Optics' video are reproduced in `examples/gates` with measured truth tables.
- `cargo build --release -p wavedude-cli -p wavedude-app` produces hand-checked app

