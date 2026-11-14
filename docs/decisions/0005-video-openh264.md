# 0005 H.264 MP4 through Cisco's OpenH264, loaded at run time

Status: accepted, 2026-11-05; amended 2026-11-07 (run-time loading), 2026-11-14 (the browser)

The original encodes MP4 with Windows Media Foundation, NV12 input, 8-bit
4:2:0, in quality mode with a bitrate fallback and the hardware encoder when
there is one. The `openh264` crate (BSD-2) gives the same sample format on every
platform; constrained baseline profile, slightly larger files than Media
Foundation's Main/High at the same look, quality rate control under the same
bitrate ceiling the Quality setting maps to. Software only: a hardware encoder
would mean a platform API per OS and is out of scope. Frames up to 3840 x 2160,
the encoder's limit. Muxing through the `mp4` crate.

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

The `mp4` crate has had no release since 2023. The in-house MOV writer already
writes `moov` with `co64` after the samples, so an in-house `avc1`/`avcC` muxer
would be small; noted for when the crate becomes a problem, not done.

## The browser (2026-11-14)

OpenH264 does not run in the browser, and Cisco's licence covers its binary on the user's device only. The browser
build encodes H.264 with the browser's own encoder, WebCodecs' `VideoEncoder`
([`webcodecs`](../../crates/wavedude-app-state/src/platform/webcodecs/mod.rs)), behind the same `H264Encoder` seam and
MP4 muxer (feature `mp4-mux`). Nothing is downloaded: no Cisco notice, no check box, no `UseOpenH264` question; the
H.264 royalties are those of the browser's or the operating system's vendor, whose encoder it is.

- The encoder writes Annex B with an IDR every second and no reordering (a picture out of order fails the
  export), variable bitrate at the bitrate the Quality setting maps to, the codec string's level the lowest of
  Table A-1 that holds the size and frame rate. Pictures go in as I420 in BT.601 studio range, tagged
  `smpte170m`.
- Which of Baseline, Main and High the browser encodes at the export's size and frame rate is asked with
  `VideoEncoder.isConfigSupported` when the export dialog opens; the dialog greys the others, and MP4 itself when
  none is encoded or the browser has no WebCodecs.
- The files differ from the desktop's: another encoder, rate control and, where the browser has one, a hardware
  encoder.
