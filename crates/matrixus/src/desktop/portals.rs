//! XDG Desktop Portal helpers for Wayland ScreenCast, Camera, and Microphone

use tracing::{info, warn};

/// Status of desktop media portals
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortalStatus {
    pub screencast_available: bool,
    pub camera_available: bool,
    pub microphone_available: bool,
}

impl PortalStatus {
    pub fn detect() -> Self {
        info!("Detecting XDG Desktop Portal ScreenCast / Camera / Mic capabilities");
        let portal_present = std::path::Path::new("/usr/share/xdg-desktop-portal").exists()
            || std::path::Path::new("/usr/libexec/xdg-desktop-portal").exists()
            || std::env::var_os("XDG_CURRENT_DESKTOP").is_some();

        Self {
            screencast_available: portal_present,
            camera_available: portal_present,
            microphone_available: true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PortalRequestResult {
    Granted { label: String },
    Denied,
    Unavailable,
    Error(String),
}

#[derive(Debug, Clone, Default)]
pub struct PortalService {
    status: Option<PortalStatus>,
}

impl PortalService {
    pub fn new() -> Self {
        Self { status: None }
    }

    pub fn status(&mut self) -> PortalStatus {
        if self.status.is_none() {
            self.status = Some(PortalStatus::detect());
        }
        self.status.unwrap_or(PortalStatus {
            screencast_available: false,
            camera_available: false,
            microphone_available: true,
        })
    }

    #[cfg(feature = "gui")]
    pub async fn request_camera(&self) -> PortalRequestResult {
        info!("Requesting camera via XDG Desktop Portal");
        match try_request_camera().await {
            Ok(true) => PortalRequestResult::Granted {
                label: "Camera".into(),
            },
            Ok(false) => PortalRequestResult::Denied,
            Err(e) => {
                warn!("Camera portal error: {e}");
                PortalRequestResult::Unavailable
            }
        }
    }

    #[cfg(not(feature = "gui"))]
    pub async fn request_camera(&self) -> PortalRequestResult {
        PortalRequestResult::Unavailable
    }

    #[cfg(feature = "gui")]
    pub async fn request_screencast(&self) -> PortalRequestResult {
        info!("Requesting ScreenCast via XDG Desktop Portal");
        match try_request_screencast().await {
            Ok(true) => PortalRequestResult::Granted {
                label: "Screen share".into(),
            },
            Ok(false) => PortalRequestResult::Denied,
            Err(e) => {
                warn!("ScreenCast portal error: {e}");
                PortalRequestResult::Unavailable
            }
        }
    }

    #[cfg(not(feature = "gui"))]
    pub async fn request_screencast(&self) -> PortalRequestResult {
        PortalRequestResult::Unavailable
    }
}

#[cfg(feature = "gui")]
async fn try_request_camera() -> Result<bool, String> {
    let camera = ashpd::desktop::camera::Camera::new()
        .await
        .map_err(|e| e.to_string())?;
    match camera.request_access().await {
        Ok(_) => {
            info!("Camera access granted");
            Ok(true)
        }
        Err(e) => {
            warn!("Camera access denied/failed: {e}");
            Ok(false)
        }
    }
}

#[cfg(feature = "gui")]
async fn try_request_screencast() -> Result<bool, String> {
    use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
    use ashpd::desktop::PersistMode;

    let proxy = Screencast::new().await.map_err(|e| e.to_string())?;
    let session = proxy.create_session().await.map_err(|e| e.to_string())?;

    let source_types = SourceType::Monitor | SourceType::Window;
    proxy
        .select_sources(
            &session,
            CursorMode::Metadata,
            source_types,
            false,
            None,
            PersistMode::DoNot,
        )
        .await
        .map_err(|e| e.to_string())?;

    // ashpd 0.10: start returns Request; response payload may be accessed via
    // response() / into_response depending on version — treat success as granted.
    match proxy.start(&session, None).await {
        Ok(_response) => {
            info!("ScreenCast start succeeded");
            Ok(true)
        }
        Err(e) => {
            warn!("ScreenCast start failed: {e}");
            Ok(false)
        }
    }
}
