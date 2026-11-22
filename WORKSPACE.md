# Workspace

The crates move to the layout of [decision 0031](docs/decisions/0031-modular-core-and-frontends.md); the last column
names where each goes.

| Crate | What it holds | Goes to |
|---|---|---|
| [`waveduderust`](crates/waveduderust) | the simulation library: model (a project and its view), file formats, scene painter with every part kind, CPU and wgpu engines, frame store, runs, renderer; `view`: what the preview and the exports draw over a picture as epaint shapes, the bundled fonts; `export`: PNG and JPEG sequences, MPEG-1, AVI, QuickTime, H.264 MP4, OpenEXR, probe CSV, burned-in overlays | in place |
| [`waveduderust_macros`](crates/waveduderust_macros) | the derives that give every model type typed fields: field and edit enums, accessors, bounds | in place |
| [`waveduderust_app`](crates/waveduderust_app) | the application without a UI toolkit: document, history, selection, commands, clipboard, live runs, playback, settings, strings, log; menus and actions, forms, the preview's models, export sessions, dialogs' filters, the settings table, messages, the updater; the window's shell as data: modals, jobs, the exports' course, the close flow | in place; gains the canvas's view model from `waveduderust_egui_preview` |
| [`waveduderust_workers`](crates/waveduderust_workers) | the browser's workers, one binary each: `wavedude-run-worker`, the live run's engine in a module worker beside the page with a WebGPU device of its own; `wavedude-spill-worker`, the frames past memory in a dedicated worker's files (its protocol is `waveduderust_app::spill`) | in place |
| [`waveduderust_egui_preview`](crates/waveduderust_egui_preview) | what draws the simulation in the egui windows: the pictures, handle overlay, graph boxes and timeline, the gradient editor, drawn icons | split from `waveduderust_egui` |
| [`waveduderust_egui_platform`](crates/waveduderust_egui_platform) | egui start-up and the OS integration: keys, input, clipboard, theme, window place, menu icons, the settings shown on the runtime, file dialogs and tasks | split from `waveduderust_egui` |
| [`waveduderust_classic`](frontends/classic) | the classic window on egui, natively and in the browser: its panes, dialogs and look | the classic look of `frontends/material`, bin-only, so `just hot classic` patches it live |
| [`waveduderust_material`](frontends/material) | the `wavedude-mineral` program: the Material window on egui-mineral, natively and in the browser; bin-only, so `just hot material` patches it live | in place; becomes the egui app with the looks as modules |
| [`waveduderust_icon_subset`](crates/waveduderust_icon_subset) | the build step both windows share: the Material Symbols fonts subset to the icons their sources use | in place |
| [`waveduderust_cli`](crates/waveduderust_cli) | the `wavedude-cli` program: headless checks, benches, dispersion tables, scripted runs and exports | in place; the program becomes `waveduderust-cli` with the packaging |
| [`waveduderust_examples`](crates/waveduderust_examples) | the example projects of [`examples`](examples), embedded: the list of the Examples dialog | in place |

```sh
cargo test --workspace --features waveduderust/openh264-source,waveduderust_cli/openh264-source   # or `just test`; GPU tests skip without an adapter
cargo run -p waveduderust_cli --release -- bench --gpu
cargo run -p waveduderust_cli --release -- dispersion
cargo run -p waveduderust_cli --release -- openh264 enable    # downloads Cisco's OpenH264 for MP4
cargo run -p waveduderust_cli --release -- export project.wdproj --out out.mp4 --layers both
cargo run -p waveduderust_classic --release -- project.wdproj
```
