# 0008 UI behaviour answers to the UI map's open questions

Status: accepted, 2026-11-05 (the user's answers; defaults where marked)

1. View > Panes lists Preview as well as Project, Properties and Messages:
   follow the manual, the broadest set that makes sense.
2. Window title shows the project's file name by default; a setting switches
   it to the full path. No project: the app name alone.
3. Colour scheme rows join a tree multi-selection for cut, copy, delete and
   duplicate; a group move on the pictures ignores them (default).
4. Rows that do not apply are disabled (probe B x/y for a point probe, graph
   box size while no box is shown). Old quirks are not carried over.
5. Scrubbing moves the position only; it pauses playback and turns Live off.
   Nothing is ever deleted by a scrub (default).
6. Playback speed and the picture split position are session state, not
   persisted.
7. A file dropped on the window with unsaved edits asks Save / Discard /
   Cancel; Cancel keeps the open project and ignores the drop.
8. Right-click on a header strip opens the picture's context menu (default).
9. Enter on a tree row moves focus into its properties form; F2 renames
   (default).
10. File types are registered at install time by the packaging, not at start.
    Packaging comes later, aiming at a single binary per platform.

## Divergences from the WPF app kept on purpose (added 2026-11-08)

11. Docking: a title bar or tab is dragged onto an outer edge of the window, onto the middle of another pane (a
    tab) or onto an end of it (a split beside it, along its side); no floating windows and no five-way cluster on
    the centre area, since the layout is four side lists around the centre, not a free split tree. A group's
    tab strip sits under its content.
12. Tree rows are dragged within their list and onto a picture (a copy lands where dropped). Drags into another
    window or into a text editor are not offered: Freya has no OS drag source; the clipboard text does the same.
13. The Edit menu acts on objects even while a text box has the focus; the keys (Ctrl+Z, X, C, V, A) go to the
    box as before. Opening a menu moves the focus, so the menu cannot tell what had it.
14. Taskbar progress, the jump list and menu click-through on the title bar are Windows-only integrations and
    are not ported; file associations come from the packaging (cargo-packager `file-associations`).
15. The manual's figures are screenshots of this program without numbered badges; the lists under them name
    the controls in reading order.
16. The MP4 export has no hardware encoder (decision 0005).
17. The live preview is one goal-driven run instead of a scratch runner beside the run, without the debounce and
    quiet timers; an edit replaces the frames rather than leaving a stale run (decision 0012). The register of all
    divergences is `docs/divergences.md`.
