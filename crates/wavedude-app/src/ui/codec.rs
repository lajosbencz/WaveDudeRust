//! The app's side of Cisco's `OpenH264` (decision 0005): the user's choice is the `UseOpenH264` setting, the
//! binary a file in the app's codec folder (`AppDirs::codec`), both shared with the CLI. Threads: the UI thread.

use std::path::PathBuf;

use freya::prelude::*;
use wavedude_app_state::Event as AppEvent;
use wavedude_export::h264;

use super::app::Cx;

/// MP4 export may run: compiled in, or enabled by the user.
pub fn enabled(choice: Option<bool>) -> bool {
	h264::BUILT_IN || choice == Some(true)
}

/// The startup page asks for the user's choice.
pub fn unasked(choice: Option<bool>) -> bool {
	!h264::BUILT_IN && choice.is_none()
}

/// The codec folder; logged when the app has none.
pub fn dir(cx: &Cx) -> Option<PathBuf> {
	let dir = cx.shared.peek(|a| a.codec_dir.clone());
	if dir.is_none() {
		tracing::error!("OpenH264: the app has no codec folder");
	}
	dir
}

/// Cisco's binary is in the codec folder.
pub fn downloaded(cx: &Cx) -> bool {
	cx.shared.peek(|a| a.codec_dir.as_deref().and_then(h264::installed)).is_some()
}

/// Records the user's choice.
pub fn set(cx: &Cx, enabled: bool) {
	cx.send(AppEvent::SetUseOpenH264(enabled));
}

/// Deletes the downloaded binary; the choice stays.
pub fn delete(cx: &Cx) {
	if let Some(dir) = dir(cx)
		&& let Err(e) = h264::delete(&dir)
	{
		tracing::error!("OpenH264: {e}");
	}
	refresh(cx);
}

/// Looks for the binary again, after an export may have downloaded it.
pub fn refresh(cx: &Cx) {
	let mut state = cx.ui.codec_downloaded;
	state.set_if_modified(downloaded(cx));
}
