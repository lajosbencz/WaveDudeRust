# 0025: The browser exports through its private storage and downloads

Status: accepted, 2026-11-14. Supersedes [0021](0021-browser-exports-parked.md); builds on
[0019](0019-egui-and-the-platform-layer.md), [0020](0020-browser-frame-spill-in-opfs.md) and
[0024](0024-browser-run-in-a-web-worker.md).

## Context

0021 parked the browser's exports and prepared the export crate for them. The pieces it named are built: output
stores that report readiness, a ZIP writer, the OPFS spool and a job that waits on both. What was left to decide is
where the export runs, how a file larger than memory reaches the user, and what the dialog asks.

## Decision

- The export runs on the page, stepped from the window's update in 8 ms slices, over the page's mirror store of the
  run worker ([0024](0024-browser-run-in-a-web-worker.md)); the worker takes no part. The job's frames read ahead
  through `FrameSource::prefetch`, and the live controller leaves read-ahead to a running export.
- A still and the probe CSV are written in memory and downloaded.
- A run's export is spooled in the origin private file system, in the session folder of
  [0020](0020-browser-frame-spill-in-opfs.md) under `export-<n>/`
  ([`spool.rs`](../../crates/wavedude-app-state/src/platform/opfs/spool.rs); the module is split into `session`,
  `spill`, `queue` and `spool`). A file is written through a `FileSystemWritableFileStream`; its writes are queued
  ([`queue.rs`](../../crates/wavedude-app-state/src/platform/opfs/queue.rs)) and one task per file posts them in
  order, each completion waking the window. At most 64 MiB are in flight: `OutputFs::poll_ready` is pending above
  that and the job waits, `OutputFs::poll_flushed` is pending until every closed file is committed, and the job ends
  only then, so a completed export has its files. The first failure is sticky.
- The download is the spooled file: `getFile`, an object URL, a click on an anchor, as the browser's Save As does.
  The browser streams it from disk, so a large output is not held in memory.
- ZIP archives are written with the `rawzip` crate, entries stored, ZIP64 where sizes or the count need it
  ([`zip.rs`](../../crates/wavedude-export/src/fs/zip.rs)). An image sequence, one or both layers, streams into one
  archive `<name>.zip` with a folder per layer. A video of both layers is spooled per layer and packed afterwards,
  its chunks read from the spool and written into the archive; a single video is downloaded as it is.
- The export dialog has no Browse and takes a file name only (what is typed, down to its last component); nothing asks
  before replacing, as a download replaces nothing. The probe CSV, which uses the platform's save dialog on the desktop,
  asks for its name in the same box. Run exports are greyed with the reason when the browser has no OPFS; stills and
  the CSV still work.
- MP4 is not yet available in the browser: OpenH264 does not run there. The plan is an encoder seam with the browser's
  WebCodecs behind it and the MP4 muxer split from OpenH264.
- The spool of an export is kept until the next export starts, so its download can be repeated; one that ends
  cancelled or failed is removed at once. Session folders whose tab is gone are removed at load
  ([0020](0020-browser-frame-spill-in-opfs.md)).
- `HAS_EXPORTS` is true in both builds; the export crate builds for the browser without default features, and the
  `zip` crate is used by its tests only.

## Consequences

- The browser writes stills, probe data, image sequences, and AVI, MOV and MPEG-1 videos; MP4 waits for WebCodecs.
- An export needs free storage for its output in the origin's quota, besides the frames of the run.
- Large outputs are bounded by the quota, not by memory.
- The export steps on the page between frames, so it shares the page's time with the preview.
