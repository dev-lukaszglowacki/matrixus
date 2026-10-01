//! Desktop UI Presentation Models and View Controllers

pub mod call;
pub mod composer;
pub mod sidebar;
pub mod timeline;
pub mod window;

#[cfg(feature = "gui")]
pub mod async_ui;
#[cfg(feature = "gui")]
pub mod gtk_app;
#[cfg(feature = "gui")]
pub mod login;
#[cfg(feature = "gui")]
pub mod settings;
#[cfg(feature = "gui")]
pub mod verification;

pub use call::CallViewController;
pub use composer::ComposerState;
pub use sidebar::SidebarState;
pub use timeline::TimelineState;
pub use window::MainWindowState;
