# 0005 H.264 MP4 through bundled openh264

Status: accepted, 2026-11-05

The original encodes MP4 with Windows Media Foundation, NV12 input, 8-bit
4:2:0. The `openh264` crate (BSD-2) gives the same sample format on every
platform; constrained baseline profile, slightly larger files than Media
Foundation's Main/High at the same look. Muxing through the `mp4` crate.

Build feature `source` compiles Cisco's encoder in (`OpenH264API::from_source`,
what `wave-export` uses). Cisco's patent licence covers only their unmodified
binary fetched from their CDN at run time (`libloading` feature); a build that
compiles the source carries no such licence, so distributing it leaves the
H.264 patent royalties to the distributor. Building `wave-export` without its
`mp4` feature drops H.264 altogether. Lossless paths stay PNG sequences and MOV
with the PNG codec.

The `mp4` crate has had no release since 2023. The in-house MOV writer already
writes `moov` with `co64` after the samples, so an in-house `avc1`/`avcC` muxer
would be small; noted for when the crate becomes a problem, not done.
