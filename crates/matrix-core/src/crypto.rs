//! End-to-End Encryption (E2EE), Cross-Signing, and SAS Device Verification

use serde::{Deserialize, Serialize};

/// SAS (Short Authentication String) Emoji for interactive device verification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SasEmoji {
    /// Unicode emoji character (e.g. "🐶")
    pub symbol: String,
    /// English description (e.g. "Dog")
    pub description: String,
}

/// Lifecycle states of a device verification transaction
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VerificationState {
    /// Verification has been requested by a remote device or user
    Requested {
        transaction_id: String,
        other_user: String,
        other_device: String,
    },
    /// Verification is in progress, negotiating protocols
    Started {
        transaction_id: String,
    },
    /// SAS emojis are ready to be visually compared between devices
    ShowEmojis {
        transaction_id: String,
        emojis: Vec<SasEmoji>,
    },
    /// Both devices matched emojis and the exchange succeeded
    Done {
        transaction_id: String,
    },
    /// Verification was declined or timed out
    Cancelled {
        transaction_id: String,
        reason: String,
    },
}

/// Device verification and trust status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeviceTrustLevel {
    /// Device has been verified via cross-signing or SAS emoji
    Verified,
    /// Device is known but not yet verified
    Unverified,
    /// Device was manually blocked by the user
    Blocked,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sas_emoji_representation() {
        let emojis = vec![
            SasEmoji {
                symbol: "🐶".to_string(),
                description: "Dog".to_string(),
            },
            SasEmoji {
                symbol: "🚗".to_string(),
                description: "Car".to_string(),
            },
        ];
        let state = VerificationState::ShowEmojis {
            transaction_id: "tx123".to_string(),
            emojis,
        };
        match state {
            VerificationState::ShowEmojis { emojis, .. } => {
                assert_eq!(emojis.len(), 2);
                assert_eq!(emojis[0].symbol, "🐶");
            }
            _ => panic!("Expected ShowEmojis"),
        }
    }
}
