// Stencil taps of buffer SRC (instantiated per buffer): the seven values i + k step, k = -3..3, the
// taps beyond a mirror dropped. bits: the cell's cut nibble for the axis (mirror at -1, -2, +1, +2).

fn taps_SRC(i: u32, step: u32, bits: u32) -> array<f32, 7> {
	let m1 = (bits & 1u) != 0u;
	let m2 = (bits & 2u) != 0u;
	let p1 = (bits & 4u) != 0u;
	let p2 = (bits & 8u) != 0u;
	return array<f32, 7>(
		select(SRC[i - 3u * step], 0.0, m1 || m2),
		select(SRC[i - 2u * step], 0.0, m1),
		SRC[i - step],
		SRC[i],
		SRC[i + step],
		select(SRC[i + 2u * step], 0.0, p1),
		select(SRC[i + 3u * step], 0.0, p1 || p2),
	);
}
