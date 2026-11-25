# 0029: wasm-opt in the web build

Status: accepted, 2026-11-16.

## Context

`tools/build-web.sh` shipped what rustc and wasm-bindgen wrote. After the manual's removal
([0028](0028-no-manual-in-the-app.md)) the app's wasm was 19,040,796 bytes raw, 5,697,106 with `gzip -9`; the run
worker's 2,104,574 / 595,929 and the spill worker's 314,866 / 89,976. The download is the browser build's main cost.

## Decision

- The script runs binaryen's `wasm-opt -O3 --debuginfo` in place over every `pkg/*_bg.wasm` after wasm-bindgen: the
  app and both workers. It fails when `wasm-opt` is missing; `WAVEDUDE_NO_WASM_OPT=1` skips it for quick local builds.
- CI (the wasm32 job of `ci.yml`, the web job of `package.yml`) installs binaryen version_133 from its release tarball
  against a pinned digest; `pins.yml` reports a newer release.
- No `--enable-*` flags: binaryen 133 enables by default the features rustc's wasm32 output uses (bulk memory, sign
  extension, non-trapping float to int, mutable globals); it takes SIMD ([0032](0032-wasm-simd128.md)) from the
  modules' target features section.

## Why

- `-O3`, not `-Oz`/`-Os`: the engine and the exports are CPU-bound in the browser, and the size options trade speed
  for bytes. `-O3` still removes dead code and merges duplicates.
- `--debuginfo` keeps the name section, which browser panic stacks read; wasm-opt strips it otherwise.

## Consequences

- Measured with `tools/build-web.sh` (wasm-opt 133), raw / `gzip -9`: app 17,121,306 / 5,537,869 (-10% / -2.8%), run
  worker 1,819,916 / 569,118, spill worker 259,375 / 83,214. The gzip gain is small; the raw size, so parse and
  compile work, drops more.
- wasm-opt takes about 9 s for the app, 1.6 s for the run worker and 0.2 s for the spill worker.
- Building the web app needs binaryen (the `binaryen` package, or a release tarball) on the `PATH`.
