//! Application coordinator managing matrix client, sync loop, and UI state

use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use matrix_call::CallSession;
use matrix_core::{
    CryptoStatus, DeviceInfo, FileSessionStore, MatrixClient, RoomEncryptionInfo, SessionStore,
    SyncEvent, SyncService, VerificationState,
};

use std::sync::Mutex as StdMutex;

use crate::desktop::NotificationService;
use crate::settings::AppSettings;
use crate::ui::MainWindowState;

/// Central desktop application controller
pub struct MatrixusApp {
    pub client: Arc<Mutex<Option<MatrixClient>>>,
    pub session_store: Arc<FileSessionStore>,
    pub notifications: NotificationService,
    pub state: Arc<Mutex<MainWindowState>>,
    /// User preferences (theme, notifications, tray, …)
    pub settings: Arc<StdMutex<AppSettings>>,
    /// Broadcast sender for sync events (cloned by subscribers)
    sync_tx: Arc<Mutex<Option<broadcast::Sender<SyncEvent>>>>,
    /// Background sync task handle
    sync_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl MatrixusApp {
    pub fn new() -> Self {
        let home_dir = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let session_path = std::path::PathBuf::from(home_dir)
            .join(".local/share/matrixus/session.json");

        let settings = AppSettings::load();
        let notifications = NotificationService::new();
        notifications.set_messages_enabled(settings.notifications_enabled);
        notifications.set_calls_enabled(settings.call_notifications_enabled);

        Self {
            client: Arc::new(Mutex::new(None)),
            session_store: Arc::new(FileSessionStore::new(session_path)),
            notifications,
            state: Arc::new(Mutex::new(MainWindowState::new())),
            settings: Arc::new(StdMutex::new(settings)),
            sync_tx: Arc::new(Mutex::new(None)),
            sync_handle: Arc::new(Mutex::new(None)),
        }
    }

    /// Try restoring an existing session on startup
    pub async fn try_auto_login(&self) -> bool {
        match self.session_store.load_session().await {
            Ok(Some(session)) => {
                info!("Found stored session for {}", session.user_id);
                match MatrixClient::new(session.homeserver_url.as_str(), None).await {
                    Ok(client) => {
                        if let Err(e) = client.restore_session(&session).await {
                            error!("Failed to restore session: {e}");
                            return false;
                        }
                        *self.client.lock().await = Some(client);
                        true
                    }
                    Err(e) => {
                        error!("Failed to initialize client: {e}");
                        false
                    }
                }
            }
            _ => false,
        }
    }

    /// Authenticate with credentials and store session
    pub async fn login(&self, homeserver: &str, user: &str, pass: &str) -> anyhow::Result<()> {
        let client = MatrixClient::new(homeserver, None).await?;
        let session = client.login_with_password(user, pass).await?;
        self.session_store.save_session(&session).await?;
        *self.client.lock().await = Some(client);
        info!("Logged in successfully as {user}");
        Ok(())
    }

    /// Whether a Matrix client session is currently active.
    pub async fn is_logged_in(&self) -> bool {
        let guard = self.client.lock().await;
        guard.as_ref().map(|c| c.is_logged_in()).unwrap_or(false)
    }

    /// Synchronous check used by the GUI on startup (client already set by try_auto_login).
    pub fn has_client(&self) -> bool {
        self.client
            .try_lock()
            .map(|g| g.is_some())
            .unwrap_or(false)
    }

    /// Start the background sync loop if a client is available and sync is not already running.
    ///
    /// Safe to call multiple times; subsequent calls are no-ops while sync is active.
    pub async fn start_sync(&self) {
        // Already running?
        {
            let handle = self.sync_handle.lock().await;
            if let Some(h) = handle.as_ref() {
                if !h.is_finished() {
                    info!("Sync already running — skipping start");
                    return;
                }
            }
        }

        let client = {
            let guard = self.client.lock().await;
            match guard.as_ref() {
                Some(c) => c.clone(),
                None => {
                    warn!("Cannot start sync: not logged in");
                    return;
                }
            }
        };

        let (service, _rx) = SyncService::new(client);
        let tx = service.event_sender_clone();
        *self.sync_tx.lock().await = Some(tx);

        let handle = tokio::spawn(async move {
            if let Err(e) = service.run().await {
                error!("Sync service exited with error: {e}");
            }
        });

        *self.sync_handle.lock().await = Some(handle);
        info!("Background sync service started");
    }

    /// Subscribe to sync events for the UI layer.
    ///
    /// Returns `None` if sync has not been started yet.
    pub async fn subscribe_sync(&self) -> Option<broadcast::Receiver<SyncEvent>> {
        self.sync_tx
            .lock()
            .await
            .as_ref()
            .map(|tx| tx.subscribe())
    }

    /// Stop the background sync loop (best-effort).
    pub async fn stop_sync(&self) {
        if let Some(handle) = self.sync_handle.lock().await.take() {
            handle.abort();
            info!("Background sync service stopped");
        }
        *self.sync_tx.lock().await = None;
    }

    /// Run a single sync cycle so the room list is populated, then return room summaries.
    ///
    /// Updates `MainWindowState.sidebar` with the fetched rooms.
    pub async fn fetch_rooms(&self) -> anyhow::Result<Vec<matrix_core::RoomSummary>> {
        let client_guard = self.client.lock().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        // One-shot sync so joined_rooms() is populated after a fresh login.
        info!("Running one-shot sync to load rooms…");
        if let Err(e) = client.sync_once().await {
            // Non-fatal: still try to list whatever is already cached.
            tracing::warn!("One-shot sync failed (rooms may be incomplete): {e}");
        }

        let rooms = client.list_rooms().await;
        info!("Fetched {} room(s)", rooms.len());

        // Keep presentation state in sync
        drop(client_guard);
        {
            let mut state = self.state.lock().await;
            state.sidebar.update_rooms(rooms.clone());
        }

        Ok(rooms)
    }

    /// List rooms from the local SDK cache without running a network sync.
    ///
    /// Prefer this for UI refreshes driven by the continuous sync loop.
    pub async fn refresh_rooms(&self) -> anyhow::Result<Vec<matrix_core::RoomSummary>> {
        let client_guard = self.client.lock().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        let rooms = client.list_rooms().await;
        drop(client_guard);
        {
            let mut state = self.state.lock().await;
            state.sidebar.update_rooms(rooms.clone());
        }
        Ok(rooms)
    }

    /// Record that the user opened a room (updates presentation state only).
    pub async fn open_room(&self, room_id: &str) {
        let mut state = self.state.lock().await;
        state.open_room(room_id);
        info!("Opened room {room_id}");
    }

    /// Load recent timeline events for a room and store them in presentation state.
    pub async fn load_timeline(
        &self,
        room_id: &str,
        limit: u32,
    ) -> anyhow::Result<Vec<matrix_core::TimelineEvent>> {
        let client_guard = self.client.lock().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        let events = client.fetch_timeline(room_id, limit).await?;

        drop(client_guard);
        {
            let mut state = self.state.lock().await;
            state.timeline = Some(crate::ui::TimelineState {
                room_id: room_id.to_string(),
                events: events.clone(),
            });
        }

        Ok(events)
    }

    /// Send a plain-text message to the given room.
    pub async fn send_message(&self, room_id: &str, text: &str) -> anyhow::Result<String> {
        let client_guard = self.client.lock().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        let event_id = client.send_text_message(room_id, text).await?;
        info!("Sent message to {room_id}: {event_id}");
        Ok(event_id)
    }

    /// Initiate an outgoing call in a room. Returns a controller ready for the call window.
    pub async fn start_call(
        &self,
        room_id: &str,
        is_video: bool,
    ) -> anyhow::Result<crate::ui::CallViewController> {
        let client_guard = self.client.lock().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        // Refuse a second concurrent call for simplicity
        {
            let state = self.state.lock().await;
            if state.active_call.is_some() {
                anyhow::bail!("A call is already in progress");
            }
        }

        let user_id = client.user_id().unwrap_or_default();
        let device_id = client.device_id().unwrap_or_default();
        let call_id = format!(
            "call_{}_{}",
            room_id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis()
        );

        info!(
            "Starting {} call in {room_id}: {call_id}",
            if is_video { "video" } else { "voice" }
        );
        let mut session = CallSession::new(&call_id, room_id, is_video);
        let _ = session.start_ringing();
        let controller = crate::ui::CallViewController::new(session, &user_id, &device_id);

        {
            let mut state = self.state.lock().await;
            // Store a clone-ish snapshot is hard without Clone on controller;
            // we move ownership to the caller and keep room_id marker via active_call
            // by reconstructing a lightweight session marker.
            state.active_call = Some(crate::ui::CallViewController::new(
                CallSession::new(&call_id, room_id, is_video),
                &user_id,
                &device_id,
            ));
        }

        Ok(controller)
    }

    /// Initiate an outgoing video call in a room.
    pub async fn start_video_call(
        &self,
        room_id: &str,
    ) -> anyhow::Result<crate::ui::CallViewController> {
        self.start_call(room_id, true).await
    }

    /// Initiate an outgoing voice call in a room.
    pub async fn start_voice_call(
        &self,
        room_id: &str,
    ) -> anyhow::Result<crate::ui::CallViewController> {
        self.start_call(room_id, false).await
    }

    /// Accept an incoming call for `room_id` and return a controller for the call window.
    pub async fn accept_incoming_call(
        &self,
        room_id: &str,
        is_video: bool,
    ) -> anyhow::Result<crate::ui::CallViewController> {
        self.start_call(room_id, is_video).await
    }

    /// Clear the active call marker after hang-up.
    pub async fn clear_active_call(&self) {
        let mut state = self.state.lock().await;
        state.active_call = None;
        info!("Active call cleared");
    }

    /// Whether a call is currently marked active.
    pub async fn has_active_call(&self) -> bool {
        self.state.lock().await.active_call.is_some()
    }

    /// Notify the user of an incoming call (Accept / Decline UI).
    pub fn notify_incoming_call(
        &self,
        room_id: &str,
        caller_name: &str,
        room_name: &str,
        is_video: bool,
    ) {
        self.notifications.show_incoming_call_notification(
            room_id, caller_name, room_name, is_video,
        );
    }

    // ── Encryption & verification (Phase 5) ─────────────────────────────────

    /// Current cross-signing / key-backup / device-trust snapshot.
    pub async fn crypto_status(&self) -> anyhow::Result<CryptoStatus> {
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        Ok(client.crypto_status().await)
    }

    /// Devices belonging to the logged-in user.
    pub async fn list_own_devices(&self) -> anyhow::Result<Vec<DeviceInfo>> {
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        Ok(client.list_own_devices().await?)
    }

    /// Encryption details for a room (lock icon, etc.).
    pub async fn room_encryption_info(
        &self,
        room_id: &str,
    ) -> anyhow::Result<RoomEncryptionInfo> {
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        Ok(client.room_encryption_info(room_id).await?)
    }

    /// Start SAS verification with one of our other devices.
    pub async fn start_device_verification(
        &self,
        other_device_id: &str,
    ) -> anyhow::Result<VerificationState> {
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        Ok(client.start_device_verification(other_device_id).await?)
    }

    pub async fn get_sas_emojis(
        &self,
        other_user: &str,
        transaction_id: &str,
    ) -> anyhow::Result<Option<Vec<matrix_core::SasEmoji>>> {
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        Ok(client.get_sas_emojis(other_user, transaction_id).await?)
    }

    pub async fn confirm_sas(
        &self,
        other_user: &str,
        transaction_id: &str,
    ) -> anyhow::Result<VerificationState> {
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        Ok(client.confirm_sas(other_user, transaction_id).await?)
    }

    pub async fn cancel_verification(
        &self,
        other_user: &str,
        transaction_id: &str,
    ) -> anyhow::Result<VerificationState> {
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        Ok(client.cancel_verification(other_user, transaction_id).await?)
    }

    pub async fn accept_verification(
        &self,
        other_user: &str,
        transaction_id: &str,
    ) -> anyhow::Result<VerificationState> {
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        Ok(client.accept_verification(other_user, transaction_id).await?)
    }
}



impl Default for MatrixusApp {
    fn default() -> Self {
        Self::new()
    }
}
