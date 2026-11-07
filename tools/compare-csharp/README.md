# compare-csharp

Dumps the C# WaveDudeNext CPU engine's composed scene, fields and probe series for `wave-cli compare`.
Needs the sibling checkout `../../../../graealex/WaveDudeNext` (github.com/graealex/WaveDudeNext) and the .NET 10 SDK.

    ./run.sh <project.wdproj> <out-dir> --steps N [--every K] [--probe-steps]

Writes `scene.png`, `scene.txt`, `field_<step>.f32` and `intensity_<step>.f32` (canvas only, little-endian f32,
row-major) every K steps, and `probes.csv` (each active probe's mean over its cells in its layer, every step with
`--probe-steps`, else at the dumped steps). Canvas-mode projects only.

    ./run.sh bench [w h] [steps]

Times both C# engines on the scene and pattern of `wave-cli bench` (1920x1080 by default, rim 32 outside, open
edges, one source at the centre, both planes captured as halves every 2 steps).
