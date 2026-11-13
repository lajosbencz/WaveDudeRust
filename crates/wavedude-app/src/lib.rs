//! `WaveDude` desktop and browser application: the window ([`ui`]) and the parts its views build on, over
//! `wavedude_app_state`. Files, exports and the updater are native only.

#[cfg(not(target_arch = "wasm32"))]
pub mod codec;
pub mod dialogs;
#[cfg(not(target_arch = "wasm32"))]
pub mod downloads;
#[cfg(not(target_arch = "wasm32"))]
pub mod export;
pub mod manual;
pub mod menus;
#[cfg(not(target_arch = "wasm32"))]
pub mod open_requests;
#[cfg(not(target_arch = "wasm32"))]
pub mod os_theme;
#[cfg(not(target_arch = "wasm32"))]
pub mod placement;
pub mod preview;
pub mod properties;
pub mod ui;
#[cfg(not(target_arch = "wasm32"))]
pub mod update;
pub mod window;

use wavedude_app_state::AppState;
use wavedude_app_state::live::CoreBackend;

/// The application state with the core's engines.
pub type App = AppState<CoreBackend>;
