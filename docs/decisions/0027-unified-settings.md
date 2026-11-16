# 0027: One Settings window holds every user setting

Status: accepted, 2026-11-16. Amends [0005](0005-video-openh264.md) (the codec question),
[0011](0011-releases-and-updates.md) (the update question), [0020](0020-browser-frame-spill-in-opfs.md) (the RAM
cache size).

## Context

The user settings ([`docs/ui/80-settings.md`](../ui/80-settings.md)) were set from five places: View (theme, UI
zoom, language, the four preview levers), Simulation (GPU, automatic re-run, memory warning, RAM cache size), Help
(update check at launch), the export dialog and the startup page, which repeated the View menu's levers and stayed
forced open while the codec or the update question was unanswered. The machine was read once, at the first start,
for a tier that was never stored; the desktop's RAM cache size, a share of the physical memory, was a second figure
beside the one the tier was read from. None of the setting entries has a shortcut.

## Decision

- File > Settings... (Ctrl+,) opens one modal with every user setting in sections, General, Preview and memory,
  Simulation, Export, Updates (Use the GPU under Preview and memory, beside what it speeds up), and a search box
  over the labels in the current language and in English. A change applies and is saved at once; the window has
  Close only.
- The setting submenus leave the menu bar: View keeps Panes, Simulation its actions, Help its actions with Check for
  updates. Ctrl+= (and Ctrl+Plus), Ctrl+- and Ctrl+0 step the UI zoom through the zoom table and back to the
  device's default.
- RAM: natively the frames take what the OS has available less 2 GiB, shown read-only as "{available} available of
  {physical}"; the share (`FrameMemoryPercent`) is gone. In the browser Auto is the detected size and a size can be
  picked in its place, as before.
- Preview presets Low, Medium (formerly Standard, `Middle`) and High set the four levers; Auto sets them from the
  tier of the machine as read now (the first start's table, the browser's picked size standing in for its memory).
  A preset is a one-shot fill: only the levers are stored.
- The welcome page (File > Welcome Page...) shows the RAM row, the four preset buttons with a line naming the
  levers, Settings... and Examples..., and "Show this page at each launch"; the Examples dialog has Welcome page...
  back to it. It opens at launch only while that is ticked (and at the first start); no question forces it.
- OpenH264 and the update check at launch default to off and are set in Settings. MP4 export asks for the codec
  where it is needed: the export dialog's check box, OK disabled while it is clear (decision 0005).

## Why

One place per setting, found by name, instead of menus and pages that each hold a part and repeat each other. The
presets fill levers the user may change, so a stored tier would be stale after the first change. The desktop's
budget already follows the available memory; a share of the physical memory on top of it was a second knob for the
same thing.

## Consequences

- Settings file version 2: a stored `FrameMemoryPercent` is logged and dropped at load.
- The memory warning's lever "RAM cache size to Auto" exists in the browser only.
- A user who never opens Settings gets no update check and no OpenH264 until MP4 export asks.
