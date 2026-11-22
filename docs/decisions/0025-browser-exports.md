# 0025: The browser exports through its private storage and downloads

Status: accepted, 2026-11-14; the download from the user's click and a name per output with the archive's added
2026-11-15. Supersedes
[0021](0021-browser-exports-parked.md); builds on [0019](0019-egui-and-the-platform-layer.md),
[0020](0020-browser-frame-spill-in-opfs.md) and [0024](0024-browser-run-in-a-web-worker.md).

## Context

0021 parked the browser's exports and prepared the export crate for them. The pieces it named are built: output
stores that report readiness, a ZIP writer, the OPFS spool and a job that waits on both. What was left to decide is
where the export runs, how a file larger than memory reaches the user, and what the dialog asks.

## Decision

- The export runs on the page, stepped from the window's update in 8 ms slices, over the page's mirror store of the
  run worker ([0024](0024-browser-run-in-a-web-worker.md)); the worker takes no part. The job's frames read ahead
  through `FrameSource::prefetch`, and the live controller leaves read-ahead to a running export.
- A still and the probe CSV are written in memory.
- A run's export is spooled in the origin private file system, in the session folder of
  [0020](0020-browser-frame-spill-in-opfs.md) under `export-<n>/`
  ([`spool.rs`](../../crates/waveduderust_app/src/platform/opfs/spool.rs); the module is split into `session`,
  `spill`, `queue` and `spool`). A file is written through a `FileSystemWritableFileStream`; its writes are queued
  ([`queue.rs`](../../crates/waveduderust_app/src/platform/opfs/queue.rs)) and one task per file posts them in
  order, each completion waking the window. At most 64 MiB are in flight: `OutputFs::poll_ready` is pending above
  that and the job waits, `OutputFs::poll_flushed` is pending until every closed file is committed, and the job ends
  only then, so a completed export has its files. The first failure is sticky.
- The download is the spooled file: `getFile`, an object URL, a click on an anchor, as the browser's Save As does.
  The browser streams it from disk, so a large output is not held in memory. The user starts it (The download from
  the user's click, below).
- ZIP archives are written with the `rawzip` crate, entries stored, ZIP64 where sizes or the count need it
  ([`zip.rs`](../../crates/waveduderust/src/export/fs/zip.rs)). An image sequence, one or both layers, streams into one
  archive with a folder per layer. A video of both layers is spooled per layer and packed afterwards, its chunks read
  from the spool and written into the archive; a single video is downloaded as it is. The archive and each output
  are named in the dialog (A name per output, below).
- The export dialog has no Browse and takes file names only (what is typed, down to its last component); nothing asks
  before replacing, as a download replaces nothing. The probe CSV, which uses the platform's save dialog on the desktop,
  asks for its name in the same box. Run exports are greyed with the reason when the browser has no OPFS; stills and
  the CSV still work.
- MP4 goes through the browser's WebCodecs encoder behind the export crate's encoder seam, the MP4 muxer split from
  OpenH264 (feature `mp4-mux`); [0005](0005-video-openh264.md), The browser, has the details.
- The spool of an export is kept until the next export starts, so a download started from it reads it to the end;
  one that ends cancelled or failed is removed at once. Session folders whose tab is gone are removed at load
  ([0020](0020-browser-frame-spill-in-opfs.md)).
- The export entries show in both builds; the export crate builds for the browser without default features, and the
  `zip` crate is used by its tests only.

## Consequences

- The browser writes stills, probe data, image sequences, and MP4, AVI, MOV and MPEG-1 videos; MP4 needs a browser
  with WebCodecs and an H.264 encoder for the export's size and frame rate.
- An export needs free storage for its output in the origin's quota, besides the frames of the run.
- Large outputs are bounded by the quota, not by memory.
- The export steps on the page between frames, so it shares the page's time with the preview.

## The download from the user's click (added 2026-11-15)

The export's end started its download, long after the click that started the export, and on a phone the download
was dropped. A browser may refuse a download it deems hostile (HTML,
[downloading hyperlinks](https://html.spec.whatwg.org/multipage/links.html#downloading-hyperlinks), the user agent
"may return"), as one a page starts without the user's activation. An activation lasts the browser's transient
activation duration after a click, a touch's end or a key (HTML,
[transient activation duration](https://html.spec.whatwg.org/multipage/interaction.html#transient-activation-duration)).

- A completed export's file is made ready (`platform::Download`: its object URL and name) and offered in a modal,
  "The export is ready: {name}", with Download (default) and Close. Download clicks the anchor in the pass that
  takes the click: eframe runs the app's logic inside the `pointerup` handler, and after a touch or a key in the next
  frame, within the activation. Every export ends so, stills and the probe CSV too, as their export may wait for a
  run.
- Save and Save As still download at once: they run in the user's action.
- The object URL of the download started last, an export's or a Save's, is kept until the next download starts: a
  browser may fetch it after the click returns, so it is not revoked with the click. The spool keeps the file until
  the next export.

## A name per output (added 2026-11-15)

The dialog took one name and derived the files from it: each layer's by its suffix, and in the browser the archive
by the extension `.zip`. What lands in the archive and what it is called were not the user's to choose, and the
desktop could not archive at all.

- The dialog shows one name box per output the export writes, their count derived from the export: one per layer,
  each a video's file or an image sequence's name (its folder), one for a still or the CSV. Each box is labelled by
  its layer while there is more than one. They start at the names the suffixes gave.
- An export of more than one file (both layers, or any image sequence) shows "Archive into one ZIP file". In the
  browser it is checked and greyed, as an export leaves as one download; on the desktop it is the user's, kept in
  the settings (`ExportArchive`, clear by default). Checked, a box names the archive (`<project>.zip`) and the
  outputs' names become the files and folders in it.
- The app's plan and the export crate's `ExportPlan` carry each layer's output, and the app's plan the archive; the
  export writes the outputs it is given. The CLI names them from its one `--out` with `naming::layer_outputs`, each
  layer's with its suffix.
- The desktop's archive is written by the same `ZipFs`: an image sequence streams into it; videos, which seek, are
  written into a temporary folder beside the archive and packed afterwards chunk by chunk
  (`ZipFs::write_entry_sync`, the synchronous form of the browser's streamed entry). The archive is written under a
  temporary name beside its path and takes its place once complete, so a failed or cancelled export keeps the file it
  would have replaced. The question before replacing asks about the archive alone.
