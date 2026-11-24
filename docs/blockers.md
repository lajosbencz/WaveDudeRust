# Blockers

Upstream bugs and gaps, worked around or accepted here; each goes once the fix is released.

- cargo-packager 0.11.8 packs the AppImage with `AppRun` 0744 (downloaded as 0764, `src/package/appimage/mod.rs:63`),
  so a user other than the builder gets "Permission denied" (the AppImage catalog's firejail
  test, appimage.github.io#12254). `package.yml`, step "AppImage file modes", extracts it, sets `AppRun` 755 and
  repacks with the cached linuxdeploy appimage plugin. To report to tauri-apps/cargo-packager.

- Slint 1.19 resolves the desktop's colour scheme into a private global (`SlintInternal.color-scheme`) that only its
  bundled styles may read; the public `Palette.color-scheme` follows it until written, and writing it (the Light and
  Dark themes) ends that binding for good. `src/main.rs` writes it only for a forced theme and, back on System, the
  scheme the desktop had then, updated from winit's `ThemeChanged`. winit 0.30 never raises that event on Linux
  (X11 and Wayland, `theme()` is `None`), where Slint reads the freedesktop settings portal itself
  (`i-slint-backend-winit/xdg_desktop_settings.rs`): there the System theme follows the desktop live until a forced
  theme is chosen, then keeps the scheme it had until restart. Accepted, not worked around. To report to
  slint-ui/slint: a public read of the resolved system scheme.

- Slint 1.19 draws its own `MenuBar` on Linux and the web, with no access keys and no keyboard entry (F10, a lone
  Alt); an open menu walks with the arrow keys. On Windows and macOS it hands the bar to `muda`: native menus with the
  platform's keys, titles passed unchanged, so Windows reads a `&` as an access key and the drawn bar would show it.
  `ui/app.slint` (`menu-text`) gives Windows Next's access keys and the other platforms plain titles. Accepted, not
  worked around. To report to slint-ui/slint: access keys and keyboard entry for the drawn `MenuBar`.
