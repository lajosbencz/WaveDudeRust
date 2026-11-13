// Canvas to stored planes (engine/capture.rs): block means, then f16 (bit-exact round to nearest
// even) or palette indices. One thread per output word (2 halves or 4 indices); z is the layer
// (0 field, 1 intensity), layer z starting at word z * layer_words.

struct Capture {
	out_w: u32,
	divisor: u32,
	samples: u32,
	layer_words: u32,
	format: u32,
	root: u32,
	field_b: f32,
	intensity_b: f32,
}

@group(0) @binding(1) var<uniform> c: Capture;
@group(0) @binding(2) var<storage, read> u: array<f32>;
@group(0) @binding(3) var<storage, read> s1: array<f32>;
@group(0) @binding(4) var<storage, read> s2: array<f32>;
@group(0) @binding(5) var<storage, read_write> out: array<u32>;

fn sample(layer: u32, x: u32, y: u32) -> f32 {
	let i = cell(x + p.rim, y + p.rim);
	if layer == 0u {
		return u[i];
	}
	return (s1[i] * 0.5 + s2[i] * 0.5) * p.intensity_scale;
}

fn mean(layer: u32, k: u32) -> f32 {
	let bx = k % c.out_w;
	let by = k / c.out_w;
	let d = c.divisor;
	if d == 1u {
		return sample(layer, bx, by);
	}
	let x0 = bx * d;
	let y0 = by * d;
	let x1 = min(x0 + d, p.canvas_w);
	let y1 = min(y0 + d, p.canvas_h);
	var sum = 0.0f;
	for (var y = y0; y < y1; y++) {
		for (var x = x0; x < x1; x++) {
			sum += sample(layer, x, y);
		}
	}
	return sum / f32((y1 - y0) * (x1 - x0));
}

// The palette indices follow field_index, intensity_root_index and intensity_index in engine/capture.rs,
// the source of truth; tests/gpu.rs compares this kernel's bytes with them.
fn convert(layer: u32, m: f32) -> u32 {
	if c.format == 0u {
		if layer == 1u && c.root != 0u {
			return half_bits(sqrt_rn(m));
		}
		return half_bits(m);
	}
	if layer == 0u {
		let h = half_bits(m);
		if (h & 0x7fffu) > 0x7c00u {
			return 127u;
		}
		return u32(clamp(half_value(h) * c.field_b, -1.0, 1.0) * 127.0 + 127.0);
	}
	if c.root != 0u {
		let h = half_bits(sqrt_rn(m));
		if h == 0u || h > 0x7c00u {
			return 0u;
		}
		return u32(min(half_value(h) * c.intensity_b, 1.0) * 254.0);
	}
	if m > 0.0 {
		return u32(min(sqrt_rn(m) * c.intensity_b, 1.0) * 254.0);
	}
	return 0u;
}

@compute @workgroup_size(256)
fn capture(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
	let w = id.x + id.y * groups.x * 256u;
	let per = select(4u, 2u, c.format == 0u);
	let first = w * per;
	if first >= c.samples {
		return;
	}
	let layer = id.z;
	let bits = 32u / per;
	var word = 0u;
	for (var j = 0u; j < per; j++) {
		let k = first + j;
		if k < c.samples {
			word |= convert(layer, mean(layer, k)) << (j * bits);
		}
	}
	out[layer * c.layer_words + w] = word;
}
