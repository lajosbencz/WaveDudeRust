# Workspace

| Crate | What it holds |
|---|---|
| [`wavedude-core`](crates/wavedude-core) | model, file formats, scene painter with every part kind, CPU and wgpu engines, frame store, runs, renderer |
| [`wavedude-model-derive`](crates/wavedude-model-derive) | the derives that give every model type typed fields: field and edit enums, accessors, bounds |
| [`wavedude-export`](crates/wavedude-export) | PNG and JPEG sequences, MPEG-1, AVI, QuickTime (Animation and PNG), H.264 MP4, OpenEXR, probe CSV, burned-in overlays |
| [`wavedude-view`](crates/wavedude-view) | what the preview and the exports draw over a picture: the handle overlay, the probe graph box, as epaint shapes; the bundled fonts |
| [`wavedude-app-state`](crates/wavedude-app-state) | the application state without a UI toolkit: document, history, selection, commands, clipboard, live runs, playback, settings, strings, log |
| [`wavedude-app`](crates/wavedude-app) | the application on egui, natively and in the browser: the window, its panes and dialogs, the updater, exports |
| [`wavedude-cli`](crates/wavedude-cli) | headless checks, benches, dispersion tables, scripted runs and exports |
| [`wavedude-examples`](crates/wavedude-examples) | the example projects, embedded: the list of the Examples dialog |

```sh
cargo test --workspace --features wavedude-export/openh264-source,wavedude-cli/openh264-source   # GPU tests skip without an adapter
cargo run -p wavedude-cli --release -- bench --gpu
cargo run -p wavedude-cli --release -- dispersion
cargo run -p wavedude-cli --release -- openh264 enable    # downloads Cisco's OpenH264 for MP4
cargo run -p wavedude-cli --release -- export project.wdproj --out out.mp4 --layers both
cargo run -p wavedude-app --release -- project.wdproj
```
