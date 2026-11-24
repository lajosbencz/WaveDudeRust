# Parity with WaveDude NextNone done | enabled while a run or an export is active; the menu, Shift+F5 and Esc (window level) cancel a running export, else stop the run |None partial | Esc cancels a running export, else stops the run, when nothing focused took it; the number box reverts its edit (plain text fields do not: Esc passes on, the edit commits on blur); drag cancel comes with the handles (piece 5) |None done | app.slint `preview-keys`: a FocusScope over the pane, focused by a click on the pictures, plain keys only; the speed box swallows Space (timeline.slint) |None done | handled by the timeline when it is focused, else by app.slint `preview-keys` |

The acceptance list for the Slint app ([`rebuild.md`](rebuild.md)): every user-visible feature of WaveDude Next, from
[`ui/`](ui/) and Next's source (`$WAVEDUDE_NEXT_DIR`), with its state in
[`frontends/waveduderust_slint`](../frontends/waveduderust_slint/). Only [`divergences.md`](divergences.md) and the
decisions excuse a difference. A feature is done when it is checked against Next side by side; the row's status changes
in the commit that ports it.

Statuses: done, partial (the note says what is missing), missing, divergence; "not in spec" marks what Next has and
[`ui/`](ui/) does not. Built on 2026-11-24 from reading the code; partial rows marked "unverified" are still to be run.

| Prefix | Area | done | partial | missing | divergence | other |
|---|---|---|---|---|---|---|
| X | Cross-cutting | 6 | 12 | 2 | 20 | 0 |
| M | Menus and shortcuts | 38 | 25 | 43 | 34 | 0 |
| D | Dialogs and windows | 18 | 29 | 24 | 8 | 0 |
| S | Settings | 7 | 10 | 12 | 3 | 0 |
| I | Icons | 13 | 28 | 23 | 19 | 0 |
| T | Project tree | 33 | 18 | 12 | 38 | 0 |
| P | Properties | 286 | 25 | 11 | 3 | 0 |
| V | Preview | 8 | 8 | 91 | 4 | 1 |
| L | Timeline, transport, status | 27 | 33 | 20 | 7 | 3 |
| R | Live editing and runs | 16 | 13 | 30 | 0 | 2 |
| G | Messages pane | 8 | 10 | 11 | 3 | 0 |
| | All | 460 | 211 | 279 | 139 | 6 |

## Cross-cutting

| ID | Feature | Next source (file:line) | Spec (docs/ui file, section) | Slint | Note |
|---|---|---|---|---|---|
| X-01 | Window title `<app> - <file>` + `*` for unsaved edits; suggested name; `Untitled`; app name alone with no project | MainWindow.xaml.cs | 00, Main window | done | app name "WaveDude Rust" (rename, not a divergence entry) |
| X-02 | Title shows the full path by setting | MainWindow.xaml.cs | 00, Main window; decision 0008 | done | app.slint `title` from `file-path` while `title-full-path` |
| X-03 | Window min 800x500, default 1280x800, centred on first start | MainWindow.xaml | 00, Main window | done | |
| X-04 | Window size, position, state persisted; never minimised; kept inside the monitor's work area | MainWindow.xaml.cs | 00, Main window; 80 | missing | settings fields exist in waveduderust_platform, unused |
| X-05 | Resize grip | MainWindow.xaml | 00, Main window | divergence | decision 0033, matched in general, not to the pixel/id |
| X-06 | Dock layout tree: splits with star weights, tabbed groups, fixed centre | Docking/DockHost.cs | 00, Panes; 90 s1 | divergence | decision 0033, dropped: fixed layout, View > Panes shows/hides |
| X-07 | Splitters between panes: drag and arrow keys | Docking/DockHost.cs | 00, Panes; 90 s1 | done | ui/splitter.slint: 4 px bars, drag, Next's minimums (60 px, 100 px for the preview); no keys, as Next's are not focusable; sizes per divergences.md, Pane sizes |
| X-08 | Pane title bar 21 px: title, dotted grip, "Window position" drop-down, close button | Docking/DockGroupControl.cs, Themes/Controls.xaml | 00, Panes; 90 s1 | divergence | decision 0033, dropped: fixed layout, View > Panes shows/hides |
| X-09 | "Window position" menu: Move to Left/Right/Top/Bottom, Close | Docking/DockCommands.cs | 00, Panes | divergence | decision 0033, dropped: fixed layout, View > Panes shows/hides |
| X-10 | Active pane title bar accent + white text + light-blue grip by focus; first group active at start | Docking/DockGroupControl.cs | 00, Panes | divergence | decision 0033, dropped: fixed layout, View > Panes shows/hides |
| X-11 | Tab strip for groups with several panes | Docking/DockGroupControl.cs | 00, Panes; 90 s1 | divergence | decision 0033, dropped: fixed layout, View > Panes shows/hides |
| X-12 | Drag pane/group to dock: edge strip, split beside, tab into; accent drop preview; ghost label; Esc cancels | Docking/DockDragManager.cs, DockDropAdorner.cs | 00, Panes (port rules) | divergence | decision 0033, dropped: fixed layout, View > Panes shows/hides |
| X-13 | Close hides the pane; reshown at its default side without a saved place | Docking/DockHost.cs | 00, Panes | partial | hide/show works; no sides |
| X-14 | Default arrangement: left column 1:3, Project over Properties 1:2; Messages hidden | Docking/DockHost.cs | 00, Panes | partial | ratios done, sized per divergences.md, Pane sizes; Messages placement differs (bottom of centre?) |
| X-15 | Layout saved as it changes and restored at start; unknown ids ignored | Docking/DockHost.cs | 00, Panes | divergence | decision 0033, dropped: fixed layout, View > Panes shows/hides |
| X-16 | View > Panes lists Project, Properties, Messages, Preview with check marks | MainWindow.xaml | 00, Panes; decision 0008 | partial | Preview not listed |
| X-17 | Status bar double-click opens or focuses Messages | MainWindow.xaml.cs | 00, Panes | partial | opens; no focus when shown |
| X-18 | No floating windows | Docking | 00, Panes | done | |
| X-19 | Status bar 22 px, padding 4,0,16,0: last log line, run status, busy text + 120x12 progress bar, frame cache | MainWindow.xaml | 00, Main window | partial | log line + run status only; no busy/progress, no frame cache item |
| X-20 | Status bar warn/error colouring | Themes/Dark.xaml StatusBarWarn*/Error* | 00, Look | missing | the warn token (StatusBarWarnBackground #ca5100) comes with the feature; status-error exists |
| X-21 | Theme tokens: 92 keys per theme (menu, title bars, tabs, tree, scroll bars, splitter, drop indicator, buttons, tooltips, text/combo/check boxes, log levels) | Themes/Dark.xaml, Light.xaml | 00, Look | divergence | ui/theme.slint: the used colours with Next's values (divergences.md, Theme tokens) |
| X-22 | Themes System/Light/Dark, live switch, native title bar follows | Theming/ThemeController.cs | 00, Look | partial | live switch done; native title bar not verified |
| X-23 | Font Segoe UI 12 px, else bundled Selawik + Noto Sans Math subset | App.xaml | 00, Look | divergence | Selawik everywhere (divergences.md, Font); Noto Sans Math is the overlays' (core) |
| X-24 | Row 23 px (tree rows, toolbar buttons, menu bar, tabs); drop-down entries 22 px | Themes/Controls.xaml | 00, Look; 90 | divergence | decision 0033, matched in general, not to the pixel/id |
| X-25 | Icons: 16 px stroked drawings in text colour, grey when disabled | Themes/Controls.xaml, Assets | 00, Look | divergence | decision 0033, matched in general, not to the pixel/id |
| X-26 | Keyboard focus = accent border everywhere | Themes/Controls.xaml | 00, Look; 90 | partial | std widgets' own focus look |
| X-27 | Square corners; WPF box model paddings (buttons 10,3; toolbar 6,1,6,0; text 3,2) | Themes/Controls.xaml, Editors.xaml | 90, cross-cutting | divergence | decision 0033, matched in general, not to the pixel/id |
| X-28 | Tooltips on every toolbar/transport button | Themes/Controls.xaml ToolTip style | 90, cross-cutting | divergence | every IconButton's tip through Slint's Tooltip (divergences.md, Tooltips) |
| X-29 | Every control reachable by Tab and arrows | - | 00, Keyboard first | partial | tree, toolbars, timeline not all tab stops |
| X-30 | Menu bar out of Tab cycle; F10/Alt enters; mnemonics unique per menu | MainWindow.xaml, Strings | 00, Keyboard first | divergence | Windows' native bar: F10/Alt and Next's access keys; Slint's drawn bar (Linux, web): none (docs/blockers.md) |
| X-31 | UI zoom 0.5x..3x: Ctrl+=, Ctrl+-, Ctrl+0, Settings > General; persisted `UiZoom`; touch default 2x | Theming/WindowZoom.cs | 00, UI zoom | divergence | decision 0033, dropped |
| X-32 | `--zoom=<f>` switch; unknown `--` switches logged | App.xaml.cs | 00, Startup | divergence | decision 0033, dropped |
| X-33 | Per-monitor DPI; lines on whole device pixels | App.manifest | 00, UI zoom | divergence | decision 0033, matched in general, not to the pixel/id |
| X-34 | Languages EN/DE/FR/NL + System; live switch; persisted `Language` | Strings/Strings.*.xaml | 00, Languages | partial | `@tr` with Slint's bundled gettext tables from Next's (tools/next-strings.py, lang/); live switch and the system language through the Settings row Language (decision 0027); our texts Next lacks (welcome page, Settings window) and strings made in Rust (property and settings labels, tree names, status words) stay English; on the web System reads the browser's language |
| X-35 | British spelling except "license" | Strings.EN.xaml | 00, Languages | partial | not audited |
| X-36 | First argument opens .wdproj, configuration.json (import) or .wdscheme | App.xaml.cs | 00, Startup | partial | .wdproj and json; .wdscheme not handled |
| X-37 | File type registration (.wdproj, .wdscheme), jump list | App.xaml.cs | 00, Platform-specific | divergence | divergences.md "Windows integrations": file types from packaging, no jump list |
| X-38 | First start: preview quality chosen from the machine | App.xaml.cs | 00, Startup; 70 | partial | welcome presets exist; no automatic first-start choice verified |
| X-39 | Status bar first text "Ready" or last start-up log line | MainWindow.xaml.cs | 00, Startup | done | |
| X-40 | Click on title bar while a menu is open closes it and lands | Theming/MenuClickThrough.cs | 00, Platform-specific | divergence | divergences.md "Windows integrations" |

## Menus, dialogs, settings, icons

Abbreviations. Next paths are under `$WAVEDUDE_NEXT_DIR`:
WPF = WaveDudeNext.WPF; MW = WPF/MainWindow.xaml; PT = WPF/Panes/ProjectTree.xaml; PP = WPF/Panes/PreviewPane.xaml;
MP = WPF/Panes/MessagesPane.xaml; CX = WPF/Themes/Controls.xaml; SE = WPF/Strings/Strings.EN.xaml; Cmd = WPF/Commands/*.cs;
UIS = WaveDudeNext.UI/Settings.cs. Slint: app = frontends/waveduderust_slint/ui/app.slint; main = src/main.rs (same crate dir); set = src/settings.rs.
Spec: ui/10 = docs/ui/10-menus-and-shortcuts.md, ui/70 = 70-dialogs-and-windows.md, ui/80 = 80-settings.md. Decisions: 0027 settings window, 0028 no manual, 0008 UI answers, 0025 browser exports, 0030 undo cap.
Status: done / partial / missing / divergence. "not in spec" = found in Next, absent from docs/ui.

Global gaps (apply to EVERY menu row below unless the Slint cell says otherwise; not repeated per row):
- G1 access keys: Next's per language on Windows' native menu bar (`menu-text`, tools/next-strings.py); none on Linux and the web, where Slint draws the bar (docs/blockers.md). Titles translate through `@tr` (X-34).
- G2 icons: Material Symbols per theme (`MenuIcons`, divergences.md Icons), see section I.
- G3 enabled rules: `Commands` (State::commands) carries Next's CanExecute rules for the menu bar; entries without a rule are noted per row.
- G4 modal greying: menu bar is not greyed or blocked while a modal shows (app.slint has no such binding).
- G5 shortcut text: native MenuBar shows only entries that declare `shortcut:`; Edit/Delete keys come from the FocusScope (app.slint:160-172), so the text is not shown.

### M: menus and shortcuts

#### M File menu
| ID | Feature | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| M-01 | File menu header, Mn F/D/F/D (EN F) | MW:217, SE:28 | ui/10 File | divergence | access keys on Windows' native bar (`menu-text`, Next's per language), none on Linux and the web (docs/blockers.md) |
| M-02 | New Project, Ctrl+N, always on, asks unsaved edits then blank project (open edges, rim -32, no sources), timeline frame 0 | MW:251, SE:29 | ui/10 File #1 | done | app:95, main.rs "new" arm, dirty prompt via pending. Not checked: blank-project defaults (rim -32) come from core Document::new |
| M-03 | Open Project..., Ctrl+O, asks unsaved edits; dialog filters: WaveDude Next (*.wdproj), Wave Dude 2D (configuration.json;*.json), XML (*.xml), All files | MW:252, SE:30, SE:440 | ui/10 File #2 | partial | app:96. files.rs:9-10 filters: "WaveDude project" (wdproj, xml) and "Wave Dude 2D" (json) only: no "All files", labels differ; configuration.json import goes via legacy::import_str (document.rs:60) done |
| M-04 | Recent Projects > : one entry per recent file, newest first, full path plain text, click opens; empty submenu when none | MW:253-255, SE:31 | ui/10 File #3 | done | app.slint `recent-files` from Settings.recent_files, full path, newest first; "recent:<i>" opens after the unsaved-edits question; none on the web (no paths) |
| M-05 | Examples... entry (spec: dialog of 53 projects; Next: File > Examples submenu of its shipped examples, tooltip=description) | MW:257-259, SE:32 | ui/10 File #4 | divergence | app:98 opens dialog (D-section); divergence row "Examples" in docs/divergences.md |
| M-06 | Welcome Page... entry (port), Mn W/K/A/W | not in Next | ui/10 File #4a | done | main.rs "welcome"; &W on Windows (English only: not in Next's tables) |
| M-07 | Settings... entry (port), Ctrl+, , Mn T/I/M/I | not in Next | ui/10 File #4b | done | app:100 Control+"," ; decision 0027 |
| M-08 | Separator after Settings | MW:260 | ui/10 File | done | app:101 |
| M-09 | Save, Ctrl+S, Save As when no file | MW:261, SE:33 | ui/10 File #5 | done | app:102, main.rs save(); web: download |
| M-10 | Save As..., Ctrl+Shift+S, filters *.wdproj *.xml, proposes suggested name beside origin | MW:244-245 (InputBinding), MW:262, SE:34, SE:441 | ui/10 File #6 | partial | app:103. Slint dialog dialog() uses the open filters (json too, no xml-only save list); suggested name only as file name, not "beside its origin" folder (main.rs save()) |
| M-11 | Export > submenu header, Mn E/E/X/E | MW:264, SE:36 | ui/10 File #7 | divergence | access keys on Windows' native bar (`menu-text`, Next's per language), none on Linux and the web (docs/blockers.md) |
| M-12 | Export field..., Ctrl+Shift+E, enabled when no export running and Steps>0 | MW:265, SE:37, Cmd/ExportCommands.cs:35 | ui/10 File>Export | partial | enabled: no export running and Steps > 0 (State::commands); dialog see D-section |
| M-13 | Export intensity..., Ctrl+Shift+I | MW:266, SE:38, ExportCommands.cs:39 | ui/10 File>Export | partial | enabled: no export running and Steps > 0 (State::commands); dialog see D-section |
| M-14 | Export both..., Ctrl+Shift+B, two outputs `_field`/`_intensity` | MW:267, SE:39, ExportCommands.cs:43 | ui/10 File>Export | partial | enabled as M-12; both layers one name via platform export; suffix rule not verified |
| M-15 | Separator in Export submenu | MW:269 | ui/10 File>Export | done | app:110 |
| M-16 | Export field frame..., Ctrl+Shift+F, the frame shown as one image | MW:271, SE:40, ExportCommands.cs:47 | ui/10 File>Export | partial | enabled as M-12; Kind::Frame uses playback.position; formats png/jpg/exr via supports_still |
| M-17 | Export intensity frame..., Ctrl+Shift+G | MW:272, SE:41, ExportCommands.cs:51 | ui/10 File>Export | partial | enabled as M-12; Kind::Frame uses playback.position; formats png/jpg/exr via supports_still |
| M-18 | Export last, Ctrl+E, enabled after first completed run export since open; repeats layers/format/path with no dialog, replaces silently; stills/cancelled/failed do not count | MW:275, SE:42, ExportCommands.cs:59 | ui/10 File #8 | partial | enabled after a run export of this project (cleared on open/new/close) and as M-12; repeats the kind only, not the path; set at start, not completion |
| M-19 | Close Project, Mn C/C/F/L, enabled unless placeholder project, asks unsaved edits, then empty project | MW:277, SE:35 | ui/10 File #9 | partial | enabled unless the blank project (divergences.md Menus: Next needs a file); shares "new" |
| M-20 | Exit, Alt+F4, Mn X/B/Q/F, asks unsaved edits | MW:279, SE:49 | ui/10 File #10 | partial | app:118 no shortcut text; Alt+F4 is the window manager's; close box handled via on_close_requested (main.rs:~600): prompt only when dirty. Missing: "An export is running. Stop it and quit?" prompt (ui/70) |
| M-21 | Separators: after Settings, after Save As, after Export last, after Close | MW:260,263,276,278 | ui/10 File | done | app:101,104,115,117 |

#### M Edit menu
| ID | Feature | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| M-22 | Edit header, Mn E/B/B/B | MW:281, SE:50 | ui/10 Edit | divergence | access keys on Windows' native bar (`menu-text`, Next's per language), none on Linux and the web (docs/blockers.md) |
| M-23 | Undo, Ctrl+Z, enabled when a step exists; a focused text box keeps its own Ctrl+Z | MW:282, SE:51, EditCommands.cs:37 | ui/10 Edit | partial | enabled can-undo; Ctrl+Z is a window key a focused text field takes first, no shortcut text (divergences.md Menus) |
| M-24 | Redo, Ctrl+Y and Ctrl+Shift+Z | MW:283, SE:52, EditCommands.cs:41 | ui/10 Edit | partial | enabled can-redo; Ctrl+Y and Ctrl+Shift+Z as M-23 |
| M-25 | Separator | MW:284 | ui/10 Edit | done | app:124 |
| M-26 | Cut, Ctrl+X, enabled when list items selected, copies then removes | MW:285, SE:53 | ui/10 Edit | done | copies the group (Explorer's rule, src/selection.rs) then removes its list items, only when the clipboard took it |
| M-27 | Copy, Ctrl+C, objects to clipboard as XML text + app format | MW:286, SE:54 | ui/10 Edit | partial | the group or a settings object as `.wdproj` fragment text (arboard; navigator.clipboard on the web); no app format (divergences.md Clipboard) |
| M-28 | Paste, Ctrl+V, enabled when clipboard parses; paste behind current; settings object replaces | MW:287, SE:55 | ui/10 Edit | divergence | after the current item of its list, else at the end, under a copy name when the name is taken and a fresh id (Project::paste_*); a settings object replaces on its row or the root; always enabled (divergences.md Clipboard); the last pasted becomes current |
| M-29 | Delete, Del, enabled as Cut, removes selection (text box: its text) | MW:288, SE:56, InputGestureText "Del" | ui/10 Edit | done | removes the group (Explorer's rule) and keeps the user at the last one's slot (tree::remove_all); Del as a window key a text field takes first |
| M-30 | Separator before Select all | MW:289 | ui/10 Edit | done | app:129 |
| M-31 | Select all, Ctrl+A, enabled when project has items; every source/part/probe, last current; tree and preview | MW:291, SE:184 (MenuProjectSelectAll), ProjectCommands.cs:57 | ui/10 Edit | done | every source, part and probe, the last current (src/selection.rs select_all); enabled when there is one; Ctrl+A from the tree and the preview |

#### M View menu
| ID | Feature | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| M-32 | View header, Mn V/A/A/A | MW:293, SE:57 | ui/10 View | done | app:132 |
| M-33 | Panes > submenu: one checkable entry per pane (Preview, Project, Properties, Messages), check = shown, click toggles | MW:294-296, SE:58, MW PaneMenuItemStyle:67-73 | ui/10 View | partial | app:134-139 has Project, Properties, Messages; no Preview entry (preview cannot be hidden); no dock host |
| M-34 | View > Theme > (System/Light/Dark, checkmark) | MW:297-299, SE:70, MW ThemeMenuItemStyle:113-125 | ui/10 View note | divergence | moved to Settings (decision 0027); see S-04 |
| M-35 | View > UI zoom > 0.5x .. 3x (8 entries, check) | MW:302-327, SE:71-79 | ui/10 View note | divergence | moved to Settings; Slint has no UI zoom at all: see S-15, M-111 |
| M-36 | View > Language > (System + each) | MW:328-330, SE:80, MW LanguageMenuItemStyle:76-88 | ui/10 View note | divergence | moved to Settings > General > Language (decision 0027, X-34) |
| M-37 | View > Preview resolution > Full/Half/Quarter (check) | MW:334-344, SE:81-84 | ui/10 View note | divergence | moved to Settings (S-10) |
| M-38 | View > Preview frames > Full/Half (every second)/Quarter (every fourth) | MW:345-355, SE:85-88 | ui/10 View note | divergence | moved to Settings (S-11) |
| M-39 | View > Preview samples > 16-bit float / 8-bit palette index | MW:356-363, SE:89-91 | ui/10 View note | divergence | moved to Settings (S-12) |
| M-40 | View > Preview dithering > Off/10/12/14/16 bits, disabled unless 16-bit samples | MW:366-383, SE:92-97 | ui/10 View note | divergence | moved to Settings (S-13) |
| M-41 | There is no Display menu | n/a | ui/10 View | done | none in Slint |

#### M Simulation menu
| ID | Feature | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| M-42 | Simulation header, Mn S/S/S/S | MW:385, SE:99 | ui/10 Simulation | divergence | access keys on Windows' native bar (`menu-text`, Next's per language), none on Linux and the web (docs/blockers.md) |
| M-43 | Run, F5, enabled: no run active, no export; runs from step 0, preview follows (Live); may ask memory warning | MW:386, SE:100, SimulationCommands.cs:30 | ui/10 Simulation | done | enabled: no run active, no export (Next's CanRun); F5; memory warning is D-21 |
| M-44 | Pause / Resume, F6, enabled while run active; header reads Resume while paused | MW:388-399, SE:101-102, SimulationCommands.cs:34 | ui/10 Simulation | done | enabled while a run is active; reads Resume while paused; F6 |
| M-45 | Stop, Shift+F5 and Esc, enabled while run or export active; Esc cancels export first else stops run | MW:400, SE:103, SimulationCommands.cs:44 | ui/10 Simulation | done | enabled while a run or an export is active; Shift+F5; Esc at window level ("escape": the export first, else the run) |
| M-46 | Discard frames (port), Mn D/V/S/V, enabled when run has frames and export not holding them; asks "Discard the frames of the run?" Yes/No (No default); drops frames, timeline to 0 | not in Next | ui/10 Simulation | missing | entry removed until discarding exists |
| M-47 | Separator | MW:401 | ui/10 Simulation | done | app:147 |
| M-48 | Play / Pause playback, F7; header and icon switch while playing; enabled CanPlayPause | MW:404-417, SE:104-105, PreviewCommands.cs:40 | ui/10 Simulation | done | Play / Pause playback by playback state, icon too; enabled with more than one frame; F7 |
| M-49 | Simulation > Use the GPU (OpenCL) checkable | MW:420-422, SE:106 | ui/10 Simulation note | divergence | moved to Settings (S-14) |
| M-50 | Simulation > Re-run automatically after edits checkable | MW:423-425, SE:107 | ui/10 note | divergence | moved to Settings (S-16) |
| M-51 | Simulation > Warn when a run does not fit in RAM checkable | MW:428-430, SE:108 | ui/10 note | divergence | moved to Settings (S-17) |
| M-52 | Simulation > RAM cache size > Auto, 10..100 % of RAM | MW:433-435, SE:109-110, MW FrameMemoryMenuItemStyle:92-112 | ui/80 (v2 drops FrameMemoryPercent) | divergence | replaced by FrameMemoryBytes (browser only), spec ui/80 + ui/70 Settings RAM row (S-09) |

#### M Help menu
| ID | Feature | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| M-53 | Help header, Mn H/H/A/H | MW:437, SE:111 | ui/10 Help | divergence | access keys on Windows' native bar (`menu-text`, Next's per language), none on Linux and the web (docs/blockers.md) |
| M-54 | About WaveDude (Next/Rust) entry, Mn A/U-uml/P/O | MW:438, SE:112 | ui/10 Help | partial | app:152 opens text modal (D-52); entry shortcut none |
| M-55 | Manual, F1 (Next) | MW:439, SE:113 | ui/10 Help | divergence | replaced by Project page on GitHub (decision 0028, divergences.md "Manual") |
| M-56 | Project page on GitHub (port), F1, opens github.com/lajosbencz/WaveDudeRust (system browser / new tab) | not in Next | ui/10 Help | done | "github" opens the repository URL in the system browser, a new tab on the web (webbrowser crate); F1 |
| M-57 | Check for updates (port; builds with updater), Mn U/U/J/U | not in Next | ui/10 Help, ui/70 Updates | divergence | decision 0033, dropped |

#### M Context menus
| ID | Feature | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| M-58 | Tree row menu: Add source, Mn S/Q/R/B, icon U+E710 | PT:256, SE:148 | ui/10 Tree row | divergence | tree.slint rows-menu: Add source, add icon; no access keys or key hints (divergences.md Row menu) |
| M-59 | Tree row menu: Add part > submenu, Mn R/B/C/O | PT:257, SE:154 | ui/10 Tree row | divergence | tree.slint rows-menu: Add part > Next's kinds (tree.slint AddKinds, shared with the toolbar); no access keys or key hints (divergences.md Row menu) |
| M-60 | Tree row menu: Add probe (Next: Add probe > Standard / Probabilistic > Counting, Imaging), Mn B/O/N/S | PT:258, PT:150-157, SE:149-153 | ui/10 Tree row; ui/20 l.19,165 | divergence | tree.slint rows-menu: Add probe > Standard / Probabilistic > Counting, Imaging; no access keys or key hints (divergences.md Row menu) |
| M-61 | Tree row menu: Add to or remove from selection, Ins, U+E762, Mn L/T/I/R | PT:260, SE:185 | ui/10 Tree row | divergence | tree.slint rows-menu: Add to or remove from selection, enabled on a source, part or probe; no access keys or key hints (divergences.md Row menu) |
| M-62 | Tree row menu: Select all, Ctrl+A, U+E8B3 | PT:261, SE:184 | ui/10 Tree row | divergence | tree.slint rows-menu: Select all, enabled when there is an item; no access keys or key hints (divergences.md Row menu) |
| M-63 | Tree row menu: Enabled (Space), toggles Enabled/Active, group flips each | PT:263, SE:179 | ui/10 Tree row | divergence | tree.slint rows-menu: Enabled flips each of the group; no access keys or key hints (divergences.md Row menu) |
| M-64 | Tree row menu: Duplicate, Ctrl+D, U+F413, copy after each original, caret in last name | PT:264, SE:180 | ui/10 Tree row | divergence | tree.slint rows-menu: Duplicate, a copy after each original of the group; the caret is not put in Name (T-71); no access keys or key hints (divergences.md Row menu) |
| M-65 | Tree row menu: Cut / Copy / Paste (no gesture text) | PT:265-267 | ui/10 Tree row | done | tree.slint rows-menu: Cut / Copy / Paste, no gesture text, as Next |
| M-66 | Tree row menu: Remove, Del, U+E738 | PT:268, SE:181 | ui/10 Tree row | divergence | tree.slint rows-menu: Remove the group (tree::remove_all); no access keys or key hints (divergences.md Row menu) |
| M-67 | Tree row menu: Move up Alt+Up U+E74A / Move down Alt+Down U+E74B, enabled where item can move | PT:270-271, SE:182-183 | ui/10 Tree row | divergence | tree.slint rows-menu: Move up / Move down, enabled where the item can move; no access keys or key hints (divergences.md Row menu) |
| M-68 | Tree probe row: Export probe data as CSV..., Mn X/X/V/G, active probe and no export running | PT:274-275, SE:48, ExportCommands ProbeCsv | ui/10 Tree row | missing | the live run keeps no probe recordings yet (probe graphs piece); the core writes them (export/csv.rs) |
| M-69 | Tree toolbar "+ Source", "+ Probe v", "+ Part v" buttons with icon+plus badge and drop-down arrow; Probe/Part open kind menu | PT:178-208, SE tips TipAddSource etc | ui/10 Add part, ui/20 | done | tree.slint AddButton x3 with the folder drawing and plus badge; Probe and Part open their menus of kinds (T-01..T-06) |
| M-70 | Tree folder "+" buttons (Parts, Probes open kind menus) | PT:333-345 | ui/10 Add part, ui/20 | done | tree.slint row add button: Sources adds, Parts and Probes open their kind menus |

#### M Add part menu (tree toolbar, folder plus, row menu)
Kind descriptions are the entries' tooltips (Kind.<Type>). All entries missing in Slint (no add menu; no add-part action in tree.rs/main.rs).
| ID | Feature | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| M-71 | Lens/Mirror, Mn L/L/L/L, icon Lens | PT:116, SE:155 | ui/10 Add part | missing | |
| M-72 | Fresnel lens, Mn "Fre_snel" S | PT:117, SE:156 | ui/20 l.71,163 (not in ui/10 list) | missing | not in ui/10 |
| M-73 | Fibre, Mn F/F/F/V | PT:118, SE:162 | ui/10 | missing | |
| M-74 | Bragg reflector, B | PT:119, SE:163 | ui/10 | missing | |
| M-75 | GRIN lens, G | PT:120, SE:164 | ui/10 | missing | |
| M-76 | Zone plate, Z | PT:121, SE:165 | ui/10 | missing | |
| M-77 | Diffuser, D | PT:122, SE:166 | ui/20 l.71 (not in ui/10 list) | missing | not in ui/10 |
| M-78 | Interference filter, Mn N | PT:123, SE:167 | ui/20 (not in ui/10 list) | missing | not in ui/10 |
| M-79 | Diffraction gratings > submenu, Mn C | PT:125, SE:168 | ui/20 l.163 (not in ui/10) | missing | not in ui/10 |
| M-80 | Diffraction gratings > Simple 2D grating, S | PT:126, SE:169 | ui/20 | missing | not in ui/10 |
| M-81 | Diffraction gratings > Facetted grating, F | PT:127, SE:170 | ui/20 | missing | not in ui/10 |
| M-82 | Meta surfaces > submenu, Mn M | PT:130, SE:171 | ui/20 | missing | not in ui/10 |
| M-83 | Meta surfaces > Meta lens, L | PT:131, SE:172 | ui/20 | missing | not in ui/10 |
| M-84 | Meta surfaces > Meta prism, P | PT:132, SE:173 | ui/20 | missing | not in ui/10 |
| M-85 | Meta surfaces > Meta axicon, A | PT:133, SE:174 | ui/20 | missing | not in ui/10 |
| M-86 | Separator, then shapes: Polygon, Rectangle, Ellipse, Triangle, Outline (Mn P/R/E/T/O) | PT:135-140, SE:157-161 | ui/10 | missing | 5 entries |
| M-87 | Optical logic > submenu, Mn I/O/I/I | PT:143, SE:175 | ui/10 | missing | |
| M-88 | Optical logic > XOR (Mach-Zehnder), AND, OR (Mn X/A/O) | PT:144-146, SE:176-178 | ui/10 | missing | 3 entries |
| M-89 | Add probe menu: Standard probe (S), Probabilistic > Counting detector (C), Imaging detector (I); tooltips Detector.* | PT:150-157, SE:150-153 | ui/20 l.19,165 (not in ui/10) | divergence | tree.slint ProbeMenu; no access keys or tooltips (divergences.md Add menus) |
| M-90 | "+ Part" keyboard: Space opens menu, arrows walk, Enter picks | PT:194-208 | ui/10 Add part | missing | |

#### M Preview picture context menu (both pictures, parameterised by layer)
| ID | Feature | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| M-91 | Right-click / Apps on a picture cell opens menu | PP:728-745, PP:754-771 | ui/10 Preview picture | missing | preview.slint Picture has no menu |
| M-92 | Brightness (focuses that picture's brightness box), U+E706, Mn B/H/L/H | PP:736,762, SE:44 | ui/10 | missing | |
| M-93 | Auto brightness, Draw:AutoBrightness, Mn A | PP:737,763, SE:45, DisplayCommands AutoBrightness | ui/10 | missing | |
| M-94 | Export this frame..., Ctrl+Shift+F (field) / Ctrl+Shift+G (intensity), U+EB9F, Mn T/D/E/D | PP:739,765, SE:43 | ui/10 | missing | |
| M-95 | Export field... / intensity... / both... (Ctrl+Shift+E/I/B), U+EDE1 | PP:741-743,767-769 | ui/10 | missing | same actions exist in File menu |
| M-96 | Probe graph box menu: Export probe data as CSV... (X/X/V/G, U+EDE1), enabled as tree row | PreviewPane probe overlay, SE:48 | ui/10 Probe graph box | missing | |
| M-97 | Header strip has no own menu; right-click/Apps opens the picture's menu | n/a | ui/10 Header strip | missing | |

#### M Dock title bar and Messages menus
| ID | Feature | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| M-98 | Dock pane title drop-down "Window position": Move to Left (L/L/G/L), Right (R/R/D/R), Top (T/O/H/B), Bottom (B/U/B/O), separator, Close (C/S/F/S) | WPF/Docking/DockGroupControl.cs:133, SE:60-66 | ui/10 Dock | divergence | decision 0033, dropped: fixed layout, View > Panes shows/hides |
| M-99 | Messages list menu: Copy (Ctrl+C, selected lines, tab-separated), Copy all (visible lines), separator, Clear (Mn C/K/C/K, A/A/T/A, L/L/E/W) | MP:184-191, SE:204-206 | ui/10 Messages | missing | messages.slint has a Clear button only (messages.slint:59-62) |

#### M Keys: window-wide
| ID | Feature | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| M-100 | F10 / lone Alt enters menu bar (Left/Right walk headers, Down/Enter/Space open, Esc back) | MW:216 (OnMenuPreviewKeyDown) | ui/10 Keys in menus | divergence | Windows' native bar has F10/Alt; Slint's drawn bar (Linux, web) has none (docs/blockers.md) |
| M-101 | Apps / Shift+F10 opens context menu of the focused element | WPF default | ui/10 window-wide | partial | the tree's row menu opens on Menu and Shift+F10 under the current row; other panes have no context menus |
| M-102 | Ctrl+N / Ctrl+O / Ctrl+S / Ctrl+Shift+S | MW:144-149,209-211 | ui/10 window-wide | done | app:95-103 |
| M-103 | Alt+F4 exit | MW:32-36 | ui/10 | partial | OS-level; see M-20 |
| M-104 | Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z undo/redo (text box: own) | EditCommands.cs:37,41 | ui/10 | partial | FocusScope app:166-168; behaviour while a LineEdit has focus not verified (FocusScope only receives unhandled keys) |
| M-105 | Ctrl+X / Ctrl+C / Ctrl+V cut/copy/paste objects (text box: text) | ApplicationCommands | ui/10 | done | window keys a focused text field takes first; act on the group (Explorer's rule) |
| M-106 | Ctrl+A select all objects (text box: text) | ProjectCommands.cs:57 | ui/10 | done | window key (app.slint `keys`) a focused text field takes first: every source, part and probe, the last current |
| M-107 | Ctrl+E export last | ExportCommands.cs:59 | ui/10 | partial | M-18 |
| M-108 | Ctrl+Shift+E / I / B, Ctrl+Shift+F / G | ExportCommands.cs:35-51 | ui/10 | partial | M-12..17 |
| M-109 | F1 project page | HelpCommands.cs:31 (Manual) | ui/10, decision 0028 | missing | binding exists, action does nothing (M-56) |
| M-110 | Ctrl+, Settings (port) | not in Next | ui/10 | done | app:100 |
| M-111 | Ctrl+= / Ctrl+Plus / Ctrl+- / Ctrl+0 UI zoom step up/down/reset across 0.5x..3x (port) | not in Next (View > UI zoom menu only) | ui/10 | divergence | decision 0033, dropped |
| M-112 | F5 Run, F6 Pause/resume, Shift+F5 Stop, F7 Play/pause playback | SimulationCommands.cs:30,34,44; PreviewCommands.cs:40 | ui/10 | done | app:143-148 |
| M-113 | Esc: cancel running export, else stop run; text box reverts edit; during drag cancels drag | SimulationCommands.cs:44 | ui/10 | partial | Esc cancels a running export, else stops the run, when nothing focused took it (an unchanged number box passes it on); drag cancel comes with the handles (piece 5) |
| M-114 | Tab / Shift+Tab next/prev control, menu bar excluded | MW:216 (TabNavigation None) | ui/10 | partial | Slint default focus handling; menu bar excluded natively |

#### M Keys: tree, preview pane, timeline, handles, widgets
| ID | Feature | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| M-115 | Tree: Up/Down/Left/Right/Home/End navigation | PT TreeView | ui/10 Project tree | missing | tree.slint rows are mouse-only (TouchArea clicked), no key-pressed |
| M-116 | Tree: Shift+Up/Down extend range from anchor | PT | ui/10 | missing | single selection |
| M-117 | Tree: Space toggles Enabled/Active | PT:231 | ui/10 | missing | |
| M-118 | Tree: Insert / Ctrl+Space toggle focused row in selection | PT:232-233 | ui/10 | missing | |
| M-119 | Tree: Ctrl+A select all, Ctrl+D duplicate | PT:234-235 | ui/10 | missing | |
| M-120 | Tree: Del remove | PT:230 | ui/10 | partial | window FocusScope Del (app:162) removes selected |
| M-121 | Tree: Alt+Up / Alt+Down move within group | PT:236-237 | ui/10 | missing | buttons only |
| M-122 | Tree: Enter moves focus into properties form (decision 0008) | n/a | ui/10 | missing | |
| M-123 | Preview pane: Space play/pause (not in speed box) | PP:390 | ui/10 | done | app.slint `preview-keys`: a FocusScope over the pane, focused by a click on the pictures; keys the timeline leaves bubble to it; a text field (speed box) keeps its Space |
| M-124 | Preview pane: , one frame back / . one frame forward | PP:391-392 | ui/10 | done | app.slint `preview-keys`: a FocusScope over the pane, focused by a click on the pictures; keys the timeline leaves bubble to it |
| M-125 | Preview pane: I set in point / O set out / X clear | PP:393-395 | ui/10 | done | app.slint `preview-keys`: a FocusScope over the pane, focused by a click on the pictures; keys the timeline leaves bubble to it |
| M-126 | Preview pane: L toggle loop / F toggle Live (only while run active) | PP:396-397 | ui/10 | done | app.slint `preview-keys`: a FocusScope over the pane, focused by a click on the pictures; keys the timeline leaves bubble to it; F acts only while a run is active |
| M-127 | Preview pane: Home / End go to in / out point | PreviewCommands.cs:47,51 | ui/10 | done | app.slint `preview-keys`: a FocusScope over the pane, focused by a click on the pictures; keys the timeline leaves bubble to it and the timeline |
| M-128 | Preview pane: Ctrl+A select all; letter keys consumed even if command cannot run | PP:398 | ui/10 | done | letter keys consumed even when the command cannot run (app.slint `preview-keys`); Ctrl+A passes to the window: select all |
| M-129 | Timeline focused: Left/Right 1 frame, PageUp/PageDown 10, Home/End in/out, wheel 1 frame | TimelineBar.cs | ui/10 Timeline | done | timeline.slint:33-38, scroll-event :77 |
| M-130 | Handles on pictures: arrows 1 px, Shift+arrows 10 px, rotation lever 1/10 degree, grip resize | PreviewPane handles | ui/10 Handles | missing | no handles (preview is a plain Image) |
| M-131 | Handles: Del removes item/selection; Esc undoes drag; Tab next handle selects item | PP | ui/10 Handles | missing | |
| M-132 | Pictures: wheel zoom about pointer, middle drag pan, pinch/two-finger zoom+pan | PP / CanvasImage.cs | ui/10 Handles | missing | Picture uses image-fit contain only |
| M-133 | Picture splitter: arrows move, Enter/double-click reset to half | PP | ui/10 Splitter | missing | |
| M-134 | Layout radio group (side by side / stacked): one tab stop, Left/Right switch | PP:485-490 | ui/10 Layout radio | partial | two Tool buttons (timeline.slint:241-242); no radio keys |
| M-135 | Number box: Up/Down step, PageUp/PageDown x10, wheel (Shift x10), Enter commit, Esc revert | WPF/Controls/NumberBox.cs | ui/10 Number box, ui/30 | done | ui/number-box.slint, src/number.rs; Enter and leaving commit (a tree action commits the typed text first, src/main.rs `draft`), Esc restores and passes on when unchanged (Next's EditBehavior) |
| M-136 | Colour swatch: Space/Enter/Down open picker; square/strip/channel keys; Enter keeps, Esc reverts | WPF/Controls/ColorPicker*.cs | ui/10 Colour picker | done | color-picker.slint ColorSwatch, ColorPicker |
| M-137 | Dialogs: Enter default, Esc cancel, mnemonics w/o Alt; About Esc/Enter close | MessageWindow.xaml.cs | ui/10 Dialogs | partial | modal.slint:25-27 Enter/Esc done; mnemonics without Alt missing |
| M-138 | File drop on window: image becomes scene image; other file to open flow (.wdproj .xml .json .wdscheme); nothing while modal | MW:12-13 (AllowDrop, OnDrop) | ui/10 File dropped | missing | no drop handler in main.rs/app.slint |
| M-139 | Browser: leaving the page asks while unsaved edits | n/a | ui/10 | missing | web.rs/index.html: no beforeunload (not verified in index.html) |
| M-140 | Menu entries 23 px high, shortcut text right-aligned, disabled entries grey text+icon | Themes/Controls.xaml MenuItem | ui/10 Conventions | divergence | decision 0033, matched in general, not to the pixel/id |

### D: dialogs and windows

| ID | Feature | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| D-01 | Main window: min 800x500, default 1280x800 centred, resizable with grip | MW:34-45 (Width/Height/Min, CenterScreen, CanResizeWithGrip) | ui/00, ui/80 | partial | app.slint:78-83 min/preferred sizes done; not centred explicitly, size/position not restored (S-01) |
| D-02 | Window title: "<app> - <file>", asterisk when dirty, "Untitled", full path option, just app name when nothing | MW:134-141 (TitleConverter) | ui/00 Main window | partial | app.slint `title`: "WaveDude Rust - " name or path, "*" when dirty, "Untitled"; no suggested-name rule check |
| D-03 | Window icon (App.ico/png) | MW:35 `Icon="App.ico"`, WPF/App.ico | ui/70 | missing | no `icon:` in app.slint (assets/icons exist for packaging only) |
| D-04 | Taskbar progress for export | MW:129-131 | divergences "Windows integrations" | divergence | not ported (decision 0008 (14)) |
| D-05 | All windows use theme, UI zoom, current language | App.xaml | ui/70 intro | partial | theme done (Theme global), zoom/language missing |
| D-06 | Modal ownership: one modal at a time; later requests queue; window close box closes modals then exits as File > Exit | n/a (WPF owner windows) | ui/70 intro | partial | Slint modal property is a single slot: a second modal overwrites the first (no queue); on_close_requested prompts only when dirty |
| D-07 | Message window: modal, 440 px wide, height to content, not resizable, centred on owner | WPF/Dialogs/MessageWindow.xaml:20-26 | ui/70 Message window | done | modal.slint:38-47 (440px, height box.preferred-height) |
| D-08 | Message window: 32 px disc red "!" for error (plus error sound), accent "?" for question, info none | MessageWindow.xaml:30-35 | ui/70 | partial | modal.slint:56-68; no error sound; info kind shows no disc (spec info disc for export ready) |
| D-09 | Message window: wrapped text beside disc, buttons right-aligned at bottom | MessageWindow.xaml:36-40 | ui/70 | done | modal.slint:69-86 |
| D-10 | Button set OK (Enter, Esc, close box = OK) | MessageWindow.xaml.cs | ui/70 | done | main.rs modal(... ["OK"],0,0) |
| D-11 | Button set Yes (default) / No (Esc); Yes/No(default) for destructive | SE:289-290 | ui/70 | missing | no Yes/No question uses exist (replace-folder, discard) |
| D-12 | Button set Save (default) / Don't save / Cancel (Esc) | SE:291-293 | ui/70 | done | main.rs:~118 modal(... ["Save","Don't save","Cancel"],0,2) |
| D-13 | Mnemonics work without Alt on buttons (_Yes _No _Save _Don't save _Cancel) | SE:288-293 | ui/70 | divergence | decision 0033, matched in general, not to the pixel/id |
| D-14 | Uses: open/save errors "Could not open/save '{0}'." | MessageWindow users | ui/70 | done | main.rs open()/save() modals |
| D-15 | Use: "The folder {0} exists and holds files. Replace them?" Yes/No | ExportPaths | ui/70 | missing | |
| D-16 | Use: "The export failed." and image save error | MainWindow.Export.cs | ui/70 | partial | main.rs timer: "The export failed." only; detail only logged |
| D-17 | Error dialog one line; detail to log and Messages tooltip (unreadable file: first 64K chars + XML error line) | n/a | ui/70 | partial | detail logged via tracing; no tooltip, no 64K quote |
| D-18 | Unsaved edits prompt before New, Open, Close, Exit, update restart, example, recent file: "Save the changes to <name>?" | MainWindow.Project.cs | ui/70 Unsaved | partial | main.rs:~117-121; covers new/open/close/exit/example; no recent/update; Save As cancel aborts (save() false) done |
| D-19 | "An export is running. Stop it and quit?" Yes/No on Exit/close/update | n/a | ui/70 Unsaved | missing | |
| D-20 | Scene image changed on disk: "The scene image file '<name>' differs from the image embedded... Load the file?" Load file (default)/Keep embedded (Esc) | WPF scene image watch | ui/70, ui/60 | missing | no file watch |
| D-21 | Memory warning window: 500 px wide, "?" disc, facts text, "Settings to change for this and later runs:", checkboxes (RAM cache to Auto, halve resolution, half the frames, 8-bit samples; pre-ticked mildest set), result line, "Do not warn me again", Run (default) / Cancel | WPF/Dialogs/MemoryWarningWindow.xaml:20-77, SE:296-304 | ui/70 Memory warning, ui/60 | missing | no dialog; WarnWhenRamInsufficient only a stored flag |
| D-22 | Welcome page modal: title "Welcome to WaveDude Rust", shown first start and each launch while ShowStartup, and File > Welcome Page | not in Next | ui/70 Welcome | done | welcome.slint, main.rs:~455 |
| D-23 | Welcome: "Preview quality" heading + one sentence | not in Next | ui/70 Welcome 1 | done | welcome.slint:38-45 |
| D-24 | Welcome: RAM row ("{available} available of {physical}") | not in Next | ui/70 Welcome 2 | partial | welcome.slint:46; main.rs show_welcome prints physical GiB only, no "available" |
| D-25 | Welcome: buttons Low / Medium / High / Auto apply presets at once + dim levers line | not in Next | ui/70 Welcome 3 | done | welcome.slint:51-62, settings::preset/levers; levers text lacks "dithering" note (spec example matches) |
| D-26 | Welcome: "Settings..." closes page and opens Settings | not in Next | ui/70 Welcome 4 | done | welcome.slint:63-69 |
| D-27 | Welcome: check box "Show this page at each launch" (ShowStartup), saved as it changes | not in Next | ui/70 Welcome 5 | done | welcome.slint:70-74 |
| D-28 | Welcome: bottom-right Open Project..., Random Example, Examples... (default, accent, Enter); no Close button; Esc closes | not in Next | ui/70 Welcome 6 | done | welcome.slint:75-95, key handler :20-25 |
| D-29 | Settings window: modal 760 px wide, title "Settings", frame/backdrop like other dialogs | not in Next | ui/70 Settings, decision 0027 | done | settings.slint:36-40 (min(760, w-40)); height fixed 520 not content |
| D-30 | Settings: search box (SettingsSearch, focused on open) filtering rows by label/English label/section name; "No setting matches"; clearing returns to last section | not in Next | ui/70 Settings | missing | settings.slint has no search |
| D-31 | Settings: section list left (General, Preview and memory, Simulation, Export, Updates), Up/Down walk, Tab cycles | not in Next | ui/70 Settings | partial | sections General/Preview and memory/Simulation/Updates (settings.rs:11); no Export section; mouse only |
| D-32 | Settings: rows = label column + control; change applies and saves at once | not in Next | ui/70 Settings | done | set_row -> save (settings.rs:164) |
| D-33 | Settings: one "Close" button, default and cancel (Enter outside a control, Esc) | not in Next | ui/70 Settings | partial | Close + Esc done (settings.slint:20-23,67); Enter not bound |
| D-34 | Settings: Preset buttons row (Low, Medium, High, Auto) in Preview and memory | not in Next | ui/70 Settings table | missing | only on Welcome |
| D-35 | Settings: RAM row read-only "{available} available of {physical}" native; browser drop-down | not in Next | ui/70 Settings table | missing | |
| D-36 | Settings: Updates > "Check now" button | not in Next | ui/70 Settings table | divergence | decision 0033, dropped |
| D-37 | Updates flow: launch check, "WaveDude Rust x is available (this is y). Download it?", progress "Downloading WaveDude Rust x", signature verify, "Restart now to install it?", failure/up-to-date messages, non-replaceable builds link to release page | not in Next | ui/70 Updates, decision 0011 | divergence | decision 0033, dropped |
| D-38 | Export dialog: in-window dialog, title per command ("Export the field", "...the intensity", "...the field and the intensity", "Export the frame", "Export the probe data") | SE:272-277 | ui/70 Export Save dialog | divergence | Next uses a native save dialog; port in-window dialog (divergences.md "Export names", decision 0025). Slint: native rfd save dialog, no titles (export.rs:~106) -> partial implementation of the old Next form |
| D-39 | Export dialog: format filter list (MP4 H.264, MPEG-1, AVI, MOV Animation, MOV PNG, PNG/JPEG/EXR I/EXR sqrt(I) sequences; stills: PNG, JPEG, EXR I, EXR sqrt) | SE:271,275 | ui/70 | partial | export.rs:35-47 Kind::formats; filter names are Debug names of the enum (`{f:?}`) not descriptions |
| D-40 | Export dialog: proposed name `<project>_field` / `_intensity` / `<project>_field_0655` / probe name | ExportPaths.cs (UI) | ui/70 | missing | export.rs: "export.<ext>" |
| D-41 | Export dialog: one path box per output with "..." browse, labels (Field/Intensity/Archive), OK disabled on empty/duplicate | not in Next | ui/70 | missing | |
| D-42 | Export dialog: "Archive into one ZIP file" check box + Archive box (ExportArchive) | not in Next | ui/70 | missing | |
| D-43 | Export dialog: MP4 OpenH264 notice + "Use Cisco's OpenH264 codec" check, "Delete the downloaded codec" | not in Next | ui/70 | missing | export.rs h264() uses setting silently |
| D-44 | Export dialog: H.264 profile drop-down with greyed unsupported entries + reason | not in Next | ui/70 | missing | Profile::default() hard-coded (export.rs:~118) |
| D-45 | Export: remember filter index, start folder rule per kind/project (ExportHints), fallback LastExportFolder | UIS:266-305 | ui/70, ui/80 | missing | |
| D-46 | Export: replace-existing questions (folder with files "Replace them?", video/still/archive "Replace it?") | MainWindow.Export.cs | ui/70 | missing | rfd overwrite confirm only |
| D-47 | Export: starts a full-quality run first when none/incomplete/reduced/no intensity; status "Export: waiting for the run" | MainWindow.Export.cs | ui/70, ui/60 | missing | export.rs starts platform export directly |
| D-48 | Export progress: "Exporting n / N frames", 120 px determinate bar, tooltip "Cancel the export (Esc)", indeterminate while waiting | MW:450-468, SE (TipExportCancel) | ui/70, status bar | partial | "Exporting d/t" (ExportProgress) over the run's status while an export runs; no progress bar |
| D-49 | Export: Esc cancels (also while waiting); completed export reports in status bar | MW:464 | ui/70 | partial | Stop menu/F5 cancels via "stop"; Esc unbound; completion only logged |
| D-50 | Probe data CSV export (point: step;amplitude; line: frame;step;distances; semicolons) | ExportCommands.ProbeCsv | ui/70 CSV | missing | |
| D-51 | Browser export dialog (names only, always ZIP, WebCodecs MP4, "export is ready" Download/Close window, OPFS) | n/a | ui/70 Browser differences, decision 0025 | missing | export.rs web mod: "exports are not in the browser build yet" |
| D-52 | About window: modal 600 px wide (min 480x620), Esc/Enter close, focus on Close | WPF/Help/AboutWindow.xaml:21-28 | ui/70 About | partial | main.rs "about" uses the 440px message modal with one Close button |
| D-53 | About: app icon 64 px, product name, "Version x.y.z (hash7)", tagline "A 2D wave simulator", copyright line | AboutWindow.xaml:44-53, SE:115-117 | ui/70 About 1 | partial | text has version (CARGO_PKG_VERSION, no hash), tagline; no icon, no copyright line |
| D-54 | About: "Based on the work of" credits word for word with 5 hyperlinks; WaveDude Next credit + GitHub link (port) | AboutWindow.xaml:56-70, SE:118 | ui/70 About 2 | partial | credits as one plain sentence, no links, no Next link line |
| D-55 | About: License section, GPL notice, toggle "Show/Hide the license text" (T), LICENSE.md rendered in scroll box | AboutWindow.xaml:72-86, SE:119-122 | ui/70 About 3 | missing | one GPL sentence only |
| D-56 | About: OpenH264 notice + toggle "Show the OpenH264 license" (O) (port) | not in Next | ui/70 About 4 | missing | |
| D-57 | About: System box (Program, Runtime, OS, Processor+cores, GPU setting, Engine, H.264 encoder) read-only, max 160 px | AboutWindow.xaml:88-94, SE:123-138 | ui/70 About 4 | missing | |
| D-58 | About: buttons "Copy system info" (C, tooltip, status "System information copied to the clipboard") and "Close" (default/cancel) | AboutWindow.xaml:96-99, SE:139-142 | ui/70 About 5 | partial | Close only |
| D-59 | Examples dialog: modal 860 px, title "Examples", six topic tabs with counts 8/10/7/5/18/5 | not in Next (Next: submenu) | ui/70 Examples | partial | examples.slint:38-39 (860px), TOPICS x6 (examples.rs:6); counts per tab not checked |
| D-60 | Examples: search box filters all tabs by title+description, dims tabs without a match, moves to first with match | not in Next | ui/70 Examples | partial | filter + move done (main.rs show_examples); tab dimming missing |
| D-61 | Examples: 360 px scrolled list, rows title + wrapped dim description | not in Next | ui/70 Examples | done | examples.slint:75-97 (height not fixed to 360) |
| D-62 | Examples: Welcome page... button (port) closes dialog, opens welcome | not in Next | ui/70 Examples | missing | buttons Open, Cancel only |
| D-63 | Examples: Open / Cancel; Enter, Open, double-click opens; Esc/Cancel closes; Left/Right tab, Up/Down row; Tab cycles | not in Next | ui/70 Examples | partial | all but Tab cycling and "not while search focused" for Left/Right; opening runs guarded open (unsaved prompt) done |
| D-64 | Manual window (contents tree, rendered MANUAL.md, splitter) | WPF/Help/ManualWindow.xaml, SE:143-146 | ui/70 Manual | divergence | removed, decision 0028 |
| D-65 | Colour picker popup (swatch button, SV square, hue strip, R/G/B number boxes, hex box, live follow <=10 Hz, Enter keeps / Esc restores) | WPF/Controls/ColorPicker.cs, ColorPickerButton.cs | ui/70 Colour picker | done | color-picker.slint; HSV in the model (Rgb::to_hsv, from_hsv); one undo step an opening |
| D-66 | Messages pane: dock pane, default bottom, hidden at first start | MW:525-527 | ui/70 Messages | done | app.slint:139 messages-shown default false |
| D-67 | Messages toolbar: level toggles "Level (count)" coloured (Trace..Fatal, tooltip "Show or hide this level") | MP:201-215 | ui/70 | done | messages.slint:36-52 LevelToggle; tooltip not checked |
| D-68 | Messages toolbar: search box (message, logger, exception), Regex toggle, Auto-scroll toggle, Clear button | MP:201-214 | ui/70 | partial | search + Clear done (messages.slint:55-62); Regex and Auto-scroll toggles missing |
| D-69 | Messages list columns Time, Level, Source (logger), Message(+exception), per-level colours, resizable | MP:207-210 | ui/70 | partial | Time, Source, Message shown (messages.slint:69-71), no Level column, no header, not resizable |
| D-70 | Messages: opens scrolled to newest; keeps last 5000; batches arrivals; row tooltip exception; source tooltip full logger | MessagesTarget.cs | ui/70 | partial | ring in log.rs; tooltips/scroll-to-end not verified |
| D-71 | Initial level visibility: Info+ always, Trace/Debug debug builds only | MessagesTarget.cs | ui/70 | partial | rule not verified in messages.rs/log.rs |
| D-72 | Status bar: left latest WaveDude Info+ log line, "Ready" at first; warnings turn bar orange, errors red until next Info | MW:486-488 | ui/70 Status bar | partial | status text set from last line (messages.rs:82); bar colour fixed Theme.status (the warn token comes with the feature); "Ready" initial not set |
| D-73 | Status bar right: run status; far right: busy text + 120 px progress bar | MW:450-485 | ui/70 Status bar | partial | run status text done (app.slint:~262), busy text/bar missing |
| D-74 | Status bar tooltip "Double-click to open the Messages pane"; double-click opens/focuses | MW:444-447, SE:215 | ui/70 Status bar | partial | double-click opens (app.slint:254); no tooltip |
| D-75 | Crash report at next start (crash.txt, last-crash.txt, "WaveDude Rust hit an internal error..." Copy path / OK) | not in Next | ui/70 Crash report | missing | main.rs only sets a wasm panic hook |
| D-76 | First start: machine facts, presets chosen, settings file written at once, one Info line, default layout, welcome opens | WPF/App.xaml.cs, UIS IsFirstStart | ui/70 First start | partial | Settings::load(..., facts) in platform crate; Welcome shows when show_startup; Info line and default dock layout not checked |
| D-77 | File type registration (.wdproj, .wdscheme) | WPF/FileAssociation.cs (UI) | ui/70, decision 0008 | divergence | installer/package only (decision 0008) |
| D-78 | Command line: open file argument | App.xaml.cs | ui/70 | done | main.rs:~457 args_os().nth(1) |
| D-79 | Dock host: drag/drop panes, tab groups, five-way drop, floating windows | WPF/Docking/*.cs, MW:491-541 | divergences "Docking", decision 0008 (11) | divergence | spec keeps four side lists; Slint has fixed layout with show/hide only (M-98) |

### S: settings

Port keeps JSON `settings.json` (crates/waveduderust_platform/src/settings.rs, platform crate, handles file I/O, sparse write, version 2, unknown keys kept). Slint reads/writes via Settings::load/save (main.rs start()).
| ID | Feature (key, type, default) | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| S-01 | WindowWidth/WindowHeight/WindowLeft/WindowTop double DIP (1280x800, centred) | UIS:69-99, MW:37-40 | ui/80 | missing | no read/write (grep window_width: 0 hits in slint src); fixed preferred size |
| S-02 | WindowState Normal/Maximized | UIS:109, MW:41 | ui/80 | missing | |
| S-03 | Layout (dock tree) saved on change | UIS:119, MW:495-515 | ui/80 | divergence | decision 0033, dropped: fixed layout, View > Panes shows/hides |
| S-04 | Theme System/Light/Dark, Settings > General | UIS:129 | ui/80, ui/70 | done | set.rs row "Theme"; applied via Theme.mode (main.rs apply_settings) |
| S-05 | Language ""/en/de/fr/nl, "System (in the current language)" first, each by own name | UIS:139, MW:295-297 | ui/80, ui/70 | partial | Settings > General > Language: System, then each by its own name (src/settings.rs); "System" is not translated (a Rust string, X-34) |
| S-06 | RecentFiles list newest first max 10 (+ OS recent list) | UIS:149, MW:253-255 | ui/80 | partial | push_recent on open/save (main.rs:213); never shown (M-04) |
| S-07 | PreviewOrientation Horizontal/Vertical (default Vertical), transport layout radios | UIS:159, MW:537 | ui/80 | partial | applied at start (apply_settings); toggling saves always Vertical: main.rs transport "layout" arm `if true { Vertical }` |
| S-08 | AutoRerun bool (true), Settings > Simulation "Re-run automatically after edits" | UIS:169 | ui/80, ui/70 | done | settings.rs row, applied to live.auto_rerun |
| S-09 | FrameMemoryBytes (0 Auto, 256 MiB..2 GiB; browser only), RAM row; replaces Next FrameMemoryPercent (10..100 %) | UIS:203 (percent) | ui/80, ui/70 | missing | no row; Next's percent setting dropped in v2 |
| S-10 | PreviewResolution 1/2/4 drop-down Full/Half/Quarter | UIS:213 | ui/80, ui/70 | done | set.rs row; apply via preview_quality |
| S-11 | PreviewFrames 1/2/4 drop-down "Full / Half (every second frame) / Quarter (every fourth frame)" | UIS:223, SE:85-88 | ui/80, ui/70 | partial | row exists; choice labels shorter ("Half", "Quarter") |
| S-12 | PreviewSampleBytes 1/2 drop-down "16-bit float / 8-bit palette index (brightness fixed when the run starts)" | UIS:233, SE:89-91 | ui/80, ui/70 | missing | no row (lever text reads it) |
| S-13 | PreviewDitherBits 0/10/12/14/16 drop-down, disabled unless 16-bit samples | UIS:243, SE:92-97 | ui/80, ui/70 | missing | no row, no dithering |
| S-14 | UseGpu bool (true), "Use the GPU (OpenCL)" -> "Use the GPU" | UIS:193, MW:420 | ui/80, ui/70 | partial | row exists; consumed by export backend only (export.rs:~100), live run not switched by apply_settings |
| S-15 | UiZoom double 0.5..3 (menu), up to 8 (`--zoom`), default 1.0 / 2.0 touch; drop-down 0.5x..3x | UIS:256, MW:302-327 | ui/80, ui/70 | divergence | decision 0033, dropped |
| S-16 | WarnWhenRamInsufficient bool (true) (+ memory warning "Do not warn me again") | UIS:183, MW:428 | ui/80, ui/70 | partial | row "Warn when the RAM cache is too small" (spec label "Warn when a run does not fit in RAM"); no warning dialog uses it (D-21) |
| S-17 | LastExportFolder path, fallback start folder | UIS:266 | ui/80 | missing | |
| S-18 | ExportFilterIndex int | UIS:276 | ui/80 | missing | |
| S-19 | ExportArchive bool (false), Settings > Export | not in Next | ui/80, ui/70 | missing | no Export section |
| S-20 | PartDescriptionExpanded bool (true), description header on part forms | UIS:286 | ui/80 | missing | no part forms with description header |
| S-21 | TitleFullPath bool (false), Settings > General "Full path in the title" | not in Next | ui/80, ui/70 | done | app.slint `title-full-path` |
| S-22 | H264Profile Baseline/Main/High, Settings > Export | not in Next | ui/80, ui/70 | missing | no row; export uses Profile::default() |
| S-23 | UseOpenH264 bool or absent, Settings > Export check box with Cisco notice | not in Next | ui/80, ui/70 | partial | consumed in export.rs h264(); no row/notice |
| S-24 | CheckForUpdates bool or absent, Settings > Updates "Look for a new version at each launch" | not in Next | ui/80, ui/70 | divergence | decision 0033, dropped |
| S-25 | ShowStartup bool (true), Settings > General + welcome check box | not in Next | ui/80, ui/70 | done | row + welcome checkbox |
| S-26 | UndoSteps int 10..100000 (1000), Settings > General number box, tooltip, decision 0030 | not in Next | ui/80, ui/70 | partial | row (LineEdit number) applies via set_limit; clamps in set_row; no tooltip "two percent a step" |
| S-27 | ExportHints list (max 20, existing project files) | UIS:300-406 (ExportHint) | ui/80 | missing | |
| S-28 | Settings file format: sparse JSON, Version 2, atomic write (temp + rename), unreadable value -> default + log, unknown keys kept, bad file never moved | UIS:321 (XML) | ui/80 | done | implemented in waveduderust_platform::settings (store.rs); Slint calls load/save |
| S-29 | Written on change and on close | UIS | ui/80 | partial | row edits save at once; window/layout never saved |
| S-30 | Not persisted: playback speed, loop, Live, picture split, Export last target, selection, undo | n/a | ui/80 | done | not stored in Slint (playback.rs transient) |
| S-31 | First-start preset choice (memory/threads/GPU/HDD) writes the settings file at once | App.xaml.cs | ui/70 First start | partial | Settings::load(facts) in platform crate; welcome shows |
| S-32 | Settings search keys, tooltips per row (e.g. UndoSteps) | n/a | ui/70 | missing | see D-30 |

### I: icons

Next draws menu/tool icons two ways: (a) glyphs of the Windows icon font (Segoe Fluent Icons / Segoe MDL2 Assets), set by `MenuIcon.Glyph` / `IconView.Glyph` (WPF/Controls/MenuIcon.cs:24-60, IconView.cs, font style CX:227); (b) the app's own 16 px stroked StreamGeometry named `Icon<Name>` (and `Icon<Name>Fill` for solid parts), `MenuIcon.Drawing` / `IconView.Drawing` (IconView.cs:49-58). Non-Windows port needs an equivalent per icon (ui/10 Conventions). Slint: (b) are converted to SVG by tools/next-icons.py into ui/icons/next; (a) are Material Symbols in ui/icons/symbols (docs/divergences.md, Icons). `Icon`/`IconButton` (ui/icons.slint) draw them; tree rows get theirs from src/icons.rs, menus from ui/menu-icons.slint.

#### I Icon font glyphs (U+code point), spec ui/10 Icon column
| ID | Feature (icon, glyph) and use | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| I-01 | File>New Project U+E8A5 | MW:251 | ui/10 File #1 | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-02 | File>Open Project U+E8E5 | MW:252 | ui/10 File #2 | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-03 | File>Recent Projects U+E81C | MW:255 | ui/10 File #3 | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-04 | File>Examples U+E8F1 | MW:259 | ui/10 File #4 | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-05 | File>Welcome Page U+E766 (port) | not in Next | ui/10 File #4a | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-06 | File>Settings U+E713 (port) | not in Next | ui/10 File #4b | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-07 | File>Save U+E74E (also scene image Save, SceneView.xaml:86) | MW:261 | ui/10 File #5 | partial | menu bar: Material Symbols stand-in (divergences.md, Icons); scene image Save not yet |
| I-08 | File>Save As U+E792 | MW:262 | ui/10 File #6 | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-09 | File>Export and field/intensity/both entries, tree probe CSV, picture menu export entries U+EDE1 | MW:265-268; PT:275; PP:741-743,767-769 | ui/10 | partial | menu bar: Material Symbols stand-in (divergences.md, Icons); tree probe CSV and picture menu not yet |
| I-10 | Export last U+E8EE | MW:275 | ui/10 File #8 | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-11 | Export frame entries / tree Scene row U+EB9F (MW:271-272; PP:739,765; PT:313) | MW:271 | ui/10 | partial | menu bar: Material Symbols stand-in (divergences.md, Icons); picture menu not yet |
| I-12 | File>Close Project U+E711 | MW:277 | ui/10 | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-13 | File>Exit U+E7E8 | MW:279 | ui/10 | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-14 | Edit>Undo U+E7A7 / Redo U+E7A6 | MW:282-283 | ui/10 Edit | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-15 | Cut U+E8C6 (MW:285, PT:265) | MW:285 | ui/10 Edit | partial | menu bar: Material Symbols stand-in (divergences.md, Icons); tree and Messages menus not yet |
| I-16 | Copy U+E8C8 (MW:286, PT:266, MP:186-187 Copy/Copy all) | MW:286 | ui/10 Edit | partial | menu bar: Material Symbols stand-in (divergences.md, Icons); tree and Messages menus not yet |
| I-17 | Paste U+E77F (MW:287, PT:267) | MW:287 | ui/10 Edit | partial | menu bar: Material Symbols stand-in (divergences.md, Icons); tree and Messages menus not yet |
| I-18 | Edit>Delete U+E74D | MW:288 | ui/10 Edit | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-19 | Select all U+E8B3 (MW:291, PT:261) | MW:291 | ui/10 | partial | menu bar: Material Symbols stand-in (divergences.md, Icons); tree and Messages menus not yet |
| I-20 | View>Panes U+E8A0 | MW:294 | ui/10 View | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-21 | View>Theme U+E790 (also tree ColorSchemes folder PT:~366) | MW:297 | not in spec (menu moved) | partial | tree ColorSchemes folder (Material Symbols stand-in); other uses not yet |
| I-22 | View>UI zoom U+E8A3 | MW:302 | not in spec (moved) | divergence | decision 0033, dropped |
| I-23 | View>Language U+E774 | MW:330 | not in spec (moved) | missing | not in spec |
| I-24 | Simulation>Run U+E768, Play (while paused) U+E768 | MW:386,408 | ui/10 Simulation | partial | menu bar: Material Symbols stand-in (divergences.md, Icons); play/pause swap not yet |
| I-25 | Simulation>Pause U+E769, Pause playback U+E769 | MW:388,412 | ui/10 | partial | menu bar: Material Symbols stand-in (divergences.md, Icons); play/pause swap not yet |
| I-26 | Simulation>Stop U+E71A | MW:400 | ui/10 | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-27 | Discard frames U+E74D (port) | not in Next | ui/10 Simulation | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-28 | Help>About U+E946 | MW:438 | ui/10 Help | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-29 | Help>Manual U+E736 (Next); port uses "open_in_new" for Project page | MW:439 | ui/10 Help, decision 0028 | divergence | Material Symbols stand-in (divergences.md, Icons) |
| I-30 | Add source/part/probe U+E710 (tree row menu) | PT:256-258 | ui/10 Tree row | missing | |
| I-31 | Add/remove from selection U+E762 | PT:260 | ui/10 Tree row | missing | |
| I-32 | Enabled toggle U+E73A | PT:263 | ui/10 | missing | |
| I-33 | Duplicate U+F413 | PT:264 | ui/10 | missing | |
| I-34 | Remove U+E738 | PT:268 | ui/10 | missing | Next toolbar also uses drawn Minus (I-78) |
| I-35 | Move up U+E74A / Move down U+E74B | PT:270-271 | ui/10 | missing | |
| I-36 | Brightness U+E706 | PP:736,762 | ui/10 Preview menu | missing | |
| I-37 | Clear (messages) U+E894 | MP:189 | ui/10 Messages (not given in spec) | missing | not in spec |
| I-38 | Tree Simulation row U+E916 / Display row U+E7F4 | PT:319,325 | ui/20 | divergence | Material Symbols stand-in (divergences.md, Icons); tree rows |
| I-39 | Welcome / Settings / Check-for-updates / Discard glyphs in port | not in Next | ui/10 | missing | see I-05, I-06, I-27; Check for updates has none |

#### I App drawings (Icon<Name> StreamGeometry, 16 px grid), file CX = WaveDudeNext.WPF/Themes/Controls.xaml
| ID | Feature (name) and where used | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| I-40 | IconSource: tree Sources folder/row, "+ Source" button | CX:248 | ui/20 | partial | tree rows (icons.rs); toolbar and add menus not yet |
| I-41 | IconProbe: tree Probes folder/row, "+ Probe", Standard probe entry | CX:249 | ui/20, ui/10 | partial | tree rows (icons.rs); toolbar and add menus not yet |
| I-42 | IconCountingDetector: Probabilistic/Counting detector entries and rows | CX:251 | ui/20 | done | tree rows (icons.rs) and the add menus (MenuIcons.next-counting-detector) |
| I-43 | IconImagingDetector: Imaging detector entry/rows | CX:253 | ui/20 | done | tree rows (icons.rs) and the add menus (MenuIcons.next-imaging-detector) |
| I-44 | IconPart: tree Parts folder, "+ Part" | CX:254 | ui/20 | partial | tree rows (icons.rs); toolbar and add menus not yet |
| I-45 | IconPlusBadge (+ IconPlusBadgeStyle): add plus on toolbar icons | CX:258-259 | ui/20 | missing | SVG generated, not used yet |
| I-46 | IconProject: tree root row | CX:273 | ui/20 | done | tree root row (icons.rs) |
| I-47 | IconLens (Lens/Mirror entry, biconvex + concave mirror) | CX:279 | ui/10 | missing | SVG generated, not used yet |
| I-48 | IconLens<Left><Right> variants (ConvexConvex, ConvexFlat, ConvexConcave, FlatConvex, FlatFlat, FlatConcave, ConcaveConvex, ConcaveFlat, ConcaveConcave): tree lens rows follow surface shapes | CX:285-293, LensIconConverter.cs:37 | ui/20 | partial | tree rows (icons.rs); Add part menu not yet |
| I-49 | IconFresnelLens | CX:295 | ui/20 | partial | tree rows (icons.rs); Add part menu not yet |
| I-50 | IconFiber | CX:297 | ui/10 | partial | tree rows (icons.rs); Add part menu not yet |
| I-51 | IconBraggReflector (Fill geometry) | CX:299 | ui/10 | partial | tree rows (icons.rs); Add part menu not yet |
| I-52 | IconGrinLens | CX:301 | ui/10 | partial | tree rows (icons.rs); Add part menu not yet |
| I-53 | IconZonePlate (Fill) | CX:303 | ui/10 | partial | tree rows (icons.rs); Add part menu not yet |
| I-54 | IconDiffuser | CX:305 | ui/20 | partial | tree rows (icons.rs); Add part menu not yet |
| I-55 | IconInterferenceFilter | CX:307 | ui/20 | partial | tree rows (icons.rs); Add part menu not yet |
| I-56 | IconDiffractionGratings (Fill) | CX:309 | ui/20 | missing | SVG generated, not used yet |
| I-57 | IconMetaSurfaces | CX:311 | ui/20 | missing | SVG generated, not used yet |
| I-58 | IconMetaLens / IconMetaPrism / IconMetaAxicon | CX:313,315,317 | ui/20 | partial | tree rows (icons.rs); Add part menu not yet |
| I-59 | IconFacettedGrating / IconSimpleGrating (Fill) | CX:319,321 | ui/20 | partial | tree rows (icons.rs); Add part menu not yet |
| I-60 | IconPolygon / Rectangle / Ellipse / Triangle / Outline | CX:323,325,327,329,331 | ui/10 | partial | tree rows (icons.rs); Add part menu not yet |
| I-61 | IconOpticalLogic, IconXorGate, IconAndGate, IconOrGate | CX:333,335,337,339 | ui/10 | partial | tree rows (icons.rs); Add part menu not yet |
| I-62 | IconAutoBrightness (+Fill): picture menu Auto brightness | CX:341-342 | ui/10 | missing | |
| I-63 | IconPreviewResolution / IconPreviewFrames / IconPreviewSamples / IconPreviewDither (+Fill): View preview-quality menus | CX:344-351 | not in spec (menus moved) | missing | not in spec |
| I-64 | IconFrameMemory: Simulation>RAM cache size | CX:353 | not in spec | missing | not in spec |

#### I Interface geometries (transport, markers, glyph buttons)
| ID | Feature (name) and where used | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| I-65 | RunGeometry (ring+triangle) Run button | PP:38, used PP:416 | ui/50 | done | transport bar (timeline.slint) |
| I-66 | StopGeometry (ring+square) | PP:42, PP:422 | ui/50 | done | transport bar (timeline.slint) |
| I-67 | PauseRunGeometry (ring+bars) pause the run | PP:40, PP:433 | ui/50 | done | transport bar (timeline.slint) |
| I-68 | PlayGeometry / PauseGeometry playback | PP:46-47, PP:448,451 | ui/50 | done | transport bar (timeline.slint) |
| I-69 | StepBackGeometry / StepForwardGeometry | PP:52-53, PP:455,458 | ui/50 | done | transport bar (timeline.slint) |
| I-70 | SetInGeometry / SetOutGeometry / ClearRangeGeometry | PP:54-56, PP:462-468 | ui/50 | done | transport bar (timeline.slint) |
| I-71 | LoopGeometry | PP:58, PP:471 | ui/50 | done | transport bar (timeline.slint) |
| I-72 | LiveGeometry | PP:60, PP:474 | ui/50 | done | transport bar (timeline.slint) |
| I-73 | SideBySideGeometry / StackedGeometry layout radios | PP:48-49, PP:485,490 | ui/50 | done | transport bar (timeline.slint) |
| I-74 | FieldGeometry / IntensityGeometry picture header icons | PP:44-45, PP:497,502 | ui/40 | missing | headers are text only (preview.slint:12-18) |
| I-75 | SpinnerGeometry busy spinner | PP:64-66 | ui/40 | missing | |
| I-76 | SourceGlyph / ProbeGlyph / PartGlyph (21 px picture handles) and SourceMarkerPainter.cs:45 | PP:177-179; Rendering/SourceMarkerPainter.cs:45 | ui/40 | missing | no handles |
| I-77 | AngleGlyph rotation lever, ResizeGripGlyph graph box grip | PP:277,329 | ui/40 | missing | |
| I-78 | PlusGeometry (folder +), MinusGeometry (row remove), ChevronUp/Down (row move) | PT:40-43, uses PT:210-214,333,376-397; Editors.xaml:532 | ui/20 | partial | row minus and chevrons done; folder plus not yet |
| I-79 | ClearGeometry (messages Clear button icon) | MP:29, MP:128 | ui/70 | missing | text button "Clear" |
| I-80 | Menu/combos/checkbox arrows: ArrowUp/Down/Left/Right, SubmenuArrow, Check, ChevronRight/Down/Up, Close, DropDown | CX:45-55 | ui/10 Conventions | partial | check mark at native size (Next: Uniform, margin 2 in the box); menu, combo and scroll glyphs are std-widgets' |
| I-81 | GripGeometry, ChainLinked/ChainParted (linked-values toggle), pane title drag handle brushes | Themes/Editors.xaml:589,623-624; CX:59-64 | ui/30, 00 | missing | pane title grip drawn as border (pane.slint:27-34) |
| I-82 | App icon: App.ico / App.png / App.svg (window, About 64 px, taskbar) | WPF/App.ico, App.png, App.svg; AboutWindow.xaml:44 | ui/70 About | partial | window icon from assets/icon.svg; About not checked |
| I-83 | Menu-item icon slot: 16 px icon, greys with text | MenuIcon.cs, IconView.cs | ui/10 Conventions | done | Slint MenuItem icon, per-theme copies in the menu text colour (menu-icons.slint) |

## Project tree and properties

Slint code: frontends/waveduderust_slint/ui/tree.slint, ui/properties.slint, src/tree.rs, src/form.rs, src/main.rs (tree_*/menu).
Next paths are relative to WaveDudeNext.WPF (Panes/ProjectTree.xaml etc). Spec files are docs/ui/20 and docs/ui/30.
Status words: done / partial / missing / divergence.
Not-in-spec rows are marked in the Spec column.

### Tree (T)

| ID | Feature | Next source (file:line) | Spec (docs/ui file, section) | Slint | Note |
|---|---|---|---|---|---|
| T-01 | Toolbar: Add source button (+Source, icon+badge) | ProjectTree.xaml:186 | 20 Layout/Toolbar | done | tree.slint AddButton: source drawing with the plus badge, Next's tip; appends at the canvas centre |
| T-02 | Toolbar: Add probe button with drop-down caret | ProjectTree.xaml:193 | 20 Toolbar | done | tree.slint AddButton with the drop-down arrow; a click or a right click opens ProbeMenu under it |
| T-03 | Add probe menu: Standard probe | ProjectTree.xaml:152 | 20 Toolbar; Automation AddProbe:Standard | divergence | tree.slint ProbeMenu (ContextMenuArea.show) with Next's drawing; no tooltip (divergences.md Add menus) |
| T-04 | Add probe menu: Probabilistic > Counting detector | ProjectTree.xaml:153-154 | 20 Toolbar | divergence | tree.slint ProbeMenu > Probabilistic, Next's drawing; no tooltip (divergences.md Add menus); named Detector n, a line probe of the integrated intensity (Project::add_detector_at) |
| T-05 | Add probe menu: Probabilistic > Imaging detector | ProjectTree.xaml:155 | 20 Toolbar | divergence | tree.slint ProbeMenu > Probabilistic, Next's drawing; no tooltip (divergences.md Add menus); named Detector n, a line probe of the integrated intensity (Project::add_detector_at) |
| T-06 | Toolbar: Add part button with drop-down caret | ProjectTree.xaml:201 | 20 Toolbar | done | tree.slint AddButton with the drop-down arrow; opens PartMenu under it |
| T-07 | Add part menu entry: Lens/Mirror (kind icon, kind tooltip) | ProjectTree.xaml:116-123 | 20 Automation AddPart:* | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-08 | Add part menu entry: Fresnel lens (kind icon, kind tooltip) | ProjectTree.xaml:116-123 | 20 Automation AddPart:* | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-09 | Add part menu entry: Fibre (kind icon, kind tooltip) | ProjectTree.xaml:116-123 | 20 Automation AddPart:* | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-10 | Add part menu entry: Bragg reflector (kind icon, kind tooltip) | ProjectTree.xaml:116-123 | 20 Automation AddPart:* | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-11 | Add part menu entry: GRIN lens (kind icon, kind tooltip) | ProjectTree.xaml:116-123 | 20 Automation AddPart:* | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-12 | Add part menu entry: Zone plate (kind icon, kind tooltip) | ProjectTree.xaml:116-123 | 20 Automation AddPart:* | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-13 | Add part menu entry: Diffuser (kind icon, kind tooltip) | ProjectTree.xaml:116-123 | 20 Automation AddPart:* | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-14 | Add part menu entry: Interference filter (kind icon, kind tooltip) | ProjectTree.xaml:116-123 | 20 Automation AddPart:* | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-15 | Add part menu: Diffraction gratings > Simple 2D grating | ProjectTree.xaml:125-126 | 20 Automation | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-16 | Add part menu: Diffraction gratings > Facetted grating | ProjectTree.xaml:127 | 20 Automation | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-17 | Add part menu: Meta surfaces > Meta lens | ProjectTree.xaml:130-133 | 20 Automation | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-18 | Add part menu: Meta surfaces > Meta prism | ProjectTree.xaml:130-133 | 20 Automation | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-19 | Add part menu: Meta surfaces > Meta axicon | ProjectTree.xaml:130-133 | 20 Automation | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-20 | Add part menu entry: Polygon | ProjectTree.xaml:136-140 | 20 Automation | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-21 | Add part menu entry: Rectangle | ProjectTree.xaml:136-140 | 20 Automation | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-22 | Add part menu entry: Ellipse | ProjectTree.xaml:136-140 | 20 Automation | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-23 | Add part menu entry: Triangle | ProjectTree.xaml:136-140 | 20 Automation | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-24 | Add part menu entry: Outline | ProjectTree.xaml:136-140 | 20 Automation | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-25 | Add part menu: Optical logic > XOR gate | ProjectTree.xaml:143-146 | 20 Automation | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-26 | Add part menu: Optical logic > AND gate | ProjectTree.xaml:143-146 | 20 Automation | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-27 | Add part menu: Optical logic > OR gate | ProjectTree.xaml:143-146 | 20 Automation | divergence | tree.slint PartMenu (ContextMenuArea.show) with Next's drawing (tools/next-icons.py MENU_DRAWINGS); no tooltip (divergences.md Add menus); new parts as Next prepares them (tree.rs new_part) |
| T-28 | Toolbar: Move up button (Alt+Up tooltip, disabled when n/a) | ProjectTree.xaml:210 | 20 Toolbar | done | toolbar IconButton, greyed when the current row cannot move up (tree::can_shift) |
| T-29 | Toolbar: Move down button | ProjectTree.xaml:212 | 20 Toolbar | done | toolbar IconButton, greyed when the current row cannot move down |
| T-30 | Toolbar: Remove button (minus, Del tooltip) | ProjectTree.xaml:214 | 20 Toolbar | done | toolbar IconButton, greyed unless an item row is current |
| T-31 | Toolbar: flat buttons, hover bg, plus badge, tab order Source>Probe>Part>move/remove>tree | ProjectTree.xaml:180-215 | 20 Toolbar | done | flat buttons with hover background, plus badge, tab order Source>Probe>Part>move/remove; Space or Enter on a focused add button opens its menu |
| T-32 | Toolbar buttons: tooltips and automation ids ToolAdd*/ToolMove*/ToolRemove | ProjectTree.xaml:186-215 | 20 Automation ids | divergence | Next's tooltips (Slint Tooltip); no automation ids (Slint has no AccessKit ids; decision 0032) |
| T-33 | Root row Project (icon Draw:Project) | ProjectTree.xaml:292-308 | 20 Tree structure | partial | row present; no icon |
| T-34 | Fixed rows Scene, Simulation, Display (icons EB9F/E916/E7F4) | ProjectTree.xaml:310-327 | 20 Tree structure | partial | rows present, selectable; no icons |
| T-35 | Folder rows Sources/Parts/Probes/Colour schemes (icons) | ProjectTree.xaml:329-370 | 20 Tree structure | partial | rows present; no icons |
| T-36 | Folder expand/collapse chevron, all expanded by default | ProjectTree.xaml:240-244 (TreeView) | 20 Tree structure | done | tree.slint expander column: the root and folders with items, all expanded at first; a click or Left/Right folds (State.collapsed, kept until restart); a selection a fold hides moves to the folder |
| T-37 | Row height 23 px | ProjectTree.xaml | 20 Tree structure; 00 overview | done | Theme.row-height (verify =23px) |
| T-38 | Localised fixed row names and folder labels | Strings/* | 20 Tree structure; 00 language | partial | English literals in tree.rs; no @tr |
| T-39 | Item rows show item Name, live rename from Name box | ProjectTree.xaml:383-455 | 20 Item row | done | label from project each refresh |
| T-40 | Folder row: right-aligned plus button (tooltip 'Add an item to this folder') | ProjectTree.xaml:333-336 | 20 Item row | done | tree.slint folder rows: right-aligned plus, 'Add an item to this folder' |
| T-41 | Parts folder plus opens Add part menu | ProjectTree.xaml:330-370 | 20 Item row | done | Parts and Probes folder plus open their menus of kinds under the plus |
| T-42 | Colour schemes folder plus adds a project scheme with default stops | ProjectTree.xaml:362 | 20 Commands (Add on folder plus) | done | Project::add_new_color_scheme (grey stops), selected |
| T-43 | Source row: Enabled check box, source icon, name | ProjectTree.xaml:383-390 | 20 Item row | done | check box (Next's check drawing) with its tooltip, the source drawing, the name |
| T-44 | Part row: Enabled check box, kind icon (per kind), name | ProjectTree.xaml:399-410 | 20 Item row; Row icons | done | check box with its tooltip, the kind's drawing (a lens's from its radii, icons.rs), the name |
| T-45 | Probe row: Active check box, probe icon, name | ProjectTree.xaml:453-460 | 20 Item row | done | check box with its tooltip, the probe's or detector's drawing, the name |
| T-46 | Colour scheme row: 36x10 gradient strip + name, no check box | ProjectTree.xaml:431-445 | 20 Item row | done | 36 x 10 strip of the scheme's gradient (icons.rs strip, the core's Gradient), no check box or icon |
| T-47 | Row glyph buttons Move up / Move down / Remove, 18x18 | ProjectTree.xaml:376-381 | 20 Item row | partial | present on depth-2 rows (all four lists) |
| T-48 | Move up/down greyed and inert where item cannot move | ProjectTree.xaml:378-381 | 20 Item row | partial | inert at ends (shift no-op) but not greyed |
| T-49 | Row check box and buttons not focusable / not tab stops | ProjectTree.xaml:383 | 20 Item row | partial | TouchArea, not focusable; fine |
| T-50 | Row tooltips (include in runs, paint part, sample probe, move, remove) | ProjectTree.xaml:384,400,454 | 20 Item row | done | check boxes: include in runs / paint the part / sample this probe; move and remove buttons: Next's tips |
| T-51 | Disabled item greys icon and name | ProjectTree.xaml:383-455 | 20 Item row | done | a disabled item's icon and name in the disabled text colour |
| T-52 | Part icons: Lens profile icon from R1,R2,diameter (9 variants, follows edits) | Converters/LensIconConverter.cs | 20 Row icons | missing |  |
| T-53 | Part icons: non-lens kinds drawings (FresnelLens..OrGate) | Controls/IconView.cs | 20 Row icons | missing |  |
| T-54 | Add menu Lens/Mirror entry fixed icon | Controls/MenuIcon.cs | 20 Row icons | done | Lens/Mirror entry shows the fixed lens drawing |
| T-55 | Click / arrows select a row alone | ProjectTree.xaml.cs | 20 Selection model | partial | a click selects the row alone; arrow keys come with tree keyboard navigation (3e) |
| T-56 | Ctrl+click toggles row in selection | ProjectTree.xaml.cs:41 | 20 Selection model | divergence | Ctrl+click toggles a source, part or probe, the current row following (src/selection.rs toggle); toggling out the last one leaves the root selected where Next leaves nothing |
| T-57 | Shift+click / Shift+arrows range select in tree order | ProjectTree.xaml.cs:41 | 20 Selection model | partial | Shift+click takes the range in tree order from the anchor (the last plain click or Ctrl+click that added), as Next; Shift+arrows come with 3e |
| T-58 | Insert / Ctrl+Space toggle focused row in selection | ProjectTree.xaml:232-233 | 20 Selection model | divergence | Insert and Ctrl+Space while the tree has focus; out of a group of one, the root stays selected (T-56) |
| T-59 | Ctrl+A selects all sources, parts, probes | ProjectTree.xaml:234 | 20 Selection model | done | Ctrl+A and Edit, Select all: every source, part and probe, the last current |
| T-60 | Selection from preview mirrored in tree, row revealed, no focus steal | MainWindow.Project.cs | 20 Selection model | missing | no handles on the pictures yet |
| T-61 | Click on empty preview deselects | ProjectTree.xaml.cs | 20 Selection model | missing |  |
| T-62 | Opening a project selects the root row; tree takes keyboard focus | MainWindow.Project.cs | 20 Selection model | partial | selected=Project on open (commit af7ee17); focus is a global FocusScope |
| T-63 | Highlight: hover colour, current=selection colours when tree focused else inactive colours | ProjectTree.xaml:260-290 | 20 Selection model | partial | hover colour; selected rows in the selection colours while the tree has focus, else the inactive ones (tree.slint `tree-keys`); the current row has no mark of its own |
| T-64 | Groups/root/settings rows can be current (Properties shows their form) | ProjectTree.xaml | 20 Selection model | done |  |
| T-65 | Multi-selection of schemes (cut/copy/delete/duplicate) | ProjectTree.xaml.cs | 20 Selection model; 0008 | divergence | the row's ask is not Next's: its group takes sources, parts and probes only (Selection.IsGroupable); a scheme row acts alone, as in Next |
| T-66 | Folder selected: Properties shows label + count (No entries / 1 entry / N entries) | Views/CollectionView.xaml:32 | 20 Properties coupling; 30 Pane behaviour | partial | Properties shows the folder's label and No entries / 1 entry / N entries; the label is not translated (T-38) |
| T-67 | Add source: appended, at canvas centre, becomes selection, caret in Name (select all) | ProjectCommands.AddSource | 20 Commands | partial | appended at the canvas centre (image scene: its size), selected; the caret is not put in Name |
| T-68 | Add probe: line end B 100 px right of A; selection + caret in Name | ProjectCommands.AddProbe | 20 Commands | partial | line end 100 px right of A, selected; the caret is not put in Name |
| T-69 | Add part: placed at canvas centre of shown preview (image scene: its size) | ProjectCommands.AddPart | 20 Commands | done | Project::canvas_centre: the image's size for an image scene |
| T-70 | Enabled toggle by Space (each selected flips on its own) | ProjectTree.xaml:231 | 20 Commands | done | Space while the tree has focus, and a row's check box, flip each item of the group on its own |
| T-71 | Duplicate Ctrl+D: copy after each original, '<name> copy', copies selected, caret in name | ProjectTree.xaml:235,264 | 20 Commands | partial | Ctrl+D while the tree has focus: a copy after each original of the group as '<name> copy'; the copies become the group, the last current; the caret is not put in Name |
| T-72 | Remove Del: removes target or selection; selection lands on slot/above/folder; tree takes focus | ProjectTree.xaml:230,268 | 20 Commands | partial | Del, the row minus and the toolbar remove the group (Explorer's rule); the user is kept at the last one's slot, else the folder (tree::remove_all); the tree keeps focus only from the toolbar |
| T-73 | Move up/down Alt+Up/Alt+Down within own list, selection kept, tree takes focus | ProjectTree.xaml:236-237,270-271 | 20 Commands | partial | Alt+Up/Alt+Down while the tree has focus, row and toolbar buttons; the current row alone moves and the group collapses to it, as Next's Move; the tree takes focus from the toolbar |
| T-74 | Cut (list items only) = Copy + Remove | ProjectTree.xaml:265 | 20 Clipboard | done | Cut copies the group, then removes its list items |
| T-75 | Copy as app format + plain XML text (items or Scene/Simulation/Display) | ProjectTree.xaml:266 | 20 Clipboard | partial | the group or Scene/Simulation/Display as XML text; no app format (divergences.md Clipboard) |
| T-76 | Paste: behind current item, rename on clash, re-derive composite emitters, settings object on own row, undo step | ProjectTree.xaml:267 | 20 Clipboard | missing |  |
| T-77 | Can-paste re-evaluated once per clipboard change | MainWindow.Clipboard.cs | 20 Clipboard | missing |  |
| T-78 | Focused text box handles Cut/Copy/Paste for its text first | MainWindow.Clipboard.cs | 20 Clipboard; divergence Edit menu | divergence | divergences.md 'Edit menu' (0008 13) |
| T-79 | Add to my schemes / Remove from my schemes (scheme rows, label follows existence) | n/a (Rust only) | 20 Commands | done | tree.slint rows menu, scheme rows below a separator, label by Commands.mine; Command.toggle-mine |
| T-80 | Export probe data as CSV (probe rows, active probes only; browser file-name box) | ProjectTree.xaml:273-275 | 20 Commands | missing | decision 0025 for browser |
| T-81 | Context menu on any row (Add Source/Part/Probe submenus) | ProjectTree.xaml:248-259 | 20 (Commands) not itemised | divergence | tree.slint rows-menu on every row: Add source, Add part, Add probe submenus; no access keys or key hints (divergences.md Row menu) |
| T-82 | Context menu: Toggle selected (Ins), Select all (Ctrl+A) with automation ids | ProjectTree.xaml:260-261 | 20 Automation ids | divergence | tree.slint rows-menu: Add to or remove from selection, Select all; no automation ids (decision 0032); no access keys or key hints (divergences.md Row menu) |
| T-83 | Context menu: Enabled (Space), Duplicate (Ctrl+D), Cut/Copy/Paste, Remove (Del), Move up/down (Alt+Up/Down) with glyphs | ProjectTree.xaml:263-271 | 20 Commands | divergence | tree.slint rows-menu: Enabled, Duplicate, Cut/Copy/Paste, Remove, Move up/down with icons; no access keys or key hints (divergences.md Row menu) |
| T-84 | Context menu: Export probe CSV only on probe rows, own separator | ProjectTree.xaml:272-275 | 20 Commands | missing | waits for probe recordings in the live run (M-68) |
| T-85 | Right-click row acts on selection or target (Explorer rule) | ProjectTree.xaml:252-254 | 20 Commands | divergence | a right click on a row outside the selection selects it, then the menu acts on the selection (Explorer's rule); Next acts on the clicked row's group without moving the selection (divergences.md Row menu) |
| T-86 | TreeView native keyboard: Up/Down/Left/Right/Home/End, expand/collapse by arrows | WPF TreeView | not in spec | done | tree.slint tree-keys: Up/Down/Home/End move the current row (Shift a range), Left folds or goes to the parent, Right unfolds or goes to the first child (tree::navigate); the current row scrolls into view |
| T-87 | Tree-row Tab order / focus ring of tree | ProjectTree.xaml:240 | 20 Toolbar (Tab order) | missing |  |
| T-88 | Drag starts from a row (not buttons) after system threshold | ProjectTree.xaml.cs:396-411 | 20 Drag and drop | done | Slint's DragArea on an item row's own area, its threshold; the row's buttons, check box and expander take their presses |
| T-89 | Drag carries item as app clipboard format + XML text | ProjectTree.xaml.cs:405-411 | 20 Drag and drop | divergence | the drag carries the row in the app (DataTransfer user data), no .wdproj text: Slint takes drags to other applications on Qt only (divergences.md Tree drag) |
| T-90 | Drop on sibling row: before/after by half, insertion line along edge | ProjectTree.xaml.cs:557-580,531 | 20 Drag and drop | done | DropArea per row: before or after a sibling by the half, the line along that edge (tree::drop_target, move_to); colour schemes reorder too, which Next marks but drops nothing for |
| T-91 | Drop on own folder: append, outline around folder row | ProjectTree.xaml.cs:570 | 20 Drag and drop | done | a drop on the item's own folder appends, an outline around the folder row |
| T-92 | No-op positions are not a target | ProjectTree.xaml.cs:570,580 | 20 Drag and drop | done | no-op positions, other lists and settings rows are no target (DragAction.none) |
| T-93 | Drop into another window's tree (paste there) | ProjectTree.xaml.cs:451-486 | 20 Drag and drop | divergence | divergences.md 'Drag and drop' (0008 12) |
| T-94 | Drop on a preview picture: pasted, moved as group under drop pixel, selection, one undo | Panes/PreviewPane.xaml.cs | 20 Drag and drop | missing | no handles/pictures drop yet |
| T-95 | Drop onto text editor receives XML | ProjectTree.xaml.cs | 20 Drag and drop | divergence | divergences.md 'Drag and drop' |
| T-96 | Insertion adorner line / outline visuals | Controls/InsertionAdorner.cs | 20 Drag and drop | done | accent line 2 px along the edge, 1 px outline around a folder |
| T-97 | File drop on window: image becomes scene image, project file opens | MainWindow.Project.cs | 20 Drag and drop; 10 | missing | owned by another area? not in tree code |
| T-98 | Settings rows (Scene, Simulation, Display) draggable, carry XML, do not reorder | ProjectTree.xaml.cs | 20 Drag and drop | missing | settings rows do not drag: no target takes them in the app yet (preview drop, T-94) |
| T-99 | Touch-first: rows sense clicks alone, drag unavailable | n/a | 20 Drag and drop | divergence | decision 0033, dropped |
| T-100 | Schemes: move up/down/remove via row buttons | ProjectTree.xaml:446-451 | 20 Item row | done |  |
| T-101 | Automation ids Tree:<name>, Enabled:<name>, InSelection, CollectionCount | ProjectTree.xaml:245,385 | 20 Automation ids | divergence | Slint has no AccessKit ids; decision 0032 |

### Properties (P)

| ID | Feature | Next source (file:line) | Spec (docs/ui file, section) | Slint | Note |
|---|---|---|---|---|---|
| P-01 | Pane shows form of the current item; 'No selection' grey when none | Panes/PropertiesPane.xaml:117 | 30 Pane behaviour | missing | heading + empty list |
| P-02 | Pane heading / form selected by node type | Panes/PropertiesPane.xaml:117 | 30 Pane behaviour | done | every node shows its typed form (ui/forms.slint), without a heading as Next; a folder shows its name and entry count |
| P-03 | Folder row: label header + count (No entries/1 entry/N entries), grey | Views/CollectionView.xaml:32 | 30 Pane behaviour | missing |  |
| P-04 | Caret into first text box with text selected on newly added item | Panes/PropertiesPane.xaml.cs:83-88 | 30 Pane behaviour | missing |  |
| P-05 | Form scrolls vertically, never horizontally | Panes/PropertiesPane.xaml | 30 Pane behaviour | done | ScrollView |
| P-06 | FormPanel two columns: label column = widest label (>=110 px), editors fill rest | Controls/FormPanel.cs | 30 Pane behaviour | done | ui/form.slint FormPanel: a grid, the label column its widest label, 110 px at least, editors fill the rest |
| P-07 | Full-width check boxes carry label as content | Controls/FormPanel.cs | 30 Pane/Check box | done | FormCheck spans both columns with the label as its text |
| P-08 | Row height >=22 px, 4 px apart; multiline label at top | Controls/FormPanel.cs | 30 Pane behaviour | divergence | decision 0033, matched in general, not to the pixel/id |
| P-09 | Section headers: name over a rule; gap after group | Themes/Editors.xaml (FormSection) | 30 Pane behaviour | done | FormSection: semibold name over a 1 px rule, 10 px after each FormPanel |
| P-10 | Secondary values (format version, folder count) grey | Views/ProjectView.xaml:37 | 30 Pane behaviour | partial | FormValue in the dim text colour: Format version; the folder count is plain (T-66) |
| P-11 | Read-outs plain text bound to inputs; '~ ' prefix 1 decimal for estimates; px/deg units | Themes/Editors.xaml (FormReadout) | 30 Pane behaviour | missing | no read-out rows |
| P-12 | Edits apply live (every keystroke in name, step, check box) | Panes/PropertiesPane.xaml.cs | 30 Pane behaviour | partial | check box/combo live; text applies on Enter only (accepted), no per-keystroke name rename |
| P-13 | Each edit marks dirty, follows live-editing rules / undo | MainWindow.Live.cs | 30 Pane behaviour; 60 | done | doc.edit per set; Display edits through doc.edit_view_merging, so the Display settings undo and dirty with the project |
| P-14 | Tooltips (Hint.*) on every editor and label | Views/*.xaml meta:Hint | 30 intro | partial | Next's Hint.* as Tooltips on the labels and editors of the settings forms |
| P-15 | Labels from Label.<Type>.<Member> string tables, 4 languages | Strings/* | 30 intro | partial | the settings forms' labels are Next's Label.* texts through @tr, translated by tools/next-strings.py |
| P-16 | Enum words localised from Enum.* | Strings/* | 30 Check box, combo box | partial | Scene's Mode and Edges words through @tr (Next's Enum.*); the items' words are raw |
| P-17 | Text box: Enter pushes text and selects it; Esc restores when differing; keys consumed only if effective | Controls/EditBehavior.cs | 30 Text box | partial | LineEdit accepted(Enter) commits; no Esc restore, no select-all |
| P-18 | Multiline text box keeps Enter as newline (Description) | Views/ProjectView.xaml:46-49 | 30 Text box | missing | LineEdit is single line |
| P-19 | Name boxes update per keystroke (tree row renames live) | Views/SourceView.xaml:54 | 30 Text box | partial | FormText applies per keystroke, the tree row renaming live, the keystrokes one undo step; sources and probes now, parts with their forms |
| P-20 | Commit policy: commit on Enter, on focus loss; flush before Save/Run/Undo/selection change (PendingEdit) | Controls/EditBehavior.cs | 30 Text box commit policy | partial | commit on Enter and on focus loss; a tree row takes the focus on the press, so a box commits before the selection moves; a shortcut (Save, Run, Undo) with a box focused does not commit first |
| P-21 | Widget press flushes pending edit first (tree row, check, button, picker, dock tab) | Controls/EditBehavior.cs | 30 Text box commit policy | partial | tree rows take the focus on the press; buttons, checks and dock tabs not checked |
| P-22 | Pending text survives unmount (dock tab switch, picker close); Esc drops it | Controls/EditBehavior.cs | 30 Text box commit policy | missing |  |
| P-23 | Rename box closes on focus leaving; Enter/Esc give focus back to tree | Controls/EditBehavior.cs | 30 Text box commit policy | missing |  |
| P-24 | Number box: text + up/down repeat buttons column | Controls/NumberBox.cs | 30 Number box | divergence | ui/number-box.slint, src/number.rs; spin column beside the LineEdit; a click focuses without placing the caret (divergences.md, Number box) |
| P-25 | Number box: Up/Down key +/- Increment (log: x/ / 1.02) | Controls/NumberBox.cs | 30 Number box | partial | ui/number-box.slint, src/number.rs; every box steps by Next's default 1 until piece 4 brings each field's increment and decimals |
| P-26 | Number box: PageUp/PageDown 10 increments (log 10 steps) | Controls/NumberBox.cs | 30 Number box | partial | ui/number-box.slint, src/number.rs; every box steps by Next's default 1 until piece 4 brings each field's increment and decimals |
| P-27 | Number box: wheel while focused (Shift x10) | Controls/NumberBox.cs | 30 Number box | partial | ui/number-box.slint, src/number.rs; every box steps by Next's default 1 until piece 4 brings each field's increment and decimals |
| P-28 | Number box: repeat buttons auto-repeat and focus the box | Controls/NumberBox.cs | 30 Number box | partial | ui/number-box.slint, src/number.rs; every box steps by Next's default 1 until piece 4 brings each field's increment and decimals |
| P-29 | Number box: horizontal drag scrub on unfocused box (Shift x0.1, Ctrl x10, log doubling), Esc restores | Controls/NumberBox.cs | 30 Number box | divergence | ui/number-box.slint, src/number.rs; no pointer wrap at the screen's edge (divergences.md, Number box) |
| P-30 | Number box: rounding to Decimals / three significant digits (log) | Controls/NumberBox.cs | 30 Number box | partial | ui/number-box.slint, src/number.rs; every box steps by Next's default 1 until piece 4 brings each field's increment and decimals |
| P-31 | Number box: step clamps to Min..Max, writes immediately, selects text | Controls/NumberBox.cs | 30 Number box | partial | ui/number-box.slint, src/number.rs; every box steps by Next's default 1 until piece 4 brings each field's increment and decimals |
| P-32 | Typed text free; model converts and clamps; culture decimal comma | Controls/NumberBox.cs | 30 Number box | partial | a decimal comma reads as a point when typed, stepped or scrubbed; numbers are written with a point; a typed form clamps each number to its box's range |
| P-33 | Non-parsing text counts as 0 for a step; log box starts from Minimum | Controls/NumberBox.cs | 30 Number box | partial | ui/number-box.slint, src/number.rs; every box steps by Next's default 1 until piece 4 brings each field's increment and decimals |
| P-34 | Disabled boxes grey out; row disabled by other setting greys label, no pointer/focus | Themes/Editors.xaml | 30 Number box | missing | no per-row enabled state; dependent fields are hidden from the form instead |
| P-35 | Validation message for bad input | n/a (Rust) | 30 not itemised | partial | single red error line below form: 'x is no number', EditError text |
| P-36 | Check box (live toggle) | Themes/Editors.xaml | 30 Check box | done | CheckBox row |
| P-37 | Combo box fills editor column, never narrower than widest word | Themes/Editors.xaml (EnumComboBox) | 30 Check box, combo box | partial | stretches; min 40px |
| P-38 | Colour scheme picker combo: project schemes first (tag 'project', grey), shipped+user of layer kind, gradient strip + name | Controls/ColorSchemePicker.cs | 30 Colour scheme picker | done | scheme-combo.slint SchemeRow/SchemeCombo; form.rs layer_schemes |
| P-39 | Colour scheme picker: '<name> (not found)' greyed | Controls/ColorSchemePicker.cs | 30 Colour scheme picker | done | SchemeEntry.missing, first row |
| P-40 | Colour scheme picker action: Load from file... (.wdscheme/.json), slims sampled tables | Controls/ColorSchemePicker.cs | 30 Colour scheme picker | done | SchemeEntry.load; files::pick_scheme through the inbox (both targets); form::scheme_file slims with ColorScheme::simplify |
| P-41 | Colour scheme picker action: Edit a copy in the project... | Controls/ColorSchemePicker.cs | 30 Colour scheme picker | done | form.rs find_scheme; main.rs on_edit_scheme_copy |
| P-42 | Colour scheme picker action: Remove from my schemes (user scheme only) | Controls/ColorSchemePicker.cs | 30 Colour scheme picker | done | SchemeEntry.remove-mine after the copy action while the layer's scheme resolves to a user scheme; main.rs on_remove_my_scheme |
| P-43 | Colour swatch button opens picker popup below | Controls/ColorPickerButton.cs | 30 Colour swatch button; 70 | done | color-picker.slint ColorSwatch |
| P-44 | Link toggle (chain) between two number boxes, accent underline on focus | Themes/Editors.xaml | 30 Link toggle | done | FormLink: chain-linked/chain-parted icons, accent while linked, accent border on focus, Space toggles; centred across both rows |
| P-45 | Description header toggle (chevron, Space), kind text, persisted PartDescriptionExpanded | Views/PartCommonView.xaml:39-44 | 30 Description header | done | FormDescription: chevron turns while open, Space or click, the kind's text, Settings.part_description_expanded saved |
| P-46 | Project: Format version RO (grey) | Views/ProjectView.xaml:36-37 | 30 Project | done | FormValue, grey |
| P-47 | Project: Random seed number box (integer) | Views/ProjectView.xaml:39-43 | 30 Project | done | number box, whole numbers clamped to i32 |
| P-48 | Project: New seed button (draws fresh seed) | Views/ProjectView.xaml:41 | 30 Project | partial | Button beside the box: a seed from Project::new_random_seed, one undo step; no access key (Next's _New seed) |
| P-49 | Project: Description multiline wrapping text | Views/ProjectView.xaml:46-49 | 30 Project | partial | FormNotes: a wrapping TextEdit, 60 px at least, applied per keystroke, the keystrokes one undo step (Document::edit_merging) |
| P-50 | Project: section General header | Views/ProjectView.xaml | 30 Project | done | FormSection General |
| P-51 | Scene: Mode combo (Canvas, Image) | Views/SceneView.xaml:61-62 | 30 Scene | done | FormChoice: Canvas, Image |
| P-52 | Scene: image rows hidden in Canvas mode | Views/SceneView.xaml:44,51 | 30 Scene | done | the Scene image, Image file and Embedded image rows show in Image mode only |
| P-53 | Scene: Scene image name text (per keystroke) | Views/SceneView.xaml:79 | 30 Scene | partial | FormText per keystroke, the keystrokes one undo step; no browse, save or remove buttons (P-54..P-56) |
| P-54 | Scene: '...' pick image button (embeds, switches to Image mode; PNG/BMP/JPG/TIF/GIF) | Views/SceneView.xaml:80 | 30 Scene | missing |  |
| P-55 | Scene: save-glyph button writes embedded bytes to file | Views/SceneView.xaml:83 | 30 Scene | missing |  |
| P-56 | Scene: X button drops name, path, bytes; canvas keeps size | Views/SceneView.xaml:88 | 30 Scene | missing |  |
| P-57 | Scene: Image file RO path ellipsis + '(not found)' | Views/SceneView.xaml:95-101 | 30 Scene | partial | the path, elided; no '(not found)' note: the app watches no scene image file yet |
| P-58 | Scene: Embedded image RO 'W x H, N KB' / '(unreadable)' | Views/SceneView.xaml:105-107 | 30 Scene | done | 'W x H, N KB' from the embedded bytes and the decoded size, '(unreadable)' when it has none |
| P-59 | Scene: Width NB 8..16384 (disabled in Image mode) | Views/SceneView.xaml:111-112 | 30 Scene | done | number box 8..16384, label and box disabled in Image mode |
| P-60 | Scene: Height NB 8..16384 (disabled in Image mode) | Views/SceneView.xaml:114-115 | 30 Scene | done | number box 8..16384, label and box disabled in Image mode |
| P-61 | Scene: Edges combo Open/Reflective (disabled while Damping edge < 0) | Views/SceneView.xaml:117-118 | 30 Scene; divergence rim | done | FormChoice Open, Reflective; Next no longer disables it (SceneView.xaml) |
| P-62 | Scene: Damping edge NB -8192..8192 | Views/SceneView.xaml:121-122 | 30 Scene | done | number box -8192..8192 |
| P-63 | Scene: Refractive index NB 1..2.55 step 0.01 | Views/SceneView.xaml:124-125 | 30 Scene | done | number box 1..2.55 step 0.01 |
| P-64 | Scene: Image sources check box (full width) | Views/SceneView.xaml:130 | 30 Scene | done | FormCheck, full width |
| P-65 | Scene: Image source amplitude NB 0..1000 step 0.1 (enabled w/ Image sources) | Views/SceneView.xaml:133-134 | 30 Scene | done | number box 0..1000 step 0.1, label and box enabled with Image sources |
| P-66 | Scene: Object sources check box | Views/SceneView.xaml:136 | 30 Scene | done | FormCheck, full width |
| P-67 | Scene: sections Canvas / Sources from the image | Views/SceneView.xaml:57-60,128 | 30 Scene | done | FormSection Canvas, Sources from the image |
| P-68 | Simulation: Steps NB 1..10,000,000 step 100 | Views/SimulationView.xaml:31-32 | 30 Simulation | done | number box 1..10,000,000 step 100 |
| P-69 | Simulation: Visualize every NB 1..1000 | Views/SimulationView.xaml:34-35 | 30 Simulation | done | number box 1..1000 |
| P-70 | Simulation: Wave speed NB 1e-6..1e6 step 0.01 | Views/SimulationView.xaml:43-44 | 30 Simulation | done | number box 1e-6..1e6 step 0.01 |
| P-71 | Simulation: Intensity time decay NB 1..100 drag 0.1, 1 dec | Views/SimulationView.xaml:46-47 | 30 Simulation | done | number box 1..100, drag 0.1, 1 decimal |
| P-72 | Simulation: Source frequency scale NB 0..1000 step 0.1 | Views/SimulationView.xaml:49-50 | 30 Simulation | done | number box 0..1000 step 0.1 |
| P-73 | Simulation: Intensity scale NB 1e-7..1e6 step 0.1 | Views/SimulationView.xaml:52-53 | 30 Simulation | done | number box 1e-7..1e6 step 0.1 |
| P-74 | Simulation: 4th-order Laplacian check box (full width) | Views/SimulationView.xaml:40 | 30 Simulation | done | FormCheck over the Stencil word: FourthOrder checked, NinePoint not |
| P-75 | Simulation: sections Run / Solver | Views/SimulationView.xaml:29-42 | 30 Simulation | done | FormSection Run (key SectionRun), Solver |
| P-76 | Display Field: Colour scheme picker (gradient swatch 56x12 + name, layer kind) | Views/DisplayLayerView.xaml:30-32 | 30 Display layer | done | DisplayLayerForm SchemeCombo, Field kind |
| P-77 | Display Field: Brightness NB logarithmic 0.0001..100000 | Views/DisplayLayerView.xaml:36-37 | 30 Display layer | done | number box, logarithmic 0.0001..100000 |
| P-78 | Display Field: Black level NB -10..1 step 0.01 | Views/DisplayLayerView.xaml:39-40 | 30 Display layer | done | number box -10..1 step 0.01 |
| P-79 | Display Field: Preview check box (shows picture; same as transport show toggle) | Views/DisplayLayerView.xaml:42 | 30 Display layer | done | FormCheck; the same field as the transport's show toggle |
| P-80 | Display Field: Outline in the preview check box | Views/DisplayLayerView.xaml:45 | 30 Display layer | done | FormCheck |
| P-81 | Display Field: Outline brightness NB 0..5 step 0.05 | Views/DisplayLayerView.xaml:47-48 | 30 Display layer | done | number box 0..5 step 0.05 |
| P-82 | Display Field: Outline in the export check box | Views/DisplayLayerView.xaml:49 | 30 Display layer | done | FormCheck |
| P-83 | Display Intensity: Colour scheme picker (gradient swatch 56x12 + name, layer kind) | Views/DisplayLayerView.xaml:30-32 | 30 Display layer | done | DisplayLayerForm SchemeCombo, Intensity kind |
| P-84 | Display Intensity: Brightness NB logarithmic 0.0001..100000 | Views/DisplayLayerView.xaml:36-37 | 30 Display layer | done | number box, logarithmic 0.0001..100000 |
| P-85 | Display Intensity: Black level NB -10..1 step 0.01 | Views/DisplayLayerView.xaml:39-40 | 30 Display layer | done | number box -10..1 step 0.01 |
| P-86 | Display Intensity: Preview check box (shows picture; same as transport show toggle) | Views/DisplayLayerView.xaml:42 | 30 Display layer | done | FormCheck; the same field as the transport's show toggle |
| P-87 | Display Intensity: Outline in the preview check box | Views/DisplayLayerView.xaml:45 | 30 Display layer | done | FormCheck |
| P-88 | Display Intensity: Outline brightness NB 0..5 step 0.05 | Views/DisplayLayerView.xaml:47-48 | 30 Display layer | done | number box 0..5 step 0.05 |
| P-89 | Display Intensity: Outline in the export check box | Views/DisplayLayerView.xaml:49 | 30 Display layer | done | FormCheck |
| P-90 | Display: Handles in the preview check box | Views/DisplayView.xaml:39 | 30 Display | done | FormCheck, full width |
| P-91 | Display: Labels in the preview check box | Views/DisplayView.xaml:41 | 30 Display | done | FormCheck, full width |
| P-92 | Display: Export From step NB min 0 | Views/DisplayView.xaml:47-48 | 30 Display | done | number box from 0 |
| P-93 | Display: Export To step NB min 0 (0 = end) | Views/DisplayView.xaml:50-51 | 30 Display | done | number box from 0; the model bounds it by the run |
| P-94 | Display: Probe graphs in the export check box | Views/DisplayView.xaml:53 | 30 Display | done | FormCheck, full width |
| P-95 | Display: Probe markers in the export check box | Views/DisplayView.xaml:55 | 30 Display | done | FormCheck, full width |
| P-96 | Display: Source markers in the export check box | Views/DisplayView.xaml:57 | 30 Display | done | FormCheck, full width |
| P-97 | Display: Video export Frame rate NB 1..240 | Views/DisplayView.xaml:63-64 | 30 Display | done | number box 1..240 |
| P-98 | Display: Video export Quality NB 1..100 | Views/DisplayView.xaml:66-67 | 30 Display | done | number box 1..100 |
| P-99 | Display: sections Field/Intensity/Preview/Export/Video export and on-screen order | Views/DisplayView.xaml:28-62 | 30 Display | done | Field, Intensity, Preview, Export (key SectionExportRange), Video export, in Next's order |
| P-100 | Source: Name TB | Views/SourceView.xaml:54 | 30 Source | done | SourceForm FormText, per keystroke |
| P-101 | Source: Enabled CB | Views/SourceView.xaml:56 | 30 Source | done | FormCheck, Next's Label.Source.Enabled through a @tr context |
| P-102 | Source: X NB integer (min 0) | Views/SourceView.xaml:63 | 30 Source | done | number box from 0, whole steps |
| P-103 | Source: Y NB integer (min 0) | Views/SourceView.xaml:66 | 30 Source | done | number box from 0, whole steps |
| P-104 | Source: Frequency NB 0.01..255 drag 0.1 2 dec | Views/SourceView.xaml:72 | 30 Source | done | number box 0.01..255, drag 0.1, 2 decimals |
| P-105 | Source: Amplitude NB 0..100 drag 0.1 2 dec | Views/SourceView.xaml:75 | 30 Source | done | number box 0..100, drag 0.1, 2 decimals |
| P-106 | Source: Phase NB -180..180 drag 0.5 2 dec | Views/SourceView.xaml:78 | 30 Source | done | number box -180..180, drag 0.5, 2 decimals |
| P-107 | Source: Mode combo Continuous/Pulsed | Views/SourceView.xaml:100 | 30 Source | done | FormChoice Continuous, Pulsed |
| P-108 | Source: Pulse shape combo Gaussian/Smoothed square (off when Continuous) | Views/SourceView.xaml:104 | 30 Source | done | FormChoice Gaussian, Smoothed square, off when Continuous |
| P-109 | Source: Pulse width NB min 1 (off when Continuous) | Views/SourceView.xaml:108 | 30 Source | done | number box from 1, off when Continuous |
| P-110 | Source: Off width NB min 1 (off when Continuous) | Views/SourceView.xaml:111 | 30 Source | done | number box from 1, off when Continuous |
| P-111 | Source: Detach after step NB min -1 | Views/SourceView.xaml:114 | 30 Source | done | number box from -1 (-1 = no detach) |
| P-112 | Source: Composite source CB | Views/SourceView.xaml:118 | 30 Source | done | FormCheck IsComposite: adds the array (its defaults) or drops it |
| P-113 | Source: Columns NB min 1 (array, on when Composite) | Views/SourceView.xaml:124 | 30 Source | done | number box from 1 |
| P-114 | Source: Rows NB min 1 | Views/SourceView.xaml:127 | 30 Source | done | number box from 1 |
| P-115 | Source: Horizontal spacing NB | Views/SourceView.xaml:130 | 30 Source | done | number box |
| P-116 | Source: Vertical spacing NB | Views/SourceView.xaml:133 | 30 Source | done | number box |
| P-117 | Source: Rotation NB -180..180 step 5 drag 0.5 1 dec | Views/SourceView.xaml:136 | 30 Source | done | number box -180..180 step 5, drag 0.5, 1 decimal |
| P-118 | Source: Frequency (random) NB min 0 | Views/SourceView.xaml:142 | 30 Source | done | number box from 0 |
| P-119 | Source: Phase (random) NB min 0 | Views/SourceView.xaml:145 | 30 Source | done | number box from 0 |
| P-120 | Source: Off width (random) NB min 0 | Views/SourceView.xaml:148 | 30 Source | done | number box from 0 |
| P-121 | Source: Position (random) NB min 0 | Views/SourceView.xaml:151 | 30 Source | done | number box from 0 |
| P-122 | Source: Wavelength RO '~ N.N px' (+ '(nominal)' with 9-point stencil), tooltip | Views/SourceView.xaml:81-84 | 30 Source | done | '~ N.N px' (U+2248) from model::wavelength, '(nominal)' with the 9-point stencil, '-' when none; Next's tip; the figure takes a decimal point in every language |
| P-123 | Source: sections General/Position/Wave/Timing/Array/Random offsets | Views/SourceView.xaml:49-150 | 30 Source | done | General, Position, Wave, Timing, Array, the random offsets' heading inside the array panel |
| P-124 | Source: array rows disabled unless Composite | Views/SourceView.xaml:122 | 30 Source | done | array labels and boxes disabled unless Composite |
| P-125 | Part: Name TB (section General, per keystroke) | Views/PartCommonView.xaml:55-56 | 30 Part common | done | PartCommon FormText, per keystroke |
| P-126 | Part: Enabled CB | Views/PartCommonView.xaml:58 | 30 Part common | done | PartCommon FormCheck |
| P-127 | Part: X NB min 0 | Views/PartCommonView.xaml:64-65 | 30 Part common | done | PartCommon number box from 0 |
| P-128 | Part: Y NB min 0 | Views/PartCommonView.xaml:67-68 | 30 Part common | done | PartCommon number box from 0 |
| P-129 | Part: Angle NB -180..180 step 5 drag 0.5 1 dec | Views/PartCommonView.xaml:70-71 | 30 Part common | done | PartCommon number box -180..180 step 5, drag 0.5, 1 decimal |
| P-130 | Part: kind-specific forms shown after shared rows (form dispatch per kind) | Panes/PropertiesPane.xaml:117 | 30 Forms | done | form::kind by PartKind, a typed form per kind (ui/forms.slint), rules in edited::Edited |
| P-131 | Material: Mirror CB | Views/ShapeMaterialView.xaml:51+ | 30 Glass and mirror material | done | ShapeMaterial FormCheck |
| P-132 | Material: Refractive index NB 1..2.55 step 0.01 (off when Mirror) | Views/ShapeMaterialView.xaml:51+ | 30 Glass and mirror material | done | number box 1..2.55 step 0.01, off for a mirror |
| P-133 | Material: AR coating NB 0..50 (off when Mirror) | Views/ShapeMaterialView.xaml:51+ | 30 Glass and mirror material | done | number box 0..50, off for a mirror; hidden where the kind takes no coating |
| P-134 | Material: Coating profile combo Layer/Graded (off when Mirror) | Views/ShapeMaterialView.xaml:51+ | 30 Glass and mirror material | done | FormChoice Layer, Graded, off for a mirror |
| P-135 | Material: Coat the front CB | Views/ShapeMaterialView.xaml:51+ | 30 Glass and mirror material | done | FormCheck, off for a mirror; hidden for a back-only kind |
| P-136 | Material: Coat the back CB | Views/ShapeMaterialView.xaml:51+ | 30 Glass and mirror material | done | FormCheck, off for a mirror |
| P-137 | Material: Coat the sides CB | Views/ShapeMaterialView.xaml:51+ | 30 Glass and mirror material | done | FormCheck, off for a mirror; hidden for back-only and sides-hidden kinds |
| P-138 | Material: Absorption NB 0..255 step 5 | Views/ShapeMaterialView.xaml:51+ | 30 Glass and mirror material | done | number box 0..255 step 5, off for a mirror |
| P-139 | Material: Absorption ramp NB 1..400 step 5 | Views/ShapeMaterialView.xaml:51+ | 30 Glass and mirror material | done | number box 1..400 step 5, off for a mirror |
| P-140 | Lens: ticking Mirror renames 'Lens' to 'Mirror' and back (only via the box) | Views/LensPartView.xaml | 30 Material | done | the Mirror box of a lens or Fresnel lens renames by names::set_lens_mirror, the form's box only |
| P-141 | Material section header 'Material'; applies to lens, polygon, rectangle, ellipse, triangle, outline | Views/ShapeMaterialView.xaml: | 30 Material | done | ShapeMaterial on lens, Fresnel lens (its lens's, by prefix), polygon, rectangle, ellipse, triangle, outline |
| P-142 | Lens: Diameter NB min 2 step 10 | Views/LensPartView.xaml:74 | 30 Kind-specific forms | done | LensShape number box from 2 step 10 |
| P-143 | Lens: Concave (diverging) CB (flips both radii signs) | Views/LensPartView.xaml:74 | 30 Kind-specific forms | done | FormCheck on Lens::is_concave; ticking it flips both radii, a flat one stays 0 (Lens::set_concave) |
| P-144 | Lens: R1 NB step 10 | Views/LensPartView.xaml:74 | 30 Kind-specific forms | done | number box step 10 |
| P-145 | Lens: R2 NB step 10 | Views/LensPartView.xaml:74 | 30 Kind-specific forms | done | number box step 10 |
| P-146 | Lens: Centre thickness NB min 1 | Views/LensPartView.xaml:74 | 30 Kind-specific forms | done | number box from 1 |
| P-147 | Lens: Edge thickness RO | Views/LensPartView.xaml:74 | 30 Kind-specific forms | done | FormValue from lens_geometry::edge_thickness, 0.# px, with Next's tip |
| P-148 | Fresnel lens: lens rows w/o Concave | Views/LensPartView.xaml (Fresnel) | 30 Kind-specific forms | done | LensShape with the Fresnel lens's lens by prefix, no Concave box |
| P-149 | Fresnel lens: Zones NB 1..100 | Views/LensPartView.xaml (Fresnel) | 30 Kind-specific forms | done | number box 1..100 |
| P-150 | Fresnel lens: Zone division combo Equal width/Equal depth | Views/LensPartView.xaml (Fresnel) | 30 Kind-specific forms | done | FormChoice Equal width, Equal depth |
| P-151 | Fresnel lens: Edge thickness RO (deepest zone) | Views/LensPartView.xaml (Fresnel) | 30 Kind-specific forms | done | FormValue from edge_thickness with the zones and division |
| P-152 | Polygon: Number of sides NB 3..64 | Views/PolygonPartView.xaml:35 | 30 Kind-specific forms | done | number box 3..64 |
| P-153 | Polygon: Side length NB min 2 step 10 | Views/PolygonPartView.xaml:35 | 30 Kind-specific forms | done | number box from 2 step 10 |
| P-154 | Rectangle: Length NB min 1 step 10 | Views/RectanglePartView.xaml:49-54 | 30 Kind-specific forms | done | LinkedSides number box from 1 step 10 |
| P-155 | Rectangle: Width NB min 1 step 10 | Views/RectanglePartView.xaml:49-54 | 30 Kind-specific forms | done | LinkedSides number box from 1 step 10 |
| P-156 | Rectangle: Linked chain toggle (square) | Views/RectanglePartView.xaml:49-54 | 30 Kind-specific forms | done | FormLink; Width follows Length and back while linked (PartKind::follow) |
| P-157 | Ellipse: Length NB | Views/EllipsePartView.xaml:49 | 30 Kind-specific forms | done | LinkedSides number box from 1 step 10 |
| P-158 | Ellipse: Width NB | Views/EllipsePartView.xaml:49 | 30 Kind-specific forms | done | LinkedSides number box from 1 step 10 |
| P-159 | Ellipse: Linked chain toggle (circle) | Views/EllipsePartView.xaml:49 | 30 Kind-specific forms | done | FormLink; Width follows Length and back while linked (PartKind::follow) |
| P-160 | Triangle: Type combo (Equilateral, Right isosceles, 30-60-90, Isosceles, Custom) | Views/TrianglePartView.xaml:60-75 | 30 Kind-specific forms | done | FormChoice; a preset sets its angles (PartKind::follow) |
| P-161 | Triangle: Base NB min 2 step 10 | Views/TrianglePartView.xaml:60-75 | 30 Kind-specific forms | done | number box from 2 step 10 |
| P-162 | Triangle: Angle A NB 1..178 (Isosceles, Custom) | Views/TrianglePartView.xaml:60-75 | 30 Kind-specific forms | done | number box 1..178, drag 0.5, 1 decimal, Isosceles and Custom only; Isosceles copies it to B |
| P-163 | Triangle: Angle B NB 1..178 (Custom only) | Views/TrianglePartView.xaml:60-75 | 30 Kind-specific forms | done | number box 1..178, drag 0.5, 1 decimal, Custom only |
| P-164 | Triangle: Apex angle RO | Views/TrianglePartView.xaml:60-75 | 30 Kind-specific forms | done | FormValue from Triangle::effective_angles, 0.# and a degree sign, with Next's tip |
| P-165 | Outline: Hint text | Views/PartPointsView.xaml:36-60 | 30 Kind-specific forms | done | PointList grey wrapped hint |
| P-166 | Outline: Heading row X,Y | Views/PartPointsView.xaml:36-60 | 30 Kind-specific forms | done | X and Y headings over the two columns, the remove column kept free |
| P-167 | Outline: Per point X NB, Y NB | Views/PartPointsView.xaml:36-60 | 30 Kind-specific forms | done | number box per coordinate, by Points.<index>.X/Y |
| P-168 | Outline: Per point remove button (tab cycle) | Views/PartPointsView.xaml:36-60 | 30 Kind-specific forms | done | IconButton (minus, 18 px) per row, a tab stop |
| P-169 | Outline: Add point button (appends after last) | Views/PartPointsView.xaml:36-60 | 30 Kind-specific forms | done | Add point: a step of 20 right of the last, at the centre for none (Next's NewPointStep) |
| P-170 | Diffuser: Length NB min 1 step 10 | Views/DiffuserPartView.xaml:40 | 30 Kind-specific forms | done | forms.slint DiffuserForm |
| P-171 | Diffuser: Width NB min 2 step 10 | Views/DiffuserPartView.xaml:40 | 30 Kind-specific forms | done | forms.slint DiffuserForm |
| P-172 | Diffuser: Centre wavelength NB 2..10000 | Views/DiffuserPartView.xaml:40 | 30 Kind-specific forms | done | forms.slint DiffuserForm |
| P-173 | Diffuser: Amplitude NB 0..10000 | Views/DiffuserPartView.xaml:40 | 30 Kind-specific forms | done | forms.slint DiffuserForm |
| P-174 | Diffuser: Sigma NB 0..10000 | Views/DiffuserPartView.xaml:40 | 30 Kind-specific forms | done | forms.slint DiffuserForm |
| P-175 | Diffuser: Profile height RO | Views/DiffuserPartView.xaml:40 | 30 Kind-specific forms | done | forms.slint DiffuserForm |
| P-176 | Diffuser: Ripples RO | Views/DiffuserPartView.xaml:40 | 30 Kind-specific forms | done | forms.slint DiffuserForm |
| P-177 | Interference filter: Coating rows (back box alone) | Views/InterferenceFilterPartView.xaml:49-118 | 30 Kind-specific forms | done | forms.slint InterferenceFilterForm |
| P-178 | Interference filter: Length NB min 1 step 10 | Views/InterferenceFilterPartView.xaml:49-118 | 30 Kind-specific forms | done | forms.slint InterferenceFilterForm |
| P-179 | Interference filter: Width NB min 1 step 10 | Views/InterferenceFilterPartView.xaml:49-118 | 30 Kind-specific forms | done | forms.slint InterferenceFilterForm |
| P-180 | Interference filter: Stack thickness RO | Views/InterferenceFilterPartView.xaml:49-118 | 30 Kind-specific forms | done | forms.slint InterferenceFilterForm |
| P-181 | Interference filter: Layers hint text | Views/InterferenceFilterPartView.xaml:49-118 | 30 Kind-specific forms | done | forms.slint InterferenceFilterForm |
| P-182 | Interference filter: Layers heading row (Thickness, Index) | Views/InterferenceFilterPartView.xaml:49-118 | 30 Kind-specific forms | done | forms.slint InterferenceFilterForm |
| P-183 | Interference filter: Layer grip: drag reorders, one undo for held mouse | Views/InterferenceFilterPartView.xaml:49-118 | 30 Kind-specific forms | done | forms.slint InterferenceFilterForm |
| P-184 | Interference filter: Layer ordinal | Views/InterferenceFilterPartView.xaml:49-118 | 30 Kind-specific forms | done | forms.slint InterferenceFilterForm |
| P-185 | Interference filter: Layer Thickness NB 0..1000 2 dec | Views/InterferenceFilterPartView.xaml:49-118 | 30 Kind-specific forms | done | forms.slint InterferenceFilterForm |
| P-186 | Interference filter: Layer Index NB 1..2.55 | Views/InterferenceFilterPartView.xaml:49-118 | 30 Kind-specific forms | done | forms.slint InterferenceFilterForm |
| P-187 | Interference filter: Layer up button | Views/InterferenceFilterPartView.xaml:49-118 | 30 Kind-specific forms | done | forms.slint InterferenceFilterForm |
| P-188 | Interference filter: Layer down button | Views/InterferenceFilterPartView.xaml:49-118 | 30 Kind-specific forms | done | forms.slint InterferenceFilterForm |
| P-189 | Interference filter: Layer remove button | Views/InterferenceFilterPartView.xaml:49-118 | 30 Kind-specific forms | done | forms.slint InterferenceFilterForm |
| P-190 | Interference filter: Add layer button (copy of last) | Views/InterferenceFilterPartView.xaml:49-118 | 30 Kind-specific forms | done | forms.slint InterferenceFilterForm |
| P-191 | Interference filter: Remove all button | Views/InterferenceFilterPartView.xaml:49-118 | 30 Kind-specific forms | done | forms.slint InterferenceFilterForm |
| P-192 | Simple 2D grating: Mirror/Refractive index/Absorption/Absorption ramp (no coating rows) | Views/SimpleGratingPartView.xaml:36 | 30 Kind-specific forms | done | forms.slint SimpleGratingForm |
| P-193 | Simple 2D grating: Columns NB min 1 | Views/SimpleGratingPartView.xaml:36 | 30 Kind-specific forms | done | forms.slint SimpleGratingForm |
| P-194 | Simple 2D grating: Rows NB min 1 | Views/SimpleGratingPartView.xaml:36 | 30 Kind-specific forms | done | forms.slint SimpleGratingForm |
| P-195 | Simple 2D grating: Column spacing NB min 2 | Views/SimpleGratingPartView.xaml:36 | 30 Kind-specific forms | done | forms.slint SimpleGratingForm |
| P-196 | Simple 2D grating: Row spacing NB min 2 | Views/SimpleGratingPartView.xaml:36 | 30 Kind-specific forms | done | forms.slint SimpleGratingForm |
| P-197 | Simple 2D grating: Dot diameter NB 1..half smaller spacing | Views/SimpleGratingPartView.xaml:36 | 30 Kind-specific forms | done | forms.slint SimpleGratingForm |
| P-198 | Meta lens: Coating rows w/o sides | Views/MetaLensPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-199 | Meta lens: Length NB min 1 step 10 | Views/MetaLensPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-200 | Meta lens: Width NB min 2 step 10 | Views/MetaLensPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-201 | Meta lens: Target frequency NB 1..255 | Views/MetaLensPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-202 | Meta lens: Wavelength RO | Views/MetaLensPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-203 | Meta lens: Focal length NB min 1 step 10 | Views/MetaLensPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-204 | Meta lens: Lens type combo Converging/Diverging | Views/MetaLensPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-205 | Meta lens: Zones RO | Views/MetaLensPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-206 | Meta lens: Pattern index NB 1..2.55 | Views/MetaLensPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-207 | Meta lens: Pitch NB 1..10000 | Views/MetaLensPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-208 | Meta lens: Height NB 1..10000 | Views/MetaLensPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-209 | Meta lens: Full-turn height RO | Views/MetaLensPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-210 | Meta prism: shared meta rows (Length, Width, Target frequency, Wavelength, Pattern, Pitch, Height, Full-turn height) | Views/MetaPrismPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-211 | Meta prism: Deflection NB -89..89 2 dec | Views/MetaPrismPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-212 | Meta prism: Zone width RO | Views/MetaPrismPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-213 | Meta prism: Zones RO | Views/MetaPrismPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-214 | Meta axicon: shared meta rows | Views/MetaAxiconPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-215 | Meta axicon: Cone angle NB 0.1..89 2 dec | Views/MetaAxiconPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-216 | Meta axicon: Line of focus RO | Views/MetaAxiconPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-217 | Meta axicon: Zone width RO | Views/MetaAxiconPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-218 | Meta axicon: Zones RO | Views/MetaAxiconPartView.xaml:41 | 30 Kind-specific forms | done | forms.slint MetaSurfaceForm |
| P-219 | Facetted grating: Coating rows w/o front, sides | Views/FacettedGratingPartView.xaml:36 | 30 Kind-specific forms | done | forms.slint FacettedGratingForm |
| P-220 | Facetted grating: Length NB min 1 | Views/FacettedGratingPartView.xaml:36 | 30 Kind-specific forms | done | forms.slint FacettedGratingForm |
| P-221 | Facetted grating: Width NB min 2 | Views/FacettedGratingPartView.xaml:36 | 30 Kind-specific forms | done | forms.slint FacettedGratingForm |
| P-222 | Facetted grating: Pattern combo Sawtooth/Square wave/Sinusoidal | Views/FacettedGratingPartView.xaml:36 | 30 Kind-specific forms | done | forms.slint FacettedGratingForm |
| P-223 | Facetted grating: Pitch NB 2..10000 | Views/FacettedGratingPartView.xaml:36 | 30 Kind-specific forms | done | forms.slint FacettedGratingForm |
| P-224 | Facetted grating: Amplitude NB 0..10000 | Views/FacettedGratingPartView.xaml:36 | 30 Kind-specific forms | done | forms.slint FacettedGratingForm |
| P-225 | Fibre core: Core index NB 1..2.55 step 0.01 | Views/FiberPartView.xaml:34 | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-226 | Fibre core: Core diameter NB min 2 | Views/FiberPartView.xaml:34 | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-227 | Fibre cladding: Cladding index NB 1..2.55 | Views/FiberPartView.xaml | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-228 | Fibre cladding: Cladding thickness NB min 0 | Views/FiberPartView.xaml | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-229 | Fibre cladding: Cladding absorption NB 0..255 step 5 | Views/FiberPartView.xaml | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-230 | Fibre cladding: Absorption band NB 1..400 | Views/FiberPartView.xaml | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-231 | Fibre cladding: AR coating NB 0..50 | Views/FiberPartView.xaml | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-232 | Fibre cladding: Coating profile combo | Views/FiberPartView.xaml | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-233 | Fibre cladding: Coat front/back/sides CB | Views/FiberPartView.xaml | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-234 | Fibre shape: Ends combo Flat/Round | Views/FiberPartView.xaml | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-235 | Fibre nodes: Hint text | Views/FiberPathView.xaml:38-87 | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-236 | Fibre nodes: Heading row X,Y | Views/FiberPathView.xaml:38-87 | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-237 | Fibre nodes: Node X NB, Y NB | Views/FiberPathView.xaml:38-87 | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-238 | Fibre nodes: Node Smooth CB (aligns handles, keeps length) | Views/FiberPathView.xaml:38-87 | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-239 | Fibre nodes: Node remove button | Views/FiberPathView.xaml:38-87 | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-240 | Fibre nodes: In InX NB, InY NB | Views/FiberPathView.xaml:38-87 | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-241 | Fibre nodes: Out OutX NB, OutY NB | Views/FiberPathView.xaml:38-87 | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-242 | Fibre nodes: Add node button (continues path) | Views/FiberPathView.xaml:38-87 | 30 Kind-specific forms | done | forms.slint FiberForm |
| P-243 | Bragg reflector: Layer pairs NB 1..200 | Views/BraggReflectorPartView.xaml:34 | 30 Kind-specific forms | done | forms.slint BraggReflectorForm |
| P-244 | Bragg reflector: Thickness 1 NB min 1 | Views/BraggReflectorPartView.xaml:34 | 30 Kind-specific forms | done | forms.slint BraggReflectorForm |
| P-245 | Bragg reflector: Index 1 NB 1..2.55 | Views/BraggReflectorPartView.xaml:34 | 30 Kind-specific forms | done | forms.slint BraggReflectorForm |
| P-246 | Bragg reflector: Thickness 2 NB min 1 | Views/BraggReflectorPartView.xaml:34 | 30 Kind-specific forms | done | forms.slint BraggReflectorForm |
| P-247 | Bragg reflector: Index 2 NB 1..2.55 | Views/BraggReflectorPartView.xaml:34 | 30 Kind-specific forms | done | forms.slint BraggReflectorForm |
| P-248 | Bragg reflector: Width NB min 2 step 10 | Views/BraggReflectorPartView.xaml:34 | 30 Kind-specific forms | done | forms.slint BraggReflectorForm |
| P-249 | GRIN lens: Centre index NB 1..2.55 | Views/GrinLensPartView.xaml:34 | 30 Kind-specific forms | done | forms.slint GrinLensForm |
| P-250 | GRIN lens: Edge index NB 1..2.55 | Views/GrinLensPartView.xaml:34 | 30 Kind-specific forms | done | forms.slint GrinLensForm |
| P-251 | GRIN lens: AR coating NB 0..50 | Views/GrinLensPartView.xaml:34 | 30 Kind-specific forms | done | forms.slint GrinLensForm |
| P-252 | GRIN lens: Coating profile combo | Views/GrinLensPartView.xaml:34 | 30 Kind-specific forms | done | forms.slint GrinLensForm |
| P-253 | GRIN lens: Coat front/back/sides CB | Views/GrinLensPartView.xaml:34 | 30 Kind-specific forms | done | forms.slint GrinLensForm |
| P-254 | GRIN lens: Length NB min 1 step 10 | Views/GrinLensPartView.xaml:34 | 30 Kind-specific forms | done | forms.slint GrinLensForm |
| P-255 | GRIN lens: Diameter NB min 2 step 10 | Views/GrinLensPartView.xaml:34 | 30 Kind-specific forms | done | forms.slint GrinLensForm |
| P-256 | Zone plate: Target frequency NB 1..255 | Views/ZonePlatePartView.xaml:39 | 30 Kind-specific forms | done | forms.slint ZonePlateForm |
| P-257 | Zone plate: Wavelength RO | Views/ZonePlatePartView.xaml:39 | 30 Kind-specific forms | done | forms.slint ZonePlateForm |
| P-258 | Zone plate: Focal length NB min 1 step 10 | Views/ZonePlatePartView.xaml:39 | 30 Kind-specific forms | done | forms.slint ZonePlateForm |
| P-259 | Zone plate: Inverse CB | Views/ZonePlatePartView.xaml:39 | 30 Kind-specific forms | done | forms.slint ZonePlateForm |
| P-260 | Zone plate: Diameter NB min 2 step 10 | Views/ZonePlatePartView.xaml:39 | 30 Kind-specific forms | done | forms.slint ZonePlateForm |
| P-261 | Zone plate: Thickness NB min 1 | Views/ZonePlatePartView.xaml:39 | 30 Kind-specific forms | done | forms.slint ZonePlateForm |
| P-262 | Zone plate: Anti-reflection NB 0..400 | Views/ZonePlatePartView.xaml:39 | 30 Kind-specific forms | done | forms.slint ZonePlateForm |
| P-263 | Zone plate: Mount CB | Views/ZonePlatePartView.xaml:39 | 30 Kind-specific forms | done | forms.slint ZonePlateForm |
| P-264 | Zone plate: Mount width NB min 1 step 10 (on when Mount) | Views/ZonePlatePartView.xaml:39 | 30 Kind-specific forms | done | forms.slint ZonePlateForm |
| P-265 | Zone plate: Number of zones RO | Views/ZonePlatePartView.xaml:39 | 30 Kind-specific forms | done | forms.slint ZonePlateForm |
| P-266 | Zone plate: Outermost zone RO | Views/ZonePlatePartView.xaml:39 | 30 Kind-specific forms | done | forms.slint ZonePlateForm |
| P-267 | XOR/AND/OR gate: Design frequency NB 1..255 | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-268 | XOR/AND/OR gate: Wavelength RO | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-269 | XOR/AND/OR gate: Core index NB 1..2.55 | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-270 | XOR/AND/OR gate: Cladding index NB 1..2.55 | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-271 | XOR/AND/OR gate: AR coating NB 0..50 | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-272 | XOR/AND/OR gate: AR on input side CB | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-273 | XOR/AND/OR gate: AR on output side CB | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-274 | XOR/AND/OR gate: Quarter-wave coating RO | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-275 | XOR/AND/OR gate: Port spacing NB 2..2000 step 2 (shows used spacing) | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-276 | XOR/AND/OR gate: Upper output CB (function word per gate kind) | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-277 | XOR/AND/OR gate: Lower output CB (function word per gate kind) | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-278 | XOR/AND/OR gate: Core width RO | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-279 | XOR/AND/OR gate: Coupler gap RO | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-280 | XOR/AND/OR gate: Coupler length RO | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-281 | XOR/AND/OR gate: Total length RO | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-282 | XOR/AND/OR gate: Input A RO (x, y) | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-283 | XOR/AND/OR gate: Input B RO | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-284 | XOR/AND/OR gate: Upper output port RO | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-285 | XOR/AND/OR gate: Lower output port RO | Views/LogicGatePartView.xaml:41 | 30 Kind-specific forms | done | forms.slint LogicGateForm |
| P-286 | Probe: Name TB | Views/ProbeView.xaml:110 | 30 Probe | done | ProbeForm FormText, per keystroke |
| P-287 | Probe: Active CB | Views/ProbeView.xaml:112 | 30 Probe | done | FormCheck |
| P-288 | Probe: Start after step NB min 0 | Views/ProbeView.xaml:116 | 30 Probe | done | number box from 0 |
| P-289 | Probe: Detector combo Standard/Counting/Imaging | Views/ProbeView.xaml:122 | 30 Probe | done | FormChoice Standard, Counting, Imaging |
| P-290 | Probe: Detector sensitivity NB 0..1e6 2 dec drag 0.5 (detector only) | Views/ProbeView.xaml:126 | 30 Probe | done | number box 0..1e6, 2 decimals, drag 0.5, detectors only |
| P-291 | Probe: Bin size NB 1..20 (imaging only) | Views/ProbeView.xaml:130 | 30 Probe | done | number box 1..20, the imaging detector only |
| P-292 | Probe: Layer combo Field/Intensity (disabled for detector) | Views/ProbeView.xaml:134 | 30 Probe | done | FormChoice Field, Intensity, off for a detector |
| P-293 | Probe: Measurement combo by layer (Field->Amplitude; Intensity->Value/Integrated value) | Views/ProbeView.xaml:139 | 30 Probe | done | one FormChoice per layer: Amplitude; Value, Integrated value (an offset into the model's words) |
| P-294 | Probe: Kind combo Point/Line (disabled for detector) | Views/ProbeView.xaml:143 | 30 Probe | done | FormChoice Point, Line, off for a detector |
| P-295 | Probe: A x NB min 0 | Views/ProbeView.xaml:150 | 30 Probe | done | number box from 0; a graph box on A moves with it (Probe::box_attached) |
| P-296 | Probe: A y NB min 0 | Views/ProbeView.xaml:153 | 30 Probe | done | number box from 0; a graph box on A moves with it (Probe::box_attached) |
| P-297 | Probe: B x NB min 0 (line only) | Views/ProbeView.xaml:156 | 30 Probe | done | number box from 0, line probes only |
| P-298 | Probe: B y NB min 0 (line only) | Views/ProbeView.xaml:159 | 30 Probe | done | number box from 0, line probes only |
| P-299 | Probe: Box size combo Tiny..Extra large/Custom | Views/ProbeView.xaml:165 | 30 Probe | done | FormChoice Tiny..Extra large, Custom |
| P-300 | Probe: Box x NB min 0 | Views/ProbeView.xaml:169 | 30 Probe | done | number box from 0 |
| P-301 | Probe: Box y NB min 0 | Views/ProbeView.xaml:172 | 30 Probe | done | number box from 0 |
| P-302 | Probe: Box width NB 60..4096 step 10 (custom) | Views/ProbeView.xaml:175 | 30 Probe | done | number box 60..4096 step 10, Custom only |
| P-303 | Probe: Box height NB 30..4096 step 10 (custom) | Views/ProbeView.xaml:178 | 30 Probe | done | number box 30..4096 step 10, Custom only |
| P-304 | Probe: Step axis combo Running/Full (point probe or counting) | Views/ProbeView.xaml:184 | 30 Probe | done | ScaleModeChoice Running, Full, off while the probe shows a profile (Probe::shows_profile: a line but a counting detector's); contexts keep Next's enum words apart from the menus' |
| P-305 | Probe: Window NB 10..1e6 step 10 | Views/ProbeView.xaml:188 | 30 Probe | done | number box 10..1e6 step 10 |
| P-306 | Probe: Auto-scale steps CB (point/counting) | Views/ProbeView.xaml:190 | 30 Probe | done | FormCheck, off while the probe shows a profile |
| P-307 | Probe: Auto-scale values CB | Views/ProbeView.xaml:192 | 30 Probe | done | FormCheck |
| P-308 | Probe: Value axis combo Linear/Logarithmic (intensity layer) | Views/ProbeView.xaml:196 | 30 Probe | done | FormChoice Linear, Logarithmic, off on the field layer |
| P-309 | Probe: Decades NB 1..12 (log axis) | Views/ProbeView.xaml:200 | 30 Probe | done | number box 1..12, a logarithmic axis on the intensity only (Next's IsLogarithmic) |
| P-310 | Probe: Value minimum NB step 0.1 | Views/ProbeView.xaml:203 | 30 Probe | done | number box step 0.1 |
| P-311 | Probe: Value maximum NB step 0.1 | Views/ProbeView.xaml:206 | 30 Probe | done | number box step 0.1 |
| P-312 | Probe: sections General/Measurement/Position/Graph/Axes | Views/ProbeView.xaml:105-206 | 30 Probe | done | FormSection General, Measurement, Position, Graph, Axes |
| P-313 | Probe: enabled/disabled rules by detector, layer, kind, box size, axis (styles lines 34-99) | Views/ProbeView.xaml:30-100 | 30 Probe | done | Next's DataTriggers as enabled bindings; an edit coerces the detector, layer, measurement, kind and value scale as Next's setters (Probe::coerce) |
| P-314 | Scheme: Name TB (renaming repoints layers; tree row live) | Views/ColorSchemeView.xaml:71-72 | 30 Colour scheme | done | forms.slint SchemeForm: Name per keystroke, the tree row live; View::rename_scheme repoints the layers in the same undo step |
| P-315 | Scheme: Kind combo Unspecified/Field/Intensity | Views/ColorSchemeView.xaml:75-76 | 30 Colour scheme | done | forms.slint SchemeForm: SchemeKindChoice |
| P-316 | Scheme: Description TB wraps, no newline | Views/ColorSchemeView.xaml:79-80 | 30 Colour scheme | partial | single-line FormText per keystroke; no wrapping |
| P-317 | Scheme: Add to / Remove from my schemes button | Views/ColorSchemeView.xaml:(Rust) | 30 Colour scheme; divergences 'User colour schemes' | done | SchemeForm button below Description, label by Form.mine; State::toggle_mine; hidden without a store |
| P-318 | Scheme: Based on combo with gradient+name rows (copies stops/kind/category/description) | Views/ColorSchemeView.xaml:87-95 | 30 Colour scheme | partial | scheme-combo.slint SchemeCombo: strip and name rows, the project's other schemes then the built-in ones; the user's schemes are not loaded yet |
| P-319 | Scheme: gradient bar with stop markers; drag marker >24 px away removes; double-click removes; last stop stays | Views/ColorSchemeView.xaml:(bar) | 30 Colour scheme | done | forms.slint GradientBar: drag moves (one undo step a gesture), pulled away or double-clicked removes, a press on the gradient adds and drags; keys as Next |
| P-320 | Scheme: stop row Position NB 0..1 step 0.01, 3 dec, 64 px | Views/ColorSchemeView.xaml:112 | 30 Colour scheme | done | forms.slint SchemeForm: 0..1, step 0.01, 3 decimals; 72 px, as 64 clips 0.375 with our line edit's padding |
| P-321 | Scheme: stop row colour swatch button | Views/ColorSchemeView.xaml:115 | 30 Colour scheme | done | ColorSwatch per stop row |
| P-322 | Scheme: stop row hex TB #RRGGBB (Enter applies) | Views/ColorSchemeView.xaml:117 | 30 Colour scheme | done | HexField: Enter or focus loss applies; text that does not read restores the colour |
| P-323 | Scheme: stop row remove button | Views/ColorSchemeView.xaml:110 | 30 Colour scheme | done | remove button, disabled on the last stop |
| P-324 | Scheme: Add stop button (widest gap, gradient colour) | Views/ColorSchemeView.xaml:125 | 30 Colour scheme | done | ColorScheme::add_stop (core) |
| P-325 | Scheme: sections General / Stops | Views/ColorSchemeView.xaml:67-125 | 30 Colour scheme | done | forms.slint SchemeForm |

## Preview, timeline, runs, messages

Slint code read: ui/preview.slint, timeline.slint, messages.slint, app.slint; src/live.rs, playback.rs, messages.rs, log.rs, main.rs, document.rs, gpu.rs (all under frontends/waveduderust_slint). Next paths relative to WaveDudeNext.WPF. Spec files: docs/ui/40, 50, 60, 70 (Messages pane, Status bar).
Status: done / partial / missing / divergence. "not in spec" = found in Next, absent from docs/ui.
No allowed divergence covers the Slint gaps below; docs/divergences.md and decisions were grepped for zoom, touch, spinner, dither, discard, memory warning and nothing excuses omission (zoom/pan/pinch are port additions, not in Next).

### V: Preview pane

| ID | Feature | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| V-01 | Two picture cells Field + Intensity | Panes/PreviewPane.xaml:553,724,750 | 40 Layout | done | preview.slint Picture x2 |
| V-02 | Stacked (rows, field above) default / side by side (columns) | PreviewPane.xaml:553-700 (Orientation triggers) | 40 Layout | partial | `stacked` property drives layout; persisted (fixed in b20dc53) |
| V-03 | PreviewOrientation setting loaded at start | PreviewPane.xaml.cs | 40 Layout | done | main.rs:256 |
| V-04 | Splitter between cells, 4 px, tab stop | PreviewPane.xaml:747 | 40 Layout | missing | fixed 4 px spacing only, no splitter |
| V-05 | Splitter drag / arrow keys resize | PreviewPane.xaml:668 (KeyboardIncrement 10) | 40 Layout | missing | |
| V-06 | Splitter double-click / Enter resets to half | PreviewPane.xaml.cs:1314,1317 | 40 Layout | missing | |
| V-07 | Splitter tooltip text | PreviewPane.xaml:747 | 40 Layout | missing | |
| V-08 | Splitter hover / focus colour (hover bg, accent on focus) | PreviewPane.xaml:690-700 | 40 Layout (not stated) | missing | not in spec as visuals |
| V-09 | Hidden picture gives room to other, splitter collapses, restore previous split | PreviewPane.xaml | 40 Layout | missing | no show toggles, both always shown |
| V-10 | Picture fitted uniformly, centred, 4 px margin, cell hugs picture | Controls/PictureFrame.cs:49 | 40 Layout | partial | Image contain, no 4 px margin, cell not sized to picture so edge line cannot hug |
| V-11 | Without run/frame: black at canvas size; image scene shows scene image | PreviewPane.xaml.cs:828 | 40 Layout | partial | black rect; no canvas-size black frame; image scene not shown before first frame (unverified) |
| V-12 | Header strip title ("Field"/"Intensity" semi-bold) | PreviewPane.xaml:560-600, Controls/HeaderStrip.cs | 40 Header strip | partial | plain title text, not semi-bold |
| V-13 | Header caption "Brightness" | PreviewPane.xaml:586 | 40 Header strip | missing | |
| V-14 | Header Brightness number box 64 px, log, 0.0001..100000, = Display brightness | PreviewPane.xaml:586, Controls/NumberBox.cs | 40 Header strip | missing | |
| V-15 | Brightness box tooltip (brightness hint) | PreviewPane.xaml:588 | 40 Header strip | missing | |
| V-16 | Brightness box keys: Up/Down 2 pct, PageUp/PageDown a fifth, 3 significant digits | Controls/NumberBox.cs | 40 Header strip | missing | |
| V-17 | Auto button (one-shot 99.5th percentile fit) | PreviewPane.xaml:591, MainWindow.Display.cs | 40 Header strip | missing | |
| V-18 | Auto: 8-bit recompute in 16-bit for fit alone, run paused meanwhile | MainWindow.Display.cs | 40 Header strip | missing | |
| V-19 | Auto is an ordinary brightness edit (8-bit: simulation edit); "nothing to fit" status text | MainWindow.Display.cs | 40 Header strip | missing | |
| V-20 | Header placement above picture when cell taller than aspect (4 px over) | PreviewPane.xaml:560-600 | 40 Header strip | partial | always above, full-width row, not 4 px over picture |
| V-21 | Header turned a quarter clockwise on picture's left when cell wider (10 px below top) | HeaderStrip.cs | 40 Header strip | missing | |
| V-22 | Turned box edits upright while focused (click/Tab/context menu focus, select text, arrows/wheel step) | NumberBox.cs | 40 Header strip; 00 Turned brightness box TODO | missing | spec TODO |
| V-23 | Turned unfocused box: arrows step, drag along strip scrubs | NumberBox.cs | 40 Header strip | missing | |
| V-24 | Narrow strip: caption dropped first, then title ellipsis; 12 px min gap | HeaderStrip.cs | 40 Header strip | missing | |
| V-25 | Edge line 1 px, blue #0000FF when damping rim outside canvas | PreviewPane.xaml:602-610 | 40 Edge line | missing | |
| V-26 | Edge line red #FF0000 for reflective edges (rim wins over wall) | PreviewPane.xaml:602-610 | 40 Edge line | missing | |
| V-27 | Edge line none for open edges / damping band inside | PreviewPane.xaml:602 | 40 Edge line | done | nothing drawn (trivially) |
| V-28 | Edge line preview only, never exported | PreviewPane.xaml | 40 Edge line | done | nothing drawn |
| V-29 | Zoom: wheel 1.25x per notch, min fit, max 64x, point under pointer fixed, shared by both pictures | none (no zoom in Next) | 40 Zoom | missing | (port) addition |
| V-30 | Pan: middle-button drag on zoomed view, no empty area | none | 40 Zoom | missing | (port) |
| V-31 | Touch: pinch zoom, two-finger pan, second finger cancels drag/marquee | none | 40 Zoom | divergence | decision 0033, dropped |
| V-32 | Zoomed 16-bit picture rendered for visible window only | none | 40 Zoom | missing | (port) |
| V-33 | Zoom is session state, kept across projects | none | 40 Zoom | missing | (port) |
| V-34 | Picture changes only when frame for position arrives; never black/other frame | PreviewPane.xaml.cs:716 | 40 What a picture shows | done | live.rs poll draws stored frame only on tag change; request() resets `drawn` but images persist in window |
| V-35 | Busy dim (black 45 pct) after 150 ms of frame computation | PreviewPane.xaml.cs:250,385; xaml:~655 | 40 What a picture shows | missing | |
| V-36 | Busy spinner (3/4 ring, 1 turn/s), not hit-testable | PreviewPane.xaml Spinner | 40 What a picture shows | missing | |
| V-37 | Overlay z-order: picture, outline, handles, graph boxes, busy dim | PreviewPane.xaml:612-660 | 40 What a picture shows | partial | picture + glowing-edge outline baked in by gpu.rs; no other layers |
| V-38 | Scene outline (glowing edges) when enabled for layer | SceneBitmaps.cs, PreviewPane.xaml:620 | 40 What a picture shows | done | live.rs `edges` via Look (commit d1d244c) |
| V-39 | 8-bit samples: palette indices at store resolution, brightness baked at run start | FramePresenter.cs | 40 8-bit vs 16-bit | partial | gpu.rs colours stored frame on GPU; sample-depth rules and brightness-edit-as-sim-edit not wired (no brightness control) |
| V-40 | 16-bit samples: exact device-pixel render, box/bilinear resample, instant recolour on brightness | FramePresenter.cs | 40 8-bit vs 16-bit | partial | GPU render exists; render at device size and brightness recolour unverified/no control |
| V-41 | Dithering 16-bit only, Bayer 2/4/8/16 | FramePresenter.cs | 40 8-bit vs 16-bit | missing | no dither code in slint crate |
| V-42 | GPU engine colouring in render pass, device copy of last frame, others uploaded | none (Rust only) | 40 8-bit vs 16-bit | done | live.rs device_frames + Uploads |
| V-43 | WebGL2 / over-limit fallback with Messages warning | none | 40 8-bit vs 16-bit | missing | |
| V-44 | Handle style: light grey over dark halo; selected bright yellow; focused accent fill; hover/focus thicker | Controls/MarkerThumb.cs:69-115 | 40 Handles | divergence | decision 0033, matched in general, not to the pixel/id |
| V-45 | Glyphs odd-sized (21/15/13 px), centre pixel, 1.5 px AA strokes | MarkerThumb.cs | 40 Handles | divergence | decision 0033, matched in general, not to the pixel/id |
| V-46 | Source position handle (ring + 4 ticks + dot) both pictures | SceneOverlay.cs:449 | 40 Handles | missing | |
| V-47 | Composite emitter dots (selected source's in selection colour, not focusable) | SceneOverlay.cs:140,1092 | 40 Handles | missing | |
| V-48 | Source rotation lever (arc arrow, dotted lever line), current item only | SceneOverlay.cs:136-139,449 | 40 Handles | missing | |
| V-49 | Part centre handle (ring+inner ring+ticks) | SceneOverlay.cs:456 | 40 Handles | missing | |
| V-50 | Part outline in selection colour for selected parts | SceneOverlay.cs:1097-1122 | 40 Handles | missing | |
| V-51 | Part rotation lever | SceneOverlay.cs:456 | 40 Handles | missing | |
| V-52 | Part size grip (15 px filled square) corner/vertex | SceneOverlay.cs:459 | 40 Handles | missing | |
| V-53 | Lens surface handles (2) | SceneOverlay.cs:464-465 | 40 Handles | missing | |
| V-54 | Outline point handles | SceneOverlay.cs:477 | 40 Handles | missing | |
| V-55 | Fibre node handles | SceneOverlay.cs:499 | 40 Handles | missing | |
| V-56 | Fibre Bezier in/out handles + dotted lines; first in / last out omitted | SceneOverlay.cs:493-497,1134-1152 | 40 Handles | missing | |
| V-57 | Probe point A / end B marker (square+ticks+dot), only on probe's layer | SceneOverlay.cs:952, SourceMarkerPainter.cs | 40 Handles | missing | |
| V-58 | Line probe: dark line under dashed light line A..B | SceneOverlay.cs:1064 | 40 Handles | missing | |
| V-59 | Labels: item name beside handle (below for composites/parts), dark halo, kept in picture | SceneOverlay.cs:131,1189 | 40 Handles | missing | |
| V-60 | Display > Preview "Handles in preview" setting (off hides all but selected; hidden leave Tab cycle) | SceneOverlay HandlesInPreview | 40 Handles | missing | |
| V-61 | Display > Preview "Labels in preview" setting | SceneOverlay LabelsInPreview | 40 Handles | missing | |
| V-62 | Graph boxes unaffected by handles/labels settings | ProbeGraphOverlay.cs | 40 Handles | missing | |
| V-63 | Z-order rules: boxes over handles, selected drawn last of kind, tab order kept | PreviewPane.xaml:632 | 40 Handles | missing | |
| V-64 | Rotation lever may extend beyond picture (clipped); other handles clamped inside canvas | SceneOverlay.cs | 40 Handles | missing | |
| V-65 | Click handle: select item (tree/properties follow) | SceneOverlay.cs:1380-1428 | 40 Interaction | missing | |
| V-66 | Ctrl+click toggle in selection, no drag | SceneOverlay.cs:1399 | 40 Interaction | missing | |
| V-67 | Shift+click add to selection, no drag | SceneOverlay.cs:1410 | 40 Interaction | missing | |
| V-68 | Drag handle moves it, live coordinates, measured from press (no clamp drift) | SceneOverlay.cs:1457 | 40 Interaction | missing | |
| V-69 | Drag one of several selected: group move, limited so none leaves canvas; no-move drag selects item | SceneOverlay.cs:1446, Movable.Limit | 40 Interaction | missing | |
| V-70 | Drag rotation lever, whole degrees, Shift snaps 15, angle in (-180,180] | SceneOverlay.cs:850,1467 | 40 Interaction | missing | |
| V-71 | Drag size grip: absolute, whole px, min size, linked rect/ellipse farther coord, polygon on ray | SceneOverlay.cs | 40 Interaction | missing | |
| V-72 | Drag lens surface handle (bulge/flatten/hollow, rim fixed) | SceneOverlay.cs | 40 Interaction | missing | |
| V-73 | Drag fibre node (with handles) / Bezier handle (smooth node mirrors length) | SceneOverlay.cs MirrorLength | 40 Interaction | missing | |
| V-74 | Drag probe A onto detached graph box picks it up | SceneOverlay.cs:952 | 40 Interaction | missing | |
| V-75 | Esc during drag puts item/group back; second finger likewise | SceneOverlay.cs:1507-1517 | 40 Interaction | missing | |
| V-76 | Click empty space deselects (unless Shift/Ctrl), focuses pane | PreviewPane.xaml.cs:1289 | 40 Interaction | missing | |
| V-77 | Marquee on empty drag (faint accent fill, dashed edge); Shift add, Ctrl toggle, else replace | SceneOverlay.cs:1256-1365 | 40 Interaction | missing | |
| V-78 | Tab through handles; focusing selects item, tree mirrors without focus | SceneOverlay.cs | 40 Interaction | missing | |
| V-79 | Arrows / Shift+Arrows on focused handle: 1 / 10 px (group), lever 1 / 10 deg (Right/Up ccw) | SceneOverlay.cs:1527-1565 | 40 Interaction | missing | |
| V-80 | Lens surface handle keeps rim fixed across key presses | SceneOverlay.cs:1560 (DragAnchors) | 40 Interaction | missing | |
| V-81 | Del on focused handle/box removes item or its selection | SceneOverlay.cs:1517-1523 | 40 Interaction | missing | main.rs "delete" works from tree selection only |
| V-82 | Right-click / Apps: picture context menu | PreviewPane.xaml:728-771 | 40 Interaction; 10 | missing | |
| V-83 | Context menu: Brightness (focus box), Auto brightness | PreviewPane.xaml:736-737,762-763; PreviewPane.xaml.cs:1347 | 40 Automation ids | missing | |
| V-84 | Context menu: Export this frame (field Ctrl+Shift+F / intensity Ctrl+Shift+G) | PreviewPane.xaml:739,765 | 40 Automation ids; 10 | missing | exports exist in File menu only |
| V-85 | Context menu: Export field / intensity / both | PreviewPane.xaml:741-743,767-769 | 40 Automation ids | missing | File menu has them |
| V-86 | Drop of objects from tree: paste and place at drop pixel | SceneOverlay.cs:1212,1228 | 40 Interaction; 20 | missing | |
| V-87 | Drop of file not claimed, window opens it | SceneOverlay.cs | 40 Interaction | missing | see R-30 |
| V-88 | Handle tooltips (Tip*Format per role) | SceneOverlay.cs:528-531 | 40 Interaction | missing | |
| V-89 | Picture hover costs nothing; source layer cached as bitmap | SceneOverlay.cs:199-234 | 40 Interaction | missing | n/a without overlay |
| V-90 | Graph box: translucent dark box, grid, value labels, step labels, curve, 11 px text | ProbeGraphDrawing.cs, ProbeGraphPainter.cs | 40 Probe graph box | missing | |
| V-91 | Graph title strip: probe name + live readout "= v" (point) / "max v" (line), 3 sig digits | ProbeGraphDrawing.cs | 40 Probe graph box | missing | |
| V-92 | Graph layout shared with export (graph.rs) | ProbeGraphPainter.cs | 40 Probe graph box | missing | export side lives in core crate |
| V-93 | Box x/y in canvas px, size presets Tiny 200x100..XL / Custom; shrink to fit; whole px | ProbeGraphOverlay.cs | 40 Probe graph box | missing | |
| V-94 | Point probe: value over steps; Running window or Full; auto-scale steps | ProbeGraphDrawing.cs | 40 Probe graph box | missing | |
| V-95 | Line probe: values along A..B at frame shown | ProbeGraphDrawing.cs | 40 Probe graph box | missing | |
| V-96 | Value axis: field linear symmetric; intensity linear or log (Decades); auto-scale or min/max | ProbeGraphDrawing.cs | 40 Probe graph box | missing | |
| V-97 | Floor rule (1/brightness, 1/brightness^2, integrated), 1.08 headroom | ProbeGraphDrawing.cs | 40 Probe graph box; divergences.md:49 | missing | divergence row 49 allows the floor |
| V-98 | Graph data: live run recording cut at frame shown; remake after probe edit | ProbeGraphOverlay.cs | 40 Probe graph box | missing | |
| V-99 | Selected probe's box accent border, drawn last | ProbeGraphOverlay.cs | 40 Probe graph box | missing | |
| V-100 | Drag box (kept in canvas); snap to probe A within radius 14, release at 28 (hysteresis) | ProbeGraphOverlay.cs:102,105,495-502 | 40 Probe graph box | missing | |
| V-101 | Box bottom-right corner grip resize, snaps to fixed sizes else Custom; arrows 1/10 px | ProbeGraphOverlay.cs:478-484 | 40 Probe graph box | missing | |
| V-102 | Arrows on box move 1/10 px (no snap); Tab reaches boxes; focus selects probe | ProbeGraphOverlay.cs:540-561 | 40 Probe graph box | missing | |
| V-103 | Moving box never touches run; moving probe keeps frames, retraces | ProbeGraphOverlay.cs | 40 Probe graph box | missing | |
| V-104 | Graph box context menu "Export probe data as CSV..." | ProbeGraphOverlay.cs:273-278 | 40 Probe graph box | missing | |
| V-105 | Box hover and focus border | ProbeGraphOverlay.cs | 40 Probe graph box | missing | |
| V-106 | Busy status "Computing frame n" + indeterminate bar mirrored on status bar | MainWindow.Live.cs | 40 Busy; 70 Status bar | missing | see L-50 |
| V-107 | Timeline shades frames held by live run | TimelineBar.cs:315 | 40 Busy and stale | done | see L-08 |
| V-108 | Show/hide each picture toggle (transport) feeds Display > Preview | PreviewPane.xaml:497-506 | 50 Transport #12,13 | missing | see L-37 |
| V-109 | Pane Ctrl+A select all inside the pane | PreviewPane.xaml:398 | not in spec | missing | not in spec; app-level Ctrl+A menu is log-only |
| V-110 | Letter keys swallowed in pane so WPF does not open menu mnemonics | PreviewPane.xaml.cs:1262-1272 | not in spec | n/a | WPF-only quirk; not applicable to Slint |
| V-111 | Brightness numbers per layer from Display > layer | PreviewPane.xaml:586 | 40 Header strip | missing | duplicate of V-14 binding; Properties pane edits Display (30) |
| V-112 | Automation ids PreviewFieldImage, PreviewIntensityImage, PreviewSplitter | PreviewPane.xaml | 40 Automation ids | divergence | decision 0033, matched in general, not to the pixel/id |

### L: Timeline, transport, status bar

| ID | Feature | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| L-01 | Timeline 36 px high, 14 px label row + 22 px bar, 4,2 margin | Controls/TimelineBar.cs:42,45 | 50 Timeline | divergence | decision 0033, matched in general, not to the pixel/id |
| L-02 | Tick labels every 1,2,5,10,...,10000 frames, min 64 px apart, 2 px after tick, last kept inside | TimelineBar.cs:48,53,429-476 | 50 Timeline | divergence | decision 0033, matched in general, not to the pixel/id |
| L-03 | Major ticks 6 px, minor 3 px every fifth of interval | TimelineBar.cs:467,474 | 50 Timeline | divergence | decision 0033, matched in general, not to the pixel/id |
| L-04 | Playhead line through bar | TimelineBar.cs:347 | 50 Timeline | done | 1 px line |
| L-05 | Playhead 10x6 triangle on top | TimelineBar.cs:355 | 50 Timeline | missing | |
| L-06 | Playhead frame number 8 px beside, patch hides tick labels | TimelineBar.cs:358-359 | 50 Timeline | partial | number 8 px beside, kept in bounds; no background patch (no ticks yet) |
| L-07 | In/out translucent accent band once range narrower than run | TimelineBar.cs:320 | 50 Timeline | partial | band + 2 px border box instead of bracket shapes |
| L-08 | In/out brackets (2 px bar, 6 px feet into range) | TimelineBar.cs:333-334,385-391 | 50 Timeline | missing | |
| L-09 | Frames-made shaded bands in accent (grow from position outward after edit) | TimelineBar.cs:315, MainWindow.Live.cs | 50 Timeline | partial | single band from frame 0 to newest made; not per-frame bands/gaps |
| L-10 | Playhead bounded by band it is in for user-started run | MainWindow.Live.cs | 50 Timeline | partial | playback.tick holds at newest made frame |
| L-11 | Hover line under pointer | TimelineBar.cs:341,518-525 | 50 Timeline | missing | |
| L-12 | Frame 0 on first pixel column inside outline, last frame on last; whole device-pixel lines | TimelineBar.cs:271-360 | 50 Timeline | divergence | decision 0033, matched in general, not to the pixel/id |
| L-13 | Timeline tooltip text | PreviewPane.xaml:546 | 50 Timeline | missing | |
| L-14 | Click/drag on bar scrubs to nearest frame; turns Live off, pauses playback | TimelineBar.cs:501-531 | 50 Timeline input | done | Timeline TouchArea + playback.seek |
| L-15 | Drag bracket moves in/out (hit +/- px, nearer wins) | TimelineBar.cs:51,501-510 | 50 Timeline input | missing | |
| L-16 | Bracket hover cursor SizeWE | TimelineBar.cs:524 | not in spec | missing | not in spec |
| L-17 | Left / Right one frame | TimelineBar.cs:596-612 | 50 Timeline input | done | when bar focused |
| L-18 | PageUp/PageDown ten frames (PageUp forward) | TimelineBar.cs:596-612 | 50 Timeline input | done | |
| L-19 | Home / End to in / out (start / end unset) | TimelineBar.cs:596-612; PreviewPane.xaml:379-380 | 50 Timeline input | done | uses range() |
| L-20 | Home/End as GoToStart/GoToEnd commands from anywhere in pane (not in text box) | PreviewPane.xaml:379-380; PreviewPane.xaml.cs:1200,1202 | not in spec (spec: timeline row only) | missing | not in spec |
| L-21 | Mouse wheel one frame | TimelineBar.cs:554 | 50 Timeline input | done | scroll-event |
| L-22 | Space play/pause | PreviewPane.xaml:390 | 50 Timeline input; Playback | partial | only when timeline focused or F7; not "anywhere in pane" |
| L-23 | Click timeline focuses it; focus border = accent | TimelineBar.cs:364,617 | 50 Timeline | done | |
| L-24 | Automation: slider with range value, "range in..out" | TimelineBar.cs:632-660 | 50 Timeline | divergence | decision 0033, matched in general, not to the pixel/id |
| L-25 | Run/Stop button (ring+triangle / ring+square), tab 0 | PreviewPane.xaml:413-430 | 50 Transport #0 | partial | text glyphs ">" "[]"; no ring icon |
| L-26 | Run tooltip "Run the simulation from the start (F5)" / Stop tooltip "(Esc)" | PreviewPane.xaml:418,424 | 50 Transport #0 | missing | `tip` property declared in Tool but never rendered |
| L-27 | Run disabled while export runs / without enabled source | SimulationCommands.cs | 50 Transport #0 | missing | button enabled always |
| L-28 | Run starts full run from 0 and follows it; same button stops | MainWindow.Simulation.cs | 50 Transport #0 | done | main.rs menu "run" / "stop" |
| L-29 | Esc stops run | PreviewPane.xaml:424 tip; SimulationCommands.cs | 50 Transport #0 | missing | only Shift+F5 menu; no Esc binding |
| L-30 | Pause run button (ring + two bars), tab 1, tooltip Pause/Resume (F6) | PreviewPane.xaml:433-442 | 50 Transport #1 | partial | glyph "||", enabled when running; tooltip not shown, Resume text not swapped |
| L-31 | Pause run disabled without active run; toggles | PreviewPane.xaml:433 | 50 Transport #1 | done | enabled: t.running; paused state may report not running (unverified) |
| L-32 | Divider after Pause run | PreviewPane.xaml:443 | 50 Transport bar | done | Divider |
| L-33 | Play button; tooltip "Play (Space)"; disabled while playing | PreviewPane.xaml:448-450 | 50 Transport #2 | partial | glyph ">", disabled while playing; tooltip not shown |
| L-34 | Pause button; disabled while not playing | PreviewPane.xaml:451-453 | 50 Transport #3 | partial | glyph "||"; tooltip not shown |
| L-35 | Play/Pause by keyboard moves focus to the other button | PreviewPane.xaml.cs:1180-1188 | 50 Transport #3 | missing | Tool has no focus/tab handling |
| L-36 | Previous frame (",") / Next frame (".") buttons and keys | PreviewPane.xaml:455-460,391-392 | 50 Transport #4,5 | partial | buttons done ("|<" ">|" glyphs); keys , . not bound |
| L-37 | Set in point (I), tooltip; in past out clears out | PreviewPane.xaml:462; PreviewPane.xaml.cs:1214 | 50 Transport #6 | partial | button + playback.set_in done; key I not bound; tooltip not shown |
| L-38 | Set out point (O); last frame stored as 0; out before in clears in | PreviewPane.xaml.cs:1229 | 50 Transport #7 | partial | set_out done (last frame -> None); key O not bound |
| L-39 | Clear range (X), disabled when no range | PreviewPane.xaml.cs:1244 | 50 Transport #8 | partial | button + enabled rule done; key X not bound |
| L-40 | Loop toggle (L) | PreviewPane.xaml:471; PreviewPane.xaml.cs:1250 | 50 Transport #9 | partial | toggle done (glyph "L"); key L not bound |
| L-41 | Live toggle (dot in ring, F), disabled without active run | PreviewPane.xaml:474-477; xaml.cs:1255 | 50 Transport #10 | partial | button enabled: running; key F not bound; F after run ended not guarded |
| L-42 | Readout "frame / last" | PreviewPane.xaml:515-523 | 50 Transport readout | done | |
| L-43 | Readout "step N" dimmer | PreviewPane.xaml:524-533 | 50 Transport readout | done | text-dim; empty when frame missing |
| L-44 | Readout tooltips ("Frame shown / last frame", "Simulation step of the frame shown") | PreviewPane.xaml:515,524 | 50 Transport readout | missing | |
| L-45 | Speed label "Speed" margins 8,0,4,0 | PreviewPane.xaml:514 | 50 Transport #11 | done | |
| L-46 | Speed editable combo 64 px, presets 0.25,0.5,1,1.5,2,4 | PreviewPane.xaml:508 | 50 Transport #11 | partial | non-editable ComboBox, presets right |
| L-47 | Typed speed 0.01..100, culture decimal separator | PreviewPane.xaml.cs, DoubleText converter | 50 Transport #11 | partial | clamp 0.01..100 in main.rs; can only pick presets |
| L-48 | Space inside speed box swallowed | PreviewPane.xaml.cs:1274 | 50 Transport #11 | n/a | box not editable |
| L-49 | Speed tooltip | PreviewPane.xaml:509 | 50 Transport #11 | missing | |
| L-50 | Show field toggle (wave icon) = Display > Field > Preview | PreviewPane.xaml:503-506 | 50 Transport #12 | missing | |
| L-51 | Show intensity toggle (sun icon) = Display > Intensity > Preview | PreviewPane.xaml:497-501 | 50 Transport #13 | missing | |
| L-52 | Layout radios side-by-side / stacked, tab stop = checked one | PreviewPane.xaml:483-495 | 50 Transport #14 | partial | two toggle Tools; not radios, both tab-less, no tooltip |
| L-53 | Radio arrow keys move check to neighbour | PreviewPane.xaml.cs:1332-1345 | not in spec (only "Enter/Space checks") | missing | not in spec |
| L-54 | Right-docked items at right end; dividers before picture toggles | PreviewPane.xaml:480-510 | 50 Transport bar | partial | right end done; dividers 1 px with 4,2 margins not honoured (no margins) |
| L-55 | Bar geometry: 1 px top border, padding 4,3, buttons 22x22, side margins 1 | PreviewPane.xaml:404-412 | 50 Transport bar | divergence | decision 0033, matched in general, not to the pixel/id |
| L-56 | Toggles stay pressed while checked; disabled look grey; focus = accent border | PreviewPane.xaml:100-160 | 50 Transport bar | partial | checked tint and grey text; no focus border, buttons not focusable |
| L-57 | Tab order 0..14 + timeline, brightness, splitter order | PreviewPane.xaml TabIndex | 50 Transport bar | divergence | decision 0033, matched in general, not to the pixel/id |
| L-58 | Speed 1 = project frame rate, wall-clock accumulation | PreviewPane.xaml.cs:1134-1178 | 50 Playback | done | playback.tick, 16 ms timer |
| L-59 | ~60 ticks/s, due frames moved, no pile-up while rendering | PreviewPane.xaml.cs:1144 | 50 Playback | partial | 16 ms timer, carried fraction; no "previous picture still rendering" accumulate check |
| L-60 | Clock not starved by mouse input | PreviewPane.xaml.cs (thread timer) | 50 Playback | partial | slint::Timer; starvation not verified |
| L-61 | Playback between in/out, stops at out or wraps with Loop | PreviewPane.xaml.cs:1150-1178 | 50 Playback | done | tests in playback.rs |
| L-62 | Play with frames missing starts automatic run; memory warning may ask; cancel turns playback off | MainWindow.Simulation.cs | 50 Playback | partial | live.fill() starts the run; no memory warning |
| L-63 | Playback holds at newest frame while run catches up | PreviewPane.xaml.cs:1144 | 50 Playback | done | tick min(made) |
| L-64 | Run ended short plays to last frame as to out point | PreviewPane.xaml.cs | 50 Playback | partial | holds at made; end handled via range, unverified |
| L-65 | Browser: read-ahead from spill for playback (decision 0020) | none | 50 Playback | missing | web.rs spill keeps nothing |
| L-66 | Space/F7 from anywhere; click on picture focuses pane | PreviewPane.xaml.cs:1289 | 50 Playback | partial | F7 menu done; click-to-focus missing |
| L-67 | Live: position follows newest frame of active run | MainWindow.Live.cs | 50 Live follow | done | playback.live + tick |
| L-68 | User run: frame 0, Live on, playback stops | MainWindow.Simulation.cs | 50 Live follow | done | playback.follow |
| L-69 | At run end one last follow then Live off | MainWindow.Live.cs | 50 Live follow | done | main.rs timer |
| L-70 | Scrub/step/Play turn Live off; F / button re-enables only while run active | PreviewPane.xaml.cs:1255 | 50 Live follow | partial | seek/play clear live; toggle unguarded in code (button disabled) |
| L-71 | Automatic re-run never followed, never moves position | MainWindow.Live.cs | 50 Live follow | done | live.edited keeps position |
| L-72 | Position rules: New/open/example/close -> frame 0, playback stops | MainWindow.Project.cs | 50 Position rules | done | state.opened -> playback.reset |
| L-73 | Run stopped: preview moves to newest frame if following; Live off | MainWindow.Live.cs | 50 Position rules | partial | Live turned off; no jump to newest |
| L-74 | Run discarded: position kept | MainWindow.Live.cs | 50 Position rules | n/a | discard missing (R-24) |
| L-75 | Frame missing for position: live run makes it first, last picture stays | MainWindow.Live.cs | 50 Position rules | done | live.seek publishes goal |
| L-76 | Status bar right: "<State>: <done> / <total> steps, <engine>", empty without run | MainWindow.Live.cs | 50 Run states | done | live.rs poll |
| L-77 | State words Running/Paused/Completed/Stopped/Failed | MainWindow.Live.cs | 50 Run states | done | |
| L-78 | Engine text "CPU (n threads)" / "GPU: adapter" / browser "engine starting", "no engine" | MainWindow.Live.cs | 50 Run states; 60 Edit classes | partial | only "GPU: <adapter>" |
| L-79 | Summary log "Run Completed: ..." / "Run Cancelled: ..." in status bar left | MainWindow.Live.cs | 50 Run states | done | live.rs tracing::info |
| L-80 | Transport state per run state (Run/Pause/Live enable matrix) | MainWindow.Live.cs | 50 Run states | partial | enabled-on-running; Paused state counts as not running so Stop/Pause-run disabled while paused (check) |
| L-81 | Pause run tooltip becomes "Resume" while paused; menu entry "Resume" | PreviewPane.xaml:436-440 | 50 Run states | missing | menu has fixed "Pause / Resume" |
| L-82 | Edit stops active run at once; no stale state | MainWindow.Live.cs | 50 Run states | done | request() restarts |
| L-83 | Status bar left: latest WaveDude Info+ log line, "Ready" at first | Logging/StatusBarTarget.cs | 70 Status bar | partial | latest Info+ shown; initial "Ready" and dependency-log exclusion unverified |
| L-84 | Warn turns whole bar orange, Error red, until next Info | StatusBarTarget.cs | 70 Status bar | missing | background fixed to Theme.status; the warn token (StatusBarWarnBackground #ca5100) comes with the feature |
| L-85 | Far-right busy text + 120 px progress bar (determinate export, indeterminate wait) | MainWindow.xaml | 70 Status bar | partial | export progress goes into run-status text; no bar, no "Computing frame n" |
| L-86 | Status bar tooltip "Double-click to open the Messages pane" | MainWindow.xaml | 70 Status bar | missing | |
| L-87 | Status bar double-click opens or focuses Messages | MainWindow.xaml | 70 Status bar; 00 Docking | partial | opens; no focus of an already shown pane |
| L-88 | Errors to status bar and log, no message box | MainWindow.Live.cs | 60 Automatic re-run | partial | run failure logged; export failure shows modal |
| L-89 | Preview/Run F5 / F6 / Shift+F5 menu shortcuts | SimulationCommands.cs | 10; 50 | done | F5 Run, F6 Pause, Shift+F5 Stop (Esc missing, L-29) |
| L-90 | Speed persisted/ restored | PreviewPane.xaml.cs | 50 Playback | n/a | not in spec; Next keeps speed per session |

### R: Live editing and runs

| ID | Feature | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| R-01 | Simulation edit class: frames go, pictures keep last, playback stops, position frame first then all if auto re-run | MainWindow.Live.cs | 60 Edit classes | partial | any model change restarts run (live.edited); playback not stopped on edit |
| R-02 | Probe edit class: frames stay, recordings remade from step 0 | MainWindow.Live.cs | 60 Edit classes; divergences.md:25 | missing | probe edits restart everything |
| R-03 | Display edit class: redraw only (colour scheme, brightness 16-bit, outline, preview on/off, handles/labels, graph box) | MainWindow.Live.cs | 60 Edit classes | missing | all edits restart run |
| R-04 | Colour scheme edit: recolour shown frame | MainWindow.Live.cs | 60 Edit classes | missing | |
| R-05 | Any edit: dirty, undo step pending (title *) | MainWindow.History.cs | 60 Edit classes | done | document.rs title/dirty |
| R-06 | Mixed-class edits take least destructive effect | MainWindow.Live.cs | 60 Edit classes | missing | single class |
| R-07 | Status bar run line words + engine | MainWindow.Live.cs | 60 Edit classes | done | see L-76 |
| R-08 | Live run: one worker/engine/store, goal published (spec, position, wanted, paused) | MainWindow.Live.cs | 60 The live run | done | live.rs Goal/RunWorker |
| R-09 | Run converges on goal, no timers; position frame first, then fill | worker.rs (core) | 60 The live run | done | core RunWorker |
| R-10 | Spec change = new generation, store cleared, engine reused/rebuilt | worker.rs | 60 The live run | done | core |
| R-11 | Probe-only spec change clears traces keeps frames | worker.rs | 60 The live run | done | core; UI never emits probe-only edit (R-02) |
| R-12 | Paused + edited project adopts edit at once; pause with nothing to do reads Completed | worker.rs | 60 The live run | partial | core; Slint treats state per status only |
| R-13 | Switching device (Use the GPU) rebuilds engine at next work | worker.rs | 60 The live run | missing | Slint engine fixed GPU (live.rs new) |
| R-14 | Showing intensity picture makes run track it | worker.rs | 60 The live run | partial | both layers always drawn; no show toggle |
| R-15 | Goal looked at every 16 steps; fit frame abandoned when not asked | worker.rs | 60 The live run | done | core |
| R-16 | Last frame stays readable through a clear; graphs follow picture | worker.rs | 60 The live run | partial | pictures hold; no graphs |
| R-17 | Playback holds at newest contiguous frame | PreviewPane.xaml.cs:1144 | 60 The live run | done | |
| R-18 | Busy feedback: status "Computing frame n", dim+spinner after 150 ms | MainWindow.Live.cs | 60 The live run | missing | see V-35, L-85 |
| R-19 | Opening a project makes frame 0 and nothing more | MainWindow.Project.cs | 60 The live run | done | live.load wanted Position |
| R-20 | Simulation > Discard frames (confirm Yes), new store, frames and traces gone | MainWindow.Simulation.cs | 60 The live run | missing | menu item exists; main.rs falls to `menu: discard` log |
| R-21 | Part edit repaints scene in background (outline follows drag) | MainWindow.Live.cs | 60 The live run | partial | outline rebuilt per request; no drag editing |
| R-22 | Automatic re-run setting (default on) | MainWindow.Live.cs | 60 Automatic re-run | done | live.auto_rerun from Settings |
| R-23 | Auto re-run on: position frame first, earlier frames on the way, then later; never moves preview | MainWindow.Live.cs | 60 Automatic re-run | partial | request wants `wanted` state; ordering is core's |
| R-24 | Auto re-run off: only position's frame; Play/F5 make the rest | MainWindow.Live.cs | 60 Automatic re-run | partial | edited() skips request, so no frame at position after an edit with setting off |
| R-25 | Export waiting for run dropped by edit/Stop (status says so) | MainWindow.Export.cs | 60 Automatic re-run | missing | export.rs own run; no wait-for-run |
| R-26 | Export reading run holds frames; edit taken up at export end; New/Open/Close refused with status text; F5 refused | MainWindow.Export.cs | 60 Automatic re-run | missing | |
| R-27 | Turning setting on asks nothing | MainWindow.Live.cs | 60 Automatic re-run | done | |
| R-28 | Memory warning window before preview run exceeding RAM cache (500 px, Run default, Cancel Esc) | Dialogs/MemoryWarningWindow.xaml | 60 Memory warning | missing | setting exists (settings.rs:113), no dialog |
| R-29 | Warning text and 4 lever check boxes (RAM Auto, half resolution, half frames, 8-bit) with grey-out rules | MemoryWarningWindow.xaml.cs | 60 Memory warning | missing | |
| R-30 | Pre-tick rule (fewest, mildest, tie order) | MemoryWarningWindow.xaml.cs | 60 Memory warning | missing | |
| R-31 | Live bar need/budget + fit line | MemoryWarningWindow.xaml | 60 Memory warning | missing | |
| R-32 | "Do not warn me again" clears the setting | MemoryWarningWindow.xaml.cs | 60 Memory warning | missing | |
| R-33 | Run applies ticks to settings then runs; Cancel changes nothing, Info line, playback off | MainWindow.Simulation.cs | 60 Memory warning | missing | |
| R-34 | Warning not for export runs; closed by export; asked every time | MainWindow.Simulation.cs | 60 Memory warning | missing | |
| R-35 | Welcome page RAM/lever presets (similar levers) | Dialogs | 70 First start | done | main.rs:245 show_welcome (outside this area) |
| R-36 | Undo up to N steps (default 1000, 10..100000), clone per step | MainWindow.History.cs | 60 Undo and redo; decision 0030 | done | document.rs limit |
| R-37 | Step ends after 0.5 s quiet, no mouse held: drag/scrub/held key = one step | MainWindow.History.cs | 60 Undo and redo | partial | one step per edit call; no 0.5 s coalescing |
| R-38 | Undo/Redo end pending step first | MainWindow.History.cs | 60 Undo and redo | n/a | no pending step |
| R-39 | Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z; menu enabled when step exists; text box keeps own undo | MainWindow.History.cs | 60 Undo and redo | done | app.slint keys FocusScope + menu |
| R-40 | Restored step counts as live edit, not recorded again | MainWindow.History.cs | 60 Undo and redo | done | generation bump |
| R-41 | Selection follows by name, else index; root when nothing left | MainWindow.History.cs | 60 Undo and redo | partial | selection not remapped on undo (unverified) |
| R-42 | Title asterisk clears when current step is saved one; dropped saved step never clears; edit after undo ends branch | MainWindow.History.cs | 60 Undo and redo | partial | dirty = project != saved snapshot; equal states clear it |
| R-43 | Paste/Cut/drop/Duplicate/Remove/image Browse+Remove one step each | MainWindow.Clipboard.cs | 60 Undo and redo | partial | Remove (delete) done; others missing |
| R-44 | New/open: new history | MainWindow.Project.cs | 60 Undo and redo | done | Document::new |
| R-45 | Clipboard copy: XML fragment in app format + plain text | MainWindow.Clipboard.cs | 60 Clipboard | missing | menu cut/copy/paste/select-all only log "menu: ..." |
| R-46 | Paste accepts app format or plain-text fragment | MainWindow.Clipboard.cs | 60 Clipboard | missing | |
| R-47 | Tree drag carries same text | ProjectTree.xaml.cs | 60 Clipboard | missing | |
| R-48 | Name clash on paste appends " copy"; composite emitters re-derived | MainWindow.Clipboard.cs | 60 Clipboard | missing | |
| R-49 | Paste enable state checked per clipboard change | MainWindow.Clipboard.cs | 60 Clipboard | missing | |
| R-50 | File dropped on window opens .wdproj / configuration.json / .wdscheme; others refused | MainWindow.xaml.cs | 60 Files and drop | missing | no drop handler |
| R-51 | Scene image differs from embedded: ask "Load file?" Load/Keep embedded; undoable | MainWindow.SceneImageWatch.cs | 60 Files and drop | missing | |
| R-52 | Scene image file watched, 400 ms debounce, retries, one question at a time, withdraws on revert | MainWindow.SceneImageWatch.cs | 60 Files and drop | missing | |
| R-53 | Project with no embedded image takes file without asking; missing/undecodable logs "(not found)" | MainWindow.SceneImageWatch.cs | 60 Files and drop | missing | |
| R-54 | Image looked up beside project and in scene_images, adopts relative path | ProjectWatcher | 60 Files and drop | missing | |
| R-55 | Image inside project folder stored relative | ProjectWatcher | 60 Files and drop | missing | |
| R-56 | Unsaved-edits ask before New/Open/Close/Exit/example/drop | MainWindow.Project.cs | 60 Files and drop; 70 | partial | done for New/Open/Close/Exit/example; drop missing |
| R-57 | Off-canvas objects warning in log, once per list change | none (divergence: Next silent) | 60 Objects off the canvas | missing | spec marks as divergence; no such log in slint crate (core?) |
| R-58 | Run start errors in status bar: "The simulation could not start." etc | MainWindow.Simulation.cs | 60 Run start errors | partial | worker failure logged as "the run failed: ..."; fixed texts absent |
| R-59 | Browser worker failure messages and reload advice | web.rs | 60 Run start errors | missing | web.rs minimal |
| R-60 | Scene image / save-first run start errors | MainWindow.Simulation.cs | 60 Run start errors | missing | |
| R-61 | Live edit while dragging a handle: nothing completes until hand rests a frame time | worker.rs | 60 The live run | n/a | no handle drags (V-68) |

### G: Messages pane

| ID | Feature | Next source | Spec | Slint | Note |
|---|---|---|---|---|---|
| G-01 | Dock pane, default bottom, hidden at start | MainWindow.xaml; Docking | 70 Messages pane; 00 Docking | partial | bottom pane hidden by default; no dock/move/resize, fixed layout |
| G-02 | Toolbar: one toggle per level "Level (count)" in level colour, tooltip "Show or hide this level" | MessagesPane.xaml:62-76 | 70 Messages pane | partial | toggle + colour + count; no tooltip |
| G-03 | Levels Trace, Debug, Info, Warn, Error, Fatal | MessagesPane.xaml.cs:63-66 | 70 Messages pane | partial | no Fatal toggle (tracing has no Fatal) |
| G-04 | Level toggle as styled ToolToggle with accent focus | MessagesPane.xaml:20-55 | 70 Messages pane | partial | clickable rectangles, not focusable |
| G-05 | Search box, placeholder "Search" | MessagesPane.xaml:82-106 | 70 Messages pane | done | LineEdit |
| G-06 | Search matches message, logger, exception, case-insensitive | MessagesPane.xaml.cs:228-245 | 70 Messages pane | partial | source + message; no exception text |
| G-07 | Regex toggle; unfinished pattern shows everything | MessagesPane.xaml:119; xaml.cs:247-262 | 70 Messages pane | missing | |
| G-08 | Auto-scroll toggle "Follow new messages" | MessagesPane.xaml:121; xaml.cs:135,274-288 | 70 Messages pane | missing | ListView not auto-following |
| G-09 | Clear button | MessagesPane.xaml:123 | 70 Messages pane | done | text button |
| G-10 | Clear button tooltip | MessagesPane.xaml:123 | 70 Messages pane | missing | |
| G-11 | List columns Time, Level, Source, Message (+exception text) | MessagesPane.xaml:192-210 | 70 Messages pane | partial | Time, Source, Message; no Level column (colour only), no exception |
| G-12 | Time format HH:mm:ss.fff | MessagesPane.xaml:194 | 70 Messages pane (Time) | partial | log.rs time string (format unverified) |
| G-13 | Source = logger name without namespace | MessagesPane.xaml:196-199 | 70 Messages pane | done | log.rs source |
| G-14 | Column headers | MessagesPane.xaml:194-203 | 70 Messages pane | missing | no header row |
| G-15 | Resizable columns | MessagesPane.xaml:192-210 | 70 Messages pane | divergence | decision 0033, matched in general, not to the pixel/id |
| G-16 | Rows coloured per level | MessagesPane.xaml:150-180 | 70 Messages pane | done | text colour per level |
| G-17 | Opens scrolled to newest; focus on list | MessagesPane.xaml.cs:151,168-188 | 70 Messages pane | missing | |
| G-18 | Initial visibility: Info+ always; Trace/Debug debug builds only | MessagesPane.xaml.cs:62-66 | 70 Messages pane | done | messages.rs shown[] |
| G-19 | Row selection (extended) | MessagesPane.xaml:140 | 70 Messages pane | missing | rows not selectable |
| G-20 | Context menu Copy (Ctrl+C, selected rows) | MessagesPane.xaml:186; xaml.cs:290-293 | 70 Messages pane | missing | |
| G-21 | Context menu Copy all (visible rows), tab-separated lines | MessagesPane.xaml:187; xaml.cs:297-313 | 70 Messages pane | missing | |
| G-22 | Context menu Clear | MessagesPane.xaml:189 | 70 Messages pane | missing | toolbar button only |
| G-23 | Keeps last 5000 entries, oldest dropped | MessagesTarget.cs | 70 Messages pane | done | log.rs KEPT |
| G-24 | Batches arrivals so bursts do not stall | MessagesTarget.cs | 70 Messages pane | partial | pane refreshed on 16 ms tick when ring changed; whole model rebuilt each time |
| G-25 | Row tooltip = exception text | MessagesPane.xaml:150 | 70 Messages pane; 70 Message window | missing | |
| G-26 | Source cell tooltip = full logger name | MessagesPane.xaml:199 | 70 Messages pane | missing | |
| G-27 | Level counts live (add/remove/clear) | MessagesPane.xaml.cs:190-226 | 70 Messages pane | partial | counts over ring; recalculated on show |
| G-28 | Pane title bar, close, "Window position" menu | Docking | 00 Docking | divergence | decision 0033, dropped: fixed layout, View > Panes shows/hides |
| G-29 | View > Panes > Messages check toggle | MainWindow.xaml | 00 Docking | done | app.slint menu |
| G-30 | Automation ids MessagesLevel:*, MessagesSearch, MessagesRegex, MessagesAutoScroll, MessagesClear, MessagesList | MessagesPane.xaml | 70 Messages pane (ids) | divergence | decision 0033, matched in general, not to the pixel/id |
| G-31 | Messages pane receives warnings (WebGL fallback, off-canvas, memory) | Logging | 40; 60 | partial | sink exists; those messages not produced |
| G-32 | Log lines not localised, decimal point always | Logging | 00 Languages | done | tracing |
