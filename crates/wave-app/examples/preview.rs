//! The preview pane alone on Example 16: opens the project, starts a run and plays it.
//! The presenter's statistics are logged at Debug to stderr.

use std::path::PathBuf;
use std::time::Duration;

use freya::prelude::*;
use wave_app::ui::preview::preview_pane;
use wave_app::ui::shared::Shared;
use wave_app_state::settings::AppDirs;
use wave_app_state::{AppState, Event, Guarded, Response};
use wave_core::run::warning::Levers;
use wave_widgets::{Theme, use_init_wave_theme};

const EXAMPLE: &str = "../wave-core/tests/fixtures/examples/Example16_Wave_in_2_boxes.wdproj";

fn main() -> anyhow::Result<()> {
	tracing_subscriber::fmt().with_max_level(tracing::Level::DEBUG).with_writer(std::io::stderr).init();
	launch(LaunchConfig::new().with_window(WindowConfig::new(app).with_title("Preview").with_size(1400., 900.)));
	Ok(())
}

fn app() -> impl IntoElement {
	use_init_wave_theme(Theme::dark);
	let dirs = use_hook(|| std::rc::Rc::new(tempfile::tempdir().expect("temp dir")));
	let shared = Shared::provide(
		AppState::start(&AppDirs { config: dirs.path().join("config"), cache: dirs.path().join("cache") }, [])
			.expect("state"),
	);
	use_hook({
		let shared = shared.clone();
		move || {
			let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(EXAMPLE);
			tracing::info!("open: {:?}", shared.send(Event::Guarded(Guarded::Open(path))));
			if shared.send(Event::Run) == Response::MemoryWarning {
				let levers = Levers { frames: true, ..Levers::default() };
				shared.send(Event::MemoryWarningRun { levers, mute: false });
			}
			shared.send(Event::SetLoop(true));
			tracing::info!("play: {:?}", shared.send(Event::Play));
			spawn(async move {
				loop {
					timer(Duration::from_millis(16)).await;
					shared.tick();
				}
			});
		}
	});
	rect().expanded().background(Color::from_rgb(30, 30, 30)).child(preview_pane())
}
