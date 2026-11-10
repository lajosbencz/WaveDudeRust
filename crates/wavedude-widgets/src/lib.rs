//! Freya widgets: dock host, tree, property rows, number box, canvas overlays, timeline.
//!
//! Every widget splits into a pure part (state, layout, hit tests, arithmetic; unit-tested) and a thin
//! Freya view on top. Sizes are logical pixels unless a doc says device pixels.

pub mod button;
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
pub mod text_box;
pub mod text_field;
pub mod theme;
pub mod timeline;
pub mod tooltip;
pub mod tree;

mod edit;
mod paint;
mod util;

pub use edit::{FocusRequest, PendingEdit, use_focus_on_layout};
pub use util::{TestIdExt, use_modifiers};

pub use button::Button;
pub use color_button::{ColorButton, ColorPicker, Hsv, PickerTips, Rgb};
pub use dock::{DockHost, DockLayout, DockPane, PaneGroup, Side};
pub use gradient_editor::{Gradient, GradientEditor, GradientStop};
pub use graph_box::{GraphBox, GraphData, GraphLayout, GraphTick, TextMeasure};
pub use header_strip::{HeaderStrip, StripOrientation};
pub use icon::{Icon, IconView};
pub use menu::{Check, ContextMenuHost, MenuBar, MenuEntry, MenuItem, MenuModel, Submenu};
pub use message_list::{LogLevel, LogLine, MessageFilter, MessageLabels, MessageList, MessageLog, MessageSelection};
pub use number_box::{NumberBox, NumberSpec, Stepping};
pub use picture::{Callback, CanvasImage, CanvasTransform, PictureEvent};
pub use property_grid::{DescriptionHeader, PropertyGrid, PropertyRow, SectionHeader};
pub use spinner::Spinner;
pub use status_bar::{Progress, StatusBar, StatusLevel, StatusLine};
pub use text_box::TextBox;
pub use text_field::TextField;
pub use theme::{Theme, ThemeKind, use_init_wave_theme, use_wave_theme};
pub use timeline::{Timeline, TimelineEvent, TimelineModel};
pub use tooltip::{DelayedTip, Tip};
pub use tree::{TreeAction, TreeRow, TreeSelection, TreeState, TreeView};
