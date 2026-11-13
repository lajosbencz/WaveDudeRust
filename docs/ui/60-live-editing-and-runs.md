# Live editing, runs, undo, clipboard

Pointers: `MainWindow.Live.cs`, `MainWindow.Simulation.cs`, `MainWindow.History.cs`,
`MainWindow.Clipboard.cs`, `MainWindow.SceneImageWatch.cs`, `Dialogs/MemoryWarningWindow.xaml(.cs)`,
core: `ProjectWatcher`, `ProjectHistory`, `ProjectClipboard`. Rust: [`wavedude-app-state/src/live.rs`](../../crates/wavedude-app-state/src/live.rs) (the
controller), [`wavedude-core/src/run/worker.rs`](../../crates/wavedude-core/src/run/worker.rs) (the live run), decision 0012.

## Edit classes

Every model change is observed. Its class decides what follows.

| Class | Examples | Effect |
|---|---|---|
| Simulation edit | scene (size, edges, damping, index, image, image sources), simulation settings, seed, any source, any optical part (incl. points, nodes, list changes), View > preview resolution/frames/samples, brightness while preview samples are 8-bit | the frames go (the pictures keep the last one); playback stops; the live run makes the position's frame first, then every frame when Re-run automatically is on, else nothing more |
| Probe edit | probe moved, measurement/layer changed, start step changed, switched on or off, added or removed | the frames stay; the recordings are made anew from step 0 (up to the position, or the whole run when every frame is wanted); the graphs follow as they come; never clears the pictures |
| Display edit | colour scheme, brightness (16-bit), black level, outline, Preview on/off, handles/labels, export settings, a probe's graph box (place, size, axes, window) | redraw only; no stale run |
| Colour scheme edit | stops, colours, rename (layers repoint) | recolour the frame shown |
| Any edit | | project dirty (title `*`); undo step pending |

Edits of two classes at once (an undo, a paste) take the least destructive effect covering both: a probe edit
with a brightness change keeps the frames unless the preview samples are 8-bit.

The status bar's run line says Running while frames are made, Stopped when some are missing and no more are wanted, Completed when all are there.

## The live run

One worker, one engine, one store ([`wavedude-core/src/run/worker.rs`](../../crates/wavedude-core/src/run/worker.rs)). The app publishes a goal: the project's
spec (scene, sources, parts, probes, quality), the position, how much is wanted (the position's frame alone,
or every frame) and whether the user paused. The run converges on it, without timers:

- Between two frames it reads the newest goal and takes the next target: the first stored frame at or after
  the position that is missing or whose probe traces are not recorded; with every frame wanted, when none
  follows, the probes' last steps and then the frames before the position.
- It steps to the target (from step 0 when the engine has passed it), captures it when missing and, with every
  frame wanted, the missing frames it passes, records the probes on the way and publishes progress; the
  engine's frame time is the only pace. So an edit fills the timeline in one pass: the frame looked at lands
  first, with the frames before it, then the rest follow in order.
- A spec whose frames differ starts a generation: the store is cleared (its buffers kept) and refilled; the
  engine is reused (same scene: new emitters; same grid: the scene swapped in; else rebuilt). A spec whose
  probes alone differ clears the traces and keeps the frames.
- A pause with an edited project adopts the edit at once: the old frames go, the new run waits paused. A
  pause with nothing left to do is no pause: the run reads Completed. A pause holds the fill, not the frame
  looked at: a scrub while paused still shows its frame.
- Switching the device (Simulation > Use the GPU) builds the engine anew at its next work; frames and traces
  stay. Showing the intensity picture makes the run track it, so the frames are made again.
- A goal change is looked at every 16 steps. Work goes on for the project as it is now alone: an edit leaves
  the frame being made and starts one run of the edited project; a frame for later or the probes' tail is
  also left when the position's frame is missing or nothing more is wanted; a fit frame (Auto brightness) is left when it is
  no longer asked for. During a drag nothing completes until the hand rests for a frame time.
- The frame written last stays readable through a clear, so the pictures hold the last state made while the
  edited project's frame is computed; that frame replaces it when it lands. The
  graphs go with the picture: they show that frame's traces, and switch with it; they never draw the traces
  of a state whose frame is not shown.
- A probe-only edit never shows an empty picture: the frames stay and the graphs redraw as the traces come.
- Playback holds at the newest contiguous frame from the position while frames keep coming.
- Busy feedback: status bar "Computing frame n" with an indeterminate bar, pictures dim under a spinner after
  150 ms while the position's frame is computed.
- Opening a project makes frame 0 and nothing more (the engine warms up on it).
- Simulation > Discard frames (port), after a Yes: a run stops and every frame and trace goes. The run gets a new
  store, so the old one's memory and spill (the disk file, the browser storage's folder) are freed once the
  pictures let go of it; the engine starts over and the timeline at frame 0, made again as after opening the
  project. The project and its undo history stay; refused while an export holds or waits for the run.
- A part edit repaints the scene in the background, so part outlines on the pictures follow a drag smoothly.

## Automatic re-run

- Setting: Simulation > Re-run automatically after edits (default on).
- With it on, a simulation edit wants every frame at once: the position's frame first, with the frames before
  it made on the way, then the frames after it. Never moves the preview, never followed.
- With it off, only the position's frame is made; Play or F5 make the rest.
- An export waiting for its run is dropped by an edit or by Stop (status bar says so). An export reading its run
  holds the frames: a simulation or probe edit made meanwhile is taken up when the export ends; New, Open and
  Close are refused meanwhile (the status bar says so); F5 and Play use the frames as they are.
- Turning the setting on asks for nothing by itself.
- Errors go to the status bar and log, no message box.

## Memory warning

Setting: Simulation > Warn when a run does not fit in RAM (default on). Asked before any preview run (F5, Play,
automatic re-run) whose frames exceed the RAM cache budget. Never for an export's run (always full quality), nor
while an export waits for or reads its run. An export closes a warning still asked.

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
  the frames it has stay; an Info line says so; playback turned off again.
- Asked every time; no remembered answer.

## Undo and redo

- Unlimited. One snapshot per step (a project clone; the scene image's bytes are shared between the clones).
- A step ends after 0.5 s of quiet with no mouse button held: a drag, a number-box scrub or a held arrow key is
  one step. Undo/Redo end a pending step first.
- Undo (Ctrl+Z), Redo (Ctrl+Y, Ctrl+Shift+Z); Edit menu entries enabled when a step exists. A focused text box
  keeps its own undo.
- A restored step is written into the same project object; it counts as an edit for the live rules (new
  frames) but is not recorded again.
- Selection follows by name, else by index in the same list; settings objects by place. A step that leaves
  nothing of it selects the project root.
- Title asterisk clears when the current step is the saved one; an edit after an undo ends the branch that held
  the saved step, so no step is saved then.
- Paste, Cut, drop, Duplicate, Remove, image Browse/Remove: one step each.
- New project / open: new history.

## Clipboard and drag text (user's view)

- Copy writes objects as their project-file XML fragment, both in the app's own clipboard format and as plain
  text: a source copied can be pasted into a text editor, edited, copied back and pasted into the project.
- Paste accepts the app format or plain text that parses as such a fragment.
- A tree drag carries the same text, nothing else (see [`20-project-tree.md`](20-project-tree.md) for targets).
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
