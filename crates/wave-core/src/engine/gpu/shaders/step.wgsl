// The grid passes of a sub-step (engine/cpu.rs): `step` / `step_intensity` the 9-point leapfrog
// (next = c + ((c - prev) d + c2 L u h), written over prev); `inner` the modified equation's
// w = c2 L u and `outer` / `outer_intensity` its next = c + ((c - prev) d + (w + c2 L w / 12) h) with
// the DRP stencil; h = (1 + d) / 2 centres the damping (engine/cpu.rs, `centred`).
// The intensity entry points also advance the two EMA stages.

@group(0) @binding(1) var<storage, read> cur: array<f32>;
@group(0) @binding(2) var<storage, read_write> nxt: array<f32>;
@group(0) @binding(3) var<storage, read_write> s1: array<f32>;
@group(0) @binding(4) var<storage, read_write> s2: array<f32>;
@group(0) @binding(6) var<storage, read> tables: array<f32>;
@group(0) @binding(7) var<storage, read_write> wbuf: array<f32>;

// Each stencil as weighted sums of differences from the centre (engine/cpu.rs). The 9-point drops a
// diagonal tap passing an axis neighbour that is a mirror (engine/pml.rs, diagonal_cuts).
fn nine_point(i: u32, cut: u32) -> f32 {
	let s = p.stride;
	let c = cur[i];
	let w = (cut & 1u) != 0u;
	let e = (cut & 4u) != 0u;
	let n = (cut & 0x10u) != 0u;
	let so = (cut & 0x40u) != 0u;
	let nw = select(cur[i - s - 1u], 0.0, w || n);
	let ne = select(cur[i - s + 1u], 0.0, e || n);
	let sw = select(cur[i + s - 1u], 0.0, w || so);
	let se = select(cur[i + s + 1u], 0.0, e || so);
	let diagonals = ((nw - c) + (ne - c)) + ((sw - c) + (se - c));
	let axes = ((cur[i - s] - c) + (cur[i + s] - c)) + ((cur[i - 1u] - c) + (cur[i + 1u] - c));
	return K9_DIAGONAL * diagonals + K9_AXIS * axes;
}

fn drp_cur(i: u32, cut: u32) -> f32 {
	let s = p.stride;
	let c = cur[i];
	if cut == 0u {
		let near = ((cur[i - 1u] - c) + (cur[i + 1u] - c)) + ((cur[i - s] - c) + (cur[i + s] - c));
		let mid = ((cur[i - 2u] - c) + (cur[i + 2u] - c)) + ((cur[i - 2u * s] - c) + (cur[i + 2u * s] - c));
		let far = ((cur[i - 3u] - c) + (cur[i + 3u] - c)) + ((cur[i - 3u * s] - c) + (cur[i + 3u * s] - c));
		return AX1 * near + AX2 * mid + AX3 * far;
	}
	let x = taps_cur(i, 1u, cut & 0xfu);
	let y = taps_cur(i, s, cut >> 4u);
	let near = ((x[2] - c) + (x[4] - c)) + ((y[2] - c) + (y[4] - c));
	let mid = ((x[1] - c) + (x[5] - c)) + ((y[1] - c) + (y[5] - c));
	let far = ((x[0] - c) + (x[6] - c)) + ((y[0] - c) + (y[6] - c));
	return AX1 * near + AX2 * mid + AX3 * far;
}

fn drp_w(i: u32, cut: u32) -> f32 {
	let s = p.stride;
	let c = wbuf[i];
	if cut == 0u {
		let near = ((wbuf[i - 1u] - c) + (wbuf[i + 1u] - c)) + ((wbuf[i - s] - c) + (wbuf[i + s] - c));
		let mid = ((wbuf[i - 2u] - c) + (wbuf[i + 2u] - c)) + ((wbuf[i - 2u * s] - c) + (wbuf[i + 2u * s] - c));
		let far = ((wbuf[i - 3u] - c) + (wbuf[i + 3u] - c)) + ((wbuf[i - 3u * s] - c) + (wbuf[i + 3u * s] - c));
		return AX1 * near + AX2 * mid + AX3 * far;
	}
	let x = taps_wbuf(i, 1u, cut & 0xfu);
	let y = taps_wbuf(i, s, cut >> 4u);
	let near = ((x[2] - c) + (x[4] - c)) + ((y[2] - c) + (y[4] - c));
	let mid = ((x[1] - c) + (x[5] - c)) + ((y[1] - c) + (y[5] - c));
	let far = ((x[0] - c) + (x[6] - c)) + ((y[0] - c) + (y[6] - c));
	return AX1 * near + AX2 * mid + AX3 * far;
}

fn centred(d: f32) -> f32 {
	return (1.0 + d) * 0.5;
}

fn accumulate(i: u32, v: f32) {
	let a = s1[i] * p.alpha + v * v * p.rest;
	s1[i] = a;
	s2[i] = s2[i] * p.alpha + a * p.rest;
}

fn leapfrog(id: vec3<u32>, intensity: bool) {
	if id.x >= p.gw || id.y >= p.gh {
		return;
	}
	let i = cell(id.x, id.y);
	let c = cur[i];
	let m = medium_at(i);
	let v = c + ((c - nxt[i]) * m.y + nine_point(i, cut_at(i)) * m.x * centred(m.y));
	nxt[i] = v;
	if intensity {
		accumulate(i, v);
	}
}

fn modified(id: vec3<u32>, intensity: bool) {
	if id.x >= p.gw || id.y >= p.gh {
		return;
	}
	let i = cell(id.x, id.y);
	let c = cur[i];
	let m = medium_at(i);
	let v = c + ((c - nxt[i]) * m.y + (wbuf[i] + m.x * drp_w(i, cut_at(i)) * INV12) * centred(m.y));
	nxt[i] = v;
	if intensity {
		accumulate(i, v);
	}
}

@compute @workgroup_size(32, 8)
fn step(@builtin(global_invocation_id) id: vec3<u32>) {
	leapfrog(id, false);
}

@compute @workgroup_size(32, 8)
fn step_intensity(@builtin(global_invocation_id) id: vec3<u32>) {
	leapfrog(id, true);
}

@compute @workgroup_size(32, 8)
fn inner(@builtin(global_invocation_id) id: vec3<u32>) {
	if id.x >= p.gw || id.y >= p.gh {
		return;
	}
	let i = cell(id.x, id.y);
	wbuf[i] = medium_at(i).x * drp_cur(i, cut_at(i));
}

@compute @workgroup_size(32, 8)
fn outer(@builtin(global_invocation_id) id: vec3<u32>) {
	modified(id, false);
}

@compute @workgroup_size(32, 8)
fn outer_intensity(@builtin(global_invocation_id) id: vec3<u32>) {
	modified(id, true);
}
