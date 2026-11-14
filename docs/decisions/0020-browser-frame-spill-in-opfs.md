# 0020: The browser spills frames past memory to the origin private file system

Status: accepted, 2026-11-13; writes in flight changed 2026-11-14. Amends [0015](0015-core-builds-for-wasm.md): on
wasm32 a store keeps the frames past its memory budget, as natively, instead of refusing them.

## Context

In the browser a store held its frames in memory alone, up to the RAM cache size's share of the device's memory
(at most 2 GiB); a run with more frames failed. Natively the frames past the budget go to a spill file. A page has
no file it can write synchronously from the main thread, where the run is pumped: the origin private file system
(OPFS) is asynchronous there, synchronous only in a Web Worker; IndexedDB and the File System Access API are
asynchronous too and ask more of the user or the code.

## Decision

- One store, two spills: the store logic (budget, which frames spill, bookkeeping, generations, the frame kept
  through a clear) is shared; natively the spill is the file, on wasm32 a pending spill whose writes and reads return
  tokens the store settles (`SpillBackend` and `SpillFactory` in
  [`pending.rs`](../../crates/wavedude-core/src/run/pending.rs)). The core takes no browser API: the OPFS backend lives in the app state's platform layer
  ([`opfs.rs`](../../crates/wavedude-app-state/src/platform/opfs.rs)).
- OPFS from the main thread: one file per frame at `wavedude-spill/<session>/<store>-<generation>/<frame>`, written
  whole and closed (`createWritable` copies an existing file at each commit, so frames are not packed into one file),
  read with `getFile`. A generation's folder is removed at a clear, the store's at its drop, after their writes in
  flight.
- A spilled frame stays in memory while its write is in flight; the pumped run yields before a capture while 4
  writes are. A read not back yet is refused as pending: the preview keeps the picture shown and asks again. At most 2
  reads are in flight; a frame asked for beyond them waits in one slot, which the next frame asked for takes over, and
  the frames read back are kept 2 at a time, so a fast scrub holds no more.
- Each tab holds a Web Lock named after its session while it lives; at load the session folders whose lock is free
  are removed (their tab is gone), so two tabs keep their own frames.
- The free storage is estimated once at load (`navigator.storage.estimate`); a store whose frames past memory do not
  fit fails its run at the start, as a full disk does natively. A write refused for quota fails the run.
- No Web Worker with synchronous access handles until scrubbing spilled frames proves slow: measured, a read takes
  under 1 ms with WebGPU ([`docs/measurements.md`](../measurements.md), The browser's frame spill).

## Consequences

- Runs in the browser are bounded by the storage quota, not by memory.
- Web Locks need web-sys's unstable APIs: [`.cargo/config.toml`](../../.cargo/config.toml) sets
  `web_sys_unstable_apis` for wasm32.
- A browser without OPFS or `createWritable` runs what fits in memory and fails a run that would spill.
- The export, hidden in the browser, would see spilled frames as pending.

## Writes in flight (added 2026-11-14)

The run worker of [0024](0024-browser-run-in-a-web-worker.md) posts every frame to the page, which acknowledges it
once its mirror holds it. A capture read back lands only below the spill's writes in flight, so they bound the frames
posted and not acknowledged. A spill takes 64 MiB of frames in flight (`WRITE_BYTES_IN_FLIGHT`, the GPU engine's
capture ring bytes), at least one frame and at most 16, the engine's largest capture ring
(`StoreLayout::writes_in_flight` in [`store.rs`](../../crates/wavedude-core/src/run/store.rs)): small frames keep all 16,
as fewer starved the ring and a run took twice as long at 4; a 1920x1080 preview of f16 samples with the intensity
(8.3 MB a frame) takes 8, 3840x2160 takes 2.

Those frames are held past the memory budget until written, in the worker until acknowledged and as many in the
page's OPFS writes: at most 64 MiB plus one frame on each side, whatever the resolution
([`docs/measurements.md`](../measurements.md), The browser's live run in a worker).
