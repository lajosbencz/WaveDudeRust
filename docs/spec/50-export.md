# 50 - Export

## Formats (`Export/ExportFormat.cs`)

Order (dialog filter order): Mp4, Mpeg1, Avi, Mov, MovPng, Png, Jpeg, Exr, ExrRoot.

| Format | Kind | Extensions | Still | Notes |
|---|---|---|---|---|
| Mp4 | video | mp4, m4v | no | H.264 via Media Foundation (Windows only) |
| Mpeg1 | video | mpg, mpeg, m1v | no | in-house, I-frames only |
| Avi | video | avi | no | uncompressed 24-bit, OpenDML |
| Mov | video | mov, qt | no | QuickTime Animation (`rle `) |
| MovPng | video | mov, qt | no | QuickTime with PNG samples (`png `) |
| Png | sequence | png | yes | 24-bit RGB |
| Jpeg | sequence | jpg, jpeg | yes | baseline 4:4:4 |
| Exr | sequence | exr | yes | raw f16 planes, intensity as I |
| ExrRoot | sequence | exr | yes | raw f16 planes, intensity as sqrt(I) |

A path without a chosen filter maps by extension to the first format in this order (`.mov` -> Mov, `.exr` -> Exr).

## Layers and naming

- Layer choice: Field, Intensity, Both. Both exports two outputs with suffixes `_field` and `_intensity` on a base name
  without a layer suffix.
- Video: one file per layer.
- Sequence: folder `dir/name/` with files `name_NNNN.ext`, `NNNN` = timeline frame number zero-padded to
  `digits = max(4, len(str(first + planned - 1)))`, numbered from the first exported frame. Never a step number.
- Still: `<project>[_field|_intensity]_NNNN.ext`, NNNN = shown timeline frame, padded to 4 (D4).
- An existing target is replaced (the UI asks unless "Export last").

## Range

```
VE = VisualizeEvery ; lastFrame = FrameCount - 1
first = clamp(ExportFrom / VE, 0, lastFrame)
last  = ExportTo <= 0 ? lastFrame : clamp(ExportTo / VE, first, lastFrame)
```
(integer division; ExportFrom/To are steps).

## Run selection

Export uses the current run only when it is fresh, complete and at Full quality (divisor 1, every frame, Half). Otherwise
a full-quality run is made first. Exr needs a run whose intensity plane holds I (`IntensityRoot = false`); ExrRoot and
all colour formats use sqrt(I) planes.

## Per-frame rendering (colour formats)

```
for frame in first..last:
  plane = store.CopyLayer(frame, layer)                       (f16)
  rgb   = halfTable(scheme, blackLevel, layer brightness, kind)[plane]  at canvas size (no resampling)
  if layer.OverlayInExport and scene edges exist: rgb = BlendAdditive(rgb, edges, OverlayGain)
  if SourceMarkersInExport: draw source markers (enabled sources: marker, composite dots, name)
  if ProbeMarkersInExport: draw probe markers
  if ProbeGraphsInExport: draw probe graph boxes (as the preview shows them, at the frame)
  sink.Write(rgb)
```
Exr/ExrRoot sinks receive the f16 plane itself: no colours, brightness, outline, markers or boxes.
Frame rate = `Display.Video.Fps`; quality = `Display.Video.Quality` (1..100).

### Export pipeline (Rust)

`wave_export::ExportPlan::build_job` is the one place the app and the CLI decide an export: the frame range (a still
beyond the run is an error; the app clamps it first), each layer's scheme (from a `SchemeStore` the caller loads),
look and path (`_field`/`_intensity` for Both), the sinks and the overlays. The scene is composed only when a layer
burns in its glowing edges, its one use, so the output equals always composing it. Overlays are dropped for a format
that takes the stored halves (OpenEXR).

- Every output is opened before the first frame; when one cannot be opened, those already open are finished and
  removed.
- Every sink is finished (and flushed) whatever the outcome; outputs of a failed or cancelled job are then removed
  unless `keep_partial`. The report lists the files left on disk.
- The job asks the source to `wait_for` each frame, so an export can read a run still in progress (the CLI does,
  except with probe graphs, which need the finished recordings).
- Single-file video sinks (MP4, MPEG-1, AVI) run on a thread of their own fed through two recycled frame buffers.
  Image sequences and MOV samples are encoded on one encoder pool per job (at most 16 threads); MOV samples are
  written in frame order.
- Probe markers are rasterised once per layer into a premultiplied layer, as source markers are; a label's
  antialiased edge can differ by one level from blending it straight onto the frame.
- The CLI cancels on Ctrl+C.

Graph boxes and source markers are drawn by the app's WPF code; their look is in "Burned-in overlays" below (graph
box layout still `TODO(verify)` there; golden exports: fixture G-12 in 80).

### Probe markers (`Export/ProbeMarkers.cs`)

```
h = frame height
arm  = max(6, h/120)                       (int)
font = max(12, h/60)
point probe at (x, y): cross of half-length arm, drawn black with radius 2 then white with radius 1
line probe: thick line A-B (black r2, white r1) plus a cross at A and at B
ThickLine: steps = max(|dx|, |dy|); point k at (x0 + (int)round_even((x1-x0)*k/steps), y0 + (int)round_even((y1-y0)*k/steps)),
           stamped as a filled square of (2r+1) x (2r+1) pixels, clipped to the frame
label: "n" (probe number, 1-based in list order) or "nA" / "nB" for line ends
label origin: (x + arm + 2, y - arm - round_even(font*1.4))
```
Label font: see "Burned-in overlays" below.

## PNG (`Export/PngEncoder.cs`)

- Signature, IHDR (8-bit, colour type 2 RGB, no interlace), IDAT chunks of at most 65536 bytes, IEND.
- zlib stream, deflate level "Optimal"; filter Paeth on every row (sequences, stills); filter None for MOV-PNG samples.
- CRC32 standard. Byte-exactness with .NET's deflate is not required; decoded pixels must match.

## JPEG (`Export/JpegEncoder.cs`, `WicJpegEncoder.cs`)

- Baseline JFIF, 4:4:4 (no chroma subsampling), YCbCr full range (JFIF), Annex K standard tables (quant and Huffman).
- Quality q (1..100) scaling (IJG): `scale = q < 50 ? 5000/q : 200 - 2q`;
  `table[i] = clamp((base[i]*scale + 50)/100, 1, 255)` (integer).
- Restart interval (DRI): one band of 16 pixel rows = two MCU rows (`2*ceil(W/8)` MCUs); the last may be shorter.
- On Windows the app prefers WIC with `ImageQuality = q/100` and 4:4:4; output bytes differ from the in-house encoder.
  Rust: in-house encoder only.

## MPEG-1 (`Export/Mpeg1Writer.cs`, `YuvPlanes.cs`)

- Program stream: pack header per picture, system header, PES packets <= 65000 bytes, end code.
- Video: I-frames only, GOP header every 12 pictures, one slice per macroblock row, default intra matrix.
- 4:2:0, studio range BT.601, f32:
  `Y = 16 + 0.257R + 0.504G + 0.098B`, `Cb = 128 - 0.148R - 0.291G + 0.439B`, `Cr = 128 + 0.439R - 0.368G - 0.071B`;
  chroma is the 2x2 mean `(a + b + c + d)*0.25f` of the unclamped values; every byte `clamp((int)round_even(v), 0, 255)`.
  (Full-range JFIF matrix used by the JPEG encoder: `Y = 0.299R + 0.587G + 0.114B`,
  `Cb = 128 - 0.168736R - 0.331264G + 0.5B`, `Cr = 128 + 0.5R - 0.418688G - 0.081312B`.)
- Quantiser scale: `clamp(1 + round_even(30*((100 - q)/99)^1.5), 1, 31)`.
- Frame rate code: nearest of [23.976, 24, 25, 29.97, 30, 50, 59.94, 60] to Fps (warning logged if it differs by
  > 0.01). Sizes padded to multiples of 16 by repeating the last column and the last row. SCR/PTS clock
  `round_even(frame*90000/rate)`. Rust: PTS = SCR + 9000 (0.1 s), so every packet of a large picture arrives
  before its presentation time.

## AVI (`Export/AviWriter.cs`)

- Uncompressed 24-bit BGR, bottom-up rows, rows padded to 4 bytes, chunks `00db`.
- OpenDML: RIFF `AVI ` then `AVIX` segments of up to 1 GiB; standard index `ix00` per segment plus super index `indx`;
  legacy `idx1` in the first segment; `dmlh` with the total frame count. Headers rewritten at close.
- Frame rate: `dwScale = 1`, `dwRate = Fps` (int, >= 1).
- Rust: a size whose frame chunk does not fit 32 bits (4 GiB) is refused when the writer opens.

## MOV (`Export/MovWriter.cs`)

- `ftyp` brand `qt  `; `moov` reserved up front as a `free` atom of `1024 + 12*frames` bytes when the frame count is
  known (moov written into it at close), else moov at the end; `mdat` with 64-bit size; chunk offsets `co64`.
- Codec `rle ` (Animation, 24-bit, every frame a key frame, full-frame RLE) or `png ` (each sample a PNG, filter None).
- Spatial quality 0x3FF (lossless). Timescale = Fps (int), each sample duration 1.

## MP4 (`Export/Mp4Writer.cs`)

- Media Foundation sink writer, H.264, input NV12 preferred (RGB32 fallback), rate control quality mode
  (`Quality`), no B-frames, GOP 1 s.
- Bitrate fallback: `clamp(round_even(W*H*fps*0.01*50^(q/100)), 300000, 200000000)` bit/s.
- Odd sizes padded to even by repeating the last row/column; maximum 4096 x 2304.
- Rust: openh264 and the `mp4` crate, decision 0005.

## OpenEXR (`Export/ExrWriter.cs`)

Single-part scanline, uncompressed, one channel `Y` of type HALF.
```
magic 20000630 (i32 LE: 76 2f 31 01), version 2 (i32, no flags)
attributes (name\0 type\0 size:i32 value), in order:
  channels          chlist   : "Y\0" pixelType=1 (HALF) i32, pLinear u8=0, reserved 3 bytes, xSampling=1, ySampling=1, then \0
  compression       compression : 0 (NONE)
  dataWindow        box2i    : 0, 0, W-1, H-1
  displayWindow     box2i    : 0, 0, W-1, H-1
  lineOrder         lineOrder: 0 (INCREASING_Y)
  pixelAspectRatio  float    : 1
  screenWindowCenter v2f     : 0, 0
  screenWindowWidth float    : 1
  comments          string   : "WaveDude Next"
header end \0
offset table: H entries u64 (one scanline per chunk)
chunks: y:i32, byteCount:i32 (= 2W), W halves LE
```
Values are the stored samples: field u, intensity I (Exr) or sqrt(I) (ExrRoot), at canvas size.

## Probe CSV (`Export/ProbeCsv.cs`)

Invariant culture, `;` separator, `.` decimal point, f64 values in .NET's default `ToString` (shortest round-trip;
exponent form `1E-05` style for small/large magnitudes, unlike Rust's `{}`; match .NET's format), UTF-8 without BOM,
line ending `Environment.NewLine` (`\r\n` on Windows; Rust: choose `\n` or `\r\n` deliberately).

Point probe:
```
step;amplitude            (or value, integrated value)
0;<value after step 1>
1;<value after step 2>
...
```
Line probe:
```
frame;step;<d0>;<d1>;...           d_k = n > 1 ? k*length/(n-1) : 0, formatted "0.###"
<frame>;<step>;<v0>;<v1>;...       one row per recorded profile p: frame = p*frameStride, step = frame*VE
```
File name `<project>_<probe>.csv`: the probe name trimmed, `probe` if then empty; characters invalid in Windows file
names (`<>:"/\|?*` and control chars 0..31) become `_`; inner spaces kept.
Range: the full run (a run is made first if needed).

## Burned-in overlays

What colour exports (not OpenEXR) draw over a rendered frame. The behaviour of the same items in the app is in
`docs/ui/40-preview-pane.md` ("Handles and overlays", "Probe graph box") and the export entries in
`docs/ui/10-menus-and-shortcuts.md` ("File > Export", the pictures' and graph boxes' context menus). Pointers:
`WaveDudeNext.WPF/MainWindow.Export.cs`, `Export/ExportJob.cs` (`Finish`), `Export/ProbeMarkers.cs`,
`WaveDudeNext.WPF/Rendering/SourceMarkerPainter.cs`, `WaveDudeNext.WPF/Rendering/ProbeGraphPainter.cs`,
`WaveDudeNext.WPF/Controls/ProbeGraphDrawing.cs`, `Probes/ProbeGraph.cs`.

### Frame and coordinates

The frame is the canvas, `W x H` pixels (exports are always full resolution), packed BGRA, opaque. Overlay positions
are canvas pixels; a source marker is centred on the pixel centre `(x + 0.5, y + 0.5)`, probe strokes on the integer
pixel `(x, y)`. Everything is clipped to the frame. Probes and sources are copied when the export starts; edits
during the export change nothing.

### Order (per frame, per layer)

```
1  colours (renderer, 40)
2  glowing edges, additive, if DisplayLayer.OverlayInExport and OverlayGain > 0 (formula in 20)
3  source markers, if Display.SourceMarkersInExport: enabled sources, on both layers
     3a every emitter dot of every composite source
     3b per source: disc, glyph, centre dot, name
4  probe markers, if Display.ProbeMarkersInExport: probes of this layer only
     4a per probe: strokes, black pass then white pass
     4b per probe: label(s)
5  probe graph boxes, if Display.ProbeGraphsInExport: active probes of this layer, in project order
```
Probes counted are the active ones (`Probe.Active`); their numbers `1..` are their 1-based position among the active
probes in project order, shared by both layers. Nothing is selected in an export: no accent border, no "selected
last" rule.

### Compositing

Antialiased items (3, 4b, 5) are rasterised into premultiplied BGRA and blended per channel:
```
out = min(255, src + (dst*(255 - a) + 127)/255)        integer arithmetic, result opaque
```
Source markers are rasterised once (they do not move) into a frame-sized layer; labels and boxes per frame into
rectangles of their own size (label: `ceil(text width) + 4` x `ceil(text height) + 4`; box: its canvas rectangle).
Probe strokes (4a) overwrite pixels (no blending; exact algorithm under "Probe markers" above).

### Scale

```
F   = max(12, H/60)          (f32)  probe label size in px
arm = max(6, H/120)          (int)  probe cross arm
k   = F / 11                        source marker scale (11 px = the preview's label size)
```
Graph boxes are not scaled with the frame: the box has its canvas size (`BoxInCanvas`, 10) and its text is 11 px.

### Fonts

| Item | Family | Weight | Size | Culture |
|---|---|---|---|---|
| probe label ("n", "nA", "nB") | Segoe UI | Bold | `F` px | invariant digits |
| source name | Segoe UI | Normal | `11*k` px | the UI culture for shaping |
| graph box text (title, readout, tick labels) | Segoe UI | Normal | 11 px, one line | numbers in the current culture |

`TODO(verify)`: Segoe UI is not redistributable outside Windows; the Rust port needs a substitute (user decision) and
cannot match the WPF rasteriser pixel for pixel. Text pixels are therefore compared by golden image tolerance
(fixture G-12), not byte for byte.

### Colours and geometry

Probe markers: black `#000000` stroke stamps of radius 2, then white `#FFFFFF` of radius 1. Label: shadow ARGB
(220, 0, 0, 0) drawn at (3, 3), white text at (2, 2) inside its rectangle; rectangle top-left at `LabelOrigin`
(above).

Source markers (all lengths times `k`; halo = ARGB (191, 0, 0, 0)):
- emitter dot: white disc radius 2.5, outlined by a halo pen 1.5 wide;
- disc: ARGB (0x60, 0, 0, 0), radius 6, at the centre;
- glyph in a 20 x 20 box centred on the source: ring of radius 6 about (10, 10), ticks (10, 0)-(10, 2.5),
  (10, 17.5)-(10, 20), (0, 10)-(2.5, 10), (17.5, 10)-(20, 10); stroked with a halo pen 3.5 wide, round caps, then a
  white pen 1.5 wide, round caps;
- centre dot: halo disc radius 2.5, white disc radius 1.5;
- name (skipped when empty): outline pen halo 3.0 wide with round joins under white text; point source: left edge at
  `cx + 13`, vertically centred on `cy`; composite source: horizontally centred on `cx`, top at `cy + 13`; clamped so
  the text stays inside the frame (`[0, W - width]`, `[0, H - height]`).

Graph boxes (fixed author colours, ARGB):

| Element | Colour |
|---|---|
| background | (235, 14, 16, 22) |
| border, 1 px | (170, 200, 200, 210) |
| grid lines | (46, 255, 255, 255) |
| zero line | (120, 255, 255, 255) |
| plot frame | (130, 200, 200, 210) |
| tick labels | (225, 225, 225, 230) |
| title | (240, 240, 240, 245) |
| curve and readout | (255, 255, 208, 64); curve width `max(1.2, boxHeight/110)`, round joins, clipped to the plot; a single sample is a disc of radius 2 |

The box shows the run's recording as it stood at the exported frame (step `frame*VisualizeEvery`). `TODO(verify)`:
the layout (margins, plot rectangle, tick choice and labels, readout text) is computed by `ProbeGraph.Layout` and is
not yet specified; it must be specified once (core crate) and shared by preview and export.

### Preview and export

- Graph boxes: the same layout and drawing routine as the preview; the preview lays the box out at its size on screen
  (11 px text in screen pixels), the export at its canvas size (11 px text in canvas pixels), so the two are identical
  when the picture is shown at 1:1 and differ in text-to-box proportion otherwise.
- Source markers: the preview's source handle glyph, emitter dots and label style, scaled by `k`, without selection
  colours and levers.
- Probe markers are the author's export style (crosses, numbers), not the preview's probe handles.
- Never exported: part handles and outlines, levers, grips, probe handles and lines of the preview, item labels other
  than source names, the edge line, the busy dim, dithering.
