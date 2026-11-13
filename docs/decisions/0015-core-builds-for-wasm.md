# 0015: The core builds for wasm32 without its runs and GPU engine

Status: accepted, 2026-11-12; amended by [0016](0016-gpu-engine-in-the-browser.md), [0019](0019-egui-and-the-platform-layer.md)
(the live run of `run` builds for wasm32, pumped from the frame loop) and [0020](0020-browser-frame-spill-in-opfs.md).

## Context

A browser build of the simulation (a demo in the docs, other people's web tools) needs the core on
`wasm32-unknown-unknown`, where `std::thread` and `std::time::Instant` panic, there is no disk or machine memory
to ask about, and the GPU cannot be waited on. The desktop programs need all four. The browser-safe part already
existed apart from the rest: reading a project from text (`io::wdproj::read_str`), composing the scene, the CPU
engine stepped by hand, probes and the renderer. Only `run` (the threaded runner, the frame store with its disk
spill, the memory checks; its positioned writes exist for Unix and Windows only, so it does not compile there) and
the `gpu` feature (`pollster` blocks on readbacks) use what a browser lacks, plus one `Instant` in the scene painter.

## Decision

- `run` is a default feature of `wavedude-core`, as `gpu` is; `sysinfo` and `tempfile` are its optional
  dependencies. Every crate that uses `wavedude_core::run` gets it by default features or names it (`wavedude-export`).
- Code outside `run` and `gpu` uses nothing a browser lacks; time is `web_time::Instant`, which is `std`'s on
  native targets.
- CI checks `cargo check -p wavedude-core --no-default-features --target wasm32-unknown-unknown` (job `wasm32`).

## Consequences

- A wasm package is a new crate over the browser-safe part (wasm-bindgen), with no change to the core's design.
- On wasm the CPU engine's rayon loops run on one thread unless that package opts into wasm threads
  (`wasm-bindgen-rayon`, cross-origin isolation); the GPU engine needs async readbacks first.
- The check proves the build, not the behaviour: `usize` is 32 bits there, and nothing runs the tests on wasm.

## Alternatives rejected

- A wasm package now: nobody needs it yet, and it is one more target to keep working.
- Making `run` work in the browser (web workers, an in-memory store): a second runtime model for a target with no user.
