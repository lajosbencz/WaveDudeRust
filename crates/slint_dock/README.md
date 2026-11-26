# slint_dock

A docking layout for Slint apps: panes in tab groups, the groups in a split tree of weighted splits around one fixed
centre area. A title bar drags its group, a tab its pane, onto an outer edge, beside a group or into it; splitters
resize; each title bar has a "Window position" menu (move the active pane to an edge, close it); the arrangement
saves and restores.

Two halves:

- `Dock`, headless Rust: registers panes, holds the tree, computes its geometry (`layout`), resizes it by its
  splitters (`resize`), finds where a drag lands (`drop_target`), moves (`move_to`), hides (`close`) and shows
  (`show`) panes. `arrangement` and `arrange` round-trip the tree as an `Arrangement`, serde-able with the `serde`
  feature; `arrange` drops panes not registered.
- `ui/dock.slint`, a Slint library exporting `DockArea`: the groups' title bars and tab strips, the splitters and a
  drag's preview, drawn from the models the app fills from `Dock::layout`.

## Use

In the build script, register the library path and import it in a `.slint` file:

```rust
let config = slint_build::CompilerConfiguration::new()
	.with_library_paths(std::collections::HashMap::from([("slint-dock".to_owned(), slint_dock::ui_dir())]));
```

```slint
import { DockArea, DockGroup, DockSplitter, DockPlace } from "@slint-dock/dock.slint";
```

Slint cannot instantiate a pane's content from a model, so each pane's content is a child element of `DockArea`
that the app places at the rectangle `Dock::layout` gives its group's content, visible while it is its group's active
pane. The app:

1. registers each pane with its default side and minimum size, then `arrange`s the first or the saved arrangement;
2. fills `DockArea`'s `groups` and `splitters` and the panes' places from `layout` of the area's size, again after
   each change;
3. answers `DockArea`'s callbacks: `drag-over` with `drop_target`'s preview, `dropped` with `move_to`,
   `splitter-moved` with `resize`, `move-to` with `move_to` to `Target::Edge`, `close` with `close`, `activate` with
   `activate`;
4. saves `arrangement()` after a change, e.g. at `dropped`, `splitter-released` and `move-to`.

`DockArea` takes its texts (`words`), colours and font size (`style`) and sizes (`metrics`) as properties, with
neutral English and light defaults.
