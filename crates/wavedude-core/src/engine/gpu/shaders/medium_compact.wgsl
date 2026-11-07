// Per cell: c2 table index in the low 16 bits, damping table index (from p.d_base) in the high 16.
@group(0) @binding(5) var<storage, read> medium: array<u32>;

fn medium_at(i: u32) -> vec2<f32> {
	let m = medium[i];
	return vec2<f32>(tables[m & 0xffffu], tables[p.d_base + (m >> 16u)]);
}
