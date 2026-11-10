# Changelog

All notable changes to this project are documented in this file, generated from the commits by
[git-cliff](https://git-cliff.org).

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0](https://github.com/lajosbencz/WaveDudeRust/compare/v0.1.9...v0.2.0) - 2026-11-10

### Added

- *(app)* an Examples dialog grouped by topic with all 44 projects embedded; a followed run ends on its last frame and an imported project's brightness fit restarts the run instead of stranding it
- **Breaking:** *(io)* one project format, WaveDude Next's .wdproj; the .wdr zip format is dropped. The adapter writes Next's brightness units and whole-pixel source positions and coatings; the gates examples become .wdproj with two half-amplitude sources per input

## [0.1.9](https://github.com/lajosbencz/WaveDudeRust/compare/v0.1.8...v0.1.9) - 2026-11-10

### Added

- *(ui)* align the UI with WaveDude Next: Selawik fonts, Next's box model, own button, text field and tooltips, the rotated header strip, timeline and graph boxes, form and tree fixes; ten optics example projects for the new parts

## [0.1.8](https://github.com/lajosbencz/WaveDudeRust/compare/v0.1.7...v0.1.8) - 2026-11-10

### Added

- *(io)* project file version 4, a .wdproj read onto its version's defaults and .wdr format 2
- *(parts)* the Fresnel lens, a lens cut into zones of equal width or depth
- *(parts)* the diffuser and the facetted grating, plates with a profiled front face
- *(parts)* the interference filter, a plate with a stack of thin layers on its front
- *(parts)* the simple 2D grating, a lattice of round dots of one material
- *(parts)* the meta lens, meta prism and meta axicon, a plate with a sub-wavelength pattern
- *(probes)* the probabilistic detectors, counting and imaging
- *(cli)* compare writes the Rust field and intensity dumps beside the report; the new parts measured against WaveDude Next

### Fixed

- *(app)* a greyed row disables its editor, the probe's step axis and value scale among them; the interference filter's layer rows fit the pane

## [0.1.7](https://github.com/lajosbencz/WaveDudeRust/compare/v0.1.6...v0.1.7) - 2026-11-10

### Changed

- *(run)* reactive live run replaces the scratch frames

### Dependencies

- json5 1.3, its error read through its accessors

## [0.1.6](https://github.com/lajosbencz/WaveDudeRust/compare/v0.1.4...v0.1.6) - 2026-11-08

### Added

- *(app)* close the UI gaps to WaveDude Next: dock drag and drop, the full manual with figures, handle tooltips, window placement, text box commit semantics, keyboard paths, export overwrite prompt, scene image watch
- *(core)* sparse .wdr a twentieth of the .wdproj, the .wdproj quirks kept in its adapter, JSON5 legacy import, TIFF and GIF scenes, gate ports that carry their sources

### Fixed

- *(app)* read the clipboard as a menu opens, not in the render path
- *(app)* packager file-association roles in cargo-packager's lower case

## [0.1.4](https://github.com/lajosbencz/WaveDudeRust/compare/v0.1.3...v0.1.4) - 2026-11-08

### Added

- *(widgets)* dock tabs are a row high with 10 px beside their text, as in WaveDude Next

### Fixed

- *(app)* follow the OS colour scheme on Linux through the XDG portal, where winit reports none

## [0.1.3](https://github.com/lajosbencz/WaveDudeRust/compare/v0.1.2...v0.1.3) - 2026-11-08

### Added

- *(app)* show downloads with their progress in the status bar, starting with OpenH264's at the first MP4 export

### Fixed

- *(app)* keep the update, file dialog and manual tasks alive past the dialog or menu that starts them, so the installed update is reported
- *(app)* install an update only after a restart that asks about unsaved edits, and stop a running export before any exit instead of killing it

### Dependencies

- bump the cargo-major group with 3 updates (#11)

## [0.1.2](https://github.com/lajosbencz/WaveDudeRust/compare/v0.1.1...v0.1.2) - 2026-11-07

### Added

- crash report on next start, scratch frames that step on, .wdproj as the default save format

### Fixed

- *(scratch)* key the frame cache by step and count probe traces in its budget

### Performance

- batch GPU probe readback, share scene and scratch data, fix export, playback, modal and fit bugs

## [0.1.1](https://github.com/lajosbencz/WaveDudeRust/compare/v0.1.0...v0.1.1) - 2026-11-07

### Fixed

- LF checkouts on every platform, and a path test with absolute paths on Windows too

### Dependencies

- bump zip to 9.4 for its security advisories, batch the open Dependabot updates
- bump time to 0.3.55 for its advisory and quick-xml to 0.42
- bump the cargo-minor group across 1 directory with 2 updates (#1)

## [0.1.0](https://github.com/lajosbencz/WaveDudeRust/releases/tag/v0.1.0) - 2026-11-07

### Added

- WaveDude Rust workspace
- MP4 export loads Cisco's OpenH264 at run time, downloaded by the user's choice
- startup page, updater and release pipeline; crates renamed wavedude-*

### Fixed

- UI strings and docs name the wgpu engine and OpenH264 instead of OpenCL and Media Foundation

