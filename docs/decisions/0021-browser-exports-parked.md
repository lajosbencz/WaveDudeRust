# 0021: Browser exports are parked, the export crate is prepared for them

Status: superseded by [0025](0025-browser-exports.md), 2026-11-14 (accepted 2026-11-13). The browser build offered no exports until the design below was built; builds on
[0019](0019-egui-and-the-platform-layer.md) and [0020](0020-browser-frame-spill-in-opfs.md).

## Context

The export writes files and encodes video on threads; the browser has neither paths nor blocking calls, and a run
past memory keeps its frames in a spill that is read back asynchronously ([0020](0020-browser-frame-spill-in-opfs.md)).
Exports in the browser need a decision on where files go, how video is encoded and how large outputs leave the page.

## Decision

- The browser build offers no exports: the File > Export submenu and Export last, the probe CSV entry of the tree
  and of the graph box and the picture's export entries are hidden through `HAS_EXPORTS`
  ([`web.rs`](../../crates/wavedude-app-state/src/platform/web.rs)); their shortcuts do nothing. The desktop
  exports are unchanged.
- Already in place: `wavedude-export` builds for wasm32 without default features (a CI clippy line); its output
  files go through the `OutputFs` seam ([`fs.rs`](../../crates/wavedude-export/src/fs.rs), `StdFs` natively), closed
  explicitly; encoders run inline on wasm32; `ExportJob::step(budget, cancel)` runs over an `Arc<dyn FrameSource>`
  whose `poll_layer` answers Pending for a spilled frame not read back yet, and the blocking run steps a frame at a
  time.
- The design for the rest:
  - Stills and the probe CSV are written in memory and downloaded; the CSV without a dialog, as Save As does in the
    browser.
  - Run exports are spooled to OPFS. A sequence streams into one ZIP through the `rawzip` crate (stored entries,
    ZIP64). A video of both layers is spooled per layer and zipped at the end; a single video is downloaded
    directly.
  - The export dialog has no Browse and takes a file name only; nothing asks before replacing.
  - H.264 goes through an encoder seam: OpenH264 natively, WebCodecs in the browser. The MP4 muxer is split from
    OpenH264 then.

## Consequences

- The browser version cannot write videos, sequences, stills or probe data.
- Nothing in the browser build reaches the export crate's job; the seams stay tested natively.
- Resuming needs the ZIP writer, the OPFS spool, the WebCodecs encoder and the muxer split.
