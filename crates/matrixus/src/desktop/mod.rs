//! Desktop integration: notifications and XDG portals

pub mod notifications;
pub mod portals;

pub use notifications::NotificationService;
pub use portals::{PortalRequestResult, PortalService, PortalStatus};
