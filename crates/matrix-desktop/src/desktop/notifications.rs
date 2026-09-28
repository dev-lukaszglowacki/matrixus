//! FreeDesktop / Linux system notifications integration

use tracing::info;

/// Action chosen by the user in an incoming call notification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallNotificationAction {
    Accept,
    Decline,
}

/// Notification service for desktop alerts
pub struct NotificationService;

impl NotificationService {
    pub fn new() -> Self {
        Self
    }

    /// Display notification for an incoming Matrix message
    pub fn show_message_notification(
        &self,
        sender_name: &str,
        room_name: &str,
        body_preview: &str,
    ) {
        info!("Notification: [{room_name}] {sender_name}: {body_preview}");
    }

    /// Display notification for an incoming voice or video call with Accept / Decline actions
    pub fn show_incoming_call_notification(
        &self,
        caller_name: &str,
        room_name: &str,
        is_video: bool,
    ) {
        let call_type = if is_video { "Video Call" } else { "Voice Call" };
        info!("Incoming Call Notification: {call_type} from {caller_name} in {room_name}");
    }
}

impl Default for NotificationService {
    fn default() -> Self {
        Self::new()
    }
}
