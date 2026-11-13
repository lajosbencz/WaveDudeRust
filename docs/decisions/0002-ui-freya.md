# 0002 UI with Freya 0.5 and an own widget crate

Status: superseded by [0019](0019-egui-and-the-platform-layer.md), 2026-11-13; accepted 2026-11-05, stack verified 2026-11-06

Freya (MIT, github.com/marc2332/freya): pure Rust, own reactive and component
model since 0.4, rsx-style DSL, rendered by Skia (skia-safe), Linux + macOS +
Windows. 0.5.0 released 2026-11-05; the dependency is pinned to 0.5.x on
crates.io, never a git revision.

Floem was considered first and dropped: no crates.io release since 0.2.0
(2024-11), so it would have meant riding git main; its maintainers' editor
(Lapce) has slowed.

Skia comes through Freya's own fork `freya-skia-safe` (marc2332/rust-skia),
with prebuilt binaries on that repository's releases, one per desktop target
and feature set (0.104.0: Linux x86_64 and aarch64 with egl, gl, vulkan,
wayland and x11; Windows x86_64 and aarch64 msvc with gl, vulkan, d3d, static
and dynamic CRT; macOS x86_64 and aarch64 with metal and gl; 14 to 22 MB
each, textlayout, svg, webp and jpeg in all). A feature set outside that list
builds Skia from source (clang, Python, ninja, 20 to 40 minutes), so the
workspace keeps Freya's default features. Linux GNU only: no musl asset.

Known costs:
- About 15 to 20 MB of Skia in the binary. CI caches the prebuilt download.
- One maintainer; the fork pins Skia and lags rust-skia by a few versions.
- The accessibility tree is AccessKit (Freya's `accessibility` feature, on by
  default); UI tests still target the app state, not the tree.

Widgets WaveDude needs that Freya does not ship live in `wavedude-widgets`:
dock host, tree with multi-select and drag, property rows, logarithmic number
box, gradient editor, canvas image with overlays, probe graph box, timeline,
menu bar with mnemonics. Freya's own components (buttons, inputs, scroll views,
tooltips, popups, menus) are used where they fit.

Stack check (2026-11-06, Linux, Wayland, release build, the user's
workstation): Freya 0.5 has no canvas element; a rect's background takes a
`RenderCallback` with the Skia canvas, which is the preview's path. A
1920x1080 RGBA8 frame built per tick (`ImageHandle::from_rgba`, no copy),
drawn scaled into the window with 200 antialiased lines over it, paced at
16 ms: 181 renders in 3.05 s, one per frame, the callback 0.35 ms on average
and 1.9 ms at worst; an unpaced loop pushed 300 frames in 0.58 s with the
window redrawing at its own rate. Skia came as a prebuilt download; the
first build took 54 s, a release build 28 s. Keyboard traversal and the two
other platforms are checked when the app exists.

## WPF box model and font (2026-11-10)

The widgets follow WaveDude Next's box model rather than Freya's defaults: `Theme::freya_theme` resizes and
recolours Freya's Button, Input and Tooltip themes (square corners, WPF paddings, 23 px buttons, 22 px combo
boxes), and the text box, number box and tooltip are drawn by the widgets themselves where Freya fixes a look
(a 2 px focus border, rounded tooltips with padding 4,10). The UI font is Segoe UI when installed, else Selawik,
Microsoft's metric-compatible stand-in under the SIL OFL, bundled and registered at start; text widths match
Segoe UI, its line is 14.4 px against Segoe UI's 16 at 12 px, so controls carry fixed heights instead of
deriving them from the line.
