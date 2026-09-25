//! Matrix session persistence and credential management

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use url::Url;

use crate::error::{MatrixError, Result};

/// Persistent Matrix user session credentials
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MatrixSession {
    /// Homeserver URL (e.g. "https://matrix.org")
    pub homeserver_url: Url,
    /// User ID (e.g. "@alice:matrix.org")
    pub user_id: String,
    /// Device ID assigned by the homeserver
    pub device_id: String,
    /// Secret access token for Matrix Client-Server API
    pub access_token: String,
    /// Optional refresh token for token rotation
    pub refresh_token: Option<String>,
}

impl MatrixSession {
    pub fn new(
        homeserver_url: Url,
        user_id: impl Into<String>,
        device_id: impl Into<String>,
        access_token: impl Into<String>,
    ) -> Self {
        Self {
            homeserver_url,
            user_id: user_id.into(),
            device_id: device_id.into(),
            access_token: access_token.into(),
            refresh_token: None,
        }
    }
}

/// Abstract storage interface for session credentials
#[async_trait::async_trait]
pub trait SessionStore: Send + Sync {
    async fn save_session(&self, session: &MatrixSession) -> Result<()>;
    async fn load_session(&self) -> Result<Option<MatrixSession>>;
    async fn clear_session(&self) -> Result<()>;
}

/// Local file-backed session store
pub struct FileSessionStore {
    file_path: PathBuf,
}

impl FileSessionStore {
    pub fn new(file_path: impl AsRef<Path>) -> Self {
        Self {
            file_path: file_path.as_ref().to_path_buf(),
        }
    }
}

#[async_trait::async_trait]
impl SessionStore for FileSessionStore {
    async fn save_session(&self, session: &MatrixSession) -> Result<()> {
        if let Some(parent) = self.file_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| MatrixError::Session(format!("Failed to create session dir: {e}")))?;
        }

        let json = serde_json::to_string_pretty(session)?;
        tokio::fs::write(&self.file_path, json)
            .await
            .map_err(|e| MatrixError::Session(format!("Failed to write session file: {e}")))?;

        Ok(())
    }

    async fn load_session(&self) -> Result<Option<MatrixSession>> {
        if !self.file_path.exists() {
            return Ok(None);
        }

        let bytes = tokio::fs::read(&self.file_path)
            .await
            .map_err(|e| MatrixError::Session(format!("Failed to read session file: {e}")))?;

        let session: MatrixSession = serde_json::from_slice(&bytes)?;
        Ok(Some(session))
    }

    async fn clear_session(&self) -> Result<()> {
        if self.file_path.exists() {
            tokio::fs::remove_file(&self.file_path)
                .await
                .map_err(|e| MatrixError::Session(format!("Failed to delete session file: {e}")))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_save_and_load() {
        let temp_dir = tempfile::tempdir().unwrap();
        let session_file = temp_dir.path().join("session.json");
        let store = FileSessionStore::new(&session_file);

        let session = MatrixSession::new(
            Url::parse("https://matrix.org").unwrap(),
            "@alice:matrix.org",
            "DEVICE123",
            "syt_token_secret_456",
        );

        store.save_session(&session).await.unwrap();
        let loaded = store.load_session().await.unwrap();
        assert_eq!(loaded, Some(session));

        store.clear_session().await.unwrap();
        let after_clear = store.load_session().await.unwrap();
        assert_eq!(after_clear, None);
    }
}
