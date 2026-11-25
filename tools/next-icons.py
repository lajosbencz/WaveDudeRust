#!/usr/bin/env python3
"""Writes WaveDude Next's drawn icons (XAML StreamGeometry resources) as SVG files for the Slint app.

Usage: tools/next-icons.py   (Next's source tree from $WAVEDUDE_NEXT_DIR)

Each icon is drawn as Next draws it: filled with the even-odd rule, or stroked at Next's thickness; the view box is
the fixed box Next draws it in, else the geometry's extent from the origin as a WPF Path measures it, widened by the
stroke and rounded out to whole pixels. Ink is black; the window colours it (`Image.colorize`).

The Material Symbols standing in for Next's Segoe Fluent glyphs (docs/divergences.md, Icons) go to the menus, whose
icons Slint draws uncoloured: each gets a copy in each theme's menu text colour and `ui/menu-icons.slint` picks one; so do
the drawings of `MENU_DRAWINGS`, the add menus' kinds.
"""

import math
import os
import re
import sys

NEXT = os.environ.get("WAVEDUDE_NEXT_DIR")
UI = "frontends/waveduderust_slint/ui"
OUT = os.path.join(UI, "icons/next")
SYMBOLS = os.path.join(UI, "icons/symbols")

# (key, svg name, style, box), all of Themes/Icons.xaml: style "fill" or a stroke thickness; box the fixed size Next draws it in, (w, h) or
# 16 for the 16 px icons, which have a Fill part too; None to measure
ICONS = [
	# glyphs of menus, combo boxes, check boxes, scroll bars, dock buttons
	("ArrowUpGeometry", "arrow-up", "fill", None),
	("ArrowDownGeometry", "arrow-down", "fill", None),
	("ArrowLeftGeometry", "arrow-left", "fill", None),
	("ArrowRightGeometry", "arrow-right", "fill", None),
	("SubmenuArrowGeometry", "submenu-arrow", "fill", None),
	("CheckGeometry", "check", 1.5, None),
	("ChevronRightGeometry", "chevron-right", 1.0, None),
	("ChevronDownGeometry", "chevron-down", 1.0, None),
	("ChevronUpGeometry", "chevron-up", 1.0, None),
	("CloseGeometry", "close", "fill", None),
	("DropDownGeometry", "drop-down", "fill", None),
	# the tree's and menus' 16 px drawings (IconView: a 1 px stroke and an optional Fill part)
	*[
		("Icon" + k, re.sub(r"(?<!^)(?=[A-Z])", "-", k).lower(), 1.0, 16)
		for k in [
			"Source", "Probe", "CountingDetector", "ImagingDetector", "Part", "PlusBadge", "Project", "Lens",
			"LensConvexConvex", "LensConvexFlat", "LensConvexConcave", "LensFlatConvex", "LensFlatFlat",
			"LensFlatConcave", "LensConcaveConvex", "LensConcaveFlat", "LensConcaveConcave", "FresnelLens", "Fiber",
			"BraggReflector", "GrinLens", "ZonePlate", "Diffuser", "InterferenceFilter", "DiffractionGratings",
			"MetaSurfaces", "MetaLens", "MetaPrism", "MetaAxicon", "FacettedGrating", "SimpleGrating", "Polygon",
			"Rectangle", "Ellipse", "Triangle", "Outline", "OpticalLogic", "XorGate", "AndGate", "OrGate",
			"AutoBrightness", "PreviewResolution", "PreviewFrames", "PreviewSamples", "PreviewDither", "FrameMemory",
		]
	],
	# transport bar and picture headers, filled
	*[
		(k + "Geometry", re.sub(r"(?<!^)(?=[A-Z])", "-", k).lower(), "fill", None)
		for k in [
			"Run", "PauseRun", "Stop", "Field", "Intensity", "Play", "Pause", "SideBySide", "Stacked", "StepBack",
			"StepForward", "SetIn", "SetOut", "ClearRange", "Loop", "Live",
		]
	],
	("SpinnerGeometry", "spinner", 3.0, (36, 36)),
	("ResizeGripGlyph", "resize-grip", "fill", None),
	# the tree rows' buttons, stroked at 1.2
	("PlusGeometry", "tree-plus", 1.2, None),
	("MinusGeometry", "tree-minus", 1.2, None),
	("RowUpGeometry", "tree-chevron-up", 1.2, None),
	("RowDownGeometry", "tree-chevron-down", 1.2, None),
	# Messages, number boxes
	("MessagesClearGeometry", "clear", 1.2, None),
	("GripGeometry", "grip", 1.0, None),
	("ChainLinkedGeometry", "chain-linked", 1.3, (18, 16)),
	("ChainPartedGeometry", "chain-parted", 1.3, (18, 16)),
]

# Next's drawings its add menus show, themed for the menus as the symbols are
MENU_DRAWINGS = [
	"probe", "counting-detector", "imaging-detector", "lens", "fresnel-lens", "fiber", "bragg-reflector", "grin-lens",
	"zone-plate", "diffuser", "interference-filter", "diffraction-gratings", "simple-grating", "facetted-grating",
	"meta-surfaces", "meta-lens", "meta-prism", "meta-axicon", "polygon", "rectangle", "ellipse", "triangle", "outline",
	"optical-logic", "xor-gate", "and-gate", "or-gate", "auto-brightness",
]
TOKEN = re.compile(r"[A-Za-z]|-?\d*\.?\d+(?:[eE][-+]?\d+)?")
ARGS = {"M": 2, "L": 2, "H": 1, "V": 1, "C": 6, "S": 4, "Q": 4, "T": 2, "A": 7, "Z": 0}


def geometry(xaml, key):
	m = re.search(r'x:Key="%s">([^<]*)</StreamGeometry>' % re.escape(key), xaml)
	return m.group(1).strip() if m else None


def arc_points(x0, y0, rx, ry, phi, large, sweep, x1, y1):
	"""Points along an SVG arc (endpoint form), for its extent."""
	if rx == 0 or ry == 0:
		return [(x1, y1)]
	c, s = math.cos(math.radians(phi)), math.sin(math.radians(phi))
	dx, dy = (x0 - x1) / 2, (y0 - y1) / 2
	x1p, y1p = c * dx + s * dy, -s * dx + c * dy
	rx, ry = abs(rx), abs(ry)
	lam = x1p**2 / rx**2 + y1p**2 / ry**2
	if lam > 1:
		rx, ry = rx * math.sqrt(lam), ry * math.sqrt(lam)
	num = rx**2 * ry**2 - rx**2 * y1p**2 - ry**2 * x1p**2
	den = rx**2 * y1p**2 + ry**2 * x1p**2
	k = math.sqrt(max(0.0, num / den)) if den else 0.0
	if large == sweep:
		k = -k
	cxp, cyp = k * rx * y1p / ry, -k * ry * x1p / rx
	cx, cy = c * cxp - s * cyp + (x0 + x1) / 2, s * cxp + c * cyp + (y0 + y1) / 2

	def angle(ux, uy, vx, vy):
		return math.atan2(ux * vy - uy * vx, ux * vx + uy * vy)

	t1 = angle(1, 0, (x1p - cxp) / rx, (y1p - cyp) / ry)
	dt = angle((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry)
	if not sweep and dt > 0:
		dt -= 2 * math.pi
	elif sweep and dt < 0:
		dt += 2 * math.pi
	pts = []
	for i in range(65):
		t = t1 + dt * i / 64
		x, y = rx * math.cos(t), ry * math.sin(t)
		pts.append((c * x - s * y + cx, s * x + c * y + cy))
	return pts


def extent(d):
	"""The smallest and largest x and y the path reaches."""
	toks = TOKEN.findall(d)
	i, cmd, x, y, sx, sy, pts = 0, None, 0.0, 0.0, 0.0, 0.0, []
	while i < len(toks):
		if toks[i].isalpha():
			cmd = toks[i]
			i += 1
			if cmd in "Zz":
				x, y = sx, sy
				continue
		up, rel = cmd.upper(), cmd.islower()
		a = [float(t) for t in toks[i : i + ARGS[up]]]
		i += ARGS[up]
		ox, oy = (x, y) if rel else (0.0, 0.0)
		if up == "H":
			x = a[0] + (x if rel else 0)
		elif up == "V":
			y = a[0] + (y if rel else 0)
		elif up == "A":
			nx, ny = a[5] + ox, a[6] + oy
			pts += arc_points(x, y, a[0], a[1], a[2], int(a[3]), int(a[4]), nx, ny)
			x, y = nx, ny
		else:
			for j in range(0, len(a), 2):
				pts.append((a[j] + ox, a[j + 1] + oy))
			x, y = a[-2] + ox, a[-1] + oy
			if up == "M":
				sx, sy = x, y
				cmd = "l" if rel else "L"
		pts.append((x, y))
	xs, ys = [p[0] for p in pts], [p[1] for p in pts]
	return min(xs), min(ys), max(xs), max(ys)


def path(d, style):
	if style == "fill":
		return f'<path d="{d}" fill="#000" fill-rule="evenodd"/>'
	return f'<path d="{d}" fill="none" stroke="#000" stroke-width="{style:g}"/>'


def svg(parts, x0, y0, w, h):
	return (
		f'<svg xmlns="http://www.w3.org/2000/svg" width="{w:g}" height="{h:g}" viewBox="{x0:g} {y0:g} {w:g} {h:g}">'
		+ "".join(parts)
		+ "</svg>\n"
	)


def colour(xaml, key):
	m = re.search(r'x:Key="%s" Color="#FF([0-9A-Fa-f]{6})"' % key, xaml)
	return "#" + m.group(1).lower()


def menu_icons(wpf):
	"""Each theme's copy of the symbols and the global that picks one."""
	themes = {t: colour(read(os.path.join(wpf, f"Themes/{t.title()}.xaml")), "MenuForeground") for t in ("dark", "light")}
	names = sorted(f[:-4] for f in os.listdir(SYMBOLS) if f.endswith(".svg"))
	for theme, ink in themes.items():
		os.makedirs(os.path.join(SYMBOLS, theme), exist_ok=True)
		for name in names:
			text = read(os.path.join(SYMBOLS, name + ".svg")).replace("<path ", f'<path fill="{ink}" ')
			with open(os.path.join(SYMBOLS, theme, name + ".svg"), "w", encoding="ascii") as f:
				f.write(text)
		os.makedirs(os.path.join(OUT, theme), exist_ok=True)
		for name in MENU_DRAWINGS:
			with open(os.path.join(OUT, theme, name + ".svg"), "w", encoding="ascii") as f:
				f.write(read(os.path.join(OUT, name + ".svg")).replace('"#000"', f'"{ink}"'))
	lines = [
		"// Generated by tools/next-icons.py: the menus' icons in the theme's menu text colour (Slint draws a menu",
		"// entry's icon uncoloured).",
		'import { Theme } from "theme.slint";',
		"export global MenuIcons {",
	]
	for name in names:
		prop = name.replace("_", "-")
		lines.append(
			f'    out property <image> {prop}: Theme.dark ? @image-url("icons/symbols/dark/{name}.svg")'
			f' : @image-url("icons/symbols/light/{name}.svg");'
		)
	for name in MENU_DRAWINGS:
		lines.append(
			f'    out property <image> next-{name}: Theme.dark ? @image-url("icons/next/dark/{name}.svg")'
			f' : @image-url("icons/next/light/{name}.svg");'
		)
	lines.append("}")
	with open(os.path.join(UI, "menu-icons.slint"), "w", encoding="ascii") as f:
		f.write("\n".join(lines) + "\n")


def read(path):
	with open(path, encoding="utf-8") as f:
		return f.read()


def main():
	if not NEXT:
		sys.exit("set WAVEDUDE_NEXT_DIR to WaveDude Next's source tree")
	wpf = os.path.join(NEXT, "WaveDudeNext.WPF")
	os.makedirs(OUT, exist_ok=True)
	xaml = read(os.path.join(wpf, "Themes/Icons.xaml"))
	for key, name, style, box in ICONS:
		d = geometry(xaml, key)
		fill = geometry(xaml, key + "Fill") if box == 16 else None
		if d is None and fill is None:
			sys.exit(f"{key} is not in Themes/Icons.xaml")
		parts = [path(p, s) for p, s in ((d, style), (fill, "fill")) if p]
		if box:
			w, h = (box, box) if box == 16 else box
			x0 = y0 = 0
		else:
			x0, y0, x1, y1 = extent(d)
			pad = 0 if style == "fill" else style
			x0, y0 = math.floor(min(0, x0 - pad)), math.floor(min(0, y0 - pad))
			w, h = math.ceil(x1 + pad) - x0, math.ceil(y1 + pad) - y0
		with open(os.path.join(OUT, name + ".svg"), "w", encoding="ascii") as f:
			f.write(svg(parts, x0, y0, w, h))
	menu_icons(wpf)
	print(f"{len(ICONS)} icons in {OUT}; menu icons in {SYMBOLS}")


if __name__ == "__main__":
	main()
