// The executor of the engine's draw list (crates/wavedude-view/src/draw.rs) on a Canvas2D, after the desktop's
// Skia one (crates/wavedude-widgets/src/paint.rs, draw_shapes).

const TAU = 2 * Math.PI;
const SOLID = [];
// Fonts by text baseline: the platform's sans-serif, and the page's for top-anchored text.
const FONTS = { Alphabetic: "sans-serif", Top: getComputedStyle(document.body).fontFamily };

const measurer = document.createElement("canvas").getContext("2d");

/** The `measureText` of `text` at `size` px in the font of `baseline`, for the engine's layout. */
export function measure(text, size, baseline) {
	measurer.font = `${size}px ${FONTS[baseline]}`;
	return measurer.measureText(text);
}

function setPen(ctx, { color, width, dash, round }) {
	ctx.strokeStyle = color;
	ctx.lineWidth = width;
	ctx.setLineDash(dash ?? SOLID);
	ctx.lineJoin = round ? "round" : "miter";
	ctx.lineCap = round ? "round" : "butt";
	ctx.miterLimit = 4;
}

const rect = (ctx, { left, top, right, bottom }) => ctx.rect(left, top, right - left, bottom - top);

/** Fills or strokes the current path. */
function paint(ctx, ink) {
	if (ink.kind === "Fill") {
		ctx.fillStyle = ink.color;
		ctx.fill();
	} else {
		setPen(ctx, ink);
		ctx.stroke();
	}
}

// The font ascent by `ctx.font` string.
const ascents = new Map();

function ascentOf(ctx, font) {
	let ascent = ascents.get(font);
	if (ascent === undefined) ascents.set(font, (ascent = ctx.measureText("0").fontBoundingBoxAscent));
	return ascent;
}

/** A text's halo, then the text; "Top" places the line box's top, through the alphabetic baseline. */
function drawText(ctx, { text, at, baseline, size, color, halo }) {
	const font = `${size}px ${FONTS[baseline]}`;
	ctx.font = font;
	const y = baseline === "Top" ? at[1] + ascentOf(ctx, font) : at[1];
	if (halo) {
		setPen(ctx, { ...halo, round: true });
		ctx.strokeText(text, at[0], y);
	}
	ctx.fillStyle = color;
	ctx.fillText(text, at[0], y);
}

/**
 * The bitmap of a Dots shape at device pixels, redrawn when its place, size, count or scale changes: one picture's
 * dots change only with its size.
 */
export class DotCache {
	canvas = document.createElement("canvas");
	key = "";

	draw(ctx, { origin, size, points, halo, dot }, scale) {
		const { canvas } = this;
		const key = `${origin} ${size} ${points.length} ${scale}`;
		if (key !== this.key) {
			this.key = key;
			canvas.width = Math.max(1, Math.ceil(size[0] * scale));
			canvas.height = Math.max(1, Math.ceil(size[1] * scale));
			const c = canvas.getContext("2d");
			c.scale(scale, scale);
			for (const { radius, color } of [halo, dot]) {
				c.fillStyle = color;
				for (const [x, y] of points) {
					c.beginPath();
					c.arc(x, y, radius, 0, TAU);
					c.fill();
				}
			}
		}
		ctx.drawImage(canvas, origin[0], origin[1], canvas.width / scale, canvas.height / scale);
	}
}

/**
 * Draws a draw list (crates/wavedude-view/src/draw.rs) in order, as the desktop's Skia does, in units of `scale`
 * device pixels. Canvas2D antialiases every shape.
 */
export function drawShapes(ctx, shapes, scale, dots) {
	for (const s of shapes) {
		ctx.beginPath();
		switch (s.kind) {
			case "Line":
				ctx.moveTo(s.from[0], s.from[1]);
				ctx.lineTo(s.to[0], s.to[1]);
				setPen(ctx, s.pen);
				ctx.stroke();
				break;
			case "Path":
				for (const [x, y] of s.points) ctx.lineTo(x, y);
				if (s.closed) ctx.closePath();
				setPen(ctx, s.pen);
				ctx.stroke();
				break;
			case "Circle":
				ctx.arc(s.centre[0], s.centre[1], s.radius, 0, TAU);
				paint(ctx, s.ink);
				break;
			case "Rect":
				rect(ctx, s.rect);
				paint(ctx, s.ink);
				break;
			case "Text":
				drawText(ctx, s);
				break;
			case "Dots":
				dots.draw(ctx, s, scale);
				break;
			case "Clip":
				ctx.save();
				rect(ctx, s.rect);
				ctx.clip();
				break;
			case "Unclip":
				ctx.restore();
				break;
		}
	}
}
