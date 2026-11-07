//! A writer that keeps its own position, so `stream_position` neither flushes nor seeks a `BufWriter`.

use std::io::{Result, Seek, SeekFrom, Write};

pub struct Counted<W> {
	inner: W,
	pos: u64,
}

impl<W: Write + Seek> Counted<W> {
	/// Takes the writer at its current position.
	pub fn new(mut inner: W) -> Result<Self> {
		let pos = inner.stream_position()?;
		Ok(Self { inner, pos })
	}

	pub fn into_inner(self) -> W {
		self.inner
	}
}

impl<W: Write> Write for Counted<W> {
	fn write(&mut self, buf: &[u8]) -> Result<usize> {
		let n = self.inner.write(buf)?;
		self.pos += n as u64;
		Ok(n)
	}

	fn write_all(&mut self, buf: &[u8]) -> Result<()> {
		self.inner.write_all(buf)?;
		self.pos += buf.len() as u64;
		Ok(())
	}

	fn flush(&mut self) -> Result<()> {
		self.inner.flush()
	}
}

impl<W: Seek> Seek for Counted<W> {
	fn seek(&mut self, to: SeekFrom) -> Result<u64> {
		if let SeekFrom::Start(p) = to
			&& p == self.pos
		{
			return Ok(p);
		}
		self.pos = self.inner.seek(to)?;
		Ok(self.pos)
	}

	fn stream_position(&mut self) -> Result<u64> {
		Ok(self.pos)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Cursor;

	#[test]
	fn position_follows_writes_and_seeks() {
		let mut c = Counted::new(Cursor::new(Vec::new())).unwrap();
		c.write_all(b"abcd").unwrap();
		assert_eq!(c.stream_position().unwrap(), 4);
		c.seek(SeekFrom::Start(1)).unwrap();
		c.write_all(b"X").unwrap();
		assert_eq!(c.stream_position().unwrap(), 2);
		assert_eq!(c.seek(SeekFrom::End(0)).unwrap(), 4);
		assert_eq!(c.into_inner().into_inner(), b"aXcd");
	}
}
