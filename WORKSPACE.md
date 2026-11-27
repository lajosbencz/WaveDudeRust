# Workspace

The crates of [decision 0032](docs/decisions/0032-slint-single-frontend.md).

| Crate | What it holds |
|---|---|
| [`waveduderust`](crates/waveduderust) | the simulation library: model (a project and its view), file formats, scene painter with every part kind, CPU and wgpu engines, frame store, runs, renderer; `view`: what the preview and the exports draw over a picture as epaint shapes, the bundled fonts; `export`: PNG and JPEG sequences, MPEG-1, AVI, QuickTime, H.264 MP4, OpenEXR, probe CSV, burned-in overlays |
| [`waveduderust_macros`](crates/waveduderust_macros) | the derives that give every model type typed fields: field and edit enums, accessors, bounds |
| [`waveduderust_platform`](crates/waveduderust_platform) | host services without a UI toolkit, shared by the CLI and the app: settings and their store, the program's folders, the OpenH264 download, the user's colour schemes, the export runner, the machine's facts |
| [`waveduderust_remote`](crates/waveduderust_remote) | the live run in the browser's run worker (decision 0024): the messages between the page and the worker, the worker's end and the page's mirror of its frames; the page's end of the spill worker (decision 0020) |
| [`waveduderust_workers`](crates/waveduderust_workers) | the browser's workers: `wavedude-run-worker`, the live run's CPU engine beside the page, and `wavedude-spill-worker`, the frames past memory in the origin private file system; `web/` holds the JS that loads each |
| [`waveduderust_cli`](crates/waveduderust_cli) | the `waveduderust-cli` program: headless checks, benches, dispersion tables, scripted runs and exports |
| [`slint_dock`](crates/slint_dock) | a docking layout for any Slint app (decision 0036): tab groups in a split tree around a fixed centre, its geometry, drop targets, moves, show and close, a serde-able arrangement; `ui/dock.slint` draws the chrome; no WaveDude dependency |
| [`waveduderust_slint`](frontends/waveduderust_slint) | the `waveduderust` program: the window on Slint, natively and in the browser on WebGPU; `.slint` files in `ui/`, their Rust bindings in `src/`; the example projects of [`examples`](examples) embedded |

```sh
cargo test --workspace --features waveduderust/openh264-source,waveduderust_cli/openh264-source   # or `just test`; GPU tests skip without an adapter
cargo run -p waveduderust_cli --release -- bench --gpu
cargo run -p waveduderust_cli --release -- dispersion
cargo run -p waveduderust_cli --release -- openh264 enable    # downloads Cisco's OpenH264 for MP4
cargo run -p waveduderust_cli --release -- export project.wdproj --out out.mp4 --layers both
cargo run -p waveduderust_slint --release -- project.wdproj
just hot    # the app with its .slint files reloaded on save
just web    # the browser build (tools/build-web.sh)
```
