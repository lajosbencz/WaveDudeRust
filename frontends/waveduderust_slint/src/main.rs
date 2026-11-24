//! WaveDude Rust's app: the Slint window over the core (decision 0032).

#[allow(
	unused_qualifications,
	unreachable_pub,
	missing_docs,
	clippy::all,
	clippy::pedantic,
	clippy::nursery,
	clippy::restriction
)]
mod ui {
	slint::include_modules!();
}

use slint::ComponentHandle;
use ui::AppWindow;

fn main() -> Result<(), slint::PlatformError> {
	let app = AppWindow::new()?;
	app.set_status_text("Ready".into());
	let weak = app.as_weak();
	app.on_menu(move |id| match id.as_str() {
		"exit" => {
			if let Some(app) = weak.upgrade() {
				let _ = app.hide();
			}
		}
		other => tracing::info!("menu: {other}"),
	});
	app.run()
}
