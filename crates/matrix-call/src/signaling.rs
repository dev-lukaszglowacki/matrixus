//! Matrix legacy VoIP signalling events (MSC2746 / m.call.*) for 1:1 WebRTC.
//!
//! These are room timeline events used to exchange SDP offers/answers and ICE
//! candidates. Group / MatrixRTC (m.call.member) is handled separately.

use serde::{Deserialize, Serialize};

/// Direction of the local party relative to the call
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VoipDirection {
    Inbound,
    Outbound,
}

/// SDP offer/answer payload as carried inside m.call.invite / m.call.answer
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallOfferSdp {
    /// Always "offer" or "answer"
    #[serde(rename = "type")]
    pub sdp_type: String,
    pub sdp: String,
}

/// m.call.invite content
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallInvite {
    pub call_id: String,
    /// Protocol version; Matrix uses 1 (or "1" as string in some clients)
    pub version: serde_json::Value,
    pub lifetime: u64,
    pub offer: CallOfferSdp,
    /// Party id of the inviting device (MSC2746)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub party_id: Option<String>,
    /// Optional invitee for multi-device rooms
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invitee: Option<String>,
}

/// m.call.answer content
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallAnswer {
    pub call_id: String,
    pub version: serde_json::Value,
    pub answer: CallOfferSdp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub party_id: Option<String>,
}

/// Single ICE candidate in m.call.candidates
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IceCandidateJson {
    pub candidate: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdp_mid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdp_m_line_index: Option<u16>,
}

/// m.call.candidates content
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallCandidate {
    pub call_id: String,
    pub version: serde_json::Value,
    pub candidates: Vec<IceCandidateJson>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub party_id: Option<String>,
}

/// m.call.hangup / m.call.reject content
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallHangup {
    pub call_id: String,
    pub version: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub party_id: Option<String>,
}

/// Unified parsed Matrix VoIP event for the native engine
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatrixVoipEvent {
    Invite(CallInvite),
    Answer(CallAnswer),
    Candidates(CallCandidate),
    Hangup(CallHangup),
    Reject(CallHangup),
}

impl MatrixVoipEvent {
    pub fn call_id(&self) -> &str {
        match self {
            Self::Invite(e) => &e.call_id,
            Self::Answer(e) => &e.call_id,
            Self::Candidates(e) => &e.call_id,
            Self::Hangup(e) | Self::Reject(e) => &e.call_id,
        }
    }

    /// Build JSON content suitable for `room.send()` of the matching event type.
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::Invite(_) => "m.call.invite",
            Self::Answer(_) => "m.call.answer",
            Self::Candidates(_) => "m.call.candidates",
            Self::Hangup(_) => "m.call.hangup",
            Self::Reject(_) => "m.call.reject",
        }
    }

    pub fn to_content_value(&self) -> Result<serde_json::Value, serde_json::Error> {
        match self {
            Self::Invite(e) => serde_json::to_value(e),
            Self::Answer(e) => serde_json::to_value(e),
            Self::Candidates(e) => serde_json::to_value(e),
            Self::Hangup(e) | Self::Reject(e) => serde_json::to_value(e),
        }
    }

    pub fn from_type_and_content(
        event_type: &str,
        content: serde_json::Value,
    ) -> Option<Self> {
        match event_type {
            "m.call.invite" => serde_json::from_value(content).ok().map(Self::Invite),
            "m.call.answer" => serde_json::from_value(content).ok().map(Self::Answer),
            "m.call.candidates" => serde_json::from_value(content).ok().map(Self::Candidates),
            "m.call.hangup" => serde_json::from_value(content).ok().map(Self::Hangup),
            "m.call.reject" => serde_json::from_value(content).ok().map(Self::Reject),
            _ => None,
        }
    }
}

/// Helper to build a version field (integer 1 is most common)
pub fn voip_version() -> serde_json::Value {
    serde_json::json!(1)
}
