//! WebKitGTK Script Message Handler and MatrixRTC Widget IPC Bridge

use matrix_call::{ElementCallUrlBuilder, WidgetApiMessage};
use tracing::debug;

/// Manages communication between the WebKitGTK Element Call widget and Matrix core
pub struct WidgetHostBridge {
    pub widget_id: String,
    pub room_id: String,
    pub user_id: String,
    pub device_id: String,
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
        }
    }

    /// Build the widget URL for Element Call
    pub fn generate_widget_url(&self, is_video: bool, theme: &str) -> String {
        ElementCallUrlBuilder::new(
            "https://call.element.io",
            &self.room_id,
            &self.user_id,
            &self.device_id,
        )
        .expect("Valid URL")
        .video(is_video)
        .theme(theme)
        .build()
    }

    /// JavaScript initialization script injected into WebKitGTK UserContentManager
    /// to intercept window.postMessage and forward to WebKit script message handler
    pub fn injection_script() -> &'static str {
        r#"
        (function() {
            window.addEventListener('message', function(event) {
                if (event.data && typeof event.data === 'object') {
                    if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.matrixWidget) {
                        window.webkit.messageHandlers.matrixWidget.postMessage(JSON.stringify(event.data));
                    }
                }
            });
        })();
        "#
    }

    /// Process an incoming JSON string message from the WebKitGTK script handler
    pub fn handle_widget_message(&self, raw_json: &str) -> Option<WidgetApiMessage> {
        match serde_json::from_str::<WidgetApiMessage>(raw_json) {
            Ok(msg) => {
                debug!("Received widget message: action={}", msg.action);
                Some(msg)
            }
            Err(e) => {
                debug!("Ignoring non-widget postMessage or parse error: {e}");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_widget_bridge_url() {
        let bridge = WidgetHostBridge::new("w1", "!room:matrix.org", "@alice:matrix.org", "DEV1");
        let url = bridge.generate_widget_url(true, "dark");
        assert!(url.contains("room=%21room%3Amatrix.org"));
        assert!(url.contains("video=true"));
    }
}
