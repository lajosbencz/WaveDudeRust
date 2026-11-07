//! Deterministic randomness: PCG32 with streams, the name hash and xorshift32.
//! Kept exactly as in WaveDude Next so composite sources land on the same cells.

/// PCG-XSH-RR 64/32.
#[derive(Clone, Debug)]
pub struct Pcg32 {
	state: u64,
	inc: u64,
}

impl Pcg32 {
	const MULT: u64 = 6364136223846793005;

	pub fn new(seed: u64, stream: u64) -> Self {
		let mut rng = Self { state: 0, inc: (stream << 1) | 1 };
		rng.next_u32();
		rng.state = rng.state.wrapping_add(seed);
		rng.next_u32();
		rng
	}

	pub fn next_u32(&mut self) -> u32 {
		let old = self.state;
		self.state = old.wrapping_mul(Self::MULT).wrapping_add(self.inc);
		let xs = (((old >> 18) ^ old) >> 27) as u32;
		let rot = (old >> 59) as u32;
		xs.rotate_right(rot)
	}

	/// Uniform in [min, max], unbiased.
	pub fn next_range(&mut self, min: i32, max: i32) -> i32 {
		debug_assert!(max >= min);
		let range = (max as i64 - min as i64 + 1) as u64;
		let limit = (1u64 << 32) - ((1u64 << 32) % range);
		loop {
			let v = self.next_u32() as u64;
			if v < limit {
				return (min as i64 + (v % range) as i64) as i32;
			}
		}
	}

	/// Uniform in [0, 1) with 24 bits.
	pub fn next_f64(&mut self) -> f64 {
		(self.next_u32() >> 8) as f64 / (1u32 << 24) as f64
	}
}

/// FNV-1a over the UTF-16 code units of `s`.
pub fn stable_hash(s: &str) -> u32 {
	let mut h: u32 = 2166136261;
	for c in s.encode_utf16() {
		h = (h ^ c as u32).wrapping_mul(16777619);
	}
	h
}

pub fn xorshift32(mut x: u32) -> u32 {
	x ^= x << 13;
	x ^= x >> 17;
	x ^= x << 5;
	x
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn pcg_is_deterministic_per_stream() {
		let a: Vec<u32> =
			(0..4).map(|_| 0).collect::<Vec<_>>().iter().scan(Pcg32::new(42, 54), |r, _| Some(r.next_u32())).collect();
		let b: Vec<u32> =
			(0..4).map(|_| 0).collect::<Vec<_>>().iter().scan(Pcg32::new(42, 54), |r, _| Some(r.next_u32())).collect();
		let c: Vec<u32> =
			(0..4).map(|_| 0).collect::<Vec<_>>().iter().scan(Pcg32::new(42, 55), |r, _| Some(r.next_u32())).collect();
		assert_eq!(a, b);
		assert_ne!(a, c);
	}

	#[test]
	fn pcg_reference_sequence() {
		// pcg32_random_r with seed 42, stream 54 (the PCG paper's demo).
		let mut r = Pcg32::new(42, 54);
		let got: Vec<u32> = (0..6).map(|_| r.next_u32()).collect();
		assert_eq!(got, [0xa15c02b7, 0x7b47f409, 0xba1d3330, 0x83d2f293, 0xbfa4784b, 0xcbed606e]);
	}

	#[test]
	fn range_is_inclusive() {
		let mut r = Pcg32::new(1, 1);
		let mut seen = [false; 11];
		for _ in 0..2000 {
			seen[(r.next_range(-5, 5) + 5) as usize] = true;
		}
		assert!(seen.iter().all(|&s| s));
	}

	#[test]
	fn hash_is_fnv1a_utf16() {
		assert_eq!(stable_hash(""), 2166136261);
		assert_eq!(stable_hash("a"), (2166136261u32 ^ 0x61).wrapping_mul(16777619));
	}
}
