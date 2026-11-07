# Changelog

All notable changes to this project are documented in this file, generated from the commits by
[git-cliff](https://git-cliff.org).

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

