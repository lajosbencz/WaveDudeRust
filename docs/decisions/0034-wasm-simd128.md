# 0034: The run worker ships a 128-bit SIMD build beside the scalar one

Status: accepted, 2026-11-25, for the classic frontend (numbered 0032 on master); applied on the Slint app
(decision 0032) 2026-11-26, whose run worker runs the CPU engine ([0035](0035-browser-run-on-the-page-gpu.md)).

## Context

In the browser the run worker runs the CPU engine on one thread when it gets no WebGPU adapter. Built for plain wasm32
its row loops run scalar. A module holding any SIMD instruction fails to compile where WebAssembly SIMD is missing
(before Chrome 91, Firefox 89, Safari 16.4), so one module cannot carry both.

## Decision

- [`tools/build-web.sh`](../../tools/build-web.sh) builds `wavedude-run-worker` a second time with
  `-C target-feature=+simd128` into `pkg-simd/`, in its own target directory, alongside the other builds.
- [`run-worker.js`](../../frontends/waveduderust_slint/web/run-worker.js) validates a SIMD probe module (wasm-feature-detect's
  bytes, inlined) and loads `pkg-simd/` where it validates, else `pkg/`.
- The app and the spill worker stay scalar: the CPU engine runs in the run worker, and the spill worker does no float
  work.

## Why

- The CPU engine in wasm, 10a6a02: Example01 61 / 97 steps a second, Example09 101 / 218 without / with simd128
  ([measurements.md](../measurements.md), The CPU engine in wasm, with and without simd128).
- simd128 for every module would leave the app unable to start where SIMD is missing, while the app runs there today
  (no WebGPU: the CPU engine; no origin private file system: no spill, [0020](0020-browser-frame-spill-in-opfs.md)).

## Consequences

- A second release build of the run worker's crates per web build, in parallel with the others, in `target/simd`.
- The site holds both run workers; a page downloads one.
- wasm-opt reads the modules' target features section, so it keeps SIMD without an `--enable-simd` flag
  ([0029](0029-wasm-opt-in-the-web-build.md)).
- No browser in CI lacks SIMD, so the scalar run worker is loaded by none of the checks; its code is the same crate.
