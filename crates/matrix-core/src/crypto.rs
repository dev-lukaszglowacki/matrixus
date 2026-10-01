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

impl DeviceTrustLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Verified => "Verified",
            Self::Unverified => "Unverified",
            Self::Blocked => "Blocked",
        }
    }

    pub fn icon_name(self) -> &'static str {
        match self {
            Self::Verified => "security-high-symbolic",
            Self::Unverified => "security-medium-symbolic",
            Self::Blocked => "security-low-symbolic",
        }
    }
}

/// Summary of a known device belonging to a user
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceInfo {
    pub user_id: String,
    pub device_id: String,
    pub display_name: Option<String>,
    pub trust: DeviceTrustLevel,
    /// Whether this is the local device
    pub is_own_device: bool,
}

/// Overall encryption / recovery status for the session
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CryptoStatus {
    /// Cross-signing keys are present and usable
    pub cross_signing_ready: bool,
    /// This device is signed by the user's master key
    pub device_cross_signed: bool,
    /// Server-side key backup is enabled
    pub key_backup_enabled: bool,
    /// Key backup is fully synced / recovered
    pub key_backup_synced: bool,
    /// Number of unverified devices for the current user
    pub unverified_own_devices: u32,
}

impl CryptoStatus {
    /// Short human-readable status line for the header / settings.
    pub fn summary_label(&self) -> String {
        if !self.cross_signing_ready {
            return "Cross-signing not set up".into();
        }
        if self.unverified_own_devices > 0 {
            return format!(
                "{} unverified device{}",
                self.unverified_own_devices,
                if self.unverified_own_devices == 1 {
                    ""
                } else {
                    "s"
                }
            );
        }
        if self.key_backup_enabled && self.key_backup_synced {
            return "Secure · Backup active".into();
        }
        if self.key_backup_enabled {
            return "Secure · Backup pending".into();
        }
        "Secure · No backup".into()
    }

    pub fn is_healthy(&self) -> bool {
        self.cross_signing_ready
            && self.device_cross_signed
            && self.unverified_own_devices == 0
    }
}

/// Room-level encryption presentation info
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoomEncryptionInfo {
    pub room_id: String,
    pub is_encrypted: bool,
    /// How many devices in the room are verified (best-effort)
    pub verified_devices: u32,
    pub total_devices: u32,
}

/// Standard SAS emoji set used by Matrix (subset for offline / mock flows).
/// Indices match the matrix-sdk / vodozemac ordering where possible.
pub fn sas_emoji_by_index(index: u8) -> SasEmoji {
    const TABLE: &[(&str, &str)] = &[
        ("🐶", "Dog"),
        ("🐱", "Cat"),
        ("🦁", "Lion"),
        ("🐎", "Horse"),
        ("🦄", "Unicorn"),
        ("🐷", "Pig"),
        ("🐘", "Elephant"),
        ("🐰", "Rabbit"),
        ("🐼", "Panda"),
        ("🐓", "Rooster"),
        ("🐧", "Penguin"),
        ("🐢", "Turtle"),
        ("🐟", "Fish"),
        ("🐙", "Octopus"),
        ("🦋", "Butterfly"),
        ("🌸", "Flower"),
        ("🌳", "Tree"),
        ("🌵", "Cactus"),
        ("🍄", "Mushroom"),
        ("🌍", "Globe"),
        ("🌙", "Moon"),
        ("☁️", "Cloud"),
        ("🔥", "Fire"),
        ("🍌", "Banana"),
        ("🍎", "Apple"),
        ("🍓", "Strawberry"),
        ("🌽", "Corn"),
        ("🍕", "Pizza"),
        ("🎂", "Cake"),
        ("❤️", "Heart"),
        ("😀", "Smiley"),
        ("🤖", "Robot"),
        ("🎩", "Hat"),
        ("👓", "Glasses"),
        ("🔧", "Spanner"),
        ("🎅", "Santa"),
        ("👍", "Thumbs Up"),
        ("☂️", "Umbrella"),
        ("⌛", "Hourglass"),
        ("⏰", "Clock"),
        ("🎁", "Gift"),
        ("💡", "Light Bulb"),
        ("📕", "Book"),
        ("✏️", "Pencil"),
        ("📎", "Paperclip"),
        ("🔑", "Key"),
        ("🔨", "Hammer"),
        ("📞", "Telephone"),
        ("🏁", "Flag"),
        ("🚂", "Train"),
        ("🚲", "Bicycle"),
        ("✈️", "Aeroplane"),
        ("🚀", "Rocket"),
        ("🏆", "Trophy"),
        ("⚽", "Ball"),
        ("🎸", "Guitar"),
        ("🎺", "Trumpet"),
        ("🔔", "Bell"),
        ("⚓", "Anchor"),
        ("🎧", "Headphones"),
        ("📁", "Folder"),
        ("📌", "Pin"),
    ];
    let (symbol, description) = TABLE
        .get(index as usize % TABLE.len())
        .copied()
        .unwrap_or(("❓", "Unknown"));
    SasEmoji {
        symbol: symbol.to_string(),
        description: description.to_string(),
    }
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

    #[test]
    fn test_crypto_status_summary() {
        let healthy = CryptoStatus {
            cross_signing_ready: true,
            device_cross_signed: true,
            key_backup_enabled: true,
            key_backup_synced: true,
            unverified_own_devices: 0,
        };
        assert!(healthy.is_healthy());
        assert!(healthy.summary_label().contains("Backup"));

        let unverified = CryptoStatus {
            cross_signing_ready: true,
            device_cross_signed: true,
            key_backup_enabled: false,
            key_backup_synced: false,
            unverified_own_devices: 2,
        };
        assert!(!unverified.is_healthy());
        assert!(unverified.summary_label().contains("unverified"));
    }
}
