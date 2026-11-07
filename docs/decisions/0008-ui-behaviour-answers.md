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
