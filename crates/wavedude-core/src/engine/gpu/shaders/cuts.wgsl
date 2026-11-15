// The cut byte of padded cell i (engine/pml.rs, cuts), four to a word; 0 when the scene has no mirror.
@group(0) @binding(8) var<storage, read> cuts: array<u32>;

override CUTS: bool = false;

fn cut_at(i: u32) -> u32 {
	if !CUTS {
		return 0u;
	}
	return (cuts[i >> 2u] >> ((i & 3u) * 8u)) & 0xffu;
}

// t with the taps beyond a mirror dropped. bits: the cell's cut nibble for the axis (mirror at -1, -2, +1, +2).
fn cut_taps(t: array<f32, 7>, bits: u32) -> array<f32, 7> {
	let m1 = (bits & 1u) != 0u;
	let m2 = (bits & 2u) != 0u;
	let p1 = (bits & 4u) != 0u;
	let p2 = (bits & 8u) != 0u;
	return array<f32, 7>(
		select(t[0], 0.0, m1 || m2),
		select(t[1], 0.0, m1),
		t[2],
		t[3],
		t[4],
		select(t[5], 0.0, p1),
		select(t[6], 0.0, p1 || p2),
	);
}

fn second(t: array<f32, 7>) -> f32 {
	let c = t[3];
	return AX1 * ((t[2] - c) + (t[4] - c)) + AX2 * ((t[1] - c) + (t[5] - c)) + AX3 * ((t[0] - c) + (t[6] - c));
}

fn first(t: array<f32, 7>) -> f32 {
	return (8.0 * (t[4] - t[2]) - (t[5] - t[1])) / 12.0;
}
