//! The app's side of Cisco's `OpenH264` (decision 0005): the user's choice is the `UseOpenH264` setting, the
//! binary a file in the app's codec folder (`AppDirs::codec`), both shared with the CLI. Threads: the UI thread.

use wavedude_export::h264;

use crate::App;

/// MP4 export may run: compiled in, or enabled by the user.
pub fn enabled(choice: Option<bool>) -> bool {
	h264::BUILT_IN || choice == Some(true)
}

/// The startup page asks for the user's choice.
pub fn unasked(choice: Option<bool>) -> bool {
	!h264::BUILT_IN && choice.is_none()
}

/// Cisco's binary is in the app's codec folder.
pub fn downloaded(app: &App) -> bool {
	app.codec_dir.as_deref().and_then(h264::installed).is_some()
}

/// Deletes the downloaded binary; the choice stays.
pub fn delete(app: &App) {
	if let Some(dir) = app.codec_dir.as_deref()
		&& let Err(e) = h264::delete(dir)
	{
		tracing::error!("OpenH264: {e}");
	}
}
