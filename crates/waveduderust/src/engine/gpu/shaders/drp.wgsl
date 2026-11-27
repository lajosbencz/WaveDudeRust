// The DRP Laplacian of buffer SRC at padded cell i (instantiated per buffer, with its taps), as weighted
// sums of differences from the centre (engine/cpu/rows.rs, `drp`). cut: the cell's cut byte.

fn drp_SRC(i: u32, cut: u32) -> f32 {
	let s = p.stride;
	let c = SRC[i];
	if cut == 0u {
		let near = ((SRC[i - 1u] - c) + (SRC[i + 1u] - c)) + ((SRC[i - s] - c) + (SRC[i + s] - c));
		let mid = ((SRC[i - 2u] - c) + (SRC[i + 2u] - c)) + ((SRC[i - 2u * s] - c) + (SRC[i + 2u * s] - c));
		let far = ((SRC[i - 3u] - c) + (SRC[i + 3u] - c)) + ((SRC[i - 3u * s] - c) + (SRC[i + 3u * s] - c));
		return AX1 * near + AX2 * mid + AX3 * far;
	}
	let x = taps_SRC(i, 1u, cut & 0xfu);
	let y = taps_SRC(i, s, cut >> 4u);
	let near = ((x[2] - c) + (x[4] - c)) + ((y[2] - c) + (y[4] - c));
	let mid = ((x[1] - c) + (x[5] - c)) + ((y[1] - c) + (y[5] - c));
	let far = ((x[0] - c) + (x[6] - c)) + ((y[0] - c) + (y[6] - c));
	return AX1 * near + AX2 * mid + AX3 * far;
}
