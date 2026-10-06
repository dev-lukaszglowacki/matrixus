//! Video and Voice Calling UI components (native WebRTC)

pub mod call_window;

pub use call_window::CallViewController;
#[cfg(feature = "gui")]
pub use call_window::open_call_window;
