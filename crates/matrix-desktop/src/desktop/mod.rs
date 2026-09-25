//! Linux Desktop Integrations (Notifications, Secret Service, Portals)

pub mod notifications;
pub mod portals;

pub use notifications::{CallNotificationAction, NotificationService};
pub use portals::PortalStatus;
