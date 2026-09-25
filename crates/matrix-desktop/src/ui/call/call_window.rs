//! Call Window / View container managing the active call view and media controls

use super::widget_host::WidgetHostBridge;
use matrix_call::CallSession;
use tracing::info;

/// Controller for an active call window / overlay
pub struct CallViewController {
    pub session: CallSession,
    pub bridge: WidgetHostBridge,
}

impl CallViewController {
    pub fn new(session: CallSession, user_id: &str, device_id: &str) -> Self {
        let bridge = WidgetHostBridge::new(
            format!("widget_{}", session.call_id),
            &session.room_id,
            user_id,
            device_id,
        );
        Self { session, bridge }
    }

    /// Get the target URL to load into WebKitGTK
    pub fn widget_url(&self) -> String {
        self.bridge
            .generate_widget_url(self.session.is_video_call, "dark")
    }

    pub fn toggle_audio(&mut self) -> bool {
        let muted = self.session.toggle_audio_mute();
        info!("Call audio mute toggled: {muted}");
        muted
    }

    pub fn toggle_video(&mut self) -> bool {
        let muted = self.session.toggle_video_mute();
        info!("Call video mute toggled: {muted}");
        muted
    }

    pub fn toggle_screen_share(&mut self) -> bool {
        let sharing = self.session.toggle_screen_share();
        info!("Screen sharing toggled: {sharing}");
        sharing
    }

    pub fn end_call(&mut self) {
        info!("Ending call session: {}", self.session.call_id);
        self.session.hang_up();
    }
}
