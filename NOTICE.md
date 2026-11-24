# Notice

WaveDudeRust is written from scratch: no code of WaveDude Next or Wave Dude 2D is carried over. It is a modified
work of WaveDude Next in the sense of the GNU General Public License, carrying its non-code material under the
same licence (GPL-3.0-or-later): the colour schemes, the example projects, the user interface strings and the
drawn icons (converted from its XAML by [`tools/next-icons.py`](tools/next-icons.py)).

## Upstream projects

- **WaveDude Next** by Alexander Graef, GPL-3.0 - <https://github.com/graealex/WaveDudeNext>. Its `.wdproj`
  project files open and save here.
- **Wave Dude 2D** by Jeroen Vleggaar (Huygens Optics) - <https://www.youtube.com/@HuygensOptics>, the predecessor
  of WaveDude Next. Its
  `configuration.json` project files open here.
- **WaveSimulator2D** by 0x23 - <https://github.com/0x23/WaveSimulator2D>, whose numerical approach Wave Dude 2D
  followed.

All names and trademarks belong to their respective owners.

## Downloaded codec

MP4 export can use OpenH264, which the program downloads from Cisco at the user's request and which is not part
of its packages (decision 0005): "OpenH264 Video Codec provided by Cisco Systems, Inc."

## Bundled fonts

- **Selawik** by Microsoft Corporation - <https://github.com/microsoft/Selawik> (release 1.01), a metric-compatible
  stand-in for Segoe UI, used when Segoe UI is not installed. Licensed under the SIL Open Font License 1.1; the
  license text ships beside the fonts in [`crates/waveduderust/assets/fonts/Selawik-OFL.txt`](crates/waveduderust/assets/fonts/Selawik-OFL.txt). Selawik is a
  Reserved Font Name and a trademark of Microsoft Corporation.
- **Noto Sans Math** by Google LLC - <https://github.com/notofonts/math> (version 3.000), subset to the Greek,
  super- and subscript, letterlike, arrow and math operator blocks: the glyphs Selawik lacks. Licensed under the SIL
  Open Font License 1.1; the license text ships beside the fonts in
  [`crates/waveduderust/assets/fonts/NotoSansMath-OFL.txt`](crates/waveduderust/assets/fonts/NotoSansMath-OFL.txt).

## Bundled icons

- **Material Symbols** by Google LLC - <https://github.com/google/material-design-icons> (Outlined, 24 px SVG), standing in
  for the Segoe Fluent Icons glyphs of WaveDude Next's menus, which are not redistributable. Licensed under the Apache
  License 2.0; in [`frontends/waveduderust_slint/ui/icons/symbols`](frontends/waveduderust_slint/ui/icons/symbols/).
