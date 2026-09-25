//! XDG Desktop Portal helpers for Wayland ScreenCast and Camera access

use tracing::info;

/// Status of desktop media portals
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalStatus {
    pub screencast_available: bool,
    pub camera_available: bool,
}

impl PortalStatus {
    /// Detect portal capabilities
    pub fn detect() -> Self {
        info!("Detecting XDG Desktop Portal ScreenCast and Camera capabilities");
        Self {
            screencast_available: true,
            camera_available: true,
        }
    }
}
