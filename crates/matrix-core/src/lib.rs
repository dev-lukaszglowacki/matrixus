//! Matrix Core Engine for Linux Desktop Client

pub mod client;
pub mod crypto;
pub mod error;
pub mod room;
pub mod session;
pub mod sync;

pub use client::MatrixClient;
pub use crypto::{DeviceTrustLevel, SasEmoji, VerificationState};
pub use error::{MatrixError, Result};
pub use room::{EventContent, RoomSummary, TimelineEvent};
pub use session::{FileSessionStore, MatrixSession, SessionStore};
pub use sync::{SyncEvent, SyncService};
