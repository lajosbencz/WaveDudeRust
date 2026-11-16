# 0005 H.264 MP4 through Cisco's OpenH264, loaded at run time

Status: accepted, 2026-11-05; amended 2026-11-07 (run-time loading), 2026-11-14 (the browser), 2026-11-15 (the
in-house muxer, the browser encoder's queue), 2026-11-16 (B-frames and the MP4 muxer: `shiguredo_mp4` replaces the
in-house muxer)

The original encodes MP4 with Windows Media Foundation, NV12 input, 8-bit
4:2:0, in quality mode with a bitrate fallback and the hardware encoder when
there is one. The `openh264` crate (BSD-2) gives the same sample format on every
platform; constrained baseline profile, slightly larger files than Media
Foundation's Main/High at the same look, quality rate control under the same
bitrate ceiling the Quality setting maps to. Software only: a hardware encoder
would mean a platform API per OS and is out of scope. Frames up to 3840 x 2160,
the encoder's limit. Muxing: the `shiguredo_mp4` crate, with an `avc1` sample
entry (`avcC` from the encoder's SPS and PPS), `stss`, and composition offsets
for pictures coded out of display order (spec 50, MP4; B-frames and the MP4
muxer, below).

Cisco's AVC patent licence covers only its unmodified binary, downloaded from
Cisco to the user's device; a build that compiles OpenH264's source carries no
such licence and leaves the H.264 royalties to the distributor. So the default
`mp4` feature uses the crate's `libloading` feature: `wavedude_export::h264`
downloads OpenH264 2.6.0 from `ciscobinary.openh264.org` into the user's data
folder (`<data dir>/WaveDudeRust/openh264`, `AppDirs::codec`), the crate checks its SHA-256 against
Cisco's builds, and the MP4 sink loads it at run time. Cisco's conditions, as
built:

- Downloaded by the user's choice: the startup page asks until answered (the
  check box "Use Cisco's OpenH264 codec", clear unless ticked); the answer is
  the `UseOpenH264` setting in the app's settings file, which the CLI reads and
  writes too, so both agree. The binary is downloaded at the first MP4 export
  after it is enabled, and at once by `wavedude-cli openh264 enable`.
- Enabled and disabled by the user: the launch question, the export dialog's
  check box "Use Cisco's OpenH264 codec" under MP4, and `wavedude-cli openh264
  enable|disable`. Disabling keeps the download; "Delete the downloaded codec"
  in the export dialog and `wavedude-cli openh264 delete` remove it, and an
  enabled codec is downloaded again at the next MP4 export.
- "OpenH264 Video Codec provided by Cisco Systems, Inc." shown where the user
  controls it: on the startup page, above the export dialog's check box, and by every
  `wavedude-cli openh264` action.
- Cisco's licence text ([`crates/wavedude-export/assets/OpenH264-BINARY_LICENSE.txt`](../../crates/wavedude-export/assets/OpenH264-BINARY_LICENSE.txt),
  verbatim) with the licensing information: the About dialog and `wavedude-cli
  openh264 license`.

The `openh264-source` feature (on `wavedude-export`, `wavedude-cli` and `wavedude-app`)
compiles OpenH264 in instead (`OpenH264API::from_source`): no download, no
question, no check box, no Cisco notice; for development and CI, never for distributed
builds. Building `wavedude-export` or `wavedude-cli` without default features drops
H.264 altogether. Lossless paths stay PNG sequences and MOV with the PNG codec.

The muxer is `shiguredo_mp4` (B-frames and the MP4 muxer, below); the MOV
writer stays in-house.

## The browser (2026-11-14)

OpenH264 does not run in the browser, and Cisco's licence covers its binary on the user's device only. The browser
build encodes H.264 with the browser's own encoder, WebCodecs' `VideoEncoder`
([`webcodecs`](../../crates/wavedude-app-state/src/platform/webcodecs/mod.rs)), behind the same `H264Encoder` seam and
MP4 muxer (feature `mp4-mux`). Nothing is downloaded: no Cisco notice, no check box, no `UseOpenH264` question; the
H.264 royalties are those of the browser's or the operating system's vendor, whose encoder it is.

- The encoder writes Annex B with an IDR every second, in decode order: at Main and High a browser may code
  B-frames, its pictures then out of display order, each chunk's timestamp naming its picture (B-frames and the MP4
  muxer, below); a picture missing or given twice fails the export. Variable bitrate at the bitrate the Quality
  setting maps to, the codec string's level the lowest of Table A-1 that holds the size and frame rate. Pictures go
  in as I420 in BT.601 studio range, tagged `smpte170m`.
- Which of Baseline, Main and High the browser encodes at the export's size and frame rate is asked with
  `VideoEncoder.isConfigSupported` when the export dialog opens; the dialog greys the others, and MP4 itself when
  none is encoded or the browser has no WebCodecs.
- The files differ from the desktop's: another encoder, rate control and, where the browser has one, a hardware
  encoder.

## The browser encoder's queue (added 2026-11-15)

The encoder takes a picture while fewer than 6 encode requests wait for it (`encodeQueueSize`, woken by `dequeue`),
not while fewer than 6 pictures are not out yet: an encoder "may emit new outputs only when new inputs are provided"
and must emit them all at a flush ([WebCodecs](https://www.w3.org/TR/webcodecs/), Internal Pending Output). Firefox
159's emits its first chunk after about 43 pictures, so the count of pictures not out stalled every MP4 export there
at its sixth picture.

## B-frames and the MP4 muxer (added 2026-11-16)

Firefox's H.264 encoder at Main and High codes B-frames and gives its chunks in decode order; the export failed there
at its second chunk ("the browser's encoder gave the picture at 66667 us where 16667 us was next"). Measured at
60 fps, the chunks' timestamps in frame units:

| Browser | Profile | Chunk order |
|---|---|---|
| Firefox 159 | Baseline | 0 1 2 3 4 5 ... (display order) |
| Firefox 159 | Main, High | 0 4 2 1 3 8 6 5 7 12 ... |
| Chrome 158 | Baseline, Main, High | 0 1 2 3 4 5 ... (display order) |

Firefox's order is the same with `latencyMode` `"quality"` and `"realtime"`. No encoder setting avoids it: WebCodecs'
`VideoEncoderConfig` has no control of B-frames or the group structure, `latencyMode` is a hint, and `avc` only
chooses the bitstream format. Baseline avoids it by having no B-frames, at the cost of the profile asked for. So the
encoder seam hands each access unit with its picture's index (`AccessUnit::picture`), the WebCodecs encoder maps
each chunk's timestamp to its picture, and the MP4 writes samples in decode order with composition offsets:
sample `k` of picture `p` is offset by `p - k` ticks, in a `ctts` of version 1 (signed); a track whose pictures all
come in order has no `ctts`. The IDR of picture 0 is decoded first, so presentation starts at 0 with no edit list
(ffprobe: `start_time` 0, frame timestamps 0, 1, 2, ...; packet decode timestamps shifted by the most negative
offset). The desktop's OpenH264 encodes Constrained Baseline and never reorders.

The in-house muxer had no composition offsets. It gave way to `shiguredo_mp4` (Apache-2.0, no dependencies, no
`unsafe`, `no_std` with `alloc`, builds for wasm32), pinned exactly (`=2026.5.0`): its file muxer does no I/O, so
the sink writes each sample to the file, or the browser's private file, as it comes and the muxer indexes it; it
reserves room for `moov` ahead of `mdat` (faststart, `moov` at the end when it does not fit), builds `avc1` and
`avcC` from the SPS and PPS, and writes per-sample composition offsets as `ctts`. Not chosen: `muxide`, which pulls
`anyhow`, `serde`, `serde_json` and `thiserror`; `mp4-atom`, which reads and writes boxes only, leaving the layout,
the offsets and faststart to us; `mp4` 0.14, unmaintained (no release since 2023), which pulls `num-bigint` and
`thiserror` 1 into the browser build. The MOV writer (`sinks::bmff`) keeps its own box code, for `QuickTime` alone.
