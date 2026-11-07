// The cut byte of padded cell i (engine/pml.rs, cuts), four to a word; 0 when the scene has no mirror.
@group(0) @binding(8) var<storage, read> cuts: array<u32>;

override CUTS: bool = false;

fn cut_at(i: u32) -> u32 {
	if !CUTS {
		return 0u;
	}
	return (cuts[i >> 2u] >> ((i & 3u) * 8u)) & 0xffu;
}

fn second(t: array<f32, 7>) -> f32 {
	let c = t[3];
	return AX1 * ((t[2] - c) + (t[4] - c)) + AX2 * ((t[1] - c) + (t[5] - c)) + AX3 * ((t[0] - c) + (t[6] - c));
}

fn first(t: array<f32, 7>) -> f32 {
	return (8.0 * (t[4] - t[2]) - (t[5] - t[1])) / 12.0;
}
