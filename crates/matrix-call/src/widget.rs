//! MatrixRTC Widget Protocol Bridge & Element Call Integration

use serde::{Deserialize, Serialize};
use url::Url;

/// Matrix Widget API message frame (MSC1236 / MSC2762)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WidgetApiMessage {
    /// Direction: "toWidget" or "fromWidget"
    pub api: String,
    /// Identifier of the widget instance
    #[serde(rename = "widgetId")]
    pub widget_id: String,
    /// Unique ID for correlating request and response pairs
    #[serde(rename = "requestId")]
    pub request_id: String,
    /// Action verb (e.g. "capabilities", "content_loaded", "send_event")
    pub action: String,
    /// Action payload
    #[serde(default)]
    pub data: serde_json::Value,
    /// Response payload if this is an answer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<serde_json::Value>,
}

impl WidgetApiMessage {
    /// Create a request message to send into the widget
    pub fn new_request(
        widget_id: impl Into<String>,
        request_id: impl Into<String>,
        action: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self {
            api: "toWidget".to_string(),
            widget_id: widget_id.into(),
            request_id: request_id.into(),
            action: action.into(),
            data,
            response: None,
        }
    }

    /// Create an approval response to a widget's request
    pub fn new_response(
        widget_id: impl Into<String>,
        request_id: impl Into<String>,
        action: impl Into<String>,
        response: serde_json::Value,
    ) -> Self {
        Self {
            api: "toWidget".to_string(),
            widget_id: widget_id.into(),
            request_id: request_id.into(),
            action: action.into(),
            data: serde_json::Value::Null,
            response: Some(response),
        }
    }
}

/// Builder for constructing Element Call embedding URLs with parameters
pub struct ElementCallUrlBuilder {
    base_url: Url,
    room_id: String,
    user_id: String,
    device_id: String,
    is_video: bool,
    theme: String,
}

impl ElementCallUrlBuilder {
    pub fn new(
        base_url: &str,
        room_id: &str,
        user_id: &str,
        device_id: &str,
    ) -> Result<Self, url::ParseError> {
        let base = Url::parse(base_url)?;
        Ok(Self {
            base_url: base,
            room_id: room_id.to_string(),
            user_id: user_id.to_string(),
            device_id: device_id.to_string(),
            is_video: true,
            theme: "dark".to_string(),
        })
    }

    pub fn video(mut self, enabled: bool) -> Self {
        self.is_video = enabled;
        self
    }

    pub fn theme(mut self, theme: impl Into<String>) -> Self {
        self.theme = theme.into();
        self
    }

    /// Generate the full widget URL
    pub fn build(self) -> String {
        let mut url = self.base_url;
        let mut fragment_query = Vec::new();

        fragment_query.push(format!("room={}", urlencoding::encode(&self.room_id)));
        fragment_query.push(format!("userId={}", urlencoding::encode(&self.user_id)));
        fragment_query.push(format!("deviceId={}", urlencoding::encode(&self.device_id)));
        fragment_query.push(format!("theme={}", self.theme));
        fragment_query.push(format!("appPrompt=false"));
        fragment_query.push(format!("confineToRoom=true"));
        fragment_query.push(format!("video={}", self.is_video));

        url.set_fragment(Some(&format!("?{}", fragment_query.join("&"))));
        url.to_string()
    }

    /// List standard MatrixRTC widget capabilities requested by Element Call
    pub fn default_capabilities() -> Vec<String> {
        vec![
            "org.matrix.msc3401.call".to_string(),
            "org.matrix.msc3401.call.member".to_string(),
            "org.matrix.msc3819.send_to_device".to_string(),
            "m.room.member".to_string(),
            "org.matrix.rageshake".to_string(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_widget_message_serialization() {
        let msg = WidgetApiMessage::new_request(
            "call_widget_1",
            "req_101",
            "capabilities",
            serde_json::json!({ "supportedVersions": ["0.0.1", "0.0.2"] }),
        );

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"api\":\"toWidget\""));
        assert!(json.contains("\"widgetId\":\"call_widget_1\""));

        let deserialized: WidgetApiMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, msg);
    }

    #[test]
    fn test_element_call_url_builder() {
        let builder = ElementCallUrlBuilder::new(
            "https://call.element.io",
            "!room123:matrix.org",
            "@alice:matrix.org",
            "DEV_DESKTOP",
        )
        .unwrap()
        .video(true)
        .theme("dark");

        let url = builder.build();
        assert!(url.starts_with("https://call.element.io/#?"));
        assert!(url.contains("room=%21room123%3Amatrix.org"));
        assert!(url.contains("userId=%40alice%3Amatrix.org"));
        assert!(url.contains("video=true"));
        assert!(url.contains("theme=dark"));
    }
}
