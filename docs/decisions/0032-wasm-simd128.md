# 0032: wasm32 builds with simd128

Status: accepted, 2026-11-25.

## Context

In the browser the run worker runs the CPU engine on one thread when it gets no WebGPU adapter. Without 128-bit SIMD
the engine's row loops run scalar there: the run worker had no SIMD instruction.

## Decision

- [`.cargo/config.toml`](../../.cargo/config.toml) builds every wasm32 crate with `-C target-feature=+simd128`.
- No scalar fallback build: one wasm per crate, as before.

## Why

- The CPU engine in wasm at 10a6a02, steps a second without / with simd128: Example01 61 / 97, Example09 101 / 218
  ([measurements.md](../measurements.md), The CPU engine in wasm, with and without simd128).
- One wasm per crate keeps the web build and its loader as they are.

## Consequences

- The app needs WebAssembly SIMD: Chrome 91, Firefox 89, Safari 16.4. Below that the modules fail to compile and the
  app does not start, where before only the APIs with a fallback were missing (no WebGPU: the CPU engine; no origin
  private file system: no spill, [0020](0020-browser-frame-spill-in-opfs.md)).
- Native builds and crates that depend on ours are unaffected: `.cargo/config.toml` applies to this workspace's
  wasm32 builds only.
- wasm-opt reads the modules' target features section, so it keeps SIMD without an `--enable-simd` flag
  ([0029](0029-wasm-opt-in-the-web-build.md)).
