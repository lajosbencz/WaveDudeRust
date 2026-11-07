//! The Freya views: thin layers over `wavedude_app_state`.

pub mod about;
pub mod app;
pub mod codec;
pub mod dialogs;
pub mod export_dialog;
pub mod manual;
pub mod memory_warning;
pub mod menus;
pub mod messages;
pub mod preview;
pub mod properties;
pub mod properties_forms;
pub mod settings_views;
pub mod shared;
pub mod shortcuts;
pub mod startup;
pub mod status;
pub mod tree;
pub mod update;
pub mod window;

#[cfg(test)]
mod tests;
