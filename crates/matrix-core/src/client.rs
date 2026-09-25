//! High-level Matrix client wrapper handling session, authentication, and communication

use std::path::PathBuf;
use std::sync::Arc;
use url::Url;

use matrix_sdk::{
    authentication::{matrix::MatrixSession as SdkMatrixSession, SessionTokens},
    ruma::{
        events::room::message::RoomMessageEventContent,
        OwnedDeviceId, RoomId, UserId,
    },
    store::RoomLoadSettings,
    Client, SessionMeta,
};
use tracing::info;

use crate::error::{MatrixError, Result};
use crate::room::RoomSummary;
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
            .initial_device_display_name("Matrix Linux Desktop")
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
}
