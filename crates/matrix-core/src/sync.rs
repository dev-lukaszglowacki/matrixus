//! Matrix sync loop and real-time event dispatcher

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::broadcast;
use tracing::{info, warn};

use matrix_sdk::config::SyncSettings;

use crate::client::MatrixClient;
use crate::error::Result;
use crate::crypto::VerificationState;
use crate::room::TimelineEvent;

/// Broadcast event types emitted by the sync engine to the UI layer
#[derive(Debug, Clone)]
pub enum SyncEvent {
    /// Room list updated (new room, rename, unread counts, previews)
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
    /// Device verification flow advanced (request, emojis, done, cancelled)
    VerificationChanged(VerificationState),
    /// Connection to the homeserver was lost / sync error
    ConnectionLost {
        message: String,
    },
    /// Connection restored after a previous error
    ConnectionRestored,
    /// Generic sync error (legacy / detailed)
    SyncError(String),
}

/// Service managing the background matrix sync stream
pub struct SyncService {
    client: MatrixClient,
    event_sender: broadcast::Sender<SyncEvent>,
    running: Arc<AtomicBool>,
}

impl SyncService {
    /// Create a new sync service and a receiver for UI updates.
    pub fn new(client: MatrixClient) -> (Self, broadcast::Receiver<SyncEvent>) {
        let (tx, rx) = broadcast::channel(256);
        (
            Self {
                client,
                event_sender: tx,
                running: Arc::new(AtomicBool::new(false)),
            },
            rx,
        )
    }

    /// Clone of the broadcast sender so the app can hand out additional subscribers.
    pub fn event_sender_clone(&self) -> broadcast::Sender<SyncEvent> {
        self.event_sender.clone()
    }

    /// Subscribe an additional receiver to sync events.
    pub fn subscribe(&self) -> broadcast::Receiver<SyncEvent> {
        self.event_sender.subscribe()
    }

    /// Whether the sync loop is currently marked as running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Request the sync loop to stop after the current iteration.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Run the sync loop continuously until [`stop`] is called or the task is aborted.
    pub async fn run(&self) -> Result<()> {
        info!("Starting Matrix sync engine loop");
        self.running.store(true, Ordering::SeqCst);

        let mut sync_settings = SyncSettings::default().timeout(Duration::from_secs(30));
        let mut was_offline = false;

        while self.running.load(Ordering::SeqCst) {
            match self.client.inner().sync_once(sync_settings.clone()).await {
                Ok(response) => {
                    sync_settings = sync_settings.token(response.next_batch);

                    if was_offline {
                        info!("Matrix connection restored");
                        let _ = self.event_sender.send(SyncEvent::ConnectionRestored);
                        was_offline = false;
                    }

                    // Notify UI that room metadata / unread counts may have changed
                    let _ = self.event_sender.send(SyncEvent::RoomListUpdated);
                }
                Err(e) => {
                    warn!("Matrix sync error: {e}, retrying in 5s…");
                    was_offline = true;
                    let msg = e.to_string();
                    let _ = self.event_sender.send(SyncEvent::ConnectionLost {
                        message: msg.clone(),
                    });
                    let _ = self.event_sender.send(SyncEvent::SyncError(msg));
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }

        info!("Matrix sync engine loop stopped");
        Ok(())
    }
}
