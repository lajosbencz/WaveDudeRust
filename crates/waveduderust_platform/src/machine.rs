//! The machine's facts for the preview presets.

use waveduderust::quality::MachineFacts;

/// The memory assumed where the browser does not tell.
pub const WEB_UNKNOWN_MEMORY: u64 = 4 << 30;

/// Memory, threads and whether the spill folder's drive is a hard disk; the GPU unknown.
#[cfg(not(target_arch = "wasm32"))]
pub fn machine_facts() -> MachineFacts {
	waveduderust::run::machine_facts(&waveduderust::run::FrameStore::default_dir())
}

/// `navigator.deviceMemory` ([`WEB_UNKNOWN_MEMORY`] where unknown) and `navigator.hardwareConcurrency`; the GPU and
/// the drive unknown.
#[cfg(target_arch = "wasm32")]
pub fn machine_facts() -> MachineFacts {
	let number = |key: &str| {
		let navigator = js_sys::Reflect::get(&js_sys::global(), &"navigator".into()).ok()?;
		js_sys::Reflect::get(&navigator, &key.into()).ok()?.as_f64().filter(|v| v.is_finite() && *v > 0.0)
	};
	MachineFacts {
		physical_bytes: number("deviceMemory").map_or(WEB_UNKNOWN_MEMORY, |gib| (gib * f64::from(1u32 << 30)) as u64),
		threads: number("hardwareConcurrency").map_or(1, |t| t as u32).max(1),
		has_gpu: None,
		cache_drive_seek_penalty: None,
	}
}
