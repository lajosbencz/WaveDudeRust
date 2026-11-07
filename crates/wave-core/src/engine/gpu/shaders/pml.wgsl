// The CPML on the rim (engine/pml.rs, CpuPml::apply), one application of the operator to `src` in
// four kernels: `pml_psi_y` and `pml_delta_y` over the y strips (psi_y, then delta_y = O_y u - D2y u
// with xi_y), `pml_psi_x` over the x strips on z~ = u + CHI (D3y u + delta_y), and `pml_correct`
// over the rim cells, which adds SCALE c2 ((O_x z~ - D2x z~) + CHI D2x delta_y + delta_y) to `out`
// and advances xi_x. psi: x strips, then y strips; xi: xi_x, xi_y, then delta_y. coef: [b, a,
// 1/kappa, (1/kappa)'] per x coordinate, then per y. Threads are numbered over a 2D dispatch of
// 64-wide groups.

@group(0) @binding(1) var<storage, read> src: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;
@group(0) @binding(3) var<storage, read_write> psi: array<f32>;
@group(0) @binding(4) var<storage, read_write> xi: array<f32>;
@group(0) @binding(6) var<storage, read> tables: array<f32>;
@group(0) @binding(7) var<storage, read> coef: array<vec4<f32>>;

override SCALE: f32 = 1.0;

fn thread(id: vec3<u32>, groups: vec3<u32>) -> u32 {
	return id.y * groups.x * 64u + id.x;
}

fn x_index(gx: i32, gy: u32) -> i32 {
	let r = i32(p.rim);
	let w = i32(p.gw);
	if gx < 0 || gx >= w {
		return -1;
	}
	if gx < r {
		return i32(gy) * 2 * r + gx;
	}
	if gx >= w - r {
		return i32(gy) * 2 * r + gx + 2 * r - w;
	}
	return -1;
}

fn y_index(gx: i32, gy: i32) -> i32 {
	let r = i32(p.rim);
	let h = i32(p.gh);
	if gx < 0 || gx >= i32(p.gw) || gy < 0 || gy >= h {
		return -1;
	}
	if gy < r {
		return i32(p.pml_x_len) + gy * i32(p.gw) + gx;
	}
	if gy >= h - r {
		return i32(p.pml_x_len) + (gy + 2 * r - h) * i32(p.gw) + gx;
	}
	return -1;
}

fn psi_at(k: i32) -> f32 {
	if k < 0 {
		return 0.0;
	}
	return psi[u32(k)];
}

// delta_y at grid cell (gx, gy), 0 outside the y strips.
fn delta_at(gx: i32, gy: u32) -> f32 {
	let k = y_index(gx, i32(gy));
	if k < 0 {
		return 0.0;
	}
	return xi[u32(k) + p.pml_len - p.pml_x_len];
}

// The y-strip cell of thread t (t < y_len).
fn y_cell(t: u32) -> vec2<u32> {
	let row = t / p.gw;
	return vec2<u32>(t % p.gw, select(p.gh - 2u * p.rim + row, row, row < p.rim));
}

// z~ at padded cell j, grid column gx, row gy.
fn z_at(j: u32, gx: i32, gy: u32) -> f32 {
	let v = src[j];
	if CHI == 0.0 {
		return v;
	}
	return v + CHI * ((src[j - p.stride] + src[j + p.stride] - 2.0 * v) + delta_at(gx, gy));
}

// The taps of z~ along x around padded cell i, the taps beyond a mirror dropped.
fn z_taps(i: u32, gx: u32, gy: u32, bits: u32) -> array<f32, 7> {
	let m1 = (bits & 1u) != 0u;
	let m2 = (bits & 2u) != 0u;
	let p1 = (bits & 4u) != 0u;
	let p2 = (bits & 8u) != 0u;
	let x = i32(gx);
	return array<f32, 7>(
		select(z_at(i - 3u, x - 3, gy), 0.0, m1 || m2),
		select(z_at(i - 2u, x - 2, gy), 0.0, m1),
		z_at(i - 1u, x - 1, gy),
		z_at(i, x, gy),
		z_at(i + 1u, x + 1, gy),
		select(z_at(i + 2u, x + 2, gy), 0.0, p1),
		select(z_at(i + 3u, x + 3, gy), 0.0, p1 || p2),
	);
}

fn stretched(tp: array<f32, 7>, c: vec4<f32>, dpsi: f32, k: u32) -> f32 {
	let d2 = second(tp);
	let q = c.z * d2 + c.w * first(tp) + dpsi;
	let x = c.x * xi[k] + c.y * q;
	xi[k] = x;
	return c.z * q + x - d2;
}

@compute @workgroup_size(64)
fn pml_psi_y(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
	let t = thread(id, groups);
	if t >= p.pml_len - p.pml_x_len {
		return;
	}
	let g = y_cell(t);
	let c = coef[p.gw + g.y];
	let i = cell(g.x, g.y);
	let k = p.pml_x_len + t;
	psi[k] = c.x * psi[k] + c.y * first(taps_src(i, p.stride, cut_at(i) >> 4u));
}

@compute @workgroup_size(64)
fn pml_delta_y(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
	let t = thread(id, groups);
	let y_len = p.pml_len - p.pml_x_len;
	if t >= y_len {
		return;
	}
	let g = y_cell(t);
	let i = cell(g.x, g.y);
	let x = i32(g.x);
	let y = i32(g.y);
	let dpsi = (8.0 * (psi_at(y_index(x, y + 1)) - psi_at(y_index(x, y - 1)))
		- (psi_at(y_index(x, y + 2)) - psi_at(y_index(x, y - 2)))) / 12.0;
	let k = p.pml_x_len + t;
	xi[k + y_len] = stretched(taps_src(i, p.stride, cut_at(i) >> 4u), coef[p.gw + g.y], dpsi, k);
}

@compute @workgroup_size(64)
fn pml_psi_x(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
	let t = thread(id, groups);
	if t >= p.pml_x_len {
		return;
	}
	let r2 = 2u * p.rim;
	let gy = t / r2;
	let col = t % r2;
	let gx = select(p.gw - r2 + col, col, col < p.rim);
	let c = coef[gx];
	let i = cell(gx, gy);
	psi[t] = c.x * psi[t] + c.y * first(z_taps(i, gx, gy, cut_at(i) & 0xfu));
}

@compute @workgroup_size(64)
fn pml_correct(@builtin(global_invocation_id) id: vec3<u32>, @builtin(num_workgroups) groups: vec3<u32>) {
	let t = thread(id, groups);
	if t >= p.rim_cells {
		return;
	}
	let r = p.rim;
	let top = 2u * r * p.gw;
	var gx: u32;
	var gy: u32;
	if t < top {
		let row = t / p.gw;
		gx = t % p.gw;
		gy = select(p.gh - 2u * r + row, row, row < r);
	} else {
		let k = t - top;
		let col = k % (2u * r);
		gy = r + k / (2u * r);
		gx = select(p.gw - 2u * r + col, col, col < r);
	}
	let i = cell(gx, gy);
	let x = i32(gx);
	var sum = delta_at(x, gy);
	if CHI != 0.0 {
		sum += CHI * (delta_at(x - 1, gy) + delta_at(x + 1, gy) - 2.0 * delta_at(x, gy));
	}
	let kx = x_index(x, gy);
	if kx >= 0 {
		let dpsi = (8.0 * (psi_at(x_index(x + 1, gy)) - psi_at(x_index(x - 1, gy)))
			- (psi_at(x_index(x + 2, gy)) - psi_at(x_index(x - 2, gy)))) / 12.0;
		sum += stretched(z_taps(i, gx, gy, cut_at(i) & 0xfu), coef[gx], dpsi, u32(kx));
	}
	out[i] = out[i] + SCALE * medium_at(i).x * sum;
}
