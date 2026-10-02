//! Application coordinator managing matrix client, sync loop, and UI state

use std::sync::Arc;
use tokio::sync::
{broadcast, Mutex};
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
    /// Base directory for session + crypto store (`~/.local/share/matrixus`).
    fn data_dir() -> std::path::PathBuf {
        let home_dir = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        std::path::PathBuf::from(home_dir).join(".local/share/matrixus")
    }

    pub fn new() -> Self {
        let data_dir = Self::data_dir();
        // Ensure the directory exists so sqlite_store and session.json can be written.
        let _ = std::fs::create_dir_all(&data_dir);
        let session_path = data_dir.join("session.json");

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
                // Pass a persistent data dir so the Olm account + OTKs survive restarts.
                match MatrixClient::new(session.homeserver_url.as_str(), Some(Self::data_dir())).await {
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
        // Persistent crypto store is required — without it the SDK re-uploads
        // the same one-time key IDs on every run and the homeserver returns 400.
        let client = MatrixClient::new(homeserver, Some(Self::data_dir())).await?;
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

        let (service, _rx) = SyncService::new(client.clone());
        let tx = service.event_sender_clone();
        // Surface incoming verification requests (to-device + in-room) to the UI.
        client.register_verification_handlers(tx.clone());
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

    /// Create an encrypted private group chat. Returns the new room ID.
    pub async fn create_group(
        &self,
        name: &str,
        topic: Option<&str>,
        invite: &[String],
    ) -> anyhow::Result<String> {
        let client_guard = self.client.lock().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let room_id = client.create_group(name, topic, invite).await?;
        drop(client_guard);
        info!("Created group {room_id}");
        // Refresh local room list so the sidebar picks it up immediately.
        let _ = self.refresh_rooms().await;
        Ok(room_id)
    }

    /// Create a Matrix Space. Returns the new room ID.
    pub async fn create_space(
        &self,
        name: &str,
        topic: Option<&str>,
        invite: &[String],
        is_public: bool,
    ) -> anyhow::Result<String> {
        let client_guard = self.client.lock().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let room_id = client
            .create_space(name, topic, invite, is_public)
            .await?;
        drop(client_guard);
        info!("Created space {room_id}");
        let _ = self.refresh_rooms().await;
        Ok(room_id)
    }

    /// Record that the user opened a room (updates presentation state only).
    pub async fn open_room(&self, room_id: &str) {
        let mut state = self.state.lock().await;
        state.open_room(room_id);
        info!("Opened room {room_id}");
    }

    /// Load recent timeline events for a room and store them in presentation state.
    ///
    /// Starts at the end of the timeline (most recent). Returns the page of
    /// events (oldest first) plus the token needed to load older history.
    pub async fn load_timeline(
        &self,
        room_id: &str,
        limit: u32,
    ) -> anyhow::Result<matrix_core::TimelinePage> {
        let client_guard = self.client.lock().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        let page = client.fetch_timeline(room_id, limit, None).await?;

        drop(client_guard);
        {
            let mut state = self.state.lock().await;
            let reached_start = page.end_token.is_none();
            state.timeline = Some(crate::ui::TimelineState {
                room_id: room_id.to_string(),
                events: page.events.clone(),
                prev_batch: page.end_token.clone(),
                reached_start,
            });
        }

        Ok(page)
    }

    /// Load older messages before the currently displayed history.
    ///
    /// Uses the stored `prev_batch` token. Returns an empty page when there is
    /// no more history or when a token is not available yet. When a request is
    /// made and the server returns no further `end` token, `reached_start` is set.
    /// Returns `(page, fetched)` where `fetched` is true only when a
    /// `/messages` request was actually issued.
    pub async fn load_earlier_messages(
        &self,
        room_id: &str,
        limit: u32,
    ) -> anyhow::Result<(matrix_core::TimelinePage, bool)> {
        let from_token = {
            let state = self.state.lock().await;
            match state.timeline.as_ref() {
                Some(tl) if tl.room_id == room_id => {
                    if tl.reached_start {
                        return Ok((
                            matrix_core::TimelinePage {
                                events: Vec::new(),
                                end_token: None,
                            },
                            false,
                        ));
                    }
                    tl.prev_batch.clone()
                }
                _ => None,
            }
        };

        let Some(from) = from_token else {
            // No token: initial load still pending, or start already reached.
            return Ok((
                matrix_core::TimelinePage {
                    events: Vec::new(),
                    end_token: None,
                },
                false,
            ));
        };

        let client_guard = self.client.lock().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        let page = client
            .fetch_timeline(room_id, limit, Some(&from))
            .await?;

        drop(client_guard);
        {
            let mut state = self.state.lock().await;
            if let Some(tl) = state.timeline.as_mut() {
                if tl.room_id == room_id {
                    // Prepend older events (page is already oldest-first).
                    let mut combined = page.events.clone();
                    combined.append(&mut tl.events);
                    tl.events = combined;
                    tl.prev_batch = page.end_token.clone();
                    if page.end_token.is_none() {
                        tl.reached_start = true;
                    }
                }
            }
        }

        Ok((page, true))
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

    /// React to a message with an emoji key (sends `m.reaction`).
    pub async fn send_reaction(
        &self,
        room_id: &str,
        event_id: &str,
        key: &str,
    ) -> anyhow::Result<String> {
        let client_guard = self.client.lock().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let reaction_id = client.send_reaction(room_id, event_id, key).await?;
        info!("Sent reaction {key} on {event_id} in {room_id}: {reaction_id}");
        Ok(reaction_id)
    }

    /// Remove a reaction we previously sent (redacts the `m.reaction` event).
    pub async fn remove_reaction(
        &self,
        room_id: &str,
        reaction_event_id: &str,
    ) -> anyhow::Result<()> {
        let client_guard = self.client.lock().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        client.remove_reaction(room_id, reaction_event_id).await?;
        info!("Removed reaction {reaction_event_id} in {room_id}");
        Ok(())
    }

    /// Toggle a reaction: add if not present, remove if we already reacted with this key.
    ///
    /// Returns `Ok(Some(new_event_id))` when added, `Ok(None)` when removed.
    pub async fn toggle_reaction(
        &self,
        room_id: &str,
        target_event_id: &str,
        key: &str,
        my_reaction_event_id: Option<&str>,
    ) -> anyhow::Result<Option<String>> {
        let client_guard = self.client.lock().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        let result = client
            .toggle_reaction(room_id, target_event_id, key, my_reaction_event_id)
            .await?;
        match &result {
            Some(id) => info!("Toggled reaction on: {key} -> {id}"),
            None => info!("Toggled reaction off: {key}"),
        }
        Ok(result)
    }

    /// Download avatar / media bytes for an MXC URI.
    pub async fn download_media(
        &self,
        mxc_uri: &str,
        thumbnail: bool,
    ) -> anyhow::Result<Vec<u8>> {
        let client_guard = self.client.lock().await;
        let client = client_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
        Ok(client.download_media(mxc_uri, thumbnail).await?)
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

    /// Drive SAS until emojis are ready (accept + key exchange via SDK streams).
    /// Clones the client so the mutex is not held for the duration of the wait.
    pub async fn wait_for_sas_emojis(
        &self,
        other_user: &str,
        transaction_id: &str,
    ) -> anyhow::Result<VerificationState> {
        let client = {
            let guard = self.client.lock().await;
            guard
                .as_ref()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Not logged in"))?
        };
        Ok(client.wait_for_sas_emojis(other_user, transaction_id).await?)
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
        // Clone so we don't hold the mutex across the potentially long SAS wait.
        let client = {
            let guard = self.client.lock().await;
            guard
                .as_ref()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Not logged in"))?
        };
        Ok(client.accept_verification(other_user, transaction_id).await?)
    }
}




impl Default for MatrixusApp {
    fn default() -> Self {
        Self::new()
    }
}
