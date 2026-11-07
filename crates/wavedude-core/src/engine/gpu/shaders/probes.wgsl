// Probe gather: one thread per cell; bit 31 of a cell selects the blended intensity.

@group(0) @binding(1) var<storage, read> u: array<f32>;
@group(0) @binding(2) var<storage, read> s1: array<f32>;
@group(0) @binding(3) var<storage, read> s2: array<f32>;
@group(0) @binding(4) var<storage, read> cells: array<u32>;
@group(0) @binding(5) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn probes(@builtin(global_invocation_id) id: vec3<u32>) {
	if id.x >= p.probe_count {
		return;
	}
	let c = cells[id.x];
	let i = c & 0x7fffffffu;
	if (c >> 31u) == 0u {
		out[id.x] = u[i];
	} else {
		out[id.x] = (s1[i] * 0.5 + s2[i] * 0.5) * p.intensity_scale;
	}
}
