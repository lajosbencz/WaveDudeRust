//! The crash report (docs/ui/70, Crash report): a panic of the main thread appended to `crash.txt` in the log
//! folder, shown once at the next start as `last-crash.txt`.

use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Appends a main-thread panic to `crash.txt` in `dir`, after the default report on stderr.
pub(crate) fn install(dir: PathBuf) {
	let default = std::panic::take_hook();
	std::panic::set_hook(Box::new(move |info| {
		default(info);
		let backtrace = std::backtrace::Backtrace::force_capture();
		tracing::error!("{info}\n{backtrace}");
		if std::thread::current().name() != Some("main") {
			return;
		}
		let file = std::fs::create_dir_all(&dir)
			.and_then(|()| std::fs::OpenOptions::new().create(true).append(true).open(dir.join("crash.txt")));
		if let Ok(mut file) = file {
			let _ = writeln!(file, "WaveDude Rust {}\n{info}\n{backtrace}\n", env!("CARGO_PKG_VERSION"));
		}
	}));
}

/// The last session's report, renamed `last-crash.txt` so it shows once; `None` without one.
pub(crate) fn take(dir: &Path) -> Option<PathBuf> {
	let last = dir.join("last-crash.txt");
	std::fs::rename(dir.join("crash.txt"), &last).ok().map(|()| last)
}
