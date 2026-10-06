//! Matrix Core Engine for Linux Desktop Client

pub mod client;
pub mod crypto;
pub mod error;
pub mod room;
pub mod session;
pub mod sync;

pub use client::MatrixClient;
pub use crypto::{
    sas_emoji_by_index, CryptoStatus, DeviceInfo, DeviceTrustLevel, RoomEncryptionInfo, SasEmoji,
    VerificationState,
};
pub use error::{MatrixError, Result};
pub use room::{
    CreateRoomOptions, EventContent, ReactionSummary, RoomSummary, TimelineEvent, TimelinePage,
};
pub use session::{FileSessionStore, MatrixSession, SessionStore};
pub use sync::{SyncEvent, SyncService};
