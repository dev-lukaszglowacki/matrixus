//! User preferences persisted under XDG config.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Application-wide user settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// Show desktop notifications for new messages
    pub notifications_enabled: bool,
    /// Show desktop notifications for incoming calls
    pub call_notifications_enabled: bool,
    /// When true, closing the main window hides it instead of quitting
    pub close_to_tray: bool,
    /// Follow system color scheme (true) or force dark/light
    pub theme: ThemePreference,
    /// Element Call base URL
    pub element_call_url: String,
    /// Enter sends message (true) vs newline
    pub enter_to_send: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    System,
    Light,
    Dark,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            notifications_enabled: true,
            call_notifications_enabled: true,
            close_to_tray: false,
            theme: ThemePreference::System,
            element_call_url: "https://call.element.io".into(),
            enter_to_send: true,
        }
    }
}

impl AppSettings {
    fn config_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home)
            .join(".config/matrixus/settings.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        match std::fs::read_to_string(&path) {
            Ok(data) => match serde_json::from_str(&data) {
                Ok(s) => {
                    info!("Loaded settings from {}", path.display());
                    s
                }
                Err(e) => {
                    warn!("Corrupt settings file, using defaults: {e}");
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(self) {
            Ok(data) => {
                if let Err(e) = std::fs::write(&path, data) {
                    warn!("Failed to write settings: {e}");
                } else {
                    info!("Saved settings to {}", path.display());
                }
            }
            Err(e) => warn!("Failed to serialize settings: {e}"),
        }
    }
}
