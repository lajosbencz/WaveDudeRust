# 0016: The GPU engine builds for the browser, and the spec and tiers leave `run`

Status: accepted, 2026-11-12. Amends [0015](0015-core-builds-for-wasm.md): the `gpu` feature builds for wasm32 too.
No WebAssembly package exists yet; this makes the engine ready for one.

## Context

0015 kept `gpu` out of the browser build because `pollster` blocks on the adapter, the device, the pipeline error scope and
every readback. In a browser nothing may block: `pollster::block_on` hangs the page, `Device::poll(Wait)` does
nothing on WebGPU, and a `map_async` callback runs only after the code returns to the event loop. wgpu's types are not
`Send` there either, while `WaveEngine` required `Send` for the run threads. The run spec (`RunSpec`, `Schedule`,
`RunRequest`) and the preview tiers (`PreviewQuality`, `PreviewDefaults`) lived in `run` though they use no thread,
disk or machine query, and the per-layer colouring of a picture lived in the application.

## Decision

- Setup is async: `GpuContext::create`, `GpuEngine::create` and `engine::build_on` (the GPU else the CPU, with its warning)
  await the adapter, the device and the pipeline error scope. On native targets `context()` (the process's cached
  device), `GpuEngine::new`, `GpuEngine::with_layout` and `engine::build` block on them with `pollster`, which is a
  dependency of non-wasm targets only. On wasm the caller awaits `create` and owns the `Arc<GpuContext>`.
- A readback's map callback sets a `sync::Notify`, which wakes a waiting future. `PendingCapture::ready()` resolves when
  the copy is mapped, `PendingProbes::ready()` when every copied batch of the probe readings it covers is, both at once
  for the CPU engine. `drain_probes` takes the `PendingProbes` token as `finish_capture` takes its capture, and hands
  out the readings queued before the token, no later ones. On wasm32 the GPU engine supports these paths only:
  `finish_capture` or `drain_probes` before `ready()` resolves returns `GpuError::NotReady` (a contract violation, which
  leaves the engine failed like any GPU error; the type cannot rule it out, as natively nothing awaits `ready()`), and
  `sample_probes` and `prepare_frame`, which have no future to await, can only return `NotReady` there. Natively every
  waiting call blocks, and `ready()` resolves once something polls the device (`finish_capture` and `drain_probes` do).
  The in-flight throttle in `submit` stays; it waits natively and is a no-op on WebGPU.
- `WaveEngine: MaybeSend`, where `MaybeSend` is `Send` on native targets and empty on wasm32. Engines cross threads only
  where threads exist, and `run` (native only) still gets `Send` engines.
- `RunSpec`, `Schedule`, `RunRequest`, `SceneCache` are in `spec`, `PreviewQuality`, `PreviewTier`, `PreviewDefaults` and
  `MachineFacts` in `quality`; `StoreLayout::of(&RunSpec)` belongs to the store, `run::machine_facts()` reads the machine.
  Nothing re-exports them from `run`.
- `render::layer` holds what colours a layer's plane (`LayerColours::of`, `paint`, `scale_nearest`), shared by the
  application and a browser package.
- `Project::new_random_seed` takes the time from `web_time`; the process id only exists off wasm32.
- CI runs clippy with `-D warnings` for `wavedude-core --no-default-features` with and without `--features gpu` on `wasm32-unknown-unknown`.

## Consequences

- A browser package can build an engine with `build_on(scene, options, Some(ctx))`, step it, and await `ready()` between
  `begin_capture` and `finish_capture`; it never blocks and never spins.
- Native behaviour is unchanged: the same blocking calls, the same shared device.
- A native caller of the async functions must poll the device for a mapping to complete; `finish_capture` does.
- On wasm32 `sample_probes` and `prepare_frame` of the GPU engine are unavailable; queued probe readings are drained
  with their `pending_probes()` token once its `ready()` resolved.
- The check proves the build, not the behaviour in a browser; nothing runs the engine on wasm in CI.

## Alternatives rejected

- wgpu's `fragile-send-sync-non-atomic-wasm` feature: it implements `Send` and `Sync` for wgpu's types on wasm without
  their being thread-safe, so the code would claim what is false. The conditional bound states the truth.
- Keeping blocking setup and running the engine in a worker: a second runtime model, and the readbacks still need the
  event loop.
- A channel crate for the readback wake-up: a value cell and a waker are a few lines, and no crate in the build needs a channel for `gpu`.
- Re-exports from `run` for the moved items: one path per item, and every caller updated.
