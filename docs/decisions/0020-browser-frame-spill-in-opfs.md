# 0020: The browser spills frames past memory to the origin private file system

Status: accepted, 2026-11-13; writes in flight changed 2026-11-14; reading ahead added 2026-11-14.
Amends [0015](0015-core-builds-for-wasm.md): on wasm32 a store keeps the frames past its memory budget, as
natively, instead of refusing them.

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
  ([`spill.rs`](../../crates/wavedude-app-state/src/platform/opfs/spill.rs)).
- OPFS from the main thread: one file per frame at `wavedude-spill/<session>/<store>-<generation>/<frame>`, written
  whole and closed (`createWritable` copies an existing file at each commit, so frames are not packed into one file),
  read with `getFile`. A generation's folder is removed at a clear, the store's at its drop, after their writes in
  flight.
- A spilled frame stays in memory while its write is in flight; the pumped run yields before a capture while 4
  writes are. A read not back yet is refused as pending: the preview keeps the picture shown and asks again. At most 2
  reads are in flight; a frame asked for beyond them waits in one slot, which the next frame asked for takes over, and
  the frames read back are kept 5 at a time, so a fast scrub holds no more (Reading ahead, below).
- Each tab holds a Web Lock named after its session while it lives; at load the session folders whose lock is free
  are removed (their tab is gone), so two tabs keep their own frames.
- The free storage is estimated once at load (`navigator.storage.estimate`); a store whose frames past memory do not
  fit fails its run at the first frame the run makes, which the page's store refuses (natively the store fails at
  the run's start: the run worker's store opens before the page's, which answers no sooner than its first frame).
  A write refused for quota fails the run.
- No Web Worker with synchronous access handles until scrubbing spilled frames proves slow: measured, a read takes
  under 1 ms with WebGPU ([`docs/measurements.md`](../measurements.md), The browser's frame spill).

## Consequences

- Runs in the browser are bounded by the storage quota, not by memory.
- Web Locks need web-sys's unstable APIs: [`.cargo/config.toml`](../../.cargo/config.toml) sets
  `web_sys_unstable_apis` for wasm32.
- A browser without OPFS or `createWritable` runs what fits in memory and fails a run that would spill.
- The export sees spilled frames as pending and waits for them ([0025](0025-browser-exports.md)).

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

## Reading ahead (added 2026-11-14)

A read completes in a later task of the page's event loop, after the pass that asked for it, so each spilled frame
playback reached was shown a pass late, the picture before it held meanwhile. Playback names the frames it shows next:
each tick the live controller hands the store the position and the frames after it at the stride of the last tick,
wrapped to the in point with Loop on, never past the out point or the newest frame of a run still going
(`Playback::ahead` in [`playback.rs`](../../crates/wavedude-app-state/src/playback.rs), `FrameStore::prefetch`). The
position is named too: a tick moves it past the frame asked for last before the pass asks for it. The pending spill
reads the first 4 of them that are spilled (the position and 3 after it), within the 2 reads in flight, a frame asked
for going first; a frame read back is kept while it is named, and the frame asked for last always, so 5 are kept, at
most 41 MB at 1920x1080 with f16 samples and the intensity. The failed read of the frame asked for last is that
frame's error; a failed read ahead is dropped. A seek, a pause or a clear names other frames or none; the frames no
longer named go as later ones land. Played at speed 1 over spilled 1920x1080 frames, the frames shown a pass late fell
from 59 to at most 2 a second; at speed 4 playback showed 62 to 80 frames a second where it showed none. Natively a
spilled frame is read from the file in the call (0.2 ms median for 8.3 MB from the page cache), so the file reads
nothing ahead ([`docs/measurements.md`](../measurements.md), The browser's frame spill read ahead).
