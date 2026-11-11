# compare-csharp

Dumps the C# WaveDudeNext CPU engine's composed scene, fields and probe series for `wavedude-cli compare`.
Needs the .NET 10 SDK and `WAVEDUDE_NEXT_DIR` set to a checkout of
[graealex/WaveDudeNext](https://github.com/graealex/WaveDudeNext); the checkout stays untouched.

    ./run.sh <project.wdproj> <out-dir> --steps N [--every K] [--probe-steps] [--render]

Writes `scene.png`, `scene.txt`, `field_<step>.f32` and `intensity_<step>.f32` (canvas only, little-endian f32,
row-major) every K steps, and `probes.csv` (each active probe's mean over its cells in its layer, every step with
`--probe-steps`, else at the dumped steps). Canvas-mode projects only.

`--render` adds, per dump, `field_<step>.png` and `intensity_<step>.png` (Next's own rendering of the planes: the
Display layers' scheme, black level and brightness, no edge glow or markers), `fit.txt` (`step field_fit intensity_fit`,
`BrightnessFit`), and `graph.txt` with each probe's series and profile (`series_<k>.f32`, `profile_<k>.f32`): the graph
layout `ProbeGraph.Layout` gives at the last step. `wavedude-cli compare` then adds the "Rendered frames" section to its
report. It always writes `summary.tsv` beside it, tab separated, `-` for an undefined ratio:
`step example step parity_pixels field_fit_ratio intensity_fit_ratio field_luma_ratio intensity_luma_ratio` (one line
per dump step, with `--render` only) and `probe example name pearson amplitude_ratio` (one line per probe). Fit ratio =
C# fit / Rust fit, luma ratio and amplitude ratio = Rust / C#.

    ./sweep.sh <out-root> [example-prefix...]

Runs the dump with `--render` and `compare` on each example fixture (default: all) into `<out-root>/<example>/{csharp,rust}`
and concatenates the `summary.tsv` files into `<out-root>/summary.tsv` (`STEPS`, `EVERY` set the schedule).

    ./run.sh bench [w h] [steps]

Times both C# engines on the scene and pattern of `wavedude-cli bench` (1920x1080 by default, rim 32 outside, open
edges, one source at the centre, both planes captured as halves every 2 steps).
