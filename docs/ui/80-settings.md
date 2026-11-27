# User settings

Pointer: `WaveDudeNext.WPF/Settings.cs`. One sparse file per user (WPF: `%LOCALAPPDATA%\WaveDudeNext\Settings.xml`,
values at their default omitted). Port: JSON in the platform's config dir under `WaveDudeRust` (`AppDirs`,
`directories` crate: `~/.config/WaveDudeRust`, `%APPDATA%\WaveDudeRust\config`,
`~/Library/Application Support/WaveDudeRust`), same sparse idea; the cache and data dirs carry the same name. The
CLI reads and writes the same file for its OpenH264 commands.
These are the program's settings, never the project's. Written on change (a change in Settings saves at once) and on close.
The user-facing ones are set in File > Settings... ([`70-dialogs-and-windows.md`](70-dialogs-and-windows.md), decision 0027).
The port's file carries a `Version` (written always, 2 now; a file without it is version 0) and is written to a
temporary file beside it, then renamed over it, so a crash mid-write leaves the old file whole (as a project is,
[spec/60](../spec/60-file-formats.md)).

A value that cannot be read (a wrong type, another version's choice): logged with its key, read as its default,
the other values kept. A key this version does not know (another version's setting) is kept and written back. A
file that is no JSON object: logged with its text quoted (the next save overwrites it with defaults); never moved
aside. An unreadable file is not a first start. Version 2 drops `FrameMemoryPercent` (logged with its value).

| Setting | Type | Default | Set where |
|---|---|---|---|
| WindowWidth | double DIP (unzoomed) | 1280 | main window resize |
| WindowHeight | double DIP | 800 | main window resize |
| WindowLeft | double DIP | NaN (system places, centred) | main window move |
| WindowTop | double DIP | NaN | main window move |
| WindowState | Normal / Maximized | Normal (never saved minimised) | maximise/restore |
| Layout | dock layout tree (unknown pane ids and empty groups dropped, active tab clamped, at load) | null = default arrangement | pane drag, close, View > Panes; saved on close (port: on change) |
| Theme | System / Light / Dark | System | Settings > General |
| Language | "" / en / de / fr / nl | "" (system: the OS's preferred locales, then `LC_ALL`, `LC_MESSAGES`, `LANG`; English fallback) | Settings > General |
| RecentFiles | list of paths, newest first, max 10 | empty | opening/saving a project (also the OS recent list) |
| PreviewOrientation | Horizontal (side by side) / Vertical (stacked) | Vertical | transport bar layout radios |
| AutoRerun | bool | true | Settings > Simulation |
| WarnWhenRamInsufficient | bool | true | Settings > Simulation; memory warning "Do not warn me again" |
| UseGpu | bool | true | Settings > Preview and memory |
| LegacyCpuEngine | bool (desktop, where the CPU_REV2 engine would run; docs/spec/30-engine.md) | false | Settings > Preview and memory |
| FrameMemoryBytes | (port) int bytes; 0 = Auto, else 256 MiB, 512 MiB, 1 GiB or 2 GiB, any other value reads as Auto (the browser; natively unused) | 0 (Auto) | Settings > Preview and memory, RAM; memory warning lever |
| PreviewResolution | 1 / 2 / 4 | 1 (first start may choose 2) | Settings > Preview and memory (lever or preset); memory warning |
| PreviewFrames | 1 / 2 / 4 | 1 (first start may choose 2) | Settings > Preview and memory (lever or preset); memory warning |
| PreviewSampleBytes | 1 (8-bit index) / 2 (16-bit half) | 1 (first start may choose 2) | Settings > Preview and memory (lever or preset); memory warning |
| PreviewDitherBits | 0 / 10 / 12 / 14 / 16 | 0 (first start may choose 16) | Settings > Preview and memory (lever or preset) |
| UiZoom | double 0.5..3 from the menu, up to 8 from `--zoom=` | 1.0; 2.0 on a touch-first device (only a value that differs from the device's default is stored) | Settings > General; Ctrl+= / Ctrl+- / Ctrl+0; command line |
| LastExportFolder | path | "" | any export (fallback start folder) |
| ExportFilterIndex | int (index into the run-export filter list; out of range reads as 0) | 0 | export Save dialog |
| ExportArchive | bool (desktop; the browser always archives) | false | Settings > Export; export dialog: Archive into one ZIP file, shown for an export of more than one file |
| PartDescriptionExpanded | bool | true | description header on any part form |
| TitleFullPath | bool | false | Settings > General |
| H264Profile | Baseline / Main / High | Baseline | Settings > Export; export dialog, MP4: H.264 profile |
| UseOpenH264 | bool or absent (never set; reads as false) | absent | Settings > Export; export dialog, MP4: Use Cisco's OpenH264 codec; `waveduderust-cli openh264 enable/disable` |
| CheckForUpdates | bool or absent (never set; reads as false) | absent | Settings > Updates (builds with the updater) |
| ShowStartup | bool | true | Settings > General; welcome page check box |
| UndoSteps | (port) int 10..100000, the steps Undo can go back; out of range clamps, logged (decision 0030) | 1000 | Settings > General |
| ExportHints | list of {Project path, Run path, Still path, Probe path}, newest project first, max 20, only existing project files | empty | each export; restored when the project is opened |

Not persisted (session or project state): playback speed (resets to 1 each start; decision 0008),
loop, Live, picture split position (decision 0008), Export last target (never restored, so a first Ctrl+E cannot
overwrite silently), selection, undo history.

Project-level settings that look like UI settings but live in the project (Display row): Handles in the preview,
Labels in the preview, per-layer Preview (show picture), brightness, black level, colour scheme, outline,
export range (in/out points), frame rate (also playback rate at speed 1).
