// Sources as forcing (engine/forcing.rs, engine/cpu.rs `force_w` and `force_next`): one thread per
// driven cell adds its live values in order. groups: [(cell, start, len, h bits) * group_count,
// entries...]; an entry indexes the sub-step's values (0..255 the image level table, 256 + k object
// emitter k); a value that is zero or has all bits set is not live. `force_w` adds F to the modified
// equation's w between its passes; `force` and `force_intensity` add h F to leapfrog's next field,
// the latter moving the intensity stages to the forced value.

@group(0) @binding(1) var<storage, read> groups: array<u32>;
@group(0) @binding(2) var<storage, read> values: array<f32>;
@group(0) @binding(3) var<storage, read_write> target_buf: array<f32>;
@group(0) @binding(4) var<storage, read_write> s1: array<f32>;
@group(0) @binding(5) var<storage, read_write> s2: array<f32>;

fn live(s: f32) -> bool {
	return bitcast<u32>(s) != 0xffffffffu && s != 0.0;
}

@compute @workgroup_size(64)
fn force_w(@builtin(global_invocation_id) id: vec3<u32>) {
	if id.x >= p.group_count {
		return;
	}
	let g = 4u * id.x;
	let cell = groups[g];
	let start = 4u * p.group_count + groups[g + 1u];
	var v = target_buf[cell];
	for (var j = 0u; j < groups[g + 2u]; j++) {
		let s = values[groups[start + j]];
		if live(s) {
			v = v + s;
		}
	}
	target_buf[cell] = v;
}

fn force_next(id: vec3<u32>, intensity: bool) {
	if id.x >= p.group_count {
		return;
	}
	let g = 4u * id.x;
	let cell = groups[g];
	let start = 4u * p.group_count + groups[g + 1u];
	let h = bitcast<f32>(groups[g + 3u]);
	let before = target_buf[cell];
	var v = before;
	var driven = false;
	for (var j = 0u; j < groups[g + 2u]; j++) {
		let s = values[groups[start + j]];
		if live(s) {
			v = v + h * s;
			driven = true;
		}
	}
	if !driven {
		return;
	}
	target_buf[cell] = v;
	if intensity {
		let a = s1[cell] + (v * v - before * before) * p.rest;
		s2[cell] = s2[cell] + (a - s1[cell]) * p.rest;
		s1[cell] = a;
	}
}

@compute @workgroup_size(64)
fn force(@builtin(global_invocation_id) id: vec3<u32>) {
	force_next(id, false);
}

@compute @workgroup_size(64)
fn force_intensity(@builtin(global_invocation_id) id: vec3<u32>) {
	force_next(id, true);
}
