//! MatrixRTC Call Session Management and State Machine

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug, PartialEq, Eq)]
pub enum CallError {
    #[error("Invalid state transition from {from:?} to {to:?}")]
    InvalidTransition { from: CallState, to: CallState },

    #[error("Participant not found: {0}")]
    ParticipantNotFound(String),

    #[error("Call already in progress in room: {0}")]
    AlreadyInProgress(String),
}

/// Real-time lifecycle state of a MatrixRTC call
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallState {
    /// No call active
    Idle,
    /// Incoming call alert / outgoing ringback
    Ringing,
    /// Negotiating WebRTC / SFU connection
    Connecting,
    /// Active media streaming
    Connected,
    /// Network interruption, attempting reconnection
    Reconnecting,
    /// Call has concluded
    Ended,
}

/// A participant currently joined to the MatrixRTC call
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallParticipant {
    pub user_id: String,
    pub device_id: String,
    pub audio_enabled: bool,
    pub video_enabled: bool,
    pub screen_sharing: bool,
}

/// Call session controller tracking local and remote call state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallSession {
    pub call_id: String,
    pub room_id: String,
    pub state: CallState,
    pub is_video_call: bool,
    pub local_audio_muted: bool,
    pub local_video_muted: bool,
    pub local_screen_sharing: bool,
    pub participants: Vec<CallParticipant>,
}

impl CallSession {
    /// Initialize a new call session for a room
    pub fn new(call_id: impl Into<String>, room_id: impl Into<String>, is_video: bool) -> Self {
        Self {
            call_id: call_id.into(),
            room_id: room_id.into(),
            state: CallState::Idle,
            is_video_call: is_video,
            local_audio_muted: false,
            local_video_muted: !is_video,
            local_screen_sharing: false,
            participants: Vec::new(),
        }
    }

    /// Transition to Ringing state
    pub fn start_ringing(&mut self) -> Result<(), CallError> {
        match self.state {
            CallState::Idle => {
                self.state = CallState::Ringing;
                Ok(())
            }
            _ => Err(CallError::InvalidTransition {
                from: self.state,
                to: CallState::Ringing,
            }),
        }
    }

    /// Transition to Connecting state
    pub fn start_connecting(&mut self) -> Result<(), CallError> {
        match self.state {
            CallState::Idle | CallState::Ringing => {
                self.state = CallState::Connecting;
                Ok(())
            }
            _ => Err(CallError::InvalidTransition {
                from: self.state,
                to: CallState::Connecting,
            }),
        }
    }

    /// Transition to Connected state
    pub fn set_connected(&mut self) -> Result<(), CallError> {
        match self.state {
            CallState::Connecting | CallState::Reconnecting => {
                self.state = CallState::Connected;
                Ok(())
            }
            _ => Err(CallError::InvalidTransition {
                from: self.state,
                to: CallState::Connected,
            }),
        }
    }

    /// End call session
    pub fn hang_up(&mut self) {
        self.state = CallState::Ended;
        self.participants.clear();
        self.local_screen_sharing = false;
    }

    /// Toggle local microphone mute
    pub fn toggle_audio_mute(&mut self) -> bool {
        self.local_audio_muted = !self.local_audio_muted;
        self.local_audio_muted
    }

    /// Toggle local video camera
    pub fn toggle_video_mute(&mut self) -> bool {
        self.local_video_muted = !self.local_video_muted;
        self.local_video_muted
    }

    /// Toggle screen sharing
    pub fn toggle_screen_share(&mut self) -> bool {
        self.local_screen_sharing = !self.local_screen_sharing;
        self.local_screen_sharing
    }

    /// Add or update a remote participant
    pub fn update_participant(&mut self, participant: CallParticipant) {
        if let Some(pos) = self.participants.iter().position(|p| p.user_id == participant.user_id && p.device_id == participant.device_id) {
            self.participants[pos] = participant;
        } else {
            self.participants.push(participant);
        }
    }

    /// Remove a participant who left the call
    pub fn remove_participant(&mut self, user_id: &str, device_id: &str) {
        self.participants.retain(|p| !(p.user_id == user_id && p.device_id == device_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_call_lifecycle() {
        let mut session = CallSession::new("call_1", "!room:matrix.org", true);
        assert_eq!(session.state, CallState::Idle);

        session.start_ringing().unwrap();
        assert_eq!(session.state, CallState::Ringing);

        session.start_connecting().unwrap();
        assert_eq!(session.state, CallState::Connecting);

        session.set_connected().unwrap();
        assert_eq!(session.state, CallState::Connected);

        assert!(!session.local_audio_muted);
        assert!(session.toggle_audio_mute());
        assert!(session.local_audio_muted);

        session.hang_up();
        assert_eq!(session.state, CallState::Ended);
    }

    #[test]
    fn test_participant_tracking() {
        let mut session = CallSession::new("call_2", "!room:matrix.org", true);
        let alice = CallParticipant {
            user_id: "@alice:matrix.org".to_string(),
            device_id: "DEV_A".to_string(),
            audio_enabled: true,
            video_enabled: true,
            screen_sharing: false,
        };

        session.update_participant(alice.clone());
        assert_eq!(session.participants.len(), 1);

        session.remove_participant("@alice:matrix.org", "DEV_A");
        assert_eq!(session.participants.len(), 0);
    }
}
