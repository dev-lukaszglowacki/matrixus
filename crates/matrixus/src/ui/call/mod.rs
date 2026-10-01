//! Video and Voice Calling UI components

pub mod call_window;
pub mod widget_host;

pub use call_window::CallViewController;
#[cfg(feature = "gui")]
pub use call_window::open_call_window;
pub use widget_host::WidgetHostBridge;
