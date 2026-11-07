# Dialogs and secondary windows

Pointers: `Help/AboutWindow.xaml(.cs)`, `Help/AboutInfo.cs`, `Help/ManualWindow.xaml(.cs)`,
`Build/MarkdownToFlowDocument.cs`, `MainWindow.Export.cs`, `Dialogs/MessageWindow.xaml(.cs)`,
`Dialogs/MemoryWarningWindow.xaml(.cs)`, `Controls/ColorPicker*.cs`, `Panes/MessagesPane.xaml(.cs)`,
`Logging/StatusBarTarget.cs`, `Logging/MessagesTarget.cs`, `FileAssociation.cs`, `App.xaml.cs`.

All windows use the theme (brushes, title bar), the UI zoom and the current language. Modal dialogs are owned by
the main window and not shown in the taskbar.

## Message window (app's own message box)

- Modal, 440 px wide, height to content, not resizable.
- A 32 px disc: red with "!" for an error (plus the error sound), accent with "?" for a question; the text
  wrapped beside it; buttons right-aligned at the bottom.
- Button sets: OK (Enter, Esc and close box all = OK); Yes (default) / No (Esc, close box); Save (default) /
  Don't save / Cancel (Esc, close box).
- Enter = default button, Esc = cancel button, mnemonics work without Alt. Labels from the string tables.
- Uses: open/save errors ("Could not open '{0}'.", "Could not save '{0}'."), "Save the changes to {0}?",
  "The folder {0} exists and holds files. Replace them?" (Yes/No), "The export failed.", image save error.
- An error dialog shows one line; the full detail (for an unreadable project/scheme/settings file: the file's text,
  first 64 K characters plus the line an XML error names) goes to the log and the Messages pane tooltip.

## Unsaved edits prompt

Before New, Open, Close, Exit, opening an example or recent file: "Save the changes to <name>?" Save (saves; Save
As for a project without a file; cancelled Save As aborts) / Don't save / Cancel (keeps the project, aborts).

## Memory warning

See `60-live-editing-and-runs.md`.

## Export Save dialog

Native save dialog (port: platform file dialog, e.g. rfd).

| Command | Title | Filter list | Proposed name |
|---|---|---|---|
| Export field / intensity / both | "Export the field" / "Export the intensity" / "Export the field and the intensity" | MP4 video, H.264 (*.mp4); MPEG-1 video (*.mpg); AVI video, uncompressed (*.avi); QuickTime movie, Animation codec, lossless (*.mov); QuickTime movie, PNG codec, lossless (*.mov); PNG image sequence (*.png); JPEG image sequence (*.jpg); OpenEXR image sequence, 16-bit float data, intensity as I (*.exr); OpenEXR image sequence, 16-bit float data, intensity as sqrt(I) (*.exr) | `<project>_field` / `<project>_intensity`; both: the chosen name without suffix, each output gets its own |
| Export field / intensity frame | "Export the frame" | PNG image; JPEG image; OpenEXR image as I; OpenEXR image as sqrt(I) | `<project>_field_0655` (frame number padded like a sequence's files) |
| Export probe data as CSV | "Export the probe data" | CSV file (*.csv) | the probe's name |

- The chosen filter is the format (the two MOV formats share `.mov`); the extension follows it.
- Port: MP4 shows "OpenH264 Video Codec provided by Cisco Systems, Inc." and the check box "Use Cisco's OpenH264
  codec (downloaded from Cisco at the first MP4 export)" (setting `UseOpenH264`); OK is disabled while it is
  clear. The download runs on the export's worker before the first frame; a failure ends the export with its reason
  in the error box. A build with OpenH264 compiled in shows neither (decision 0005).
- The last filter index is remembered (`ExportFilterIndex`); a remembered video means PNG in the still dialog.
- Start folder rule, per kind (run export, still, probe data) and per project file: the first export after a
  project is opened starts in the project's folder under the project's name; every later one in the last export's
  folder (run export: last name with the layer suffix swapped). Kept in user settings (`ExportHints`, 20 projects,
  only while their files exist), restored when the project is opened. A project without a file keeps the
  session's alone. Fallback: settings' `LastExportFolder`.
- A sequence goes into a folder of its own; an existing folder with files asks "Replace them?" (Export last asks
  nothing).
- After the dialog: when there is no run, a stale one, a reduced-quality one, or one lacking the intensity plane, a
  full-quality run is started first (status "Export: waiting for the run", indeterminate); then "Exporting n / N
  frames" with a determinate bar (tooltip "Cancel the export (Esc)"). Esc cancels. The status bar reports the end.
- A still renders exactly what a video export would show at that frame, never the scratch frame.
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
   Shown: LICENSE.md rendered as a document (headings, lists, links) in a scrollable box (260 px, min 120);
   a taller window gives its extra height to the document; hidden: window shrinks back.
4. "System": read-only text box (max 160 px, scrolls) with eight lines: Program, Runtime, Operating system,
   Processor (+ logical cores), Memory ("x free of y"), GPU (OpenCL) setting (on/off), OpenCL devices (or "OpenCL
   is not installed"), H.264 encoders (or "Media Foundation is not available" / "none").
Port, between 3 and 4: "OpenH264": Cisco's notice and the toggle "Show the OpenH264 license" / "Hide the OpenH264
   license" (O), showing Cisco's licence text verbatim, as plain text, in the same box; absent when OpenH264 is
   compiled in.
5. Buttons: "Copy system info" (C; tooltip "Copy the system lines to the clipboard, for a bug report"; status
   "System information copied to the clipboard"), "Close" (default and cancel).

Port: H.264 encoder line becomes whatever encoder the export crate uses (openh264, decision 0005); OpenCL line
becomes the wgpu adapter(s).

## Manual window

- Opened by Help > Manual or F1. Not modal, owned by the main window: minimised/restored with it, stays above it,
  closed with it. Opened once; F1 again brings it to the front. Esc closes even with focus in the text.
- Size 1200 x 820 default, min 480 x 300. Title "WaveDude Next manual".
- Left: "Contents" tree (260 px, min 120) of the headings by level, every branch open; the title (the single `#`)
  is not a row; chapters are roots. Splitter (4 px, tab stop, arrows move it; tooltip "Drag or use the arrow keys
  to resize").
- Right: the document (min 200 px): MANUAL.md rendered (headings, paragraphs, tight lists, bold, links, code,
  figures). Scrolls by whole pixels; text selectable and copyable.
- Opens with the text at its top and the first chapter row taken (without scrolling), focus in the tree.
- Taking a row scrolls so its heading stands at the top (padding below the top edge, previous section hidden).
  Clicking the already-taken row goes there again. Enter on a row goes there and moves focus into the text.
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
- Right: run status (`50-timeline-transport.md`).
- Far right: busy text + 120 px progress bar (export frames determinate; waiting for run / computing a frame
  indeterminate); hidden when idle.
- Tooltip "Double-click to open the Messages pane"; double-click opens or focuses it.

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
