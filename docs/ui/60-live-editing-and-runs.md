# Live editing, runs, undo, clipboard

Pointers: `MainWindow.Live.cs`, `MainWindow.Simulation.cs`, `MainWindow.History.cs`,
`MainWindow.Clipboard.cs`, `MainWindow.SceneImageWatch.cs`, `Dialogs/MemoryWarningWindow.xaml(.cs)`,
core: `ProjectWatcher`, `ScratchRunner`, `ProjectHistory`, `ProjectClipboard`.

## Edit classes

Every model change is observed. Its class decides what follows.

| Class | Examples | Effect |
|---|---|---|
| Simulation edit | scene (size, edges, damping, index, image, image sources), simulation settings, seed, any source, any optical part (incl. points, nodes, list changes), View > preview resolution/frames/samples, brightness while preview samples are 8-bit, a probe switched on or added that the run does not record | run marked stale; playback stops; any active run (user or automatic) stops at once; scratch frame for the position after 120 ms; automatic re-run after 1.5 s of quiet (if enabled) |
| Probe edit | probe moved, measurement/layer changed, start step changed, a recorded probe switched off | frames stay valid, only recordings stale; graphs redraw from a scratch frame for the position; never starts a run; Play or F5 makes a run that refreshes them |
| Display edit | colour scheme, brightness (16-bit), black level, outline, Preview on/off, handles/labels, export settings, a probe's graph box (place, size, axes, window) | redraw only; no stale run |
| Colour scheme edit | stops, colours, rename (layers repoint) | recolour the frame shown |
| Any edit | | project dirty (title `*`); undo step pending |

Status bar shows ", edited since" on the run line while stale.

## Scratch frame

- "Frame k of the project as it is now", computed from step 0 on a separate kept engine, probes sampled.
- Requested 120 ms after the last edit, and every 120 ms while scrubbing, whenever the store cannot show the
  position: no run, stale run, stale probes, or a run that has not got there. A later frame steps on from the
  previous request's progress, so a forward drag follows the pointer.
- A frame the run can show when it arrives is not shown (the run's frame is); for stale probes alone it feeds the
  graphs, the pictures keep the run's frame and do not dim.
- Newest request wins: a superseded request is abandoned within a few steps; the newest is computed in full,
  however long (no time budget).
- Not requested while playback runs; requested when playback stops on a position the store cannot show.
- Any active run is paused while a scratch frame computes, then resumes (a user-paused run stays paused).
- Busy feedback: status bar "Computing frame n" with an indeterminate bar, taskbar indeterminate, pictures dim
  under a spinner after 150 ms.
- The first scratch frame warms the engine right after a project opens.
- A part edit repaints the scene in the background (newest composition after the one in flight), so part
  outlines on the pictures follow a drag smoothly.

## Automatic re-run

- Setting: Simulation > Re-run automatically after edits (default on).
- After 1.5 s of quiet following a simulation edit, while the run is stale, and after a pending scratch frame
  for the position: starts a full run. Never moves the preview, never followed.
- An edit during it stops it; the next quiet period starts a fresh one.
- Not started or replaced while an export reads the current run.
- Errors go to the status bar and log, no message box.
- When a run ends with edits pending, the check runs again.

## Memory warning

Setting: Simulation > Warn when a run does not fit in RAM (default on). Asked before any preview run (F5, Play,
automatic re-run) whose frames exceed the RAM cache budget. Never for an export's run (always full quality).

Window (message-window look, question disc, 500 px wide, not resizable, Run default, Cancel = Esc):
- Text: "The run's frames need {need} of memory, and the RAM cache has {budget}. The frames beyond that go to the
  cache file on disk, which is slower."
- "Settings to change for this and later runs:" then four check boxes:
  1. "RAM cache size to Auto: {budget} for the frames (Simulation menu)" - greyed when Auto gains less than a frame.
  2. "Halve the resolution: a quarter of the memory (View menu)" - greyed at Quarter already.
  3. "Store half the frames: half the memory (View menu)" - greyed at every fourth frame already.
  4. "8-bit samples: half the memory (View menu)" - greyed when already 8-bit.
- Pre-tick rule: among combinations of available levers, the fewest ticks that make the run fit; among as many,
  the mildest (largest need that still fits, so the cache comes before any quality loss); ties in order 8-bit,
  half frames, half resolution. When none fits, every available lever is ticked.
- Live line under the boxes: "The frames then need {n} and fit in the RAM cache." or "The frames then need {n};
  {m} of them still go to disk."
- "Do not warn me again" check box: clears the setting.
- Run: applies the ticks to the Simulation and View settings (persisted), then runs. Cancel: nothing changes;
  the stale run stays; an Info line says so; playback turned off again.
- Asked every time; no remembered answer.

## Undo and redo

- Unlimited. One snapshot per step (project XML without the scene image + image bytes by reference).
- A step ends after 0.5 s of quiet with no mouse button held: a drag, a number-box scrub or a held arrow key is
  one step. Undo/Redo end a pending step first.
- Undo (Ctrl+Z), Redo (Ctrl+Y, Ctrl+Shift+Z); Edit menu entries enabled when a step exists. A focused text box
  keeps its own undo.
- A restored step is written into the same project object; it counts as an edit for the live rules (stale run,
  scratch frame, re-run) but is not recorded again.
- Selection follows by name, else by index in the same list; settings objects by place.
- Title asterisk clears when the current step is the saved one.
- Paste, Cut, drop, Duplicate, Remove, image Browse/Remove: one step each.
- New project / open: new history.

## Clipboard and drag text (user's view)

- Copy writes objects as their project-file XML fragment, both in the app's own clipboard format and as plain
  text: a source copied can be pasted into a text editor, edited, copied back and pasted into the project.
- Paste accepts the app format or plain text that parses as such a fragment.
- A tree drag carries the same text, nothing else (see `20-project-tree.md` for targets).
- Name clash on paste: " copy" appended. Composite emitters re-derived from the target's seed.
- Clipboard checked once per clipboard change (cheap enable state for Paste).

## Files and drop

- A file dropped anywhere on the window (not claimed by tree or pictures) is opened: `.wdproj`,
  `configuration.json` (import), `.wdscheme` (added to the project's schemes and selected). Other data refused.
- Opening a project whose scene image file on disk differs from the embedded bytes takes the file (project
  dirty, status bar says it was re-embedded). While open, the image file is watched; a change on disk is
  re-embedded after 400 ms debounce (retries while the writer holds it), the picture updates. A missing file
  shows "(not found)" and changes nothing.
- A project without an image path looks for its image by name beside the project file and in a `scene_images`
  folder next to it, and adopts the found path (relative).
- An image inside the project folder is stored with a relative path.
- Unsaved edits are asked about before New, Open, Close, Exit, opening an example and dropping a file
  (a drop asks too, Cancel keeps the project and ignores the drop; decision 0008).

## Run start errors (status bar / message)

"The simulation could not start.", "The scene uses an image, but no image file is set.", "Save the project
first, so the scene image can be found relative to it."
