# WaveDudeRust

A cross-platform (Linux, macOS, Windows) reimplementation in Rust of
[WaveDude Next](https://github.com/graealex/WaveDudeNext) by Alexander Graef,
itself the successor of **Wave Dude 2D** by Jeroen Vleggaar (Huygens Optics),
whose numerics followed 0x23's WaveSimulator2D. This project rebuilds the same
idea from scratch: a 2D wave-equation simulator with field and intensity
views, scenes from images and optical parts, point and composite sources,
probes with live graphs and video export.

The maths is the project's own: unscaled Laplacians, sub-steps from the CFL
bound, analytic physics tests (`docs/physics.md`). Project files of WaveDude
Next (`.wdproj`) and of Wave Dude 2D (`configuration.json`) open as imports;
the native format is `.wdr`, a zip with JSON and the scene image.

## Highlights

- Runs on Linux, macOS and Windows; GPU through wgpu (Vulkan, Metal, DX12),
  and a CPU engine with the same features.
- More accurate: the CPML rim reflects -74 to -104 dB of an outgoing wave
  (the original's rim about -61 to -70 dB); the phase error at 6 cells per
  wavelength is 0.08 % (the original's 4th-order scheme 0.55 %).

## Workspace

| Crate | What it holds |
|---|---|
| `wave-core` | model, file formats, scene painter with every part kind, CPU and wgpu engines, frame store, runs, renderer |
| `wave-export` | PNG and JPEG sequences, MPEG-1, AVI, QuickTime (Animation and PNG), H.264 MP4, OpenEXR, probe CSV, burned-in overlays |
| `wave-widgets` | the Freya widgets: dock host, tree, property grid, number box, timeline, pictures, graph boxes, colour tools |
| `wave-app-state` | the application state without a UI toolkit: document, history, selection, commands, clipboard, live runs, playback, settings, strings, log |
| `wave-app` | the desktop application: thin Freya views over `wave-app-state` |
| `wave-cli` | headless checks, benches, dispersion tables, scripted runs and exports |

```sh
cargo test --workspace --features wave-export/openh264-source,wave-cli/openh264-source   # GPU tests skip without an adapter
cargo run -p wave-cli --release -- bench --gpu
cargo run -p wave-cli --release -- dispersion
cargo run -p wave-cli --release -- openh264 enable    # downloads Cisco's OpenH264 for MP4
cargo run -p wave-cli --release -- export project.wdproj --out out.mp4 --layers both
cargo run -p wave-app --release -- project.wdr
```

## Dependencies

MP4 export uses Cisco's prebuilt OpenH264, which Cisco's H.264 patent licence
covers only when it is downloaded from Cisco to the user's device. Release
builds therefore carry no H.264 encoder: the app downloads it at the first MP4
export once "Use Cisco's OpenH264 codec" is ticked in the export dialog, the
CLI on `wave-cli openh264 enable` (decision 0005). The `openh264-source`
feature compiles OpenH264 in for development and tests; a build made with it
must not be distributed without an H.264 patent licence. `wave-export` and
`wave-cli` build without MP4 through `--no-default-features`.

`docs/spec` is what the core must do, `docs/ui` what the application must do,
`docs/decisions` why, `docs/measurements.md` how fast.

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
version. It is distributed WITHOUT ANY WARRANTY; see `LICENSE.md`.
