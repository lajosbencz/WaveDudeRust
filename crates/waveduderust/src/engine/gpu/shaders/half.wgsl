// f32 to f16 bits (round to nearest even, as the half crate), f16 bits to f32, and the correctly rounded
// square root: shared by the capture kernel and the colouring of planes (render/gpu.rs).

fn half_bits(f: f32) -> u32 {
	let x = bitcast<u32>(f);
	let sign = (x >> 16u) & 0x8000u;
	let a = x & 0x7fffffffu;
	if a >= 0x7f800000u {
		let man = a & 0x7fffffu;
		return sign | 0x7c00u | select(0u, 0x200u, man != 0u) | (man >> 13u);
	}
	if a >= 0x477ff000u {
		return sign | 0x7c00u;
	}
	if a >= 0x38800000u {
		let r = a - 0x38000000u;
		return sign | ((r + 0xfffu + ((r >> 13u) & 1u)) >> 13u);
	}
	if a < 0x33000000u {
		return sign;
	}
	let s = 126u - (a >> 23u);
	let m = (a & 0x7fffffu) | 0x800000u;
	let q = m >> s;
	let rem = m & ((1u << s) - 1u);
	let halfway = 1u << (s - 1u);
	return sign | (q + select(0u, 1u, rem > halfway || (rem == halfway && (q & 1u) == 1u)));
}

fn half_value(h: u32) -> f32 {
	let sign = (h & 0x8000u) << 16u;
	let e = (h >> 10u) & 0x1fu;
	let m = h & 0x3ffu;
	if e == 0u {
		return bitcast<f32>(sign | bitcast<u32>(f32(m) * 5.9604645e-8f));
	}
	if e == 31u {
		return bitcast<f32>(sign | 0x7f800000u | (m << 13u));
	}
	return bitcast<f32>(sign | ((e + 112u) << 23u) | (m << 13u));
}

// (hi, lo) of x * x for x < 2^25.
fn square64(x: u32) -> vec2<u32> {
	let xh = x >> 16u;
	let xl = x & 0xffffu;
	let lo = xl * xl;
	let mid = 2u * xh * xl;
	let low = lo + (mid << 16u);
	return vec2<u32>(xh * xh + (mid >> 16u) + select(0u, 1u, low < lo), low);
}

// Whether m (normal, positive) lies above the square of the midpoint between f32 r (normal) and
// the next f32 up, compared exactly in integers: (2R + 1)^2 2^(2e - 48) against M 2^(f - 23).
fn above_mid(m: f32, r: f32) -> bool {
	let rb = bitcast<u32>(r);
	let mb = bitcast<u32>(m);
	let x = 2u * ((rb & 0x7fffffu) | 0x800000u) + 1u;
	let k = i32(mb >> 23u) - 2 * i32(rb >> 23u) + 152;
	let mm = (mb & 0x7fffffu) | 0x800000u;
	let a = square64(x);
	if k <= 0 || k >= 32 {
		return k >= 32;
	}
	let b = vec2<u32>(mm >> (32u - u32(k)), mm << u32(k));
	return b.x > a.x || (b.x == a.x && b.y > a.y);
}

// The correctly rounded square root (the CPU's), from the card's within a few ulps.
fn sqrt_rn(m: f32) -> f32 {
	var r = sqrt(m);
	let mb = bitcast<u32>(m);
	if mb < 0x00800000u || mb >= 0x7f800000u {
		return r;
	}
	for (var i = 0; i < 4; i++) {
		if above_mid(m, r) {
			r = bitcast<f32>(bitcast<u32>(r) + 1u);
		} else if !above_mid(m, bitcast<f32>(bitcast<u32>(r) - 1u)) {
			r = bitcast<f32>(bitcast<u32>(r) - 1u);
		} else {
			break;
		}
	}
	return r;
}
