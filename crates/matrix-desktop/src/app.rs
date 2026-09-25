//! Application coordinator managing matrix client, sync loop, and UI state

use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

use matrix_call::CallSession;
use matrix_core::{FileSessionStore, MatrixClient, SessionStore};

use crate::desktop::NotificationService;
use crate::ui::MainWindowState;

/// Central desktop application controller
pub struct MatrixDesktopApp {
    pub client: Arc<Mutex<Option<MatrixClient>>>,
    pub session_store: Arc<FileSessionStore>,
    pub notifications: NotificationService,
    pub state: Arc<Mutex<MainWindowState>>,
}

impl MatrixDesktopApp {
    pub fn new() -> Self {
        let home_dir = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let session_path = std::path::PathBuf::from(home_dir)
            .join(".local/share/matrixclient/session.json");

        Self {
            client: Arc::new(Mutex::new(None)),
            session_store: Arc::new(FileSessionStore::new(session_path)),
            notifications: NotificationService::new(),
            state: Arc::new(Mutex::new(MainWindowState::new())),
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

    /// Initiate an outgoing video call in a room
    pub async fn start_video_call(&self, room_id: &str) -> anyhow::Result<()> {
        let client_guard = self.client.lock().await;
        let client = client_guard.as_ref().ok_or_else(|| anyhow::anyhow!("Not logged in"))?;

        let user_id = client.user_id().unwrap_or_default();
        let device_id = client.device_id().unwrap_or_default();
        let call_id = format!("call_{}_{}", room_id, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis());

        info!("Starting video call in {room_id}: {call_id}");
        let session = CallSession::new(&call_id, room_id, true);
        let controller = crate::ui::CallViewController::new(session, &user_id, &device_id);

        let mut state = self.state.lock().await;
        state.active_call = Some(controller);

        Ok(())
    }
}

impl Default for MatrixDesktopApp {
    fn default() -> Self {
        Self::new()
    }
}
