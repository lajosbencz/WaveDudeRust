# Documentation

- [`spec/`](spec/): what the core and export crates must do, mapped from WaveDude Next (step 1 of the plan).
- [`ui/`](ui/): what the desktop UI must do, mapped from WaveDude Next's WPF app (step 4).
- [`decisions/`](decisions/): architecture decision records, one file per decision, numbered.
- [`divergences.md`](divergences.md): what differs from WaveDude Next on purpose, one line each, pointing to the decisions.
- [`conventions.md`](conventions.md): how the code is written, each rule with a place in the code that shows it.
- The crates are listed in the root [`README.md`](../README.md) and [`decisions/0007-workspace-layout.md`](decisions/0007-workspace-layout.md).
- [`physics.md`](physics.md): discretisation, stability, dispersion and absorber references the engine tests follow.
- [`figures/`](figures/): pictures the README and the manual show, rendered headless by [`crates/wavedude-app/tests/screenshots.rs`](../crates/wavedude-app/tests/screenshots.rs)
  (`WAVEDUDE_FIGURES` for the manual's, `WAVEDUDE_SHOTS` for the window states). The README's animation
  `logic-gates.gif` comes from its `readme_animation` frames (`WAVEDUDE_GIF=<folder>`), joined with:

  ```sh
  ffmpeg -framerate 6 -i frame-%02d.png -vf "palettegen=max_colors=48:stats_mode=diff" palette.png
  ffmpeg -framerate 6 -i frame-%02d.png -i palette.png -lavfi "[0:v][1:v] paletteuse=dither=none:diff_mode=rectangle" -loop 0 logic-gates.gif
  ```
