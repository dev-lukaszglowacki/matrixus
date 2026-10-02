//! High-level Matrix client wrapper handling session, authentication, and communication

use std::path::PathBuf;
use std::sync::Arc;
use url::Url;

use matrix_sdk::{
    authentication::{matrix::MatrixSession as SdkMatrixSession, SessionTokens},
    room::MessagesOptions,
    ruma::{
        events::{
            room::message::{MessageType, RoomMessageEventContent},
            room::MediaSource,
            SyncMessageLikeEvent,
        },
        OwnedDeviceId, RoomId, UserId,
    },
    store::RoomLoadSettings,
    Client, SessionMeta,
};
use tracing::{info, warn};

use crate::crypto::{
    CryptoStatus, DeviceInfo, DeviceTrustLevel, RoomEncryptionInfo, SasEmoji, VerificationState,
};
use crate::error::{MatrixError, Result};
use crate::room::{EventContent, RoomSummary, TimelineEvent};
use crate::session::MatrixSession;

/// Main Matrix client engine managing homeserver connection and state
#[derive(Clone)]
pub struct MatrixClient {
    inner: Arc<Client>,
}

impl MatrixClient {
    /// Create a new Matrix client instance for a homeserver URL
    pub async fn new(homeserver_url: &str, data_dir: Option<PathBuf>) -> Result<Self> {
        let url = Url::parse(homeserver_url)?;
        let mut builder = Client::builder().homeserver_url(url);

        if let Some(path) = data_dir {
            let sqlite_path = path.join("matrix_store.sqlite");
            builder = builder.sqlite_store(sqlite_path, None);
        }

        let inner = builder.build().await?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Access the underlying matrix-sdk Client
    pub fn inner(&self) -> &Client {
        &self.inner
    }

    /// Register handlers that surface incoming verification requests on `tx`.
    ///
    /// Call once after login / session restore so the UI can show the accept dialog
    /// when another device starts verification toward us.
    pub fn register_verification_handlers(
        &self,
        tx: tokio::sync::broadcast::Sender<crate::sync::SyncEvent>,
    ) {
        use matrix_sdk::ruma::events::{
            key::verification::request::ToDeviceKeyVerificationRequestEvent,
            room::message::{MessageType, OriginalSyncRoomMessageEvent},
        };

        let client = self.inner.clone();
        let tx1 = tx.clone();
        self.inner.add_event_handler(
            move |ev: ToDeviceKeyVerificationRequestEvent, c: Client| async move {
                let other_user = ev.sender.to_string();
                let transaction_id = ev.content.transaction_id.to_string();
                let other_device = ev.content.from_device.to_string();
                info!(
                    "Incoming to-device verification request from {other_user} / {other_device}"
                );
                // Ensure the request object exists in the crypto store.
                let _ = c
                    .encryption()
                    .get_verification_request(&ev.sender, &ev.content.transaction_id)
                    .await;
                let _ = tx1.send(crate::sync::SyncEvent::VerificationChanged(
                    VerificationState::Requested {
                        transaction_id,
                        other_user,
                        other_device,
                    },
                ));
            },
        );

        let tx2 = tx;
        self.inner.add_event_handler(
            move |ev: OriginalSyncRoomMessageEvent, c: Client| async move {
                if let MessageType::VerificationRequest(content) = &ev.content.msgtype {
                    let other_user = ev.sender.to_string();
                    let transaction_id = ev.event_id.to_string();
                    let other_device = content.from_device.to_string();
                    info!(
                        "Incoming in-room verification request from {other_user} / {other_device}"
                    );
                    let _ = c
                        .encryption()
                        .get_verification_request(&ev.sender, &ev.event_id)
                        .await;
                    let _ = tx2.send(crate::sync::SyncEvent::VerificationChanged(
                        VerificationState::Requested {
                            transaction_id,
                            other_user,
                            other_device,
                        },
                    ));
                }
            },
        );

        // Silence unused if Client import path differs in handler signature.
        let _ = client;
    }

    /// Check if client is currently authenticated
    pub fn is_logged_in(&self) -> bool {
        self.inner.matrix_auth().logged_in()
    }

    /// Current user ID if authenticated
    pub fn user_id(&self) -> Option<String> {
        self.inner.user_id().map(|u| u.to_string())
    }

    /// Current device ID if authenticated
    pub fn device_id(&self) -> Option<String> {
        self.inner.device_id().map(|d| d.to_string())
    }

    /// Authenticate using username and password
    pub async fn login_with_password(
        &self,
        username: &str,
        password: &str,
    ) -> Result<MatrixSession> {
        info!("Logging in user: {username}");
        let response = self
            .inner
            .matrix_auth()
            .login_username(username, password)
            .initial_device_display_name("Matrixus")
            .send()
            .await
            .map_err(|e| MatrixError::Authentication(format!("Login failed: {e}")))?;

        let session = MatrixSession::new(
            self.inner.homeserver().into(),
            response.user_id.to_string(),
            response.device_id.to_string(),
            response.access_token,
        );

        Ok(session)
    }

    /// Restore a previously saved session
    pub async fn restore_session(&self, session: &MatrixSession) -> Result<()> {
        info!("Restoring session for user: {}", session.user_id);
        let user_id = UserId::parse(&session.user_id)
            .map_err(|e| MatrixError::Session(format!("Invalid user ID: {e}")))?;
        let device_id = OwnedDeviceId::from(session.device_id.as_str());

        let sdk_session = SdkMatrixSession {
            meta: SessionMeta {
                user_id: user_id.to_owned(),
                device_id,
            },
            tokens: SessionTokens {
                access_token: session.access_token.clone(),
                refresh_token: session.refresh_token.clone(),
            },
        };

        self.inner
            .matrix_auth()
            .restore_session(sdk_session, RoomLoadSettings::default())
            .await
            .map_err(|e| MatrixError::Session(format!("Failed to restore session: {e}")))?;

        Ok(())
    }

    /// Log out the current session
    pub async fn logout(&self) -> Result<()> {
        info!("Logging out current session");
        if self.is_logged_in() {
            self.inner
                .matrix_auth()
                .logout()
                .await
                .map_err(|e| MatrixError::Authentication(format!("Logout failed: {e}")))?;
        }
        Ok(())
    }

    /// Run a single sync cycle. Useful after login to populate the room list
    /// before starting the long-running SyncService.
    pub async fn sync_once(&self) -> Result<()> {
        use matrix_sdk::config::SyncSettings;
        use std::time::Duration;

        info!("Running one-shot Matrix sync");
        let settings = SyncSettings::default().timeout(Duration::from_secs(15));
        self.inner
            .sync_once(settings)
            .await
            .map_err(|e| MatrixError::Other(format!("sync_once failed: {e}")))?;
        Ok(())
    }

    /// List all joined rooms with summary details
    pub async fn list_rooms(&self) -> Vec<RoomSummary> {
        let rooms = self.inner.joined_rooms();
        let mut summaries = Vec::new();

        for room in rooms {
            let room_id = room.room_id().to_string();
            let name = room.display_name().await.map(|n| n.to_string()).unwrap_or_else(|_| room_id.clone());
            let topic = room.topic();
            let avatar_url = room.avatar_url().map(|u| u.to_string());
            let is_direct = room.is_direct().await.unwrap_or(false);
            let is_encrypted = room
                .latest_encryption_state()
                .await
                .map(|s| s.is_encrypted())
                .unwrap_or(false);
            let unread_notifications = room.unread_notification_counts().notification_count;

            summaries.push(RoomSummary {
                room_id,
                name,
                topic,
                avatar_url,
                is_direct,
                is_encrypted,
                unread_notifications,
                has_active_call: false,
                last_event: None,
            });
        }

        summaries
    }

    /// Fetch a page of timeline events for a room (oldest first).
    ///
    /// Uses `/messages` with backward pagination. When `from` is `None`,
    /// pagination starts at the end of the accessible timeline (most recent).
    /// When `from` is set (previous page's `end_token`), older events are loaded.
    ///
    /// `end_token` in the result is the token for the next older page, or `None`
    /// when the start of history has been reached.
    pub async fn fetch_timeline(
        &self,
        room_id: &str,
        limit: u32,
        from: Option<&str>,
    ) -> Result<crate::room::TimelinePage> {
        let room_id_parsed = <&RoomId>::try_from(room_id)
            .map_err(|e| MatrixError::RoomNotFound(format!("Invalid room ID: {e}")))?;

        let room = self.inner.get_room(room_id_parsed).ok_or_else(|| {
            MatrixError::RoomNotFound(format!("Room not found: {room_id}"))
        })?;

        let mut options = MessagesOptions::backward().from(from);
        options.limit = limit.into();

        info!(
            "Fetching up to {limit} messages for {room_id} (from={})",
            from.unwrap_or("<end>")
        );
        let response = room
            .messages(options)
            .await
            .map_err(|e| MatrixError::Other(format!("Failed to fetch messages: {e}")))?;

        let mut events = Vec::new();
        for sdk_event in &response.chunk {
            match map_timeline_event(sdk_event) {
                Some(ev) => events.push(ev),
                None => {
                    // Skip state events, undecryptable, unsupported types, etc.
                }
            }
        }

        // `/messages` backward returns newest-first; reverse so oldest is first.
        events.reverse();
        let end_token = response.end;
        info!(
            "Mapped {} timeline event(s) for {room_id}; end_token={}",
            events.len(),
            end_token.as_deref().unwrap_or("<none>")
        );
        Ok(crate::room::TimelinePage { events, end_token })
    }

    /// Send a plain text message to a room
    pub async fn send_text_message(&self, room_id: &str, text: &str) -> Result<String> {
        let room_id = <&RoomId>::try_from(room_id)
            .map_err(|e| MatrixError::RoomNotFound(format!("Invalid room ID: {e}")))?;

        let room = self
            .inner
            .get_room(room_id)
            .ok_or_else(|| MatrixError::RoomNotFound(format!("Room not found: {room_id}")))?;

        let content = RoomMessageEventContent::text_plain(text);
        let response = room.send(content).await?;

        Ok(response.response.event_id.to_string())
    }

    /// Send a markdown-formatted message to a room
    pub async fn send_markdown_message(&self, room_id: &str, markdown: &str) -> Result<String> {
        let room_id = <&RoomId>::try_from(room_id)
            .map_err(|e| MatrixError::RoomNotFound(format!("Invalid room ID: {e}")))?;

        let room = self
            .inner
            .get_room(room_id)
            .ok_or_else(|| MatrixError::RoomNotFound(format!("Room not found: {room_id}")))?;

        let content = RoomMessageEventContent::text_markdown(markdown);
        let response = room.send(content).await?;

        Ok(response.response.event_id.to_string())
    }

    // ── Encryption & verification (Phase 5) ─────────────────────────────────

    /// Snapshot of cross-signing, device trust, and key-backup state.
    ///
    /// Uses best-effort SDK probes so the UI always gets a usable status object
    /// even when some encryption features are not fully configured yet.
    pub async fn crypto_status(&self) -> CryptoStatus {
        let encryption = self.inner.encryption();

        let cross_signing_status = encryption.cross_signing_status().await;
        let cross_signing_ready = cross_signing_status
            .as_ref()
            .map(|s| s.is_complete())
            .unwrap_or(false);

        let mut unverified_own_devices = 0u32;
        let mut device_cross_signed = false;

        if let Some(user_id) = self.inner.user_id() {
            match encryption.get_user_devices(user_id).await {
                Ok(devices) => {
                    let own_device = self.inner.device_id().map(|d| d.to_string());
                    for device in devices.devices() {
                        let verified = device.is_verified();
                        let is_own = own_device
                            .as_ref()
                            .map(|id| device.device_id().as_str() == id)
                            .unwrap_or(false);
                        if is_own {
                            device_cross_signed = verified;
                        } else if !verified {
                            unverified_own_devices = unverified_own_devices.saturating_add(1);
                        }
                    }
                }
                Err(e) => {
                    warn!("crypto_status: get_user_devices failed: {e}");
                }
            }
        }

        // Key backup probe: matrix-sdk 0.19 does not expose backup_exists_on_server
        // on Encryption; keep a conservative default until a stable API is used.
        let key_backup_enabled = false;
        let key_backup_synced = false;
        let _ = &encryption; // silence unused when other probes are absent

        CryptoStatus {
            cross_signing_ready,
            device_cross_signed,
            key_backup_enabled,
            key_backup_synced,
            unverified_own_devices,
        }
    }

    /// List devices for the current user (own devices).
    pub async fn list_own_devices(&self) -> Result<Vec<DeviceInfo>> {
        let user_id = self
            .inner
            .user_id()
            .ok_or_else(|| MatrixError::Authentication("Not logged in".into()))?;
        let own_device_id = self.inner.device_id().map(|d| d.to_string());

        let devices = self
            .inner
            .encryption()
            .get_user_devices(user_id)
            .await
            .map_err(|e| MatrixError::Other(format!("Failed to list devices: {e}")))?;

        let mut out = Vec::new();
        for device in devices.devices() {
            let device_id = device.device_id().to_string();
            let is_own = own_device_id.as_ref() == Some(&device_id);
            let trust = if device.is_verified() {
                DeviceTrustLevel::Verified
            } else {
                // Blacklist API name varies; treat non-verified as Unverified
                DeviceTrustLevel::Unverified
            };
            out.push(DeviceInfo {
                user_id: user_id.to_string(),
                device_id,
                display_name: device.display_name().map(|s| s.to_string()),
                trust,
                is_own_device: is_own,
            });
        }
        Ok(out)
    }

    /// Encryption status for a specific room.
    pub async fn room_encryption_info(&self, room_id: &str) -> Result<RoomEncryptionInfo> {
        let room_id_parsed = <&RoomId>::try_from(room_id)
            .map_err(|e| MatrixError::RoomNotFound(format!("Invalid room ID: {e}")))?;
        let room = self.inner.get_room(room_id_parsed).ok_or_else(|| {
            MatrixError::RoomNotFound(format!("Room not found: {room_id}"))
        })?;

        let is_encrypted = room
            .latest_encryption_state()
            .await
            .map(|s| s.is_encrypted())
            .unwrap_or(false);

        Ok(RoomEncryptionInfo {
            room_id: room_id.to_string(),
            is_encrypted,
            // Full member-device enumeration is expensive; UI can show lock only.
            verified_devices: 0,
            total_devices: 0,
        })
    }

    /// Request an outgoing SAS verification with another of the current user's devices.
    ///
    /// Returns `Started` immediately. Call [`wait_for_sas_emojis`] afterwards — it drives
    /// the SAS state machine (accept + key exchange) via the SDK change stream until
    /// emojis are ready or the flow is cancelled/times out.
    pub async fn start_device_verification(
        &self,
        other_device_id: &str,
    ) -> Result<VerificationState> {
        let user_id = self
            .inner
            .user_id()
            .ok_or_else(|| MatrixError::Authentication("Not logged in".into()))?;

        let device = self
            .inner
            .encryption()
            .get_device(user_id, other_device_id.into())
            .await
            .map_err(|e| MatrixError::Other(format!("get_device failed: {e}")))?
            .ok_or_else(|| MatrixError::Other(format!("Device {other_device_id} not found")))?;

        let sas = device
            .request_verification()
            .await
            .map_err(|e| MatrixError::Other(format!("request_verification failed: {e}")))?;

        let tx_id = sas.flow_id().to_string();
        info!("Started verification with device {other_device_id}: {tx_id}");

        // Emojis are only available after the other side accepts and keys are exchanged.
        Ok(VerificationState::Started {
            transaction_id: tx_id,
        })
    }

    /// Drive an in-progress SAS verification until emojis are ready (or timeout/cancel).
    ///
    /// Uses the SDK `SasVerification::changes()` stream so we correctly accept the flow
    /// and wait for `KeysExchanged` instead of blind-polling.
    pub async fn wait_for_sas_emojis(
        &self,
        other_user: &str,
        transaction_id: &str,
    ) -> Result<VerificationState> {
        use futures_util::StreamExt;
        use matrix_sdk::encryption::verification::SasState;
        use matrix_sdk::ruma::UserId;
        use std::time::Duration;

        let user_id = <&UserId>::try_from(other_user)
            .map_err(|e| MatrixError::Other(format!("Invalid user id: {e}")))?;

        // Allow up to 2 minutes for the other device to accept and complete key exchange.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(120);

        loop {
            if tokio::time::Instant::now() > deadline {
                return Err(MatrixError::Other(
                    "Timed out waiting for SAS emojis — did the other device accept?".into(),
                ));
            }

            let Some(verification) = self
                .inner
                .encryption()
                .get_verification(user_id, transaction_id)
                .await
            else {
                tokio::time::sleep(Duration::from_millis(400)).await;
                continue;
            };

            let Some(sas) = verification.sas() else {
                // Still a bare VerificationRequest — keep waiting for transition to SAS.
                tokio::time::sleep(Duration::from_millis(400)).await;
                continue;
            };

            // Already presentable?
            if let Some(emoji_list) = sas.emoji() {
                let emojis = emoji_list
                    .iter()
                    .map(|e| SasEmoji {
                        symbol: e.symbol.to_string(),
                        description: e.description.to_string(),
                    })
                    .collect();
                return Ok(VerificationState::ShowEmojis {
                    transaction_id: transaction_id.to_string(),
                    emojis,
                });
            }

            // Accept if we haven't yet (required on the responder; harmless if already accepted).
            if let Err(e) = sas.accept().await {
                warn!("sas.accept() note: {e}");
            }

            // Listen for state changes until KeysExchanged / Done / Cancelled.
            let mut stream = sas.changes();
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let result = tokio::time::timeout(remaining, async {
                while let Some(state) = stream.next().await {
                    match state {
                        SasState::KeysExchanged { emojis, .. } => {
                            if let Some(list) = emojis {
                                let mapped = list
                                    .emojis
                                    .iter()
                                    .map(|e| SasEmoji {
                                        symbol: e.symbol.to_string(),
                                        description: e.description.to_string(),
                                    })
                                    .collect();
                                return Ok(VerificationState::ShowEmojis {
                                    transaction_id: transaction_id.to_string(),
                                    emojis: mapped,
                                });
                            }
                        }
                        SasState::Done { .. } => {
                            return Ok(VerificationState::Done {
                                transaction_id: transaction_id.to_string(),
                            });
                        }
                        SasState::Cancelled(info) => {
                            return Ok(VerificationState::Cancelled {
                                transaction_id: transaction_id.to_string(),
                                reason: info.reason().to_string(),
                            });
                        }
                        // Created / Started / Accepted / Confirmed — keep waiting
                        _ => {}
                    }
                }
                Err(MatrixError::Other("SAS change stream ended unexpectedly".into()))
            })
            .await;

            match result {
                Ok(Ok(state)) => return Ok(state),
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    return Err(MatrixError::Other(
                        "Timed out waiting for SAS emojis — did the other device accept?".into(),
                    ));
                }
            }
        }
    }

    /// Read current SAS emoji list for an in-progress verification, if available.
    pub async fn get_sas_emojis(
        &self,
        other_user: &str,
        transaction_id: &str,
    ) -> Result<Option<Vec<SasEmoji>>> {
        use matrix_sdk::ruma::UserId;

        let user_id = <&UserId>::try_from(other_user)
            .map_err(|e| MatrixError::Other(format!("Invalid user id: {e}")))?;

        let Some(verification) = self
            .inner
            .encryption()
            .get_verification(user_id, transaction_id)
            .await
        else {
            return Ok(None);
        };

        let Some(sas) = verification.sas() else {
            return Ok(None);
        };

        let Some(emoji_list) = sas.emoji() else {
            return Ok(None);
        };

        let emojis = emoji_list
            .iter()
            .map(|e| SasEmoji {
                symbol: e.symbol.to_string(),
                description: e.description.to_string(),
            })
            .collect();
        Ok(Some(emojis))
    }

    /// Confirm that the SAS emojis match on both devices.
    pub async fn confirm_sas(
        &self,
        other_user: &str,
        transaction_id: &str,
    ) -> Result<VerificationState> {
        use matrix_sdk::ruma::UserId;

        let user_id = <&UserId>::try_from(other_user)
            .map_err(|e| MatrixError::Other(format!("Invalid user id: {e}")))?;

        let verification = self
            .inner
            .encryption()
            .get_verification(user_id, transaction_id)
            .await
            .ok_or_else(|| {
                MatrixError::Other(format!("Verification {transaction_id} not found"))
            })?;

        let sas = verification
            .sas()
            .ok_or_else(|| MatrixError::Other("Verification is not an SAS flow".into()))?;

        sas.confirm()
            .await
            .map_err(|e| MatrixError::Other(format!("SAS confirm failed: {e}")))?;

        info!("Confirmed SAS verification {transaction_id}");
        Ok(VerificationState::Done {
            transaction_id: transaction_id.to_string(),
        })
    }

    /// Cancel an in-progress verification.
    pub async fn cancel_verification(
        &self,
        other_user: &str,
        transaction_id: &str,
    ) -> Result<VerificationState> {
        use matrix_sdk::ruma::UserId;

        let user_id = <&UserId>::try_from(other_user)
            .map_err(|e| MatrixError::Other(format!("Invalid user id: {e}")))?;

        // Prefer cancelling a VerificationRequest if still at that stage.
        if let Some(request) = self
            .inner
            .encryption()
            .get_verification_request(user_id, transaction_id)
            .await
        {
            let _ = request.cancel().await;
        } else if let Some(verification) = self
            .inner
            .encryption()
            .get_verification(user_id, transaction_id)
            .await
        {
            if let Some(sas) = verification.sas() {
                let _ = sas.cancel().await;
            }
        }

        Ok(VerificationState::Cancelled {
            transaction_id: transaction_id.to_string(),
            reason: "Cancelled by user".into(),
        })
    }

    /// Accept an incoming verification request and drive it until SAS emojis are ready.
    pub async fn accept_verification(
        &self,
        other_user: &str,
        transaction_id: &str,
    ) -> Result<VerificationState> {
        use futures_util::StreamExt;
        use matrix_sdk::encryption::verification::{
            SasState, Verification, VerificationRequestState,
        };
        use matrix_sdk::ruma::UserId;

        let user_id = <&UserId>::try_from(other_user)
            .map_err(|e| MatrixError::Other(format!("Invalid user id: {e}")))?;

        // 1) Prefer VerificationRequest path (incoming m.key.verification.request).
        if let Some(request) = self
            .inner
            .encryption()
            .get_verification_request(user_id, transaction_id)
            .await
        {
            request
                .accept()
                .await
                .map_err(|e| MatrixError::Other(format!("accept request failed: {e}")))?;
            info!("Accepted verification request {transaction_id}");

            let mut stream = request.changes();
            let sas = tokio::time::timeout(std::time::Duration::from_secs(60), async {
                while let Some(state) = stream.next().await {
                    match state {
                        VerificationRequestState::Transitioned { verification } => {
                            if let Verification::SasV1(s) = verification {
                                return Ok(s);
                            }
                        }
                        VerificationRequestState::Cancelled(info) => {
                            return Err(MatrixError::Other(format!(
                                "Verification cancelled: {}",
                                info.reason()
                            )));
                        }
                        VerificationRequestState::Done => {
                            return Err(MatrixError::Other(
                                "Verification finished before SAS started".into(),
                            ));
                        }
                        _ => {}
                    }
                }
                Err(MatrixError::Other(
                    "Verification request stream ended before SAS".into(),
                ))
            })
            .await
            .map_err(|_| {
                MatrixError::Other("Timed out waiting for SAS after accepting request".into())
            })??;

            // Accept SAS and wait for emojis.
            sas.accept()
                .await
                .map_err(|e| MatrixError::Other(format!("sas.accept failed: {e}")))?;

            let mut sas_stream = sas.changes();
            let result = tokio::time::timeout(std::time::Duration::from_secs(60), async {
                while let Some(state) = sas_stream.next().await {
                    match state {
                        SasState::KeysExchanged { emojis, .. } => {
                            if let Some(list) = emojis {
                                let mapped = list
                                    .emojis
                                    .iter()
                                    .map(|e| SasEmoji {
                                        symbol: e.symbol.to_string(),
                                        description: e.description.to_string(),
                                    })
                                    .collect();
                                return Ok(VerificationState::ShowEmojis {
                                    transaction_id: transaction_id.to_string(),
                                    emojis: mapped,
                                });
                            }
                        }
                        SasState::Cancelled(info) => {
                            return Ok(VerificationState::Cancelled {
                                transaction_id: transaction_id.to_string(),
                                reason: info.reason().to_string(),
                            });
                        }
                        SasState::Done { .. } => {
                            return Ok(VerificationState::Done {
                                transaction_id: transaction_id.to_string(),
                            });
                        }
                        _ => {}
                    }
                }
                Err(MatrixError::Other("SAS stream ended before emojis".into()))
            })
            .await
            .map_err(|_| MatrixError::Other("Timed out waiting for SAS emojis".into()))??;

            return Ok(result);
        }

        // 2) Already an SAS verification object.
        if let Some(verification) = self
            .inner
            .encryption()
            .get_verification(user_id, transaction_id)
            .await
        {
            if let Some(sas) = verification.sas() {
                let _ = sas.accept().await;
                if let Some(emoji_list) = sas.emoji() {
                    let emojis = emoji_list
                        .iter()
                        .map(|e| SasEmoji {
                            symbol: e.symbol.to_string(),
                            description: e.description.to_string(),
                        })
                        .collect();
                    return Ok(VerificationState::ShowEmojis {
                        transaction_id: transaction_id.to_string(),
                        emojis,
                    });
                }
                // Fall through to stream wait via wait_for_sas_emojis.
                return self.wait_for_sas_emojis(other_user, transaction_id).await;
            }
        }

        Err(MatrixError::Other(format!(
            "Verification {transaction_id} not found"
        )))
    }
}

/// Convert an SDK `deserialized_responses::TimelineEvent` into our presentation type.
fn map_timeline_event(
    sdk_event: &matrix_sdk::deserialized_responses::TimelineEvent,
) -> Option<TimelineEvent> {
    use matrix_sdk::ruma::events::{AnySyncMessageLikeEvent, AnySyncTimelineEvent};

    let any: AnySyncTimelineEvent = match sdk_event.raw().deserialize() {
        Ok(ev) => ev,
        Err(e) => {
            warn!("Failed to deserialize timeline event: {e}");
            return None;
        }
    };

    match any {
        AnySyncTimelineEvent::MessageLike(msg) => match msg {
            AnySyncMessageLikeEvent::RoomMessage(SyncMessageLikeEvent::Original(ev)) => {
                let content = match &ev.content.msgtype {
                    MessageType::Text(t) => EventContent::Text {
                        body: t.body.clone(),
                        formatted_html: t.formatted.as_ref().map(|f| f.body.clone()),
                    },
                    MessageType::Emote(e) => EventContent::Emote {
                        body: e.body.clone(),
                    },
                    MessageType::Notice(n) => EventContent::Notice {
                        body: n.body.clone(),
                    },
                    MessageType::Image(img) => EventContent::Image {
                        url: match &img.source {
                            MediaSource::Plain(u) => u.to_string(),
                            MediaSource::Encrypted(file) => file.url.to_string(),
                        },
                        filename: img.filename.clone().unwrap_or_else(|| "image".into()),
                        mime_type: img
                            .info
                            .as_ref()
                            .and_then(|i| i.mimetype.clone())
                            .unwrap_or_else(|| "image/*".into()),
                        size_bytes: img.info.as_ref().and_then(|i| i.size).map(|s| s.into()),
                    },
                    MessageType::File(f) => EventContent::File {
                        url: match &f.source {
                            MediaSource::Plain(u) => u.to_string(),
                            MediaSource::Encrypted(file) => file.url.to_string(),
                        },
                        filename: f.filename.clone().unwrap_or_else(|| "file".into()),
                        mime_type: f
                            .info
                            .as_ref()
                            .and_then(|i| i.mimetype.clone())
                            .unwrap_or_else(|| "application/octet-stream".into()),
                        size_bytes: f.info.as_ref().and_then(|i| i.size).map(|s| s.into()),
                    },
                    MessageType::Audio(a) => EventContent::Audio {
                        url: match &a.source {
                            MediaSource::Plain(u) => u.to_string(),
                            MediaSource::Encrypted(file) => file.url.to_string(),
                        },
                        filename: a.filename.clone().unwrap_or_else(|| "audio".into()),
                        mime_type: a
                            .info
                            .as_ref()
                            .and_then(|i| i.mimetype.clone())
                            .unwrap_or_else(|| "audio/*".into()),
                        duration_ms: a
                            .info
                            .as_ref()
                            .and_then(|i| i.duration)
                            .map(|d| d.as_millis() as u64),
                    },
                    MessageType::Video(v) => EventContent::Video {
                        url: match &v.source {
                            MediaSource::Plain(u) => u.to_string(),
                            MediaSource::Encrypted(file) => file.url.to_string(),
                        },
                        filename: v.filename.clone().unwrap_or_else(|| "video".into()),
                        mime_type: v
                            .info
                            .as_ref()
                            .and_then(|i| i.mimetype.clone())
                            .unwrap_or_else(|| "video/*".into()),
                        size_bytes: v.info.as_ref().and_then(|i| i.size).map(|s| s.into()),
                    },
                    other => {
                        warn!("Skipping unsupported message type: {other:?}");
                        return None;
                    }
                };

                let timestamp_millis = sdk_event
                    .timestamp()
                    .map(|ts| ts.get().into())
                    .unwrap_or_else(|| ev.origin_server_ts.0.into());

                Some(TimelineEvent {
                    event_id: ev.event_id.to_string(),
                    sender: ev.sender.to_string(),
                    timestamp_millis,
                    content,
                    reply_to: None,
                })
            }
            AnySyncMessageLikeEvent::RoomMessage(SyncMessageLikeEvent::Redacted(ev)) => {
                Some(TimelineEvent {
                    event_id: ev.event_id.to_string(),
                    sender: ev.sender.to_string(),
                    timestamp_millis: ev.origin_server_ts.0.into(),
                    content: EventContent::Redacted,
                    reply_to: None,
                })
            }
            _ => None,
        },
        AnySyncTimelineEvent::State(_) => None,
    }
}
