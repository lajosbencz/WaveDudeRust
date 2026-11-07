# User settings

Pointer: `WaveDudeNext.WPF/Settings.cs`. One sparse file per user (WPF: `%LOCALAPPDATA%\WaveDudeNext\Settings.xml`,
values at their default omitted). Port: the platform config dir (e.g. `directories` crate), same sparse idea.
These are the program's settings, never the project's. Written on change (menu choices save at once) and on close.
The port's file carries a `Version` (written always, 1 now; a file without it is version 0) and is written to a
temporary file beside it, then renamed over it, so a crash mid-write leaves the old file whole.

An unreadable settings file: logged with its text quoted (the next save overwrites it with defaults); never
moved aside. An unreadable file is not a first start.

| Setting | Type | Default | Set where |
|---|---|---|---|
| WindowWidth | double DIP (unzoomed) | 1280 | main window resize |
| WindowHeight | double DIP | 800 | main window resize |
| WindowLeft | double DIP | NaN (system places, centred) | main window move |
| WindowTop | double DIP | NaN | main window move |
| WindowState | Normal / Maximized | Normal (never saved minimised) | maximise/restore |
| Layout | dock layout tree (unknown pane ids and empty groups dropped, active tab clamped, at load) | null = default arrangement | pane drag, close, View > Panes; saved on close |
| Theme | System / Light / Dark | System | View > Theme |
| Language | "" / en / de / fr / nl | "" (system: the OS's preferred locales, then `LC_ALL`, `LC_MESSAGES`, `LANG`; English fallback) | View > Language |
| RecentFiles | list of paths, newest first, max 10 | empty | opening/saving a project (also the OS recent list) |
| PreviewOrientation | Horizontal (side by side) / Vertical (stacked) | Vertical | transport bar layout radios |
| AutoRerun | bool | true | Simulation > Re-run automatically after edits |
| WarnWhenRamInsufficient | bool | true | Simulation > Warn when a run does not fit in RAM; memory warning "Do not warn me again" |
| UseGpu | bool | true | Simulation > Use the GPU |
| FrameMemoryPercent | int; 0 or less = Auto, else 10..100 | 0 (Auto) | Simulation > RAM cache size; memory warning lever |
| PreviewResolution | 1 / 2 / 4 | 1 (first start may choose 2) | View > Preview resolution; memory warning |
| PreviewFrames | 1 / 2 / 4 | 1 (first start may choose 2) | View > Preview frames; memory warning |
| PreviewSampleBytes | 1 (8-bit index) / 2 (16-bit half) | 1 (first start may choose 2) | View > Preview samples; memory warning |
| PreviewDitherBits | 0 / 10 / 12 / 14 / 16 | 0 (first start may choose 16) | View > Preview dithering |
| UiZoom | double 0.5..3 from the menu, up to 8 from `--zoom=` | 1.0 | View > UI zoom; command line |
| LastExportFolder | path | "" | any export (fallback start folder) |
| ExportFilterIndex | int (index into the run-export filter list; out of range reads as 0) | 0 | export Save dialog |
| PartDescriptionExpanded | bool | true | description header on any part form |
| TitleFullPath | bool | false | View > Full path in the title |
| UseOpenH264 | bool | false | export dialog, MP4: Use Cisco's OpenH264 codec |
| ExportHints | list of {Project path, Run path, Still path, Probe path}, newest project first, max 20, only existing project files | empty | each export; restored when the project is opened |

Not persisted (session or project state): playback speed (resets to 1 each start; decision 0008),
loop, Live, picture split position (decision 0008), Export last target (never restored, so a first Ctrl+E cannot
overwrite silently), selection, undo history.

Project-level settings that look like UI settings but live in the project (Display row): Handles in the preview,
Labels in the preview, per-layer Preview (show picture), brightness, black level, colour scheme, outline,
export range (in/out points), frame rate (also playback rate at speed 1).
