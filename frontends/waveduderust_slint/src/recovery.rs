//! Restarting after a lost GPU device, with the unsaved edits carried over in a recovery file.

use std::path::PathBuf;

use crate::document::Document;
use crate::state::State;

/// When a start after a lost GPU device began (Unix seconds): a loss soon after it ends the app instead.
const RESTARTED: &str = "WAVEDUDE_RESTARTED";
/// The file holding the unsaved edits of the project a lost GPU device closed.
const RECOVER: &str = "WAVEDUDE_RECOVER";

fn unix_now() -> u64 {
	std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The window died with its GPU device (`why`): a running export stops, unsaved edits go to a recovery file and the
/// app starts again on the project, on the CPU when the GPU stays gone.
pub(crate) fn restart_after_loss(
	state: &mut State,
	why: &dyn std::fmt::Display,
) -> Result<(), Box<dyn std::error::Error>> {
	tracing::error!("the GPU device was lost: {why}");
	if let Some(e) = state.exporting.as_mut() {
		e.stop();
		tracing::warn!("the export was stopped");
	}
	let recovery = state.doc.dirty().then(|| write_recovery(&state.doc)).transpose()?;
	let looping = std::env::var(RESTARTED)
		.ok()
		.and_then(|at| at.parse::<u64>().ok())
		.is_some_and(|at| unix_now().saturating_sub(at) < 60);
	if looping {
		return Err(format!("the GPU device was lost again right after a restart: {why}").into());
	}
	let mut again = std::process::Command::new(std::env::current_exe()?);
	again.args(state.doc.path.clone()).env(RESTARTED, unix_now().to_string()).env_remove(RECOVER);
	if let Some(file) = recovery {
		again.env(RECOVER, file);
	}
	again.spawn()?;
	Ok(())
}

/// Writes `doc` to the recovery file, whole or not at all; its path.
fn write_recovery(doc: &Document) -> std::io::Result<PathBuf> {
	let dirs = waveduderust_platform::settings::AppDirs::platform()
		.ok_or_else(|| std::io::Error::other("no app folder for the recovery file"))?;
	let file = dirs.config.join("recovered.wdproj");
	let partial = file.with_extension("wdproj.partial");
	std::fs::write(&partial, doc.text())?;
	std::fs::rename(&partial, &file)?;
	tracing::info!("unsaved edits written to {}", file.display());
	Ok(file)
}

/// Takes up the recovery file a lost GPU device left as an unsaved edit of the project opened.
pub(crate) fn recover(state: &mut State) {
	let Some(file) = std::env::var_os(RECOVER).map(PathBuf::from) else { return };
	let recovered = std::fs::read_to_string(&file)
		.map_err(|e| e.to_string())
		.and_then(|text| waveduderust::io::wdproj::read_str(&text).map_err(|e| e.to_string()));
	match recovered {
		Ok((project, view)) => {
			state.doc.edit_both(None, |p, v| (*p, *v) = (project, view));
			let _ = std::fs::remove_file(&file);
			tracing::warn!("WaveDude Rust started again after the GPU device was lost; its unsaved edits are restored");
		}
		Err(e) => tracing::error!("the unsaved edits in {} could not be restored: {e}", file.display()),
	}
}
