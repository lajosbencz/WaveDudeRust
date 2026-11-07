# WaveDude Rust manual

WaveDude Rust simulates waves on a flat canvas. You place sources that send out waves, optical parts that
bend, reflect and absorb them, and probes that measure them, and you watch the field and its intensity
develop frame by frame.

## The main window and its panes

The window holds the menu bar at the top, the preview in the middle, the Project and Properties panes at the
side and the status bar at the bottom. Drag a pane's title bar to move it to another side; View > Panes shows
or hides each pane.

### Project pane

The tree lists the project's settings, sources, optical parts, probes and colour schemes. Click a row to show
its settings in the Properties pane. Ctrl+click and Shift+click select several items; Space switches the
selected items on or off; F2 renames; Del removes.

### Properties pane

The settings of the row you picked. Every change applies at once and shows in the preview.

### Preview

The field and the intensity pictures with the handles of your sources, parts and probes. Drag a handle to
move its item. The mouse wheel over a picture zooms both pictures in on the point under the pointer and out again,
down to the whole canvas; drag with the middle button to move a zoomed view. The timeline below the pictures plays
the run and lets you pick a frame.

### Messages pane

Everything the program reports, newest last. Double-click the status bar to open it.

## The menus

File opens, saves and exports projects; Edit undoes, redoes and copies; View sets the theme, the zoom, the
language and the preview quality; Simulation runs, pauses and stops the simulation; Help opens this manual.

## Project settings

The Project, Scene, Simulation and Display rows of the tree hold the canvas size, the edges, the number of
steps, the colour schemes and the export range.

## Sources

A source sends out a wave of its frequency, amplitude and phase, continuously or in pulses. A composite source
is a grid of emitters.

## Optical parts

Lenses, mirrors, shapes, fibres, Bragg reflectors, GRIN lenses, zone plates and optical logic gates are painted
into the scene. Each part's form starts with a description of its kind.

## Probes

A probe measures the field or the intensity at a point or along a line and draws a graph over the pictures.

## Colour schemes

A colour scheme maps the values of a picture to colours. Edit a copy in the project to change its stops.

## Examples

File > Examples opens one of 24 example projects as an unsaved project.
