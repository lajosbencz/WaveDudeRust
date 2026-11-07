# 0005 H.264 MP4 through Cisco's OpenH264, loaded at run time

Status: accepted, 2026-11-05; amended 2026-11-07 (run-time loading)

The original encodes MP4 with Windows Media Foundation, NV12 input, 8-bit
4:2:0. The `openh264` crate (BSD-2) gives the same sample format on every
platform; constrained baseline profile, slightly larger files than Media
Foundation's Main/High at the same look. Muxing through the `mp4` crate.

Cisco's AVC patent licence covers only its unmodified binary, downloaded from
Cisco to the user's device; a build that compiles OpenH264's source carries no
such licence and leaves the H.264 royalties to the distributor. So the default
`mp4` feature uses the crate's `libloading` feature: `wave_export::h264`
downloads OpenH264 2.6.0 from `ciscobinary.openh264.org` into the user's data
folder (`<data dir>/WaveDude/openh264`), the crate checks its SHA-256 against
Cisco's builds, and the MP4 sink loads it at run time. Cisco's conditions, as
built:

- Downloaded by the user's choice: the app's export dialog has "Use Cisco's
  OpenH264 codec" (setting `UseOpenH264`, off by default) under MP4, and the
  download happens at the first MP4 export after it is ticked; the CLI
  downloads on `wave-cli openh264 enable`.
- Enabled and disabled by the user: the same check box; `wave-cli openh264
  disable` removes the binary.
- "OpenH264 Video Codec provided by Cisco Systems, Inc." shown where the user
  controls it: above the check box, and by every `wave-cli openh264` action.
- Cisco's licence text (`crates/wave-export/assets/OpenH264-BINARY_LICENSE.txt`,
  verbatim) with the licensing information: the About dialog and `wave-cli
  openh264 license`.

The `openh264-source` feature (on `wave-export`, `wave-cli` and `wave-app`)
compiles OpenH264 in instead (`OpenH264API::from_source`): no download, no
check box, no Cisco notice; for development and CI, never for distributed
builds. Building `wave-export` or `wave-cli` without default features drops
H.264 altogether. Lossless paths stay PNG sequences and MOV with the PNG codec.

The `mp4` crate has had no release since 2023. The in-house MOV writer already
writes `moov` with `co64` after the samples, so an in-house `avc1`/`avcC` muxer
would be small; noted for when the crate becomes a problem, not done.
