//! SAS emoji verification dialog and security status UI (Phase 5).

use std::sync::Arc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use matrix_core::{DeviceInfo, DeviceTrustLevel, SasEmoji, VerificationState};
use tracing::{error, info};

use crate::app::MatrixusApp;
use crate::ui::async_ui::{self, UiSend};

/// Show a modal dialog for comparing SAS emojis and confirming/cancelling.
pub fn show_sas_dialog(
    parent: &impl IsA<gtk4::Window>,
    app_state: Arc<MatrixusApp>,
    other_user: String,
    transaction_id: String,
    emojis: Vec<SasEmoji>,
) {
    let parent_win = parent.upcast_ref::<gtk4::Window>().clone();
    let dialog = adw::MessageDialog::builder()
        .transient_for(&parent_win)
        .modal(true)
        .heading("Verify device")
        .body("Compare these emojis with the other device. They must match in order.")
        .build();

    dialog.add_response("cancel", "They don't match");
    dialog.add_response("confirm", "They match");
    dialog.set_response_appearance("confirm", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("confirm"));
    dialog.set_close_response("cancel");

    let emoji_box = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(12)
        .halign(gtk4::Align::Center)
        .margin_top(12)
        .margin_bottom(8)
        .build();

    for emoji in &emojis {
        let cell = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Vertical)
            .spacing(4)
            .halign(gtk4::Align::Center)
            .build();
        let symbol = gtk4::Label::new(None);
        symbol.set_markup(&format!("<span size='xx-large'>{}</span>", emoji.symbol));
        let desc = gtk4::Label::builder()
            .label(&emoji.description)
            .css_classes(["caption", "dim-label"])
            .build();
        cell.append(&symbol);
        cell.append(&desc);
        emoji_box.append(&cell);
    }
    dialog.set_extra_child(Some(&emoji_box));

    let app = app_state.clone();
    let user = other_user.clone();
    let tx = transaction_id.clone();
    dialog.connect_response(None, move |dialog, response| {
        let app = app.clone();
        let user = user.clone();
        let tx = tx.clone();
        let dialog = UiSend::new(dialog.clone());
        if response == "confirm" {
            tokio::spawn(async move {
                let result = app.confirm_sas(&user, &tx).await;
                async_ui::on_ui(move || {
                    if let Ok(VerificationState::Done { .. }) = &result {
                        info!("Device verification completed");
                    } else if let Err(e) = &result {
                        error!("confirm_sas failed: {e}");
                    }
                    dialog.into_inner().close();
                });
            });
        } else {
            tokio::spawn(async move {
                let _ = app.cancel_verification(&user, &tx).await;
                async_ui::on_ui(move || {
                    dialog.into_inner().close();
                });
            });
        }
    });

    dialog.present();
}

/// Incoming verification request dialog.
pub fn show_verification_request_dialog(
    parent: &impl IsA<gtk4::Window>,
    app_state: Arc<MatrixusApp>,
    other_user: String,
    other_device: String,
    transaction_id: String,
) {
    let parent_win = parent.upcast_ref::<gtk4::Window>().clone();
    let dialog = adw::MessageDialog::builder()
        .transient_for(&parent_win)
        .modal(true)
        .heading("Incoming verification")
        .body(&format!(
            "Device {other_device} of {other_user} wants to verify with this session."
        ))
        .build();

    dialog.add_response("decline", "Decline");
    dialog.add_response("accept", "Accept");
    dialog.set_response_appearance("accept", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("accept"));
    dialog.set_close_response("decline");

    let app = app_state.clone();
    let user = other_user.clone();
    let tx = transaction_id.clone();
    let parent_ui = UiSend::new(parent_win.clone());
    dialog.connect_response(None, move |dialog, response| {
        let app = app.clone();
        let user = user.clone();
        let tx = tx.clone();
        let dialog = UiSend::new(dialog.clone());
        let parent_ui = UiSend::new(parent_ui.0.clone());
        if response == "accept" {
            tokio::spawn(async move {
                let result = app.accept_verification(&user, &tx).await;
                async_ui::on_ui(move || {
                    let dialog = dialog.into_inner();
                    let parent = parent_ui.into_inner();
                    dialog.close();
                    match result {
                        Ok(VerificationState::ShowEmojis {
                            transaction_id,
                            emojis,
                        }) => {
                            show_sas_dialog(&parent, app, user, transaction_id, emojis);
                        }
                        Ok(VerificationState::Started { transaction_id }) => {
                            // Poll for emojis on a background task
                            let app2 = app.clone();
                            let user2 = user.clone();
                            let parent = UiSend::new(parent);
                            tokio::spawn(async move {
                                for _ in 0..30 {
                                    tokio::time::sleep(std::time::Duration::from_millis(500))
                                        .await;
                                    if let Ok(Some(emojis)) =
                                        app2.get_sas_emojis(&user2, &transaction_id).await
                                    {
                                        let app3 = app2.clone();
                                        let user3 = user2.clone();
                                        let tx3 = transaction_id.clone();
                                        async_ui::on_ui(move || {
                                            show_sas_dialog(
                                                &parent.into_inner(),
                                                app3,
                                                user3,
                                                tx3,
                                                emojis,
                                            );
                                        });
                                        return;
                                    }
                                }
                            });
                        }
                        Ok(other) => error!("Unexpected state after accept: {other:?}"),
                        Err(e) => error!("accept_verification failed: {e}"),
                    }
                });
            });
        } else {
            tokio::spawn(async move {
                let _ = app.cancel_verification(&user, &tx).await;
                async_ui::on_ui(move || {
                    dialog.into_inner().close();
                });
            });
        }
    });

    dialog.present();
}

/// Security / devices dialog.
pub fn show_security_dialog(parent: &impl IsA<gtk4::Window>, app_state: Arc<MatrixusApp>) {
    let dialog = adw::Window::builder()
        .transient_for(parent)
        .modal(true)
        .title("Security")
        .default_width(420)
        .default_height(480)
        .build();

    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&gtk4::Label::new(Some("Security"))));

    let status_label = gtk4::Label::builder()
        .label("Loading security status…")
        .css_classes(["title-4"])
        .margin_top(16)
        .margin_start(16)
        .margin_end(16)
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build();

    let detail_label = gtk4::Label::builder()
        .label("")
        .css_classes(["dim-label", "caption"])
        .margin_start(16)
        .margin_end(16)
        .margin_top(4)
        .halign(gtk4::Align::Start)
        .wrap(true)
        .build();

    let devices_header = gtk4::Label::builder()
        .label("Your devices")
        .css_classes(["title-4"])
        .margin_top(20)
        .margin_start(16)
        .margin_end(16)
        .halign(gtk4::Align::Start)
        .build();

    let device_list = gtk4::ListBox::builder()
        .selection_mode(gtk4::SelectionMode::None)
        .css_classes(["boxed-list"])
        .margin_start(12)
        .margin_end(12)
        .margin_top(8)
        .margin_bottom(16)
        .build();

    let scrolled = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .vexpand(true)
        .child(&device_list)
        .build();

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    content.append(&status_label);
    content.append(&detail_label);
    content.append(&devices_header);
    content.append(&scrolled);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    dialog.set_content(Some(&toolbar));

    let app = app_state.clone();
    let ui = UiSend::new((status_label, detail_label, device_list, dialog.clone(), app_state.clone()));
    tokio::spawn(async move {
        let status = app.crypto_status().await;
        let devices = app.list_own_devices().await;
        async_ui::on_ui(move || {
            let (status_label, detail_label, device_list, dialog, app_for_devices) = ui.into_inner();
            match status {
                Ok(s) => {
                    status_label.set_text(&s.summary_label());
                    detail_label.set_text(&format!(
                        "Cross-signing: {}\nThis device signed: {}\nKey backup: {}",
                        if s.cross_signing_ready {
                            "ready"
                        } else {
                            "not set up"
                        },
                        if s.device_cross_signed { "yes" } else { "no" },
                        if s.key_backup_enabled {
                            if s.key_backup_synced {
                                "active"
                            } else {
                                "enabled (syncing)"
                            }
                        } else {
                            "not enabled"
                        }
                    ));
                }
                Err(e) => status_label.set_text(&format!("Failed to load status: {e}")),
            }
            match devices {
                Ok(list) => populate_device_list(&device_list, &list, app_for_devices, &dialog),
                Err(e) => {
                    let row = adw::ActionRow::builder()
                        .title("Failed to list devices")
                        .subtitle(&e.to_string())
                        .sensitive(false)
                        .build();
                    device_list.append(&row);
                }
            }
        });
    });

    dialog.present();
}

fn populate_device_list(
    list: &gtk4::ListBox,
    devices: &[DeviceInfo],
    app_state: Arc<MatrixusApp>,
    parent: &adw::Window,
) {
    while let Some(row) = list.row_at_index(0) {
        list.remove(&row);
    }
    if devices.is_empty() {
        list.append(
            &adw::ActionRow::builder()
                .title("No devices found")
                .sensitive(false)
                .build(),
        );
        return;
    }

    for device in devices {
        let title = device
            .display_name
            .clone()
            .unwrap_or_else(|| device.device_id.clone());
        let mut subtitle = device.device_id.clone();
        if device.is_own_device {
            subtitle = format!("{subtitle} · this device");
        }
        subtitle = format!("{subtitle} · {}", device.trust.label());

        let row = adw::ActionRow::builder()
            .title(&title)
            .subtitle(&subtitle)
            .build();
        row.add_prefix(&gtk4::Image::from_icon_name(device.trust.icon_name()));

        if !device.is_own_device && device.trust == DeviceTrustLevel::Unverified {
            let verify_btn = gtk4::Button::builder()
                .label("Verify")
                .css_classes(["flat"])
                .valign(gtk4::Align::Center)
                .build();
            let app = app_state.clone();
            let device_id = device.device_id.clone();
            let parent = UiSend::new(parent.clone());
            let user_id = device.user_id.clone();
            verify_btn.connect_clicked(move |_| {
                let app = app.clone();
                let device_id = device_id.clone();
                let parent = UiSend::new(parent.0.clone());
                let user_id = user_id.clone();
                tokio::spawn(async move {
                    let result = app.start_device_verification(&device_id).await;
                    async_ui::on_ui(move || {
                        let parent = parent.into_inner();
                        match result {
                            Ok(VerificationState::ShowEmojis {
                                transaction_id,
                                emojis,
                            }) => show_sas_dialog(&parent, app, user_id, transaction_id, emojis),
                            Ok(VerificationState::Started { transaction_id }) => {
                                let app2 = app.clone();
                                let user2 = user_id.clone();
                                let parent = UiSend::new(parent);
                                tokio::spawn(async move {
                                    for _ in 0..40 {
                                        tokio::time::sleep(std::time::Duration::from_millis(500))
                                            .await;
                                        if let Ok(Some(emojis)) =
                                            app2.get_sas_emojis(&user2, &transaction_id).await
                                        {
                                            let app3 = app2.clone();
                                            let user3 = user2.clone();
                                            let tx = transaction_id.clone();
                                            async_ui::on_ui(move || {
                                                show_sas_dialog(
                                                    &parent.into_inner(),
                                                    app3,
                                                    user3,
                                                    tx,
                                                    emojis,
                                                );
                                            });
                                            return;
                                        }
                                    }
                                    error!("Timed out waiting for SAS emojis");
                                });
                            }
                            Ok(other) => error!("Unexpected verification state: {other:?}"),
                            Err(e) => error!("start_device_verification failed: {e}"),
                        }
                    });
                });
            });
            row.add_suffix(&verify_btn);
        }
        list.append(&row);
    }
}
