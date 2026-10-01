//! FreeDesktop / GNotification helpers for messages and incoming calls

use std::sync::{Arc, Mutex};

use tracing::info;

pub type IncomingCallAccept = Arc<dyn Fn(String) + Send + Sync>;
pub type IncomingCallDecline = Arc<dyn Fn(String) + Send + Sync>;

pub struct NotificationService {
    accept_handler: Option<IncomingCallAccept>,
    decline_handler: Option<IncomingCallDecline>,
    pub messages_enabled: Arc<Mutex<bool>>,
    pub calls_enabled: Arc<Mutex<bool>>,
}

impl NotificationService {
    pub fn new() -> Self {
        Self {
            accept_handler: None,
            decline_handler: None,
            messages_enabled: Arc::new(Mutex::new(true)),
            calls_enabled: Arc::new(Mutex::new(true)),
        }
    }

    pub fn set_messages_enabled(&self, enabled: bool) {
        if let Ok(mut g) = self.messages_enabled.lock() {
            *g = enabled;
        }
    }

    pub fn set_calls_enabled(&self, enabled: bool) {
        if let Ok(mut g) = self.calls_enabled.lock() {
            *g = enabled;
        }
    }

    pub fn set_incoming_call_handlers(
        &mut self,
        on_accept: IncomingCallAccept,
        on_decline: IncomingCallDecline,
    ) {
        self.accept_handler = Some(on_accept);
        self.decline_handler = Some(on_decline);
    }

    pub fn show_message_notification(
        &self,
        room_name: &str,
        sender_name: &str,
        body_preview: &str,
    ) {
        if !self.messages_enabled.lock().map(|g| *g).unwrap_or(true) {
            return;
        }
        info!("Notification [{room_name}] {sender_name}: {body_preview}");

        #[cfg(feature = "gui")]
        {
            use gtk4::{gio, glib};

            let title = room_name.to_string();
            let body = format!("{sender_name}: {body_preview}");
            glib::MainContext::default().invoke(move || {
                let app = gtk4::Application::default();
                let notif = gio::Notification::new(&title);
                notif.set_body(Some(&body));
                notif.set_priority(gio::NotificationPriority::Normal);
                notif.set_category(Some("im.received"));
                // GtkApplicationExt::send_notification
                gio::prelude::ApplicationExt::send_notification(
                    &app,
                    Some("matrix-message"),
                    &notif,
                );
            });
        }
    }

    pub fn show_incoming_call_notification(
        &self,
        room_id: &str,
        caller_name: &str,
        room_name: &str,
        is_video: bool,
    ) {
        if !self.calls_enabled.lock().map(|g| *g).unwrap_or(true) {
            return;
        }

        let call_type = if is_video { "Video Call" } else { "Voice Call" };
        info!(
            "Incoming Call Notification: {call_type} from {caller_name} in {room_name} ({room_id})"
        );

        #[cfg(feature = "gui")]
        {
            use gtk4::prelude::*;
            use gtk4::{gio, glib};
            use libadwaita::prelude::*;

            let room_id = room_id.to_string();
            let caller_name = caller_name.to_string();
            let room_name = room_name.to_string();
            let accept = self.accept_handler.clone();
            let decline = self.decline_handler.clone();
            let call_type_owned = call_type.to_string();

            glib::MainContext::default().invoke({
                let room_name = room_name.clone();
                let caller_name = caller_name.clone();
                let call_type = call_type_owned.clone();
                move || {
                    let app = gtk4::Application::default();
                    let notif = gio::Notification::new(&format!("Incoming {call_type}"));
                    notif.set_body(Some(&format!("{caller_name} — {room_name}")));
                    notif.set_priority(gio::NotificationPriority::Urgent);
                    notif.set_category(Some("call.incoming"));
                    gio::prelude::ApplicationExt::send_notification(
                        &app,
                        Some("matrix-call"),
                        &notif,
                    );
                }
            });

            glib::MainContext::default().invoke(move || {
                let dialog = libadwaita::AlertDialog::builder()
                    .heading(&format!("Incoming {call_type_owned}"))
                    .body(&format!("{caller_name} is calling in {room_name}"))
                    .build();
                dialog.add_response("decline", "Decline");
                dialog.add_response("accept", "Accept");
                dialog.set_response_appearance(
                    "accept",
                    libadwaita::ResponseAppearance::Suggested,
                );
                dialog.set_default_response(Some("accept"));
                dialog.set_close_response("decline");

                dialog.connect_response(None, move |_dialog, response| {
                    if response == "accept" {
                        if let Some(cb) = &accept {
                            cb(room_id.clone());
                        }
                    } else if let Some(cb) = &decline {
                        cb(room_id.clone());
                    }
                });

                let app = gtk4::Application::default();
                dialog.present(app.active_window().as_ref());
            });
        }
    }
}

impl Default for NotificationService {
    fn default() -> Self {
        Self::new()
    }
}
