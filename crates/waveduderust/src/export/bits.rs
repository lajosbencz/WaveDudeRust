//! A most-significant-bit-first bit writer.

#[derive(Clone, Debug, Default)]
pub(crate) struct BitWriter {
	bytes: Vec<u8>,
	acc: u64,
	count: u32,
}

impl BitWriter {
	pub(crate) fn with_capacity(bytes: usize) -> Self {
		Self { bytes: Vec::with_capacity(bytes), acc: 0, count: 0 }
	}

	/// Appends the low `bits` (0..=32) of `value`.
	pub(crate) fn write(&mut self, value: u32, bits: u32) {
		debug_assert!(bits <= 32);
		if bits == 0 {
			return;
		}
		self.acc = (self.acc << bits) | (value as u64 & ((1u64 << bits) - 1));
		self.count += bits;
		while self.count >= 8 {
			self.count -= 8;
			self.bytes.push((self.acc >> self.count) as u8);
		}
	}

	pub(crate) fn write_bytes(&mut self, bytes: &[u8]) {
		if self.count == 0 {
			self.bytes.extend_from_slice(bytes);
		} else {
			for &b in bytes {
				self.write(b as u32, 8);
			}
		}
	}

	/// Pads with zero bits to the next byte boundary.
	pub(crate) fn align_zero(&mut self) {
		if self.count > 0 {
			self.write(0, 8 - self.count);
		}
	}

	pub(crate) fn clear(&mut self) {
		self.bytes.clear();
		self.acc = 0;
		self.count = 0;
	}

	/// The whole bytes written; call after [`BitWriter::align_zero`].
	pub(crate) fn bytes(&self) -> &[u8] {
		debug_assert_eq!(self.count, 0, "aligned");
		&self.bytes
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn msb_first() {
		let mut b = BitWriter::default();
		b.write(0b101, 3);
		b.write(0x1F, 5);
		b.write(1, 1);
		b.align_zero();
		b.write_bytes(&[0xAB]);
		assert_eq!(b.bytes(), &[0b1011_1111, 0x80, 0xAB]);
	}
}
