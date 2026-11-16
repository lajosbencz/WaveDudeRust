# Dialogs and secondary windows

Pointers: `Help/AboutWindow.xaml(.cs)`, `Help/AboutInfo.cs`, `Help/ManualWindow.xaml(.cs)`,
`Build/MarkdownToFlowDocument.cs`, `MainWindow.Export.cs`, `Dialogs/MessageWindow.xaml(.cs)`,
`Dialogs/MemoryWarningWindow.xaml(.cs)`, `Controls/ColorPicker*.cs`, `Panes/MessagesPane.xaml(.cs)`,
`Logging/StatusBarTarget.cs`, `Logging/MessagesTarget.cs`, `FileAssociation.cs`, `App.xaml.cs`.

All windows use the theme (brushes, title bar), the UI zoom and the current language. Modal dialogs are owned by
the main window and not shown in the taskbar. (port) One modal shows at a time; one asked for while another is open
(an update check's result, the memory warning of an automatic re-run) waits and shows when it closes, so no answer
is lost; the memory warning waits once. The window's close box pressed while a modal is open (port) closes the modals,
shown and waiting, but a save or confirm question, then exits as File > Exit; an open save or confirm question is
answered first.

## Message window (app's own message box)

- Modal, 440 px wide, height to content, not resizable.
- A 32 px disc: red with "!" for an error (plus the error sound), accent with "?" for a question; the text
  wrapped beside it; buttons right-aligned at the bottom.
- Button sets: OK (Enter, Esc and close box all = OK); Yes (default) / No (Esc, close box), or Yes / No (default,
  Esc, close box) where Yes destroys something (port: Discard frames); Save (default) / Don't save / Cancel (Esc,
  close box).
- Enter = default button, Esc = cancel button, mnemonics work without Alt. Labels from the string tables.
- Uses: open/save errors ("Could not open '{0}'.", "Could not save '{0}'."), "Save the changes to {0}?",
  "The folder {0} exists and holds files. Replace them?" (Yes/No), "The export failed.", image save error.
- An error dialog shows one line; the full detail (for an unreadable project/scheme/settings file: the file's text,
  first 64 K characters plus the line an XML error names) goes to the log and the Messages pane tooltip.

## Unsaved edits prompt

Before New, Open, Close, Exit, an update's restart, opening an example or recent file: "Save the changes to
<name>?" Save (saves; Save As for a project without a file; cancelled Save As aborts) / Don't save / Cancel (keeps
the project, aborts).

Exit, closing the window and an update's restart during an export (port): "An export is running. Stop it and
quit?" Yes stops it and quits once it has ended; No keeps both.

## Scene image changed on disk

"The scene image file '<name>' differs from the image embedded in the project. Load the file?" Load file
(default) / Keep embedded (Esc). See [`60-live-editing-and-runs.md`](60-live-editing-and-runs.md) (Files and drop).

## Memory warning

See [`60-live-editing-and-runs.md`](60-live-editing-and-runs.md).

## Welcome page (port, decision 0027)

A modal at the first start and at each launch while `ShowStartup` is on (the default), and from File > Welcome
Page.... Title "Welcome to WaveDude Rust". Top to bottom:
1. "Preview quality": one sentence on what it changes and that the frames past the RAM go to disk.
2. The RAM row of Settings (below): read-only natively, Auto or a size in the browser.
3. Buttons "Low", "Medium", "High", "Auto": the presets of Settings; below them one dim line naming the levers as
   they are, e.g. "Full resolution, every frame, 8-bit samples".
4. "Settings..." and "Examples...": close the page and open that window.
5. Check box "Show this page at each launch" (`ShowStartup`), saved as it changes.
6. "Close" (default and cancel; Enter or Esc).

A preset button applies at once, as in Settings; Close changes nothing more.

## Settings (port, decision 0027)

Modal (id `SettingsDialog`), 760 px wide, from File > Settings... or Ctrl+,; the frame, dimmed backdrop and buttons
of the other dialogs. Title "Settings". A search box (`SettingsSearch`, focused on open) above a section list on the
left and the section's rows on the right; one "Close" button (default and cancel; Enter outside a control or Esc).
Each row is a label column and its control; a change applies and is saved at once, as every setting
([`80-settings.md`](80-settings.md)).

| Section | Row | Control | Setting |
|---|---|---|---|
| General | Theme | drop-down System, Light, Dark | `Theme` |
| General | Language | drop-down System (in the current language), then each language by its own name | `Language` |
| General | UI zoom | drop-down 0.5x, 0.75x, 1x, 1.25x, 1.5x, 2x, 2.5x, 3x; a zoom from `--zoom=` outside it shows as its factor | `UiZoom` |
| General | Full path in the title | check box | `TitleFullPath` |
| General | Show the welcome page at each launch | check box | `ShowStartup` |
| General | Undo steps | number box 10..100000, two percent a step; tooltip (decision 0030) | `UndoSteps` |
| Preview and memory | RAM | natively read-only: "{available} available of {physical}" (the frames take the available less 2 GiB); in the browser a drop-down "Auto ({detected})", 256 MiB, 512 MiB, 1.0 GiB, 2.0 GiB (decision 0020) | `FrameMemoryBytes` (browser) |
| Preview and memory | Preset | buttons Low, Medium, High, Auto | none: fills the four rows below |
| Preview and memory | Preview resolution | drop-down Full, Half, Quarter | `PreviewResolution` |
| Preview and memory | Preview frames | drop-down Full, Half (every second frame), Quarter (every fourth frame) | `PreviewFrames` |
| Preview and memory | Preview samples | drop-down 16-bit float, 8-bit palette index (brightness fixed when the run starts) | `PreviewSampleBytes` |
| Preview and memory | Preview dithering | drop-down Off, 10, 12, 14, 16 bits per channel; disabled unless samples are 16-bit | `PreviewDitherBits` |
| Preview and memory | Use the GPU | check box (CPU fallback logged) | `UseGpu` |
| Simulation | Re-run automatically after edits | check box | `AutoRerun` |
| Simulation | Warn when a run does not fit in RAM | check box | `WarnWhenRamInsufficient` |
| Export | Use Cisco's OpenH264 codec (downloaded from Cisco at the first MP4 export) | check box with Cisco's notice; natively, absent when OpenH264 is compiled in | `UseOpenH264` |
| Export | H.264 profile | drop-down Baseline, Main, High; greyed entries as in the export dialog | `H264Profile` |
| Export | Archive several files into one ZIP file | check box; natively | `ExportArchive` |
| Updates | Look for a new version at each launch | check box; builds with the updater | `CheckForUpdates` |
| Updates | Check now | button: Help > Check for updates | none |

Presets: Low = half resolution, every second frame, 8-bit, no dithering; Medium = full resolution, every frame,
8-bit; High = 16-bit samples, 16-bit dithering; Auto = the preset of the machine now, by the first start's table
([`spec/40`](../spec/40-frames-store-render.md), Preview quality defaults), in the browser the picked RAM size
standing in for the device memory. A preset only sets the four levers; none is stored.

A change of resolution, frames or samples replaces the frames like a simulation edit; dithering only redraws.

Search: case-insensitive substring of a row's label in the current language, of its English label and of its
section's name. While the box holds text the section list is replaced by every matching row under its section's
name; "No setting matches" when none does. Clearing the box returns to the section last shown. Keys: Tab cycles the
search box, the section list and the rows; Up/Down walk the section list.

## Updates (port, decision 0011)

With "Look for a new version at each launch" (Settings > Updates) on, a check runs in the background at each launch; Help > Check for updates
runs one on demand. A newer release asks "WaveDude Rust x is available (this is y). Download it?" where the
updater can replace the program (Windows installer, macOS app, Linux AppImage); Yes downloads it, with
"Downloading WaveDude Rust x" and its progress in the status bar, and verifies the signature, then asks "WaveDude
Rust x is downloaded. Restart now to install it?" Yes exits as Exit does (unsaved edits and a running export asked
about first; Cancel keeps the program running), installs, and starts the new version (on Windows its installer
does). A failed install starts the old version again and shows the reason. No drops the download. A plain Linux binary,
and a macOS app run from a disk image, a volume other than the system's or a quarantined (translocated) location, gets
"... Download it from <release page>" instead, on macOS with "Move WaveDude Rust to the Applications folder to get
automatic updates." A launch check says
nothing when there is no update or the check fails (logged); an on-demand one says "WaveDude Rust is up to date."
or "The update failed." with the reason.

## Export Save dialog

Native save dialog (port: an in-window dialog with the format and a path box per output, each box's "..." opening
the platform save dialog, which cannot report its filter; the probe CSV's is the platform's). The browser's export
dialog is described under Browser differences, below.

| Command | Title | Filter list | Proposed name |
|---|---|---|---|
| Export field / intensity / both | "Export the field" / "Export the intensity" / "Export the field and the intensity" | MP4 video, H.264 (*.mp4); MPEG-1 video (*.mpg); AVI video, uncompressed (*.avi); QuickTime movie, Animation codec, lossless (*.mov); QuickTime movie, PNG codec, lossless (*.mov); PNG image sequence (*.png); JPEG image sequence (*.jpg); OpenEXR image sequence, 16-bit float data, intensity as I (*.exr); OpenEXR image sequence, 16-bit float data, intensity as sqrt(I) (*.exr) | `<project>_field` / `<project>_intensity`; both: each in its box; the archive `<project>.zip` |
| Export field / intensity frame | "Export the frame" | PNG image; JPEG image; OpenEXR image as I; OpenEXR image as sqrt(I) | `<project>_field_0655` (frame number padded like a sequence's files) |
| Export probe data as CSV | "Export the probe data" | CSV file (*.csv) | the probe's name |

- The chosen filter is the format (the two MOV formats share `.mov`); the extension follows it in every path box.
- Port: one path box per output the export writes: one per layer (a video's file, or an image sequence's
  `<name>.<ext>`, its folder `<name>`), one for a still or the CSV. While there is more than one box, each is
  labelled left of it by its layer ("Field", "Intensity") or "Archive". OK is disabled while a box has no file name
  or two outputs share one (a sequence's folder, a video's file). The CLI's `--layers both` keeps one path, each
  layer's output named with its suffix.
- Port: an export of more than one file (both layers, or any image sequence) shows the check box "Archive into one
  ZIP file" (setting `ExportArchive`, default clear). Checked, an "Archive" box follows (`<project>.zip`, its "..."
  filtering "ZIP archive (*.zip)"), and the outputs' file names become the files and folders in the archive. The
  archive is written beside its path under a temporary name and takes its place once complete; an image sequence
  streams into it, videos are written into a temporary folder beside it first and packed afterwards, stored, chunk by
  chunk. A failed or cancelled export leaves no archive and keeps the one it would have replaced.
- Port: MP4 shows "OpenH264 Video Codec provided by Cisco Systems, Inc." and the check box "Use Cisco's OpenH264
  codec (downloaded from Cisco at the first MP4 export)" (setting `UseOpenH264`); OK is disabled while it is
  clear. Once downloaded, a "Delete the downloaded codec" button follows; the choice stays. The download runs on the export's worker before the first frame; a failure ends the export with its reason
  in the error box. A build with OpenH264 compiled in shows neither (decision 0005).
- MP4 also shows "H.264 profile" (Baseline, Main, High; setting `H264Profile`). A profile the encoder lacks is
  greyed, its tooltip the reason ("OpenH264 encodes Baseline only"), and the reason shows below while the setting
  names one; the export then uses the encoder's best profile.
- The last filter index is remembered (`ExportFilterIndex`); a remembered video means PNG in the still dialog.
- Start folder rule, per kind (run export, still, probe data) and per project file: the first export after a
  project is opened starts in the project's folder under the project's name; every later one in the last completed
  export's folder (run export: last name with the layer suffix swapped). Kept in user settings (`ExportHints`, 20
  projects, only while their files exist), restored when the project is opened. A project without a file keeps the
  session's alone. Fallback: settings' `LastExportFolder`.
- A sequence goes into a folder of its own; an existing folder with files asks "Replace them?", an existing video,
  still or archive "Replace it?", one question per output, or the archive alone when archived (Export last, which
  repeats the outputs and the archive, asks nothing).
- After the dialog: when there is no run, an incomplete one, a reduced-quality one, or one lacking the intensity plane, a
  full-quality run is started first (status "Export: waiting for the run", indeterminate); then "Exporting n / N
  frames" with a determinate bar (tooltip "Cancel the export (Esc)"). Esc cancels, while waiting too (the run
  stops). A run that ends without completing while the export waits (Stop, a simulation edit, a failure) drops
  the export with a warning in the status bar; Play meanwhile plays along the run. The status bar reports the end.
- A still renders exactly what a video export would show at that frame, never the preview's frame.
- CSV: a point probe one row per step ("step;amplitude" header), a line probe one row per frame
  ("frame;step;<distances>"), semicolons, decimal point. Full run first when needed.

Browser differences (decision 0025):

- Each name box holds a file name only: what is typed is cut to its last component, and the format's (or `.zip`)
  extension is added when it has none. There is no "..." button, no start folder and no question before replacing,
  as a download replaces nothing. The probe CSV, which uses the platform's save dialog on the desktop, opens this
  dialog for its name too.
- An export of more than one file is always one download: "Archive into one ZIP file" is checked and greyed, and
  the "Archive" box names it. Both layers show three boxes: the field's, the intensity's and the archive's.
- MP4 encodes with the browser's WebCodecs encoder: no Cisco notice, no check box. As the dialog opens the browser
  is asked which H.264 profiles it encodes at the project's canvas size and frame rate; meanwhile MP4 shows "Asking
  the browser which H.264 profiles it encodes..." and OK is disabled. "H.264 profile" then greys the profiles it
  lacks, their tooltip and the line below "not supported by this browser". MP4 itself is greyed in the format list,
  and shows its reason with OK disabled when chosen: "This browser has no video encoder (WebCodecs)." or "This
  browser encodes no H.264 profile at this picture size and frame rate." A run's export (not a still) shows "This browser has no private file storage to write the export to; frames and probe data
  still export." and is disabled when the browser has no OPFS. A still and the CSV do not need it.
- The export is stepped on the page; the status bar shows "Exporting n / N frames", and "Export: packing the
  archive" while two videos are packed into their ZIP. Esc cancels as on the desktop.
- Download names, from the typed names, `<name>` an output's and `<archive>` the archive's (extension as chosen):

| Export | Downloaded as |
|---|---|
| Frame (still), field or intensity | `<name>.<ext>` as typed, e.g. `run_field_0042.png` |
| Probe CSV | `<name>.csv` |
| Image sequence, one layer | `<archive>.zip`: the files in the folder `<name>` |
| Image sequence, both layers | `<archive>.zip`: a folder per layer, each named by its box |
| Video, one layer | `<name>.<ext>` |
| Videos, both layers | `<archive>.zip` holding each layer's `<name>.<ext>` |

- A completed export shows a message window (info disc): "The export is ready: <download name>", with Download
  (default) and Close (Esc). Download starts the browser's download and closes it; a browser may drop a download a
  page starts without the user's click, so the end of an export never starts one by itself. The status bar reports
  "n frames exported as <name>". The file stays available to the download it started until the next export or
  download (Save, Save As) starts.
- The export's files wait in the browser's private storage; they are kept until the next export starts and removed
  at once when an export is cancelled or fails. Folders of closed tabs are removed at the next load.

## About dialog

Modal, 600 px wide (min 480 x 620), height to content until the license is shown. Esc or Enter closes. Every link
and button in the Tab cycle; focus starts on Close.

Content top to bottom:
1. App icon (64 px), product name, "Version x.y.z (hash7)" (informational version with the commit shortened to
   seven chars), tagline "A 2D wave simulator", copyright line.
2. "Based on the work of": credits word for word as the source header (Jeroen Vleggaar, Huygens Optics, author of
   Wave Dude 2D, Copyright (C) 2026; 0x23 (Diffraction Limited), author of WaveSimulator2D) with five links
   (YouTube channels, websites, GitHub). English regardless of language.
3. "License": the GPL notice sentence; toggle button "Show the license text" / "Hide the license text" (T).
   Shown: [LICENSE.md](../../LICENSE.md) rendered as a document (headings, lists, links) in a scrollable box (260 px, min 120);
   a taller window gives its extra height to the document; hidden: window shrinks back.
4. "System": read-only text box (max 160 px, scrolls), one line each: Program, Runtime, Operating system,
   Processor (+ logical cores), GPU setting (on/off), Engine ("GPU: <wgpu adapter name>" or "CPU (n threads)"),
   H.264 encoder ("Cisco's OpenH264 2.6.0" when enabled and downloaded, "not enabled", or "OpenH264, compiled
   in"). The original's Memory line is not ported.
   Port, before "System": "OpenH264": Cisco's notice and the toggle "Show the OpenH264 license" / "Hide the
   OpenH264 license" (O), showing Cisco's licence text verbatim, as plain text, in the same kind of box; absent
   when OpenH264 is compiled in.
5. Buttons: "Copy system info" (C; tooltip "Copy the system lines to the clipboard, for a bug report"; status
   "System information copied to the clipboard"), "Close" (default and cancel).

## Examples dialog

Modal (id `ExamplesDialog`), 860 px wide, from File > Examples or the welcome page; the frame, dimmed backdrop and buttons of the
other dialogs. Title "Examples"; a tab strip of six topics (`ExamplesTab:<n>`: Sources and coherence 8, Lenses and
imaging 10, Gratings and filters 7, Metasurfaces and diffusers 5, Guides cavities and logic 18, Detectors and
statistics 5); a search box
(`ExamplesSearch`) filtering every tab by title and description, dimming the tabs without a match and moving to
the first with one; a 360 px scrolled list, one row per example (`Example:<file stem>`): the title, below it the
description wrapped in the dim text colour; (port) Welcome page..., Open and Cancel. Welcome page... closes the
dialog and opens the welcome page. Left/Right switch the tab (not while the search box
has the focus), Up/Down move the row, Enter or Open or a double-click on a row opens it, Esc or Cancel closes,
Tab cycles the controls. Opening closes the dialog and runs the guarded open (unsaved-changes prompt included).
Every project is a `.wdproj` of [`examples`](../../examples/), embedded, the folder being its tab: Next's 33 as our
tuned copies, ours the gates and the optics series ([`EXAMPLES.md`](../../EXAMPLES.md)).

## Manual window

Removed (port): Help > Project page on GitHub (F1) opens the project's GitHub page instead; no manual in the
app ([decision 0028](../decisions/0028-no-manual-in-the-app.md)).

## Colour picker (popup of a colour swatch)

- Swatch button: Space, Enter or Down (or click) opens a popup below it; focus goes to the square.
- Popup: saturation/value square (pure hue under white-to-clear and clear-to-black), a vertical hue strip, R, G, B
  number boxes (0..255, Up/Down step), hex text box.
- Keys: square arrows Left/Right saturation, Up/Down value, by 1/100; strip Up/Down hue by 1 degree; Shift x10.
  Mouse drags both markers.
- Hue is kept for greys and when the incoming colour already matches (no jumps while dragging).
- The bound colour follows live (gradient and preview recolour), committed to the owner at most ten times a second
  and once more when a drag ends.
- Enter keeps and closes; Esc restores the original colour and closes; click outside closes (a click on the swatch
  itself does not reopen). Focus returns to the swatch. Tooltips: square "Saturation (Left, Right) and brightness
  (Up, Down); Shift moves by ten steps", strip "Hue: Up and Down move by a degree, ten with Shift", channels
  "Channel 0 to 255; Up and Down step it".

## Messages pane

Dock pane (default bottom, hidden at first start).

- Toolbar left to right (all tab stops): one toggle per level "Level (count)" in the level's colour (Trace,
  Debug, Info, Warn, Error, Fatal; tooltip "Show or hide this level"), a search box (placeholder "Search"; text
  matched against message, logger, exception), "Regex" toggle (an unfinished pattern shows everything until it
  parses), "Auto-scroll" toggle ("Follow new messages"), "Clear" button.
- List with columns Time, Level, Source (logger name without namespace), Message (+ exception text); rows
  coloured per level; resizable columns. Opens scrolled to the newest line; focus on the list.
- Initial level visibility: Info and above always; Trace and Debug visible in debug builds only (release logs no
  Trace at all).
- Context menu: Copy (Ctrl+C, selected rows), Copy all (visible rows), Clear. Lines are tab-separated.
- Keeps the last 5000 entries, oldest dropped; batches arrivals so bursts do not stall.
- Row tooltip: the exception text (for an unreadable file this quotes the file). Source cell tooltip: full logger name.

## Status bar

- Left: the latest log line of level Info and up ("Opened ...", "Saved ...", errors); "Ready" at first. Warnings
  turn the whole bar orange, errors red (theme tokens), until the next Info line.
- Right: run status ([`50-timeline-transport.md`](50-timeline-transport.md)).
- Far right: busy text + 120 px progress bar (export frames determinate; waiting for run / computing a frame
  indeterminate); hidden when idle.
- Tooltip "Double-click to open the Messages pane"; double-click opens or focuses it.

## Crash report (port)

A panic is logged at Error with its backtrace and printed to stderr. A panic of the main thread, which ends the
program, is also appended to `crash.txt` in the log folder; a worker's panic is caught by its owner (a run fails, an
export reports an error) or ends that thread alone. At the next start a message window (error disc) says "WaveDude Rust hit an internal error in
the last session. The report, to attach to a bug report:" and the path; the file is renamed `last-crash.txt` so it
shows once. Buttons: Copy path (path to the clipboard, the window stays), OK (Enter, Esc). It comes before the
welcome page and the update check, which wait for the next start. In the browser, where every panic ends the program,
the report is kept in the page's storage; the window shows its text, and Copy report puts the text on the
clipboard.

## First start

When no settings file exists:
- Read machine facts (memory, threads, GPU presence, whether the cache drive has a seek penalty) and choose:
  - 32 GB or more: 16-bit samples, dithering 16 bits.
  - Under 16 GB, or a cache drive that is a hard disk, or no GPU, or 4 threads or fewer: half resolution, every
    second frame, 8-bit samples.
  - Otherwise: full resolution, every frame, 8-bit samples.
  - (port) These are the presets High, Low and Medium of Settings; the welcome page opens.
- Write the settings file at once (deleting it redoes the choice); log one Info line naming facts and choice
  (shown in the status bar).
- Default dock layout, window 1280 x 800 centred, theme System, language System.

## File type registration (platform-specific)

Windows: per-user registration of `.wdproj` ("WaveDude Next project") and `.wdscheme` at every start (cheap when
unchanged), removing a user "Open with" override; failures are logged, never shown. Double-click starts the app
with the file. Port: desktop entry + MIME types on Linux, Info.plist on macOS, registry on Windows, all done by the
installer or package, never at start (decision 0008).
