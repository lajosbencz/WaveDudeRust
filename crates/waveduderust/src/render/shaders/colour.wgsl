// A stored plane coloured into a render pass as render/layer.rs `paint` and the outline blend colour it on the CPU
// (render/gpu.rs). Prepended: half.wgsl.
//
// A half plane: per picture pixel the screen renderer's resampling with its own axes' weights (rows first, f32,
// squared and rooted for an intensity root), rounded to f16, looked up in the half table, or in the deep table and
// dithered at the pixel; then the outline added, clamped at 255. An index plane: each stored sample through the
// palette plus the outline at the stored size, scaled bilinearly over the picture as a texture's sampler does.

struct Colour {
	// The picture's rectangle in framebuffer pixels.
	origin: vec2<f32>,
	size: vec2<f32>,
	mode: u32,
	first_word: u32,
	stored_w: u32,
	stored_h: u32,
	// The pixels coloured: the picture's for a half plane, the stored samples for an index plane.
	grid_w: u32,
	grid_h: u32,
	energy: u32,
	outline: u32,
	side: u32,
	thresholds: u32,
	x_first: u32,
	x_offsets: u32,
	y_first: u32,
	y_offsets: u32,
	x_weights: u32,
	y_weights: u32,
	srgb: u32,
	_pad0: u32,
	_pad1: u32,
	_pad2: u32,
}

const PLAIN: u32 = 0u;
const DITHERED: u32 = 1u;

@group(0) @binding(0) var<uniform> c: Colour;
@group(0) @binding(1) var<storage, read> plane: array<u32>;
@group(0) @binding(2) var<storage, read> lut: array<u32>;
@group(0) @binding(3) var<storage, read> axes: array<u32>;
@group(0) @binding(4) var<storage, read> outline: array<u32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
	let x = f32((i << 1u) & 2u);
	let y = f32(i & 2u);
	return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

fn half_sample(k: u32) -> f32 {
	let word = plane[c.first_word + k / 2u];
	return half_value((word >> (16u * (k % 2u))) & 0xffffu);
}

fn resampled(px: u32, py: u32) -> f32 {
	let x0 = axes[c.x_first + px];
	let xa = axes[c.x_offsets + px];
	let xb = axes[c.x_offsets + px + 1u];
	let y0 = axes[c.y_first + py];
	let ya = axes[c.y_offsets + py];
	let yb = axes[c.y_offsets + py + 1u];
	var v = 0.0;
	for (var k = ya; k < yb; k++) {
		let row = (y0 + k - ya) * c.stored_w + x0;
		var r = 0.0;
		for (var i = xa; i < xb; i++) {
			var s = half_sample(row + i - xa);
			if c.energy != 0u {
				s = s * s;
			}
			let t = bitcast<f32>(axes[c.x_weights + i]) * s;
			r = select(r + t, t, i == xa);
		}
		v += bitcast<f32>(axes[c.y_weights + k]) * r;
	}
	if c.energy != 0u {
		return sqrt_rn(v);
	}
	return v;
}

fn added(i: u32) -> vec3<u32> {
	if c.outline == 0u {
		return vec3<u32>(0u);
	}
	let o = outline[i];
	return vec3<u32>(o & 0xffu, (o >> 8u) & 0xffu, (o >> 16u) & 0xffu);
}

fn dither(level: u32, threshold: u32) -> u32 {
	return (level >> 8u) + select(0u, 1u, (level & 0xffu) * 2u >= threshold);
}

// A pixel of a half plane's picture, 0..255 per channel.
fn half_pixel(px: u32, py: u32) -> vec3<u32> {
	let bits = half_bits(resampled(px, py));
	var rgb: vec3<u32>;
	if c.mode == PLAIN {
		let e = lut[bits];
		rgb = vec3<u32>(e & 0xffu, (e >> 8u) & 0xffu, (e >> 16u) & 0xffu);
	} else {
		let lo = lut[2u * bits];
		let hi = lut[2u * bits + 1u];
		let m = c.side - 1u;
		let t = lut[c.thresholds + (py & m) * c.side + (px & m)];
		rgb = vec3<u32>(dither(hi & 0xffffu, t), dither(lo >> 16u, t), dither(lo & 0xffffu, t));
	}
	return min(rgb + added(py * c.grid_w + px), vec3<u32>(255u));
}

// A stored sample of an index plane through the palette, plus the outline.
fn index_texel(x: i32, y: i32) -> vec3<f32> {
	let sx = u32(clamp(x, 0, i32(c.stored_w) - 1));
	let sy = u32(clamp(y, 0, i32(c.stored_h) - 1));
	let k = sy * c.stored_w + sx;
	let index = (plane[c.first_word + k / 4u] >> (8u * (k % 4u))) & 0xffu;
	let e = lut[min(index, 254u)];
	let rgb = vec3<u32>(e & 0xffu, (e >> 8u) & 0xffu, (e >> 16u) & 0xffu);
	return vec3<f32>(min(rgb + added(k), vec3<u32>(255u)));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
	return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

@fragment
fn fs(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
	let uv = (at.xy - c.origin) / c.size;
	var rgb: vec3<f32>;
	if c.mode == PLAIN || c.mode == DITHERED {
		let grid = vec2<f32>(f32(c.grid_w), f32(c.grid_h));
		let p = vec2<u32>(clamp(floor(uv * grid), vec2<f32>(0.0), grid - 1.0));
		rgb = vec3<f32>(half_pixel(p.x, p.y)) / 255.0;
	} else {
		let t = uv * vec2<f32>(f32(c.stored_w), f32(c.stored_h)) - 0.5;
		let f = floor(t);
		let w = t - f;
		let i = vec2<i32>(f);
		let top = mix(index_texel(i.x, i.y), index_texel(i.x + 1, i.y), w.x);
		let bottom = mix(index_texel(i.x, i.y + 1), index_texel(i.x + 1, i.y + 1), w.x);
		rgb = mix(top, bottom, w.y) / 255.0;
	}
	if c.srgb != 0u {
		rgb = to_linear(rgb);
	}
	return vec4<f32>(rgb, 1.0);
}
