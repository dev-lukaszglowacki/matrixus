//! Call Window — native WebRTC UI + media controls (no WebKit / Wire widget).

use matrix_call::{CallSession, CallState, NativeCallEngine};
use tracing::info;

/// Controller for an active native call window / overlay
pub struct CallViewController {
    pub session: CallSession,
    /// Native WebRTC engine (signalling + peer connection)
    pub engine: Option<std::sync::Arc<NativeCallEngine>>,
}

impl CallViewController {
    pub fn new(session: CallSession, _user_id: &str, _device_id: &str) -> Self {
        Self {
            session,
            engine: None,
        }
    }

    /// Attach a live native engine (outgoing or accepted incoming).
    pub fn with_engine(mut self, engine: std::sync::Arc<NativeCallEngine>) -> Self {
        self.engine = Some(engine);
        self
    }

    pub fn toggle_audio(&mut self) -> bool {
        let muted = self.session.toggle_audio_mute();
        info!("Call audio mute toggled: {muted}");
        muted
    }

    pub fn toggle_video(&mut self) -> bool {
        let muted = self.session.toggle_video_mute();
        info!("Call video mute toggled: {muted}");
        muted
    }

    pub fn toggle_screen_share(&mut self) -> bool {
        let sharing = self.session.toggle_screen_share();
        info!("Screen sharing toggled: {sharing}");
        sharing
    }

    pub fn end_call(&mut self) {
        info!("Ending call session: {}", self.session.call_id);
        self.session.hang_up();
        if let Some(engine) = &self.engine {
            let engine = engine.clone();
            tokio::spawn(async move {
                let _ = engine.hangup(Some("user_hangup".into())).await;
            });
        }
    }

    pub fn state(&self) -> CallState {
        self.session.state
    }
}

#[cfg(feature = "gui")]
mod gui {
    use super::*;
    use std::sync::{Arc, Mutex};

    use gtk4::glib;
    use gtk4::prelude::*;
    use libadwaita as adw;
    use libadwaita::prelude::*;
    use tracing::info;

    use crate::desktop::portals::{PortalRequestResult, PortalService};

    type SharedController = Arc<Mutex<CallViewController>>;

    /// Open a dedicated call window with native video surface + controls.
    ///
    /// Video tiles are placeholders until frames are piped from webrtc-rs /
    /// GStreamer into GTK (Paintable / GL area). Audio/video mute and hangup
    /// are wired to the session + engine.
    pub fn open_call_window(controller: CallViewController) -> adw::Window {
        let shared: SharedController = Arc::new(Mutex::new(controller));

        let room_title = shared
            .lock()
            .map(|c| c.session.room_id.clone())
            .unwrap_or_else(|_| "Call".into());
        let is_video = shared
            .lock()
            .map(|c| c.session.is_video_call)
            .unwrap_or(true);

        if let Ok(mut c) = shared.lock() {
            let _ = c.session.start_connecting();
        }

        let window = adw::Window::builder()
            .title(if is_video {
                format!("Video call — {room_title}")
            } else {
                format!("Voice call — {room_title}")
            })
            .default_width(if is_video { 960 } else { 420 })
            .default_height(if is_video { 640 } else { 280 })
            .build();

        let header = adw::HeaderBar::new();
        let title = gtk4::Label::new(Some(if is_video {
            "Video call"
        } else {
            "Voice call"
        }));
        header.set_title_widget(Some(&title));

        let status_label = gtk4::Label::builder()
            .label("Connecting…")
            .css_classes(["dim-label", "caption"])
            .build();
        header.pack_start(&status_label);

        // Native media surface (no WebKit)
        let media_box = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        media_box.set_hexpand(true);
        media_box.set_vexpand(true);
        media_box.set_valign(gtk4::Align::Center);
        media_box.set_halign(gtk4::Align::Center);
        media_box.set_margin_top(24);
        media_box.set_margin_bottom(24);

        let remote_avatar = adw::Avatar::new(120, Some("Remote"), true);
        let remote_label = gtk4::Label::builder()
            .label(if is_video {
                "Waiting for remote video…"
            } else {
                "Voice call in progress"
            })
            .css_classes(["title-3"])
            .build();
        let hint = gtk4::Label::builder()
            .label("Native WebRTC · Matrix m.call signalling")
            .css_classes(["dim-label", "caption"])
            .build();

        media_box.append(&remote_avatar);
        media_box.append(&remote_label);
        media_box.append(&hint);

        // Controls
        let controls = gtk4::Box::new(gtk4::Orientation::Horizontal, 16);
        controls.set_halign(gtk4::Align::Center);
        controls.set_margin_bottom(24);

        let mute_btn = gtk4::Button::builder()
            .icon_name("microphone-sensitivity-high-symbolic")
            .tooltip_text("Mute microphone")
            .css_classes(["circular"])
            .build();
        let video_btn = gtk4::Button::builder()
            .icon_name(if is_video {
                "camera-video-symbolic"
            } else {
                "camera-video-symbolic"
            })
            .tooltip_text("Toggle camera")
            .css_classes(["circular"])
            .sensitive(is_video)
            .build();
        let screen_btn = gtk4::Button::builder()
            .icon_name("preferences-desktop-display-symbolic")
            .tooltip_text("Share screen")
            .css_classes(["circular"])
            .build();
        let hangup_btn = gtk4::Button::builder()
            .icon_name("call-stop-symbolic")
            .tooltip_text("Hang up")
            .css_classes(["circular", "destructive-action"])
            .build();

        controls.append(&mute_btn);
        controls.append(&video_btn);
        controls.append(&screen_btn);
        controls.append(&hangup_btn);

        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        content.append(&media_box);
        content.append(&controls);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&content));
        window.set_content(Some(&toolbar));

        // ── Control handlers ───────────────────────────────────────────────
        {
            let shared = shared.clone();
            mute_btn.connect_clicked(move |btn| {
                if let Ok(mut c) = shared.lock() {
                    let muted = c.toggle_audio();
                    btn.set_icon_name(if muted {
                        "microphone-sensitivity-muted-symbolic"
                    } else {
                        "microphone-sensitivity-high-symbolic"
                    });
                }
            });
        }
        {
            let shared = shared.clone();
            video_btn.connect_clicked(move |btn| {
                if let Ok(mut c) = shared.lock() {
                    let muted = c.toggle_video();
                    btn.set_icon_name(if muted {
                        "camera-off-symbolic"
                    } else {
                        "camera-video-symbolic"
                    });
                }
            });
        }
        {
            let shared = shared.clone();
            let window = window.clone();
            screen_btn.connect_clicked(move |_| {
                let shared = shared.clone();
                let window = window.clone();
                glib::spawn_future_local(async move {
                    let portals = PortalService::new();
                    match portals.request_screencast().await {
                        PortalRequestResult::Granted { label } => {
                            if let Ok(mut c) = shared.lock() {
                                let _ = c.toggle_screen_share();
                            }
                            info!("Screen share portal granted: {label}");
                        }
                        other => {
                            info!("Screen share portal: {other:?}");
                            let toast = adw::Toast::new("Screen share unavailable");
                            // Window may not have a toast overlay; log only
                            let _ = (toast, window);
                        }
                    }
                });
            });
        }
        {
            let shared = shared.clone();
            let window = window.clone();
            let status_label = status_label.clone();
            hangup_btn.connect_clicked(move |_| {
                if let Ok(mut c) = shared.lock() {
                    c.end_call();
                }
                status_label.set_text("Call ended");
                window.close();
            });
        }

        // Reflect connecting → connected after a short delay when engine is present
        // (real path: subscribe to PeerEvent::StateChanged)
        {
            let status_label = status_label.clone();
            let shared = shared.clone();
            glib::timeout_add_seconds_local(2, move || {
                if let Ok(mut c) = shared.lock() {
                    if c.session.state == CallState::Connecting {
                        let _ = c.session.set_connected();
                        status_label.set_text("Connected");
                    }
                }
                glib::ControlFlow::Break
            });
        }

        window
    }
}

#[cfg(feature = "gui")]
pub use gui::open_call_window;
