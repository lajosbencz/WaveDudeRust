//! What the About dialog (docs/ui/70) names: the program, its credits, its license and the system lines of a bug
//! report.

#[cfg(not(target_arch = "wasm32"))]
use crate::strings::Strings;

use crate::App;

/// The license text, Markdown.
pub const LICENSE: &str = include_str!("../../../LICENSE.md");
/// The program icon, 64 px PNG.
pub const ICON: &[u8] = include_bytes!("../../../frontends/classic/assets/icons/64x64.png");
/// The window icon, 256 px PNG.
pub const WINDOW_ICON: &[u8] = include_bytes!("../../../frontends/classic/assets/icons/256x256.png");
/// The copyright notice of this program (GPL-3.0 section 5 d); names are not translated.
pub const COPYRIGHT: &str = "Copyright (C) 2026 Lajos Bencz";
/// Names and works as the source headers give them; English in every language.
pub const CREDITS: [&str; 3] = [
	"Alexander Graef, author of WaveDude Next, Copyright (C) 2026",
	"Jeroen Vleggaar, Huygens Optics, author of Wave Dude 2D, Copyright (C) 2026",
	"0x23 (Diffraction Limited), author of WaveSimulator2D",
];
/// WaveDude Next's repository, linked from its credit, the first.
pub const NEXT_REPOSITORY: &str = "https://github.com/wavedudenext/WaveDudeNext";

/// The MP4 encoder as the export would load it.
#[cfg(not(target_arch = "wasm32"))]
fn h264_encoder(s: &Strings, choice: Option<bool>) -> String {
	use waveduderust::unstable::export::h264;
	if h264::BUILT_IN {
		s.get("AboutH264BuiltIn").to_owned()
	} else if crate::codec::enabled(choice) {
		format!("Cisco's OpenH264 {}", h264::VERSION)
	} else {
		s.get("AboutH264Off").to_owned()
	}
}

/// The system lines of the bug-report box.
pub fn system_lines(app: &App) -> String {
	let s = &app.strings;
	let threads = crate::platform::machine_facts().threads;
	let on_off = |on| s.get(if on { "AboutOn" } else { "AboutOff" });
	let gpu = on_off(app.settings.use_gpu);
	let legacy_cpu = crate::settings_table::Need::Rev2
		.met(crate::settings_table::BUILD)
		.then(|| format!("{}: {}", s.get("AboutLegacyCpuSetting"), on_off(app.settings.legacy_cpu_engine)));
	#[cfg(not(target_arch = "wasm32"))]
	let h264 = Some(format!("{}: {}", s.get("AboutH264Encoder"), h264_encoder(s, app.settings.use_openh264)));
	#[cfg(target_arch = "wasm32")]
	let h264 = None;
	let lines = [
		format!("{}: {} {}", s.get("AboutProductLine"), s.get("Title"), crate::build_id()),
		format!("{}: Rust, egui", s.get("AboutRuntime")),
		format!("{}: {} {}", s.get("AboutOperatingSystem"), std::env::consts::OS, std::env::consts::ARCH),
		format!("{}: {threads} {}", s.get("AboutProcessor"), s.get("AboutCores")),
		format!("{}: {gpu}", s.get("AboutGpuSetting")),
		format!("{}: {}", s.get("AboutEngine"), app.live.engine().text(s)),
	];
	lines.into_iter().chain(legacy_cpu).chain(h264).collect::<Vec<_>>().join("\n")
}
