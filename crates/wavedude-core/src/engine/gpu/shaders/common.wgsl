// Prepended to every kernel after the generated constants (K9_*, AX*, INV12).
// Buffers are the padded grid: stride = grid width + 2 PAD, grid cell (gx, gy) at
// (gy + PAD) * stride + gx + PAD.

const PAD: u32 = 3u;

struct Params {
	stride: u32,
	gw: u32,
	gh: u32,
	rim: u32,
	canvas_w: u32,
	canvas_h: u32,
	mur_count: u32,
	probe_count: u32,
	alpha: f32,
	rest: f32,
	intensity_scale: f32,
	d_base: u32,
	group_count: u32,
	pml_x_len: u32,
	pml_len: u32,
	rim_cells: u32,
	_pad0: u32,
	_pad1: u32,
	_pad2: u32,
	_pad3: u32,
}

@group(0) @binding(0) var<uniform> p: Params;

fn cell(gx: u32, gy: u32) -> u32 {
	return (gy + PAD) * p.stride + gx + PAD;
}

// The displayed intensity from the two EMA stages (engine/shared.rs, `blend`).
fn blend(a: f32, b: f32) -> f32 {
	return (a * 0.5 + b * 0.5) * p.intensity_scale;
}
