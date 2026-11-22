# Changelog

All notable changes to this project are documented in this file, generated from the commits by
[git-cliff](https://git-cliff.org).

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0](https://github.com/lajosbencz/WaveDudeRust/compare/v0.3.7...v0.4.0) - 2026-11-22

### Added

- the first start and the Auto preset read whether the frame cache's drive is a hard disk (sysinfo), and links open through the opener crate instead of a hand-picked command
- **Breaking:** modular workspace (ADR 0031): the waveduderust library with typed ids, non_exhaustive model and View split from Project, a headless app crate with the shared shell, and two egui frontends, classic on its own egui-mineral kit and material
- **Breaking:** the programs are waveduderust, waveduderust-material and waveduderust-cli; updates restart the macOS bundle and the installers drop the 0.3 names

### Fixed

- *(icon_subset)* cargo metadata runs online, since it needs the dev-dependencies a plain build never downloads

### Changed

- *(classic)* dialogs adopt egui_mineral's ModalFocus and default_action_pressed, replacing focus_outside and the focus give-back

### Dependencies

- bump openh264 from 0.9.9 to 0.9.10 in the cargo-minor group (#43)
- bump ureq from 3.5.1 to 3.5.2 in the cargo-minor group (#44)
- bump wgpu from 30.0.1 to 30.0.2 in the cargo-minor group (#45)
- wgpu 30.0.2 and openh264 0.9.10, the Dependabot bumps after the rebase
- url 2.5.9 for RUSTSEC-2026-0592
- bump the cargo-minor group with 2 updates (#48)

## [0.3.7](https://github.com/lajosbencz/WaveDudeRust/compare/v0.3.6...v0.3.7) - 2026-11-16

### Added

- the in-app manual is gone (decision 0028), Help opens the GitHub project page through a platform opener that reports failures; the browser download drops from 7.5 to 5.7 MB
- undo keeps at most UndoSteps steps (Settings > General, default 1000, decision 0030); whole-number logarithmic boxes step by at least one
- the welcome page opens a project, a random example or Examples from the bottom right, without Close; Run needs an enabled source; About links WaveDude Next as not affiliated

### Fixed

- the live run keeps the adopted and 256 newest requests, not one per edit; a failed OpenH264 install removes its part file

### Dependencies

- bump the cargo-minor group with 2 updates (#40)

## [0.3.6](https://github.com/lajosbencz/WaveDudeRust/compare/v0.3.5...v0.3.6) - 2026-11-16

### Added

- one Settings window (File > Settings..., Ctrl+,) holds every user setting with search and Low/Medium/High/Auto presets; setting submenus leave the menu bar, the welcome page shows RAM, presets, Settings and Examples, and OpenH264 and the update check default off (decision 0027)

### Fixed

- MP4 export muxes through shiguredo_mp4 and accepts B-frames in decode order, so Main and High exports from Firefox no longer fail
- the Settings rows stack top-down beside the section list, and section names show without their mnemonic marks
- closing the window while a modal is open closes the modal and exits, asking to save first when the project changed

## [0.3.5](https://github.com/lajosbencz/WaveDudeRust/compare/v0.3.4...v0.3.5) - 2026-11-16

### Added

- browser export and spill fixes: the encoder is gated on encodeQueueSize, frames spill through a dedicated worker into one file per store, the export dialog names every output plus its archive, and the RAM cache size is user-selectable in the browser; CITATION.cff lists the software and literature the project builds on

### Fixed

- a click on a turned brightness box focuses its upright editor with the text selected, as does the context menu on either strip; a turned editor is a TODO

## [0.3.4](https://github.com/lajosbencz/WaveDudeRust/compare/v0.3.3...v0.3.4) - 2026-11-15

### Added

- codebase cleanup: crash, data-loss and browser fixes (atomic saves, EXR intensity, memory bounds, export downloads, worker build id), one run worker and one MP4/MOV writer, dead code and the mp4 crate removed, Linux packages on glibc 2.28, and hardened CI

### Fixed

- probe CSV writes values as .NET's invariant float.ToString does (1E-05 style); conventions point at has_ffmpeg's line
- sysinfo 0.40 and the Windows unused_mut in write_atomic; one result check per workflow, unversioned CI-only tools and a monthly pin check issue

## [0.3.3](https://github.com/lajosbencz/WaveDudeRust/compare/v0.3.2...v0.3.3) - 2026-11-15

### Added

- a scene image file that differs from the embedded image is asked about, Load file or Keep embedded, at open and while watched
- objects off the canvas are named in a warning when the run's spec changes them, sources and parts left out, probes read at the edge
- preview handles follow Next's glyphs, colours and focus/hover emphasis
- generic touch support (pinch zoom and two-finger pan, a second finger cancels edits, the tree drag-scrolls, 2x UI zoom on touch-first devices) and the probe graph auto-range keeps its full-scale floor on all-zero data

### Fixed

- closed strokes cut corners sharper than a right angle, so label halos no longer spike
- the rim carries the canvas edge's wave speed but no damping, so an absorber on the edge no longer diverges in the PML
- brightness and sensitivity are model values in files, and the shipped examples are our own tuned copies
- a scene image offer is withdrawn by kept or undecodable bytes, and its question closes or rewords with the offer
- context menus get icons and shortcuts, graph titles elide, the box grip shows, scene outlines keep thin edges when shrunk, and the lever arrow uses libm

### Changed

- examples live in one folder per domain, the folder being the dialog's tab, indexed by EXAMPLES.md

## [0.3.2](https://github.com/lajosbencz/WaveDudeRust/compare/v0.3.1...v0.3.2) - 2026-11-14

### Added

- the browser exports stills, probe data, sequences and videos, MP4 through WebCodecs (#29)

### Fixed

- GPU readbacks ride their steps' batch and the browser run kicks the queue while waiting (#32)
- the browser page reuses its frame buffers across reruns, and the form greys Edges under a rim (#33)

## [0.3.1](https://github.com/lajosbencz/WaveDudeRust/compare/v0.3.0...v0.3.1) - 2026-11-14

### Fixed

- *(app)* an export proposes its own layer's name, and asks before replacing each output in the way (#26)

### Performance

- the browser runs simulations in a Web Worker, and spilled frames stream ahead of playback (#28)

## [0.3.0](https://github.com/lajosbencz/WaveDudeRust/compare/v0.2.2...v0.3.0) - 2026-11-14

### Added

- *(app)* the Windows installer registers the file types only when asked
- *(wasm)* a wasm engine package and a browser demo on GitHub Pages
- *(core)* probe readings read back without waiting on wasm32
- *(wasm)* the web demo shows the app's gizmos and probe graph boxes
- *(wasm)* the web demo links to the source and opens with a note on the desktop releases
- UI on egui/eframe instead of Freya, natively and in the browser
- *(app)* the startup page opens at each launch until its new check box turns it off
- *(app)* File > Welcome Page... opens the startup page

### Fixed

- *(app)* the macOS event loop runs the window it built instead of borrowing itself after the move

### Changed

- *(core)* the GPU engine, the run spec and the preview tiers build for wasm32
- *(view)* the preview's overlay and probe graph boxes draw from a shared draw list
- *(view)* text styles and a painter that owns its caches
- *(wasm)* the page's pictures and overlay are drawn from Rust
- *(core)* draining probes takes the pending token
- **Breaking:** the browser app moves to /WaveDudeRust/app/, the Pages root is a landing page

## [0.2.2](https://github.com/lajosbencz/WaveDudeRust/compare/v0.2.1...v0.2.2) - 2026-11-12

### Added

- *(state)* WaveDude Next's Examples 25 to 33 (Fresnel lens, diffusers, interference filter, gratings, meta prism, the three with detectors) in the Examples dialog, with a Metasurfaces and diffusers tab and the dialog widened to 860 px; a probe's Detector, Sensitivity and BinSize are written after Kind as Next writes them
- *(widgets)* a colour scheme's stop is removed by dragging its marker away from the gradient bar until it fades or by double-clicking it; the last stop stays, a drag that ends pulled away is one undo step
- *(app)* an interference filter's layer rows carry a grip that drags the layer through the list and the fibre's nodes are laid out as in WaveDude Next (a heading row, Smooth and remove on the node's row); warnings and errors also go to stderr; on Windows the app is a GUI program with no console, a failure before the window shows is a native message box; the manual's figures retaken
- *(core)* typed model fields: a derive gives every model type its field and edit enums, typed accessor consts, bounds and a hash feed, so forms, commands and the file adapter can address fields without strings. The scene key hashes parts through the feed instead of a serde serializer.
- *(compare)* rendered frames, brightness fits and probe-graph layouts against WaveDude Next, with a sweep over the examples; colour tables take the brightness in single precision as Next's do.
- *(app)* new icon from an SVG master with committed PNG, ICO and ICNS renders; the Windows executable embeds it, the NSIS installer uses it, and the app id matches the installed .desktop file.
- *(app)* file types and desktop integration on every OS

### Fixed

- *(core)* a smoothed-square pulse lasts its pulse width with both edges whole; a burst longer than its gap was cut at half the period and each edge lost its outer half
- *(app)* a counting detector's Step axis and Auto-scale steps take edits; the two were greyed for every line probe, detectors included. The probe form tests run in a window tall enough to show the Axes section, which the disabled-row test had been clicking below
- *(widgets)* the tree's drop line is centred on the edge between two rows, so a drop after a row and one before the next draw the same line
- *(widgets)* the timeline's ticks are drawn in a colour of their own, about 60 levels from the bar's fill; in the dark theme they had the fill's own colour
- *(core)* one recording key per probe decides when a run keeps its traces, so a detector's sensitivity, bin size, kind or name edit re-records where the worker kept stale detections; the scene picture is keyed on the core's scene key, so a seed change repaints a seeded part where the hand-kept scene revision missed it
- *(state)* an export waiting for a run that fails is dropped with a notice instead of waiting for a cancel, and the out point is an Option so an out point at frame 0 no longer means the whole timeline
- *(state)* the gate follower pairs a gate's ports before and after a command by the gate's name, so a removal, reorder or paste that shifts part indices no longer moves the sources feeding one gate onto another
- *(core)* the Fresnel lens writes Division before Zones and the DTD lists the part elements in Next's order; a simple grating carries its coating attributes through the file as Next does; a scheme's stop button removes down to one stop as the bar does; the timeline's hover marker, a disabled check box and the tree's enable mark get colours that show in the dark theme
- *(io)* a detector's Sensitivity converts by 64 between WaveDude Next's units and ours, as Brightness does by 8; decision 0014 records why Next's source blend is not adopted, and the mirror painting rule is listed as a deviation.
- *(core)* the live run reports a panic, cannot hang on close and no longer spins on probe readings; project files parse in linear time
- *(state)* cut removes only what reached the clipboard, undo keeps the scene image path found on opening, and an edit clamped back is no change
- *(export)* a failed export leaves no output behind, and probe markers far off the canvas draw at once
- *(cli)* every command refuses --steps outside the model's range
- *(app)* a failed update install or update thread is shown, not only logged

### Performance

- *(core)* the sub-step medium is computed once per run of equal cells

### Changed

- *(core)* the frames-in-memory count cannot underflow across its two counters, an imaging detector's bin sum is widened past u32, one level_omega in the model serves the engine, the wavelength and the gate design, the store's frame count comes from the schedule, and stepping frames reuses the playback's own clamp
- the next frame store's producer waits on a signal the old store's drop fires instead of polling its handle count every millisecond, and the scene image file is followed by a notify watcher with the 400 ms debounce the spec names instead of a modification-time check every second
- one poison-tolerant lock helper in core serves every crate, and clippy's unwrap_used is denied in the two library crates outside their tests
- *(core)* Frame, Step and Timeline in the model hold the arithmetic between frames and steps that the schedule, the simulation and the playback each wrote out; the playback's in and out points and the readout's step are typed steps
- *(core)* the .wdproj adapter reads and writes through typed attribute tables over the model's fields; attribute names, order, presence and rounding stay in the adapter, and a test proves every model field is carried or skipped on purpose.
- *(app-state)* Command::Edit carries a typed model edit applied through the fields, classified and logged like the path edits; history steps hold project clones instead of JSON text.
- *(app)* the properties forms and the canvas edits address model fields through typed props and send typed edits; Command::Set, the JSON paths and the string range table are gone, and the string keys follow the model's type and member names.
- *(core)* the model without serde: the file adapter and the typed fields carry every value, so the model types keep no serialisation derives; docs describe the typed fields, the adapter tables and the decision behind them.
- review round on the typed fields: a generated type walk replaces the hand-typed type lists in the tests, Bounds collapses to an optional range, a Name edit follows the rename rules, history holds project clones only, duplicate tests merge, and a removed layer no longer leaves a stale grip top.
- remove four functions nobody calls

### Dependencies

- bump syn from 2.0.119 to 3.0.9 in the cargo-major group (#24)

## [0.2.1](https://github.com/lajosbencz/WaveDudeRust/compare/v0.2.0...v0.2.1) - 2026-11-11

### Fixed

- spill files are unlinked and named leftovers swept at start; update, export progress and hover tooltips are event-driven; own text field gets a multi-line mode and replaces Freya's Input everywhere; KB read-out grouped per language
- *(widgets)* the caret-scroll test sends the platform's command modifier (Cmd on macOS, as freya-edit expects); Ctrl-or-Cmd through one helper for the tree, the Messages list and the number box; the scroll bounds have one writer
- *(widgets)* the pending edit holds the typed text and applies before a model-writing press and when its editor unmounts; a rename box closes when the focus leaves and gives the focus back only on Enter or Esc; a press that focuses a field no longer unfocuses it on release; dock drop rectangles of hidden groups are ignored
- *(app)* picker results are dropped when their dialog or project is gone and the CSV probe is found by name; an empty preview no longer re-requests at 4 ms; a selected block at the list edge keeps its order; a cancelled or failed export is not recorded as the last one; the presenter keys its source before rendering; the memory warning closes outside render; a close-box press waits for the modal
- *(state)* the history owns the saved marker and forgets it when a new branch drops it; Stop ends a waiting export; no memory warning while an export waits or reads, so Run during a read keeps its frames; the store is indexed by the schedule of the revision it holds; the worker publishes the adopted revision with its state and store in one status; a probe and brightness edit keeps 16-bit frames

## [0.2.0](https://github.com/lajosbencz/WaveDudeRust/compare/v0.1.9...v0.2.0) - 2026-11-10

### Added

- *(app)* an Examples dialog grouped by topic with all 44 projects embedded; a followed run ends on its last frame and an imported project's brightness fit restarts the run instead of stranding it
- **Breaking:** *(io)* one project format, WaveDude Next's .wdproj; the .wdr zip format is dropped. The adapter writes Next's brightness units and whole-pixel source positions and coatings; the gates examples become .wdproj with two half-amplitude sources per input

### Fixed

- *(widgets)* a text field selects its text on the first key after taking focus when that key beats the render, so a new object's name is replaced on macOS too
- *(widgets)* a focusing click that beats the render selects the text too, as the key path does
- *(app)* editors commit when focus leaves and every command flushes the pending edit first; focus requests fire on layout instead of timers; tests pin a followed run ending on its last frame under half frames

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

