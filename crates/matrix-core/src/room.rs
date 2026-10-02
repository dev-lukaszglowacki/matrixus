//! Matrix room representations, timeline events, and messaging types

use serde::{Deserialize, Serialize};

/// Summary of a Matrix room for the UI sidebar and room list
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoomSummary {
    /// Canonical Matrix Room ID (e.g. "!abc:matrix.org")
    pub room_id: String,
    /// Display name or calculated name
    pub name: String,
    /// Room topic / description
    pub topic: Option<String>,
    /// MXC URI of room avatar
    pub avatar_url: Option<String>,
    /// Whether this is a 1:1 direct message room
    pub is_direct: bool,
    /// Whether End-to-End Encryption (Megolm) is active
    pub is_encrypted: bool,
    /// Count of unread highlight / notification events
    pub unread_notifications: u64,
    /// Whether a voice/video call is currently active in this room
    pub has_active_call: bool,
    /// Latest timeline event for preview
    pub last_event: Option<TimelineEvent>,
}

/// Aggregated emoji reaction on a message
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ReactionSummary {
    /// Emoji / reaction key (e.g. "👍")
    pub key: String,
    /// Number of users who reacted with this key
    pub count: u32,
    /// Whether the current account has reacted with this key
    pub reacted_by_me: bool,
}

/// A parsed timeline event for presentation in chat views
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimelineEvent {
    /// Matrix Event ID (e.g. "$123456")
    pub event_id: String,
    /// User ID of the sender
    pub sender: String,
    /// Display name of the sender when known (room member profile)
    pub sender_display_name: Option<String>,
    /// MXC URI of the sender's avatar when known
    pub sender_avatar_url: Option<String>,
    /// Unix timestamp in milliseconds
    pub timestamp_millis: u64,
    /// Parsed event content
    pub content: EventContent,
    /// In reply to another event ID if applicable
    pub reply_to: Option<String>,
    /// Aggregated reactions on this event
    pub reactions: Vec<ReactionSummary>,
}

/// One page of room history from `/messages` (oldest first).
#[derive(Debug, Clone)]
pub struct TimelinePage {
    pub events: Vec<TimelineEvent>,
    /// Token to request older events (`MessagesOptions::from`). `None` means
    /// the start of the accessible timeline has been reached.
    pub end_token: Option<String>,
}

/// Types of timeline content supported by the client
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventContent {
    /// Text message with optional HTML formatted body
    Text {
        body: String,
        formatted_html: Option<String>,
    },
    /// An emote (/me action)
    Emote {
        body: String,
    },
    /// System / Bot notice
    Notice {
        body: String,
    },
    /// Image attachment
    Image {
        url: String,
        filename: String,
        mime_type: String,
        size_bytes: Option<u64>,
    },
    /// Video attachment
    Video {
        url: String,
        filename: String,
        mime_type: String,
        size_bytes: Option<u64>,
    },
    /// Audio attachment / Voice note
    Audio {
        url: String,
        filename: String,
        mime_type: String,
        duration_ms: Option<u64>,
    },
    /// Generic file attachment
    File {
        url: String,
        filename: String,
        mime_type: String,
        size_bytes: Option<u64>,
    },
    /// MatrixRTC / MSC3401 Call Member event
    CallMember {
        membership: String,
        active: bool,
    },
    /// Message was redacted/deleted
    Redacted,
}

impl EventContent {
    /// Returns a short text preview suitable for notifications or room list snippets
    pub fn preview_text(&self) -> &str {
        match self {
            EventContent::Text { body, .. } => body,
            EventContent::Emote { body } => body,
            EventContent::Notice { body } => body,
            EventContent::Image { .. } => "📷 Image",
            EventContent::Video { .. } => "🎥 Video",
            EventContent::Audio { .. } => "🎵 Audio",
            EventContent::File { .. } => "📎 File attachment",
            EventContent::CallMember { active: true, .. } => "📞 Active call in progress",
            EventContent::CallMember { active: false, .. } => "📞 Call ended",
            EventContent::Redacted => "🗑️ Message removed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_preview() {
        let text_event = EventContent::Text {
            body: "Hello, Matrix!".to_string(),
            formatted_html: None,
        };
        assert_eq!(text_event.preview_text(), "Hello, Matrix!");

        let call_event = EventContent::CallMember {
            membership: "join".to_string(),
            active: true,
        };
        assert_eq!(call_event.preview_text(), "📞 Active call in progress");
    }
}
