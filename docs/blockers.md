# Blockers

Upstream bugs worked around here; each workaround goes once the fix is released.

- cargo-packager 0.11.8 packs the AppImage with `AppRun` 0744 (downloaded as 0764, `src/package/appimage/mod.rs:63`),
  so a user other than the builder gets "Permission denied" (the AppImage catalog's firejail
  test, appimage.github.io#12254). `package.yml`, step "AppImage file modes", extracts it, sets `AppRun` 755 and
  repacks with the cached linuxdeploy appimage plugin. To report to tauri-apps/cargo-packager.
