# 0024: The browser runs the simulation in a Web Worker

Status: accepted, 2026-11-14 (the user's decision); losses and versions added 2026-11-14. Amends [0015](0015-core-builds-for-wasm.md),
[0016](0016-gpu-engine-in-the-browser.md), [0019](0019-egui-and-the-platform-layer.md) and
[0020](0020-browser-frame-spill-in-opfs.md): the run leaves the page's thread.

## Context

The browser pumped the run from the frame loop, up to 8 ms of every frame, before playback and drawing: playback
stuttered while a run went on, even past the playhead. A capture's readback resolves only after control returns to
the event loop, so the run waited a frame per capture ring, and `Device::poll` does nothing on WebGPU, so nothing
paced its submissions. Without cross-origin isolation, which GitHub Pages cannot set (0017), there are no wasm threads.

A spike (Chromium, WebGPU) measured in a module worker: `GpuContext::create` and the GPU engine work, the logging
works, a transferred 4 MiB buffer posts in 0.02 ms and is detached, the page copies it into wasm in 0.2 ms. The
worker's binary is 2.1 MB (0.6 MB gzipped) against the app's 21 MB.

## Decision

- A binary crate of its own, `wavedude-run-worker`, runs the engine, the `RunWorker`, the scene cache and the
  probes in a module worker with a WebGPU device of its own. [`tools/build-web.sh`](../../tools/build-web.sh) builds it beside the
  app; `run-worker.js` loads it.
- The worker's frame store keeps no frames in memory: its spill backend posts each frame's planes as one transferred
  `ArrayBuffer`. The page keeps the mirror store, which the preview, presenter and status read as before, with the
  memory budget and OPFS spill of 0020. A frame is acknowledged once the mirror holds it, so the worker stays at most
  the spill's writes in flight ahead.
- Messages: postcard over a hand-written channel on `postMessage` with a transfer list (gloo-worker posts without
  one, copying every frame). The project goes as serde data behind the core's `serde` feature, not as `.wdproj`
  text, which rounds decimals. A `Port` trait has a loopback, so the protocol is tested natively.
- The worker waits on readiness (a capture, a spill write, the engine, a goal) instead of a time slice, and gates
  its submissions on `on_submitted_work_done`.
- The engine reports what was built and why a GPU fell back to the CPU; the status bar names the engine built and
  the Messages show the reason. A worker that fails to load or crashes is an error shown to the user; the run never
  moves to the page's thread.
- The preview colours stored frames on the page's GPU, as there are no engine device frames on the page.
- Desktop keeps its run thread; nothing on desktop changes.

## Consequences

- Playback and the UI no longer share a thread with the run.
- Each frame costs two copies: worker memory to the buffer, buffer to the mirror.
- Scenes are composed twice in the browser: in the worker for runs, on the page for the scene picture.
- The core's model derives serde behind a feature; postcard is a new dependency.
- A WebGPU adapter has no name; the engine label says WebGPU.

## Alternatives rejected

- Wasm threads with `SharedArrayBuffer`: no cross-origin isolation on GitHub Pages.
- The app's own wasm as the worker: 21 MB loaded twice, egui and the examples instantiated for nothing.
- A fallback to the page's thread: a second run path for a deployment error; the error is shown instead.
- The spill in the worker through `FileSystemSyncAccessHandle`: the playhead's reads would cross agents and need
  prefetching; deferred until scrubbing measures slow.

## Losses and versions (added 2026-11-14)

- The worker's GPU device lost, the engine built next requests a new one, as natively; a failed request is kept and
  the CPU engine runs with it as the fallback's reason. No GPU engine is built on a lost device.
- `ToWorker::Init` and `ToPage::Ready` carry the protocol's `VERSION`, bumped when a message changes; they stay the
  first variants with the version first, so either end reads the other's version. A page of another version is not
  listened to; the page reports a worker of another version.
- A worker that does not start (not made, not loaded, silent), stops once ready, or is of another version fails
  every run until the page is reloaded, the user told which with a localised message; Run tells it again. The page
  posts nothing more to it.
- The traces the worker posts start over by the run's own count of their starts, not by the published status, which
  can lag behind a goal adopted.
