//! Matrix sync loop and real-time event dispatcher

use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{info, warn};

use matrix_sdk::config::SyncSettings;

use crate::client::MatrixClient;
use crate::error::Result;
use crate::room::TimelineEvent;

/// Broadcast event types emitted by the sync engine to the UI layer
#[derive(Debug, Clone)]
pub enum SyncEvent {
    /// Room list updated (new room, rename, avatar change)
    RoomListUpdated,
    /// New message or event appended to a room's timeline
    TimelineUpdated {
        room_id: String,
        event: TimelineEvent,
    },
    /// MSC3401 Call membership changed (call started, participant joined/left)
    CallStateChanged {
        room_id: String,
        has_active_call: bool,
    },
    /// Sync error occurred
    SyncError(String),
}

/// Service managing the background matrix sync stream
pub struct SyncService {
    client: MatrixClient,
    event_sender: broadcast::Sender<SyncEvent>,
}

impl SyncService {
    pub fn new(client: MatrixClient) -> (Self, broadcast::Receiver<SyncEvent>) {
        let (tx, rx) = broadcast::channel(128);
        (
            Self {
                client,
                event_sender: tx,
            },
            rx,
        )
    }

    /// Run the sync loop continuously until cancelled
    pub async fn run(&self) -> Result<()> {
        info!("Starting Matrix sync engine loop");
        let mut sync_settings = SyncSettings::default().timeout(Duration::from_secs(30));

        loop {
            match self.client.inner().sync_once(sync_settings.clone()).await {
                Ok(response) => {
                    sync_settings = sync_settings.token(response.next_batch);
                    let _ = self.event_sender.send(SyncEvent::RoomListUpdated);
                }
                Err(e) => {
                    warn!("Matrix sync error: {e}, retrying in 5s...");
                    let _ = self.event_sender.send(SyncEvent::SyncError(e.to_string()));
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
}
