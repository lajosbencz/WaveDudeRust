// Stencil taps of buffer SRC (instantiated per buffer): the seven values i + k step, k = -3..3, the
// taps beyond a mirror dropped (cuts.wgsl, cut_taps).

fn taps_SRC(i: u32, step: u32, bits: u32) -> array<f32, 7> {
	return cut_taps(array<f32, 7>(
		SRC[i - 3u * step],
		SRC[i - 2u * step],
		SRC[i - step],
		SRC[i],
		SRC[i + step],
		SRC[i + 2u * step],
		SRC[i + 3u * step],
	), bits);
}
