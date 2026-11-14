# Examples

53 example projects, which the app and the web build ship through
[`crates/wavedude-examples`](crates/wavedude-examples/). [`examples`](examples/) has one folder per domain, and each
folder is one tab of the Examples dialog:

| Folder | Tab |
|---|---|
| [`sources`](examples/sources/) | Sources and coherence |
| [`lenses`](examples/lenses/) | Lenses and imaging |
| [`gratings`](examples/gratings/) | Gratings and filters |
| [`metasurfaces`](examples/metasurfaces/) | Metasurfaces and diffusers |
| [`guides`](examples/guides/) | Guides, cavities and logic |
| [`detectors`](examples/detectors/) | Detectors and statistics |

The folders mix three series, each documented in its own notes:

- `ExampleNN_*.wdproj`: WaveDude Next's 33 examples, with brightness tuned to this engine
  ([`examples/next.md`](examples/next.md)).
- `NN-*.wdproj` in [`guides`](examples/guides/): the photonic logic gates from
  [Huygens Optics](https://www.youtube.com/@HuygensOptics)' video, with measured truth tables
  ([`examples/gates.md`](examples/gates.md)).
- The remaining `NN-*.wdproj`: textbook optics problems ([`examples/optics.md`](examples/optics.md)).

[`examples/figures`](examples/figures/) has the pictures used by the gates and optics notes.
