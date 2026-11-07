// Mur's first-order condition on the first padding ring, after a sub-step, the second and third
// continuing the field linearly (engine/cpu.rs, `apply_open_edges` and `mur_weights`): one thread
// per chain (a grid cell on the edge and the padding cells outward of it).

struct Mur {
	inner: u32,
	r1: u32,
	r2: u32,
	r3: u32,
	a: f32,
	b: f32,
	c: f32,
}

@group(0) @binding(1) var<storage, read> cur: array<f32>;
@group(0) @binding(2) var<storage, read_write> nxt: array<f32>;
@group(0) @binding(3) var<storage, read> mur: array<Mur>;

@compute @workgroup_size(64)
fn edges(@builtin(global_invocation_id) id: vec3<u32>) {
	if id.x >= p.mur_count {
		return;
	}
	let m = mur[id.x];
	let edge = nxt[m.inner];
	let first = m.a * cur[m.inner] + m.b * edge + m.c * cur[m.r1];
	nxt[m.r1] = first;
	nxt[m.r2] = 2.0 * first - edge;
	nxt[m.r3] = 3.0 * first - 2.0 * edge;
}
