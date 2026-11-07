# WaveDudeRust

A cross-platform (Linux, macOS, Windows) reimplementation in Rust of
[WaveDude Next](https://github.com/graealex/WaveDudeNext) by Alexander Graef,
itself the successor of **Wave Dude 2D** by Jeroen Vleggaar (Huygens Optics),
whose numerics followed 0x23's WaveSimulator2D. This project rebuilds the same
idea from scratch: a 2D wave-equation simulator with field and intensity
views, scenes from images and optical parts, point and composite sources,
probes with live graphs and video export.

The maths is the project's own: unscaled Laplacians, sub-steps from the CFL
bound, analytic physics tests ([`docs/physics.md`](docs/physics.md)). Project files of WaveDude
Next (`.wdproj`) and of Wave Dude 2D (`configuration.json`) open; projects save
as `.wdproj` by default, so WaveDude Next opens them too. `.wdr`, a zip with
JSON and the scene image, opens and saves as well.

## Highlights

- Runs on Linux, macOS and Windows; GPU through wgpu (Vulkan, Metal, DX12),
  and a CPU engine with the same features.
- More accurate: the CPML rim reflects -74 to -104 dB of an outgoing wave
  (the original's rim about -61 to -70 dB); the phase error at 6 cells per
  wavelength is 0.08 % (the original's 4th-order scheme 0.55 %).

## Workspace

| Crate | What it holds |
|---|---|
| [`wavedude-core`](crates/wavedude-core) | model, file formats, scene painter with every part kind, CPU and wgpu engines, frame store, runs, renderer |
| [`wavedude-export`](crates/wavedude-export) | PNG and JPEG sequences, MPEG-1, AVI, QuickTime (Animation and PNG), H.264 MP4, OpenEXR, probe CSV, burned-in overlays |
| [`wavedude-widgets`](crates/wavedude-widgets) | the Freya widgets: dock host, tree, property grid, number box, timeline, pictures, graph boxes, colour tools |
| [`wavedude-app-state`](crates/wavedude-app-state) | the application state without a UI toolkit: document, history, selection, commands, clipboard, live runs, playback, settings, strings, log |
| [`wavedude-app`](crates/wavedude-app) | the desktop application: thin Freya views over `wavedude-app-state` |
| [`wavedude-cli`](crates/wavedude-cli) | headless checks, benches, dispersion tables, scripted runs and exports |

```sh
cargo test --workspace --features wavedude-export/openh264-source,wavedude-cli/openh264-source   # GPU tests skip without an adapter
cargo run -p wavedude-cli --release -- bench --gpu
cargo run -p wavedude-cli --release -- dispersion
cargo run -p wavedude-cli --release -- openh264 enable    # downloads Cisco's OpenH264 for MP4
cargo run -p wavedude-cli --release -- export project.wdproj --out out.mp4 --layers both
cargo run -p wavedude-app --release -- project.wdr
```

## Releases

Installers for Windows (NSIS), macOS (app and DMG, Apple Silicon and Intel)
and Linux (AppImage, and a tarball of the two programs) are on the GitHub
releases page, unsigned by Apple and Microsoft, so Gatekeeper and SmartScreen
warn at the first start; each file carries a build provenance attestation
(`gh attestation verify <file> --repo <owner>/<repo>`). The installed program
updates itself from the releases when the user agrees. How a release is made:
[`docs/release.md`](docs/release.md)
([decision 0011](docs/decisions/0011-releases-and-updates.md)).

## Dependencies

MP4 export uses Cisco's prebuilt OpenH264, which Cisco's H.264 patent licence
covers only when it is downloaded from Cisco to the user's device. Release
builds therefore carry no H.264 encoder: the app's startup page asks whether
to enable it and the app downloads it at the first MP4 export, the CLI on
`wavedude-cli openh264 enable`; both share the choice ([decision 0005](docs/decisions/0005-video-openh264.md)). The `openh264-source`
feature compiles OpenH264 in for development and tests; a build made with it
must not be distributed without an H.264 patent licence. `wavedude-export` and
`wavedude-cli` build without MP4 through `--no-default-features`.

[`docs/spec`](docs/spec) is what the core must do, [`docs/ui`](docs/ui) what the application must do,
[`docs/decisions`](docs/decisions) why, [`docs/measurements.md`](docs/measurements.md) how fast.

## Copyright and license

Copyright (C) 2026 Lajos Bencz.

WaveDudeRust is a modified work, written in 2026, of WaveDude Next, Copyright
(C) 2026 Alexander Graef, itself based on Wave Dude 2D, Copyright (C) 2026
Jeroen Vleggaar (Huygens Optics). It carries material of WaveDude Next: the
colour schemes, the example projects, the user interface strings and the
manual's content.

WaveDudeRust is free software: you can redistribute it and/or modify it under
the terms of the GNU General Public License as published by the Free Software
Foundation, either version 3 of the License, or (at your option) any later
version. It is distributed WITHOUT ANY WARRANTY; see [`LICENSE.md`](LICENSE.md).
