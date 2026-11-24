//! WaveDude Rust's host services without a UI toolkit: the settings and their store, the program's folders, Cisco's
//! OpenH264 download and the user's colour schemes. Shared by the CLI and the app.

#[cfg(not(target_arch = "wasm32"))]
pub mod codec;
#[cfg(not(target_arch = "wasm32"))]
pub mod export;
pub mod machine;
pub mod settings;
pub mod store;
pub mod user_schemes;
