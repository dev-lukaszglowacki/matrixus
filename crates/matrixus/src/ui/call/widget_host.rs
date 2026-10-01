//! WebKitGTK Script Message Handler and MatrixRTC Widget IPC Bridge

use matrix_call::{ElementCallUrlBuilder, WidgetApiMessage};
use tracing::{debug, info, warn};

/// Manages communication between the WebKitGTK Element Call widget and Matrix core
pub struct WidgetHostBridge {
    pub widget_id: String,
    pub room_id: String,
    pub user_id: String,
    pub device_id: String,
    /// Base URL for Element Call (configurable via ELEMENT_CALL_URL)
    pub element_call_url: String,
}

impl WidgetHostBridge {
    pub fn new(
        widget_id: impl Into<String>,
        room_id: impl Into<String>,
        user_id: impl Into<String>,
        device_id: impl Into<String>,
    ) -> Self {
        Self {
            widget_id: widget_id.into(),
            room_id: room_id.into(),
            user_id: user_id.into(),
            device_id: device_id.into(),
            element_call_url: std::env::var("ELEMENT_CALL_URL")
                .unwrap_or_else(|_| "https://call.element.io".to_string()),
        }
    }

    /// Build the widget URL for Element Call
    pub fn generate_widget_url(&self, is_video: bool, theme: &str) -> String {
        match ElementCallUrlBuilder::new(
            &self.element_call_url,
            &self.room_id,
            &self.user_id,
            &self.device_id,
        ) {
            Ok(builder) => builder.video(is_video).theme(theme).build(),
            Err(e) => {
                warn!("Failed to build Element Call URL: {e}");
                self.element_call_url.clone()
            }
        }
    }

    /// Handle an incoming postMessage from the Element Call widget.
    ///
    /// Returns an optional JSON response string to post back into the page.
    pub fn handle_widget_message(&self, raw_json: &str) -> Option<String> {
        let msg: WidgetApiMessage = match serde_json::from_str(raw_json) {
            Ok(m) => m,
            Err(e) => {
                debug!("Ignoring non-widget message: {e}");
                return None;
            }
        };

        info!(
            "Widget message action={} requestId={} from {}",
            msg.action, msg.request_id, msg.widget_id
        );

        match msg.action.as_str() {
            "capabilities" | "org.matrix.msc2876.supported_versions" => {
                let caps = ElementCallUrlBuilder::default_capabilities();
                let response = WidgetApiMessage::new_response(
                    &self.widget_id,
                    &msg.request_id,
                    &msg.action,
                    serde_json::json!({
                        "capabilities": caps,
                        "supportedVersions": ["0.0.1", "0.0.2"]
                    }),
                );
                serde_json::to_string(&response).ok()
            }
            "content_loaded" => {
                info!("Element Call widget content loaded");
                let response = WidgetApiMessage::new_response(
                    &self.widget_id,
                    &msg.request_id,
                    &msg.action,
                    serde_json::json!({ "success": true }),
                );
                serde_json::to_string(&response).ok()
            }
            "send_event" | "org.matrix.msc4157.send_event" => {
                debug!("Widget requested send_event: {}", msg.data);
                let response = WidgetApiMessage::new_response(
                    &self.widget_id,
                    &msg.request_id,
                    &msg.action,
                    serde_json::json!({ "success": true }),
                );
                serde_json::to_string(&response).ok()
            }
            "hangup_call" | "org.matrix.msc3401.hangup" => {
                info!("Widget requested hangup");
                None
            }
            other => {
                debug!("Unhandled widget action: {other}");
                None
            }
        }
    }
}
