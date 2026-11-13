# Dialogs and secondary windows

Pointers: `Help/AboutWindow.xaml(.cs)`, `Help/AboutInfo.cs`, `Help/ManualWindow.xaml(.cs)`,
`Build/MarkdownToFlowDocument.cs`, `MainWindow.Export.cs`, `Dialogs/MessageWindow.xaml(.cs)`,
`Dialogs/MemoryWarningWindow.xaml(.cs)`, `Controls/ColorPicker*.cs`, `Panes/MessagesPane.xaml(.cs)`,
`Logging/StatusBarTarget.cs`, `Logging/MessagesTarget.cs`, `FileAssociation.cs`, `App.xaml.cs`.

All windows use the theme (brushes, title bar), the UI zoom and the current language. Modal dialogs are owned by
the main window and not shown in the taskbar. (port) One modal shows at a time; one asked for while another is open
(an update check's result, the memory warning of an automatic re-run) waits and shows when it closes, so no answer
is lost; the memory warning waits once. The window's close box pressed while a modal is open takes effect when it
closes.

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

## Memory warning

See [`60-live-editing-and-runs.md`](60-live-editing-and-runs.md).

## Startup page (port)

A modal at launch on the first start (no settings file), and later while one of its questions is unanswered
(`UseOpenH264` absent and OpenH264 not compiled in; `CheckForUpdates` absent in a build with the updater). Every
section shows; OK (Enter or Esc) saves every answer and closes. Title "Welcome to WaveDude Rust". The quality
selector comes first; the codec and update questions follow it. "Examples..." saves the answers the
same way, closes the page and opens the Examples window.
1. "Preview quality": one sentence on what it changes; buttons "Low", "Standard", "High" (the first start's tiers:
   Low = half resolution, every second frame, 8-bit; Standard = full, 8-bit; High = 16-bit, 16-bit dithering), each
   setting the four drop-downs below: Preview resolution, Preview frames, Preview samples, Preview dithering (shown
   with 16-bit samples only), the View menu's words. Pre-set from the settings, so the first start's choice for the
   machine.
2. "MP4 video export" (absent when OpenH264 is compiled in): why the codec is needed, Cisco's notice, the check box
   "Use Cisco's OpenH264 codec (downloaded from Cisco at the first MP4 export)", clear unless already enabled.
3. "Updates" (absent without the updater): check box "Look for a new version at each launch (Help menu)", ticked
   unless already declined. Ticked, OK or Examples... starts the first check.

## Updates (port, decision 0011)

With "Check for updates at launch" on, a check runs in the background at each launch; Help > Check for updates
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

Native save dialog (port: an in-window dialog with the format and the path, its "..." opening the platform save
dialog, which cannot report its filter; the probe CSV's is the platform's). The browser has no exports: their menu
entries are hidden.

| Command | Title | Filter list | Proposed name |
|---|---|---|---|
| Export field / intensity / both | "Export the field" / "Export the intensity" / "Export the field and the intensity" | MP4 video, H.264 (*.mp4); MPEG-1 video (*.mpg); AVI video, uncompressed (*.avi); QuickTime movie, Animation codec, lossless (*.mov); QuickTime movie, PNG codec, lossless (*.mov); PNG image sequence (*.png); JPEG image sequence (*.jpg); OpenEXR image sequence, 16-bit float data, intensity as I (*.exr); OpenEXR image sequence, 16-bit float data, intensity as sqrt(I) (*.exr) | `<project>_field` / `<project>_intensity`; both: the chosen name without suffix, each output gets its own |
| Export field / intensity frame | "Export the frame" | PNG image; JPEG image; OpenEXR image as I; OpenEXR image as sqrt(I) | `<project>_field_0655` (frame number padded like a sequence's files) |
| Export probe data as CSV | "Export the probe data" | CSV file (*.csv) | the probe's name |

- The chosen filter is the format (the two MOV formats share `.mov`); the extension follows it.
- Port: MP4 shows "OpenH264 Video Codec provided by Cisco Systems, Inc." and the check box "Use Cisco's OpenH264
  codec (downloaded from Cisco at the first MP4 export)" (setting `UseOpenH264`); OK is disabled while it is
  clear. Once downloaded, a "Delete the downloaded codec" button follows; the choice stays. The download runs on the export's worker before the first frame; a failure ends the export with its reason
  in the error box. A build with OpenH264 compiled in shows neither (decision 0005).
- The last filter index is remembered (`ExportFilterIndex`); a remembered video means PNG in the still dialog.
- Start folder rule, per kind (run export, still, probe data) and per project file: the first export after a
  project is opened starts in the project's folder under the project's name; every later one in the last completed
  export's folder (run export: last name with the layer suffix swapped). Kept in user settings (`ExportHints`, 20
  projects, only while their files exist), restored when the project is opened. A project without a file keeps the
  session's alone. Fallback: settings' `LastExportFolder`.
- A sequence goes into a folder of its own; an existing folder with files asks "Replace them?" (Export last asks
  nothing).
- After the dialog: when there is no run, an incomplete one, a reduced-quality one, or one lacking the intensity plane, a
  full-quality run is started first (status "Export: waiting for the run", indeterminate); then "Exporting n / N
  frames" with a determinate bar (tooltip "Cancel the export (Esc)"). Esc cancels, while waiting too (the run
  stops). A run that ends without completing while the export waits (Stop, an edit, a failure, F5 or Play
  replacing it) drops the export with a warning in the status bar. The status bar reports the end.
- A still renders exactly what a video export would show at that frame, never the preview's frame.
- CSV: a point probe one row per step ("step;amplitude" header), a line probe one row per frame
  ("frame;step;<distances>"), semicolons, decimal point. Full run first when needed.

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

Modal (id `ExamplesDialog`), 860 px wide, from File > Examples; the frame, dimmed backdrop and buttons of the
other dialogs. Title "Examples"; a tab strip of six topics (`ExamplesTab:<n>`: Sources and coherence 8, Lenses and
imaging 10, Gratings and filters 7, Metasurfaces and diffusers 5, Guides cavities and logic 18, Detectors and
statistics 5); a search box
(`ExamplesSearch`) filtering every tab by title and description, dimming the tabs without a match and moving to
the first with one; a 360 px scrolled list, one row per example (`Example:<file stem>`): the title, below it the
description wrapped in the dim text colour; Open and Cancel. Left/Right switch the tab (not while the search box
has the focus), Up/Down move the row, Enter or Open or a double-click on a row opens it, Esc or Cancel closes,
Tab cycles the controls. Opening closes the dialog and runs the guarded open (unsaved-changes prompt included).
Next's 33 projects come from its `Examples` folder, ours from [`examples/gates`](../../examples/gates/) and [`examples/optics`](../../examples/optics/)
(all `.wdproj`), all embedded.

## Manual window

- Opened by Help > Manual or F1. Not modal, owned by the main window: minimised/restored with it, stays above it,
  closed with it. Opened once; F1 again brings it to the front. Esc closes even with focus in the text. (port) In
  the browser it is a window inside the page, whose keys act while the focus is in it, or while it is on top with
  nothing focused.
- Size 1200 x 820 default, min 480 x 300. Title "WaveDude Next manual".
- Left: "Contents" tree (260 px, min 120) of the headings by level, every branch open; the title (the single `#`)
  is not a row; chapters are roots. Splitter (4 px, tab stop, arrows move it; tooltip "Drag or use the arrow keys
  to resize").
- Right: the document (min 200 px): MANUAL.md rendered (headings, paragraphs, tight lists, bold, links, code,
  figures). Scrolls by whole pixels; text selectable and copyable.
- Opens with the text at its top and the first chapter row taken (without scrolling); Up/Down take the previous or
  next row while the window has the keys.
- Taking a row scrolls so its heading stands at the top (padding below the top edge, previous section hidden).
  Clicking the already-taken row goes there again. Enter or Space on a focused row goes there; the focus stays.
- `[x](#anchor)` links go to the heading with GitHub's anchor rules; web links open in the browser.
- Figures: the SVG's embedded PNG drawn 1:1 when it fits the column (scaled down when wider), each badge a round
  numbered disc at its position with the badge's `<title>` as tooltip.
- A UI zoom change that changes text rendering reloads the document keeping the scroll position as a share of
  the height (text sharpens about 0.5 s later).
- Opening cost about 1-2 s in WPF; acceptable, but aim lower.

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
startup page and the update check, which wait for the next start. In the browser, where every panic ends the program,
the report is kept in the page's storage; the window shows its text, and Copy report puts the text on the
clipboard.

## First start

When no settings file exists:
- Read machine facts (memory, threads, GPU presence, whether the cache drive has a seek penalty) and choose:
  - 32 GB or more: 16-bit samples, dithering 16 bits.
  - Under 16 GB, or a cache drive that is a hard disk, or no GPU, or 4 threads or fewer: half resolution, every
    second frame, 8-bit samples.
  - Otherwise: full resolution, every frame, 8-bit samples.
- Write the settings file at once (deleting it redoes the choice); log one Info line naming facts and choice
  (shown in the status bar).
- Default dock layout, window 1280 x 800 centred, theme System, language System.

## File type registration (platform-specific)

Windows: per-user registration of `.wdproj` ("WaveDude Next project") and `.wdscheme` at every start (cheap when
unchanged), removing a user "Open with" override; failures are logged, never shown. Double-click starts the app
with the file. Port: desktop entry + MIME types on Linux, Info.plist on macOS, registry on Windows, all done by the
installer or package, never at start (decision 0008).
