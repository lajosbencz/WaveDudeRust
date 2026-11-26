# 0035: The browser's GPU engine runs on the page, the run worker runs the CPU engine

Status: accepted, 2026-11-26 (the user's decision), for the Slint app. Supersedes the GPU part of
[0024](0024-browser-run-in-a-web-worker.md); its run worker is kept for the CPU engine.

## Context

The Slint app ported 0024's run worker with its own WebGPU device, its frames posted to the page's mirror store and
uploaded again to the page's device to be coloured. Against the run pumped on the page's thread on the page's device
(c9b1b5c), measured on Chromium on Xvfb with native Vulkan, `lens.wdproj` at 1200 steps and 600 frames, 3 passes each
([measurements.md](../measurements.md), The browser's live run in a worker):

| | Pumped on the page | Run worker |
|---|---|---|
| Run | 0.6 s | 0.8 s |
| Page's main-thread time during the run | 0.37 to 0.39 s | 0.57 to 0.60 s |
| Longest frame gap | 17 ms (one 33 ms) | 17 ms |

The GPU engine leaves the page's thread once a submission is made; its frames stay on the device the pictures are
coloured on. In the worker each frame is read back, posted, copied into the mirror and uploaded again, which costs the
page more than the steps it no longer takes.

## Decision

- With a WebGPU device on the page and Use the GPU on, the live run is a `RunWorker` pumped on the page's thread
  (`Pump::Waiting` with a waker that wakes the window, the queue kicked while completions are owed, decision 0026), its
  engines on the page's device, its frames drawn from the device ([`web.rs`](../../frontends/waveduderust_slint/src/web.rs),
  `gpu_run`).
- Otherwise (no device on the page, or Use the GPU off) the run is 0024's run worker with the CPU engine
  (`waveduderust_remote::Remote`, `wavedude-run-worker`), started the first time it is needed (`cpu_run`).
- Use the GPU swaps the run: the old one's frames go, as Discard frames does (the page's GPU run is dropped, the
  worker's kept idle), and the new one makes the position's frame ([`live.rs`](../../frontends/waveduderust_slint/src/live.rs), `Live::set_gpu`).

## Consequences

- The CPU engine, which takes the page's thread for its steps, stays off it; the GPU engine's run shares the page's
  thread in 8 ms slices.
- Both runs serve `live.rs` through one trait, `Run`; the window's poll and drawing are the same for both.
- The worker builds only the CPU engine and requests no WebGPU device; the protocol carries no engine choice.
- One worker serves the page's life: swapped out, its run gets an empty goal and lets go of its frames, and the next
  time Use the GPU is off it is taken up again.
