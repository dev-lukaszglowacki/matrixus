//! Call Window — WebKitGTK Element Call host + media controls (Phase 6).

use matrix_call::{CallSession, CallState};
use tracing::info;

use super::widget_host::WidgetHostBridge;

/// Controller for an active call window / overlay
pub struct CallViewController {
    pub session: CallSession,
    pub bridge: WidgetHostBridge,
}

impl CallViewController {
    pub fn new(session: CallSession, user_id: &str, device_id: &str) -> Self {
        let bridge = WidgetHostBridge::new(
            format!("widget_{}", session.call_id),
            &session.room_id,
            user_id,
            device_id,
        );
        Self { session, bridge }
    }

    /// Get the target URL to load into WebKitGTK
    pub fn widget_url(&self) -> String {
        self.bridge
            .generate_widget_url(self.session.is_video_call, "dark")
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
    use tracing::{info, warn};
    use webkit6::prelude::{PermissionRequestExt, WebViewExt};

    use crate::desktop::portals::{PortalRequestResult, PortalService};

    type SharedController = Arc<Mutex<CallViewController>>;

    /// Open a dedicated call window hosting Element Call in WebKitGTK.
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
        let widget_url = shared
            .lock()
            .map(|c| c.widget_url())
            .unwrap_or_default();

        if let Ok(mut c) = shared.lock() {
            let _ = c.session.start_connecting();
        }

        let window = adw::Window::builder()
            .title(if is_video {
                format!("Video call — {room_title}")
            } else {
                format!("Voice call — {room_title}")
            })
            .default_width(960)
            .default_height(640)
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

        let webview = webkit6::WebView::new();
        webview.set_hexpand(true);
        webview.set_vexpand(true);

        if let Some(settings) = webkit6::prelude::WebViewExt::settings(&webview) {
            settings.set_enable_media_stream(true);
            // WebRTC / mediasource flags vary by webkit6 version; ignore if absent.
            let _ = settings;
        }

        webview.connect_permission_request(|_view, request| {
            info!("WebKit permission request — allowing");
            request.allow();
            true
        });

        info!("Loading Element Call widget: {widget_url}");
        webview.load_uri(&widget_url);

        let status_for_load = status_label.clone();
        let shared_for_load = shared.clone();
        webview.connect_load_changed(move |_view, event| {
            use webkit6::LoadEvent;
            if event == LoadEvent::Finished {
                if let Ok(mut c) = shared_for_load.lock() {
                    let _ = c.session.set_connected();
                }
                status_for_load.set_text("Connected");
            }
        });

        let mic_btn = gtk4::ToggleButton::builder()
            .icon_name("microphone-sensitivity-high-symbolic")
            .tooltip_text("Mute microphone")
            .css_classes(["circular"])
            .build();

        let cam_btn = gtk4::ToggleButton::builder()
            .icon_name("camera-web-symbolic")
            .tooltip_text("Toggle camera")
            .css_classes(["circular"])
            .sensitive(is_video)
            .build();

        let share_btn = gtk4::ToggleButton::builder()
            .icon_name("media-record-symbolic")
            .tooltip_text("Share screen")
            .css_classes(["circular"])
            .build();

        let hangup_btn = gtk4::Button::builder()
            .icon_name("call-stop-symbolic")
            .tooltip_text("Hang up")
            .css_classes(["circular", "destructive-action"])
            .build();

        let controls = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .spacing(16)
            .halign(gtk4::Align::Center)
            .margin_top(12)
            .margin_bottom(16)
            .build();
        controls.append(&mic_btn);
        controls.append(&cam_btn);
        controls.append(&share_btn);
        controls.append(&hangup_btn);

        {
            let shared = shared.clone();
            mic_btn.connect_toggled(move |btn| {
                if let Ok(mut c) = shared.lock() {
                    let muted = c.toggle_audio();
                    if muted {
                        btn.set_icon_name("microphone-sensitivity-muted-symbolic");
                        btn.set_tooltip_text(Some("Unmute microphone"));
                    } else {
                        btn.set_icon_name("microphone-sensitivity-high-symbolic");
                        btn.set_tooltip_text(Some("Mute microphone"));
                    }
                }
            });
        }

        {
            let shared = shared.clone();
            cam_btn.connect_toggled(move |btn| {
                let enabling = btn.is_active();
                if enabling {
                    let btn = btn.clone();
                    let shared = shared.clone();
                    let (tx, rx) = crate::ui::async_ui::ui_channel::<PortalRequestResult>();
                    crate::ui::async_ui::attach_ui_receiver(rx, move |result| {
                        match result {
                            PortalRequestResult::Granted { .. }
                            | PortalRequestResult::Unavailable => {
                                if let Ok(mut c) = shared.lock() {
                                    let muted = c.toggle_video();
                                    if muted {
                                        btn.set_active(false);
                                    }
                                }
                            }
                            PortalRequestResult::Denied | PortalRequestResult::Error(_) => {
                                btn.set_active(false);
                                warn!("Camera permission not granted");
                            }
                        }
                        });
                    tokio::spawn(async move {
                        let portals = PortalService::new();
                        let result = portals.request_camera().await;
                        let _ = tx.send(result);
                    });
                } else if let Ok(mut c) = shared.lock() {
                    let _ = c.toggle_video();
                }
            });
        }

        {
            let shared = shared.clone();
            share_btn.connect_toggled(move |btn| {
                let enabling = btn.is_active();
                if enabling {
                    let btn = btn.clone();
                    let shared = shared.clone();
                    let (tx, rx) = crate::ui::async_ui::ui_channel::<PortalRequestResult>();
                    crate::ui::async_ui::attach_ui_receiver(rx, move |result| {
                        match result {
                            PortalRequestResult::Granted { label } => {
                                info!("Screen share granted: {label}");
                                if let Ok(mut c) = shared.lock() {
                                    if !c.session.local_screen_sharing {
                                        let _ = c.toggle_screen_share();
                                    }
                                }
                            }
                            PortalRequestResult::Unavailable => {
                                if let Ok(mut c) = shared.lock() {
                                    let _ = c.toggle_screen_share();
                                }
                            }
                            PortalRequestResult::Denied | PortalRequestResult::Error(_) => {
                                btn.set_active(false);
                                warn!("Screen share denied");
                            }
                        }
                        });
                    tokio::spawn(async move {
                        let portals = PortalService::new();
                        let result = portals.request_screencast().await;
                        let _ = tx.send(result);
                    });
                } else if let Ok(mut c) = shared.lock() {
                    if c.session.local_screen_sharing {
                        let _ = c.toggle_screen_share();
                    }
                }
            });
        }

        {
            let shared = shared.clone();
            let window_for_hangup = window.clone();
            hangup_btn.connect_clicked(move |_| {
                if let Ok(mut c) = shared.lock() {
                    c.end_call();
                }
                window_for_hangup.close();
            });
        }

        {
            let shared = shared.clone();
            window.connect_close_request(move |_w| {
                if let Ok(mut c) = shared.lock() {
                    if c.session.state != CallState::Ended {
                        c.end_call();
                    }
                }
                glib::Propagation::Proceed
            });
        }

        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        content.append(&webview);
        content.append(&controls);

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&content));
        window.set_content(Some(&toolbar));

        window
    }
}

#[cfg(feature = "gui")]
pub use gui::open_call_window;
