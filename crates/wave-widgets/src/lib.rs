//! Freya widgets: dock host, tree, property rows, number box, canvas overlays, timeline.
//!
//! Every widget splits into a pure part (state, layout, hit tests, arithmetic; unit-tested) and a thin
//! Freya view on top. Sizes are logical pixels unless a doc says device pixels.

pub mod color_button;
pub mod dock;
pub mod gradient_editor;
pub mod graph_box;
pub mod header_strip;
pub mod icon;
pub mod menu;
pub mod message_list;
pub mod number_box;
pub mod picture;
pub mod property_grid;
pub mod spinner;
pub mod status_bar;
pub mod theme;
pub mod timeline;
pub mod tree;

mod paint;
mod util;

pub use util::{TestIdExt, use_modifiers};

pub use color_button::{ColorButton, ColorPicker, Hsv, Rgb};
pub use dock::{DockHost, DockLayout, DockPane, PaneGroup, Side};
pub use gradient_editor::{Gradient, GradientEditor, GradientStop};
pub use graph_box::{GraphBox, GraphData, GraphLayout, ValueAxis};
pub use header_strip::{HeaderStrip, StripOrientation};
pub use icon::{Icon, IconView};
pub use menu::{Check, ContextMenuHost, MenuBar, MenuEntry, MenuItem, MenuModel, Submenu};
pub use message_list::{LogLevel, LogLine, MessageFilter, MessageLabels, MessageList, MessageLog, MessageSelection};
pub use number_box::{NumberBox, NumberSpec, Stepping};
pub use picture::{Callback, CanvasImage, CanvasTransform, PictureEvent};
pub use property_grid::{DescriptionHeader, PropertyGrid, PropertyRow, SectionHeader};
pub use spinner::Spinner;
pub use status_bar::{Progress, StatusBar, StatusLevel, StatusLine};
pub use theme::{Theme, ThemeKind, use_init_wave_theme, use_wave_theme};
pub use timeline::{Timeline, TimelineEvent, TimelineModel};
pub use tree::{TreeAction, TreeRow, TreeSelection, TreeState, TreeView};
