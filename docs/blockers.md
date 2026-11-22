# Blockers

Upstream bugs worked around here; each workaround goes once the fix is released.

- cargo-packager 0.11.8 packs the AppImage with `AppRun` 0744 (downloaded as 0764, `src/package/appimage/mod.rs:63`)
  and 0700 directories, so a user other than the builder gets "Permission denied" (the AppImage catalog's firejail
  test, appimage.github.io#12254). `package.yml`, step "AppImage file modes", extracts it, sets `u=rwX,go=rX` and
  `AppRun` 755, and repacks with the cached linuxdeploy appimage plugin. To report to tauri-apps/cargo-packager.

## egui_mineral

Classic frontend (0.12.x):

- A number box stepped from blank fills in the nearest value; `kit/number.rs` `from_zero`. Ask:
  `NumberEdit::set_blank_steps(BlankStep::FromZero)`.
- egui cancels a drag on Esc, so a scrub never sees it; `kit/number.rs` `hold_escape`. Ask:
  `NumberPass::cancel_scrub_on_escape(ctx)`.
- `apply_text` parses only through `LocaleFormat`; `kit/number.rs` keeps a parse adapter. Ask: `apply_text_with(parse)`.
- No per-widget splitter `min_target`; `kit/mod.rs` overrides it for the whole kit. Ask: `Splitter::min_target`.
- `MenuNav` has no target for the level itself; `kit/menu.rs` keeps a panel-focus flag.

Material frontend: the rows marked open in [`frontends/material/DESIGN.md`](../frontends/material/DESIGN.md)
(84, 88, 89, 90), and the outline's TreeView taking Ctrl+A, which `commands.rs` re-issues as Select all.
