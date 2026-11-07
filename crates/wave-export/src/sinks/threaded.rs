//! A sink run on a thread of its own, so its encoding overlaps the job's rendering of the next frame.

use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::JoinHandle;

use super::{FrameSink, check_size};
use crate::{ExportError, Result};

/// Frames in flight: one being written, one being filled.
const BUFFERS: usize = 2;

/// Runs `S` on its own thread, fed through two recycled frame buffers. Takes rendered frames only.
pub struct Threaded<S: FrameSink + 'static> {
	width: u32,
	height: u32,
	tx: Option<SyncSender<Vec<u8>>>,
	/// Buffers the thread is done with, or the sink's error (after which the thread ends).
	back: Receiver<Result<Vec<u8>>>,
	allocated: usize,
	submitted: usize,
	thread: Option<JoinHandle<S>>,
	sink: Option<S>,
}

impl<S: FrameSink + 'static> Threaded<S> {
	/// Starts the thread for `sink`, which takes `width x height` frames.
	pub fn new(mut sink: S, width: u32, height: u32) -> Result<Self> {
		let (tx, rx) = sync_channel::<Vec<u8>>(BUFFERS);
		let (back_tx, back) = sync_channel(BUFFERS);
		let thread = std::thread::Builder::new().name("export sink".into()).spawn(move || {
			for frame in rx {
				let written = sink.write_frame(&frame, width, height);
				let failed = written.is_err();
				// The receiver is gone only when the sink was dropped unfinished.
				let _ = back_tx.send(written.map(|()| frame));
				if failed {
					break;
				}
			}
			sink
		})?;
		Ok(Self { width, height, tx: Some(tx), back, allocated: 0, submitted: 0, thread: Some(thread), sink: None })
	}

	/// Ends the thread and takes the sink back, with the first error the thread reported.
	fn join(&mut self) -> Result<()> {
		self.tx.take();
		let mut first = Ok(());
		while let Ok(r) = self.back.recv() {
			if let (Err(e), Ok(())) = (r, &first) {
				first = Err(e);
			}
		}
		if let Some(thread) = self.thread.take() {
			let sink =
				thread.join().map_err(|_| ExportError::Io(std::io::Error::other("the sink's thread panicked")))?;
			self.sink = Some(sink);
		}
		first
	}
}

impl<S: FrameSink + 'static> FrameSink for Threaded<S> {
	fn write_frame(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<()> {
		check_size(self.width, self.height, width, height, rgba.len(), 4)?;
		let tx = self.tx.as_ref().ok_or(ExportError::Finished)?;
		let mut frame = if self.allocated < BUFFERS {
			self.allocated += 1;
			Vec::with_capacity(rgba.len())
		} else {
			self.back.recv().map_err(|_| ExportError::Finished)??
		};
		frame.clear();
		frame.extend_from_slice(rgba);
		tx.send(frame).map_err(|_| ExportError::Finished)?;
		self.submitted += 1;
		Ok(())
	}

	fn frames_written(&self) -> usize {
		self.sink.as_ref().map_or(self.submitted, FrameSink::frames_written)
	}

	fn finish(&mut self) -> Result<()> {
		let joined = self.join();
		let finished = self.sink.as_mut().map_or(Ok(()), FrameSink::finish);
		joined.and(finished)
	}

	fn files(&self) -> Vec<std::path::PathBuf> {
		self.sink.as_ref().map_or_else(Vec::new, FrameSink::files)
	}

	fn discard(&mut self) -> Result<()> {
		self.sink.as_mut().map_or(Ok(()), FrameSink::discard)
	}
}
