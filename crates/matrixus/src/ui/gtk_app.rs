//! GTK4 / Libadwaita application shell and main window.
//!
//! Compiled only when the `gui` feature is enabled.

use std::sync::Arc;

use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use matrix_core::{RoomSummary, SyncEvent};
use tokio::sync::broadcast;
use tracing::info;

use crate::app::MatrixusApp;
use crate::settings::AppSettings;
use crate::ui::async_ui::{self, UiSend};
use crate::ui::call::open_call_window;
use crate::ui::login;
use crate::ui::settings as settings_ui;
use crate::ui::verification;

const APP_ID: &str = "com.matrixus.Matrixus";

/// Launch the GTK4 / Libadwaita desktop application.
///
/// Blocks until the application window is closed.
pub fn run(app_state: Arc<MatrixusApp>) -> gtk4::glib::ExitCode {
    info!("Initializing libadwaita");
    libadwaita::init().expect("Failed to initialize libadwaita");

    // Apply saved theme preference
    if let Ok(s) = app_state.settings.lock() {
        settings_ui::apply_theme(s.theme);
    }

    let gtk_app = adw::Application::builder()
        .application_id(APP_ID)
        .build();

    // Application-level keyboard shortcuts
    gtk_app.set_accels_for_action("win.preferences", &["<Primary>comma"]);
    gtk_app.set_accels_for_action("win.quit", &["<Primary>q"]);
    gtk_app.set_accels_for_action("win.focus-composer", &["<Primary>l"]);
    gtk_app.set_accels_for_action("win.search-rooms", &["<Primary>k", "<Primary>f"]);

    let state = app_state.clone();
    gtk_app.connect_activate(move |app| {
        build_ui(app, state.clone());
    });

    info!("Starting GTK application main loop");
    // Empty args so cargo/test flags are not treated as GTK options.
    gtk_app.run_with_args(&[] as &[&str])
}

fn build_ui(app: &adw::Application, app_state: Arc<MatrixusApp>) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Matrixus")
        .default_width(1100)
        .default_height(700)
        .build();

    // If try_auto_login already restored a session, go straight to the main UI.
    // Otherwise show the login page.
    if app_state.has_client() {
        info!("Session already active — showing main window");
        show_main_window(&window, app_state);
    } else {
        info!("No active session — showing login page");
        let window_for_success = window.clone();
        let state_for_success = app_state.clone();
        login::build_login_page(
            &window,
            app_state,
            std::rc::Rc::new(move || {
                show_main_window(&window_for_success, state_for_success.clone());
            }),
        );
    }

    window.present();
}

/// Construct and display the main chat window (sidebar + timeline + composer).
fn show_main_window(window: &adw::ApplicationWindow, app_state: Arc<MatrixusApp>) {
    window.set_default_size(1100, 700);
    window.set_title(Some("Matrixus"));

    // ── Header bar ──────────────────────────────────────────────────────────
    let header = adw::HeaderBar::new();
    let title_label = gtk4::Label::new(Some("Matrixus"));
    header.set_title_widget(Some(&title_label));

    let menu_button = gtk4::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Main menu")
        .build();
    header.pack_end(&menu_button);

    // Primary menu model
    let menu = gio::Menu::new();
    menu.append(Some("Preferences"), Some("win.preferences"));
    menu.append(Some("Keyboard Shortcuts"), Some("win.show-help-overlay"));
    menu.append(Some("About Matrixus"), Some("win.about"));
    menu.append(Some("Quit"), Some("win.quit"));
    menu_button.set_menu_model(Some(&menu));

    // Security / encryption status (Phase 5)
    let security_button = gtk4::Button::builder()
        .icon_name("security-medium-symbolic")
        .tooltip_text("Security & devices")
        .css_classes(["flat"])
        .build();
    header.pack_end(&security_button);

    // Start video call in the selected room (Phase 6)
    let call_button = gtk4::Button::builder()
        .icon_name("video-call-symbolic")
        .tooltip_text("Start video call")
        .css_classes(["flat"])
        .sensitive(false)
        .build();
    header.pack_end(&call_button);

    let voice_call_button = gtk4::Button::builder()
        .icon_name("call-start-symbolic")
        .tooltip_text("Start voice call")
        .css_classes(["flat"])
        .sensitive(false)
        .build();
    header.pack_end(&voice_call_button);

    // Room encryption indicator (updated on room select)
    let encryption_badge = gtk4::Label::builder()
        .label("")
        .css_classes(["dim-label", "caption"])
        .margin_end(8)
        .build();
    header.pack_start(&encryption_badge);

    {
        let app = app_state.clone();
        let win = window.clone();
        security_button.connect_clicked(move |_| {
            verification::show_security_dialog(&win, app.clone());
        });
    }


    // Refresh security icon from crypto status
    {
        let app = app_state.clone();
        let btn = UiSend::new(security_button.clone());
        tokio::spawn(async move {
            if let Ok(status) = app.crypto_status().await {
                let icon = if status.is_healthy() {
                    "security-high-symbolic"
                } else if status.cross_signing_ready {
                    "security-medium-symbolic"
                } else {
                    "security-low-symbolic"
                };
                let tip = status.summary_label();
                async_ui::on_ui(move || {
                    let btn = btn.into_inner();
                    btn.set_icon_name(icon);
                    btn.set_tooltip_text(Some(&tip));
                });
            }
        });
    }

    // ── Connection status banner (Phase 4) ──────────────────────────────────
    let status_banner = adw::Banner::builder()
        .title("Reconnecting to homeserver…")
        .revealed(false)
        .build();

    // ── Search / filter ─────────────────────────────────────────────────────
    let search_entry = gtk4::SearchEntry::builder()
        .placeholder_text("Filter rooms…")
        .margin_start(8)
        .margin_end(8)
        .margin_top(8)
        .margin_bottom(4)
        .build();

    // ── Sidebar (room list) ─────────────────────────────────────────────────
    let sidebar_list = gtk4::ListBox::builder()
        .selection_mode(gtk4::SelectionMode::Single)
        .css_classes(["navigation-sidebar"])
        .build();

    // Loading placeholder while rooms are fetched
    let loading_row = adw::ActionRow::builder()
        .title("Loading rooms…")
        .subtitle("Syncing with homeserver")
        .sensitive(false)
        .build();
    sidebar_list.append(&loading_row);

    let sidebar_scrolled = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .child(&sidebar_list)
        .hexpand(true)
        .vexpand(true)
        .build();

    let sidebar_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    let sidebar_header = gtk4::Label::builder()
        .label("Rooms")
        .css_classes(["title-4"])
        .margin_top(12)
        .margin_bottom(6)
        .margin_start(12)
        .halign(gtk4::Align::Start)
        .build();
    sidebar_box.append(&sidebar_header);
    sidebar_box.append(&search_entry);
    sidebar_box.append(&sidebar_scrolled);

    let sidebar_page = adw::NavigationPage::builder()
        .title("Rooms")
        .child(&sidebar_box)
        .build();

    // ── Timeline (message area) ─────────────────────────────────────────────
    let timeline_list = gtk4::ListBox::builder()
        .selection_mode(gtk4::SelectionMode::None)
        .css_classes(["boxed-list"])
        .margin_start(12)
        .margin_end(12)
        .margin_top(8)
        .margin_bottom(8)
        .build();

    let empty_label = gtk4::Label::builder()
        .label("Select a room to start chatting")
        .css_classes(["dim-label"])
        .margin_top(48)
        .halign(gtk4::Align::Center)
        .build();
    timeline_list.append(&empty_label);

    let timeline_scrolled = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .vexpand(true)
        .child(&timeline_list)
        .build();

    // ── Composer ────────────────────────────────────────────────────────────
    let composer_entry = gtk4::Entry::builder()
        .placeholder_text("Write a message…")
        .hexpand(true)
        .sensitive(false) // enabled after a room is selected
        .build();

    let send_button = gtk4::Button::builder()
        .icon_name("mail-send-symbolic")
        .tooltip_text("Send")
        .css_classes(["suggested-action", "circular"])
        .sensitive(false)
        .build();

    let composer_box = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(8)
        .margin_start(12)
        .margin_end(12)
        .margin_top(8)
        .margin_bottom(12)
        .build();
    composer_box.append(&composer_entry);
    composer_box.append(&send_button);

    // Currently selected room (shared with selection handler and send)
    let selected_room: Arc<std::sync::Mutex<Option<String>>> =
        Arc::new(std::sync::Mutex::new(None));

    // Wire call buttons now that selected_room exists
    {
        let selected_room = selected_room.clone();
        let app = app_state.clone();
        call_button.connect_clicked(move |_| {
            let room_id = selected_room.lock().ok().and_then(|g| g.clone());
            let Some(room_id) = room_id else { return };
            let app = app.clone();
            tokio::spawn(async move {
                match app.start_video_call(&room_id).await {
                    Ok(controller) => {
                        crate::ui::async_ui::on_ui(move || {
                            let win = open_call_window(controller);
                            win.present();
                        });
                    }
                    Err(e) => tracing::error!("Failed to start video call: {e}"),
                }
            });
        });
    }
    {
        let selected_room = selected_room.clone();
        let app = app_state.clone();
        voice_call_button.connect_clicked(move |_| {
            let room_id = selected_room.lock().ok().and_then(|g| g.clone());
            let Some(room_id) = room_id else { return };
            let app = app.clone();
            tokio::spawn(async move {
                match app.start_voice_call(&room_id).await {
                    Ok(controller) => {
                        crate::ui::async_ui::on_ui(move || {
                            let win = open_call_window(controller);
                            win.present();
                        });
                    }
                    Err(e) => tracing::error!("Failed to start voice call: {e}"),
                }
            });
        });
    }

    // Event IDs already shown in the timeline (for live-update dedup)
    let known_event_ids: Arc<std::sync::Mutex<std::collections::HashSet<String>>> =
        Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));

    // Send message via network (optimistic UI + real send)
    let timeline_for_send = timeline_list.clone();
    let composer_for_send = composer_entry.clone();
    let send_button_for_send = send_button.clone();
    let selected_room_for_send = selected_room.clone();
    let app_state_for_send = app_state.clone();
    let known_for_send = known_event_ids.clone();
    send_button.connect_clicked(move |_| {
        let text = composer_for_send.text().trim().to_string();
        if text.is_empty() {
            return;
        }
        let room_id = match selected_room_for_send.lock() {
            Ok(g) => g.clone(),
            Err(_) => None,
        };
        let Some(room_id) = room_id else {
            return;
        };

        // Optimistic UI: append immediately
        clear_timeline_placeholder(&timeline_for_send);
        let optimistic = adw::ActionRow::builder()
            .title("You")
            .subtitle(&text)
            .css_classes(["success"])
            .build();
        timeline_for_send.append(&optimistic);
        composer_for_send.set_text("");

        // Disable send briefly while in flight
        send_button_for_send.set_sensitive(false);
        let send_btn = send_button_for_send.clone();
        let timeline = timeline_for_send.clone();
        let app_state = app_state_for_send.clone();
        let text_for_err = text.clone();
        let known = known_for_send.clone();

        let send_btn = UiSend::new(send_btn);
        let timeline = UiSend::new(timeline);
        tokio::spawn(async move {
            let result = app_state.send_message(&room_id, &text).await;
            async_ui::on_ui(move || {
                let send_btn = send_btn.into_inner();
                let timeline = timeline.into_inner();
                send_btn.set_sensitive(true);
                match result {
                    Ok(event_id) => {
                        if let Ok(mut set) = known.lock() {
                            set.insert(event_id);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Send failed: {e}");
                        let err_row = adw::ActionRow::builder()
                            .title("Failed to send")
                            .subtitle(&format!("{text_for_err} — {e}"))
                            .css_classes(["error"])
                            .build();
                        timeline.append(&err_row);
                    }
                }
            });
        });
    });

    // Also send on Enter in the composer
    let send_button_activate = send_button.clone();
    composer_entry.connect_activate(move |_| {
        send_button_activate.emit_clicked();
    });

    // ── Content column (timeline + composer) ────────────────────────────────
    let content_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    content_box.append(&timeline_scrolled);
    content_box.append(&composer_box);

    let content_page = adw::NavigationPage::builder()
        .title("Chat")
        .child(&content_box)
        .build();

    // ── Navigation split view (sidebar | content) ───────────────────────────
    let split = adw::NavigationSplitView::new();
    split.set_sidebar(Some(&sidebar_page));
    split.set_content(Some(&content_page));
    split.set_min_sidebar_width(240.0);
    split.set_max_sidebar_width(360.0);

    // ── Outer layout: banner above toolbar ──────────────────────────────────
    let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    outer.append(&status_banner);

    // ── Toolbar view (header + split) ───────────────────────────────────────
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&split));
    outer.append(&toolbar);

    window.set_content(Some(&outer));
    info!("Main window presented — loading rooms & starting sync");

    // ── Window actions (shortcuts + menu) ───────────────────────────────────
    {
        let settings = app_state.settings.clone();
        let app_for_settings = app_state.clone();
        let win = window.clone();
        let preferences = gio::SimpleAction::new("preferences", None);
        preferences.connect_activate(move |_, _| {
            let settings = settings.clone();
            let app = app_for_settings.clone();
            let on_changed = Arc::new(move |s: AppSettings| {
                settings_ui::apply_theme(s.theme);
                app.notifications.set_messages_enabled(s.notifications_enabled);
                app.notifications.set_calls_enabled(s.call_notifications_enabled);
                // Propagate Element Call URL to environment for new call windows
                std::env::set_var("ELEMENT_CALL_URL", &s.element_call_url);
            });
            settings_ui::show_settings_window(&win, settings, on_changed);
        });
        window.add_action(&preferences);

        let win = window.clone();
        let about = gio::SimpleAction::new("about", None);
        about.connect_activate(move |_, _| {
            let dialog = adw::AboutDialog::builder()
                .application_name("Matrixus")
                .application_icon(APP_ID)
                .developer_name("Matrixus Developers")
                .version(env!("CARGO_PKG_VERSION"))
                .comments("Native Linux Matrix client with MatrixRTC calls")
                .website("https://github.com/dev-lukaszglowacki/matrixus")
                .issue_url("https://github.com/dev-lukaszglowacki/matrixus/issues")
                .license_type(gtk4::License::Apache20)
                .build();
            dialog.present(Some(&win));
        });
        window.add_action(&about);

        let win = window.clone();
        let quit = gio::SimpleAction::new("quit", None);
        quit.connect_activate(move |_, _| {
            win.application().map(|app| app.quit());
        });
        window.add_action(&quit);

        let composer_for_focus = composer_entry.clone();
        let focus_composer = gio::SimpleAction::new("focus-composer", None);
        focus_composer.connect_activate(move |_, _| {
            composer_for_focus.grab_focus();
        });
        window.add_action(&focus_composer);

        let search_for_focus = search_entry.clone();
        let search_rooms = gio::SimpleAction::new("search-rooms", None);
        search_rooms.connect_activate(move |_, _| {
            search_for_focus.grab_focus();
        });
        window.add_action(&search_rooms);

        // Shortcuts window
        let shortcuts = gio::SimpleAction::new("show-help-overlay", None);
        let win = window.clone();
        shortcuts.connect_activate(move |_, _| {
            let overlay = build_shortcuts_window();
            overlay.set_transient_for(Some(&win));
            overlay.present();
        });
        window.add_action(&shortcuts);
    }

    // Close-to-background (tray-style) when enabled in settings
    {
        let settings = app_state.settings.clone();
        window.connect_close_request(move |win| {
            let close_to_tray = settings
                .lock()
                .map(|s| s.close_to_tray)
                .unwrap_or(false);
            if close_to_tray {
                win.set_visible(false);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
    }

    // Accessibility: tooltips + accessible names on primary chrome
    search_entry.set_tooltip_text(Some("Filter rooms"));
    composer_entry.set_tooltip_text(Some("Write a message"));
    send_button.set_tooltip_text(Some("Send message"));
    call_button.set_tooltip_text(Some("Start video call"));
    voice_call_button.set_tooltip_text(Some("Start voice call"));
    security_button.set_tooltip_text(Some("Security and devices"));
    sidebar_list.set_accessible_role(gtk4::AccessibleRole::List);
    timeline_list.set_accessible_role(gtk4::AccessibleRole::List);

    // Keep a shared copy of the full room list for client-side filtering
    let all_rooms: Arc<std::sync::Mutex<Vec<RoomSummary>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

    // ── Initial room fetch ──────────────────────────────────────────────────
    let sidebar_list_for_fetch = sidebar_list.clone();
    let title_label_for_select = title_label.clone();
    let timeline_list_for_select = timeline_list.clone();
    let composer_entry_for_select = composer_entry.clone();
    let send_button_for_select = send_button.clone();
    let app_state_for_select = app_state.clone();
    let search_entry_for_filter = search_entry.clone();
    let all_rooms_for_fetch = all_rooms.clone();
    let selected_room_for_fetch = selected_room.clone();
    let known_for_fetch = known_event_ids.clone();
    let encryption_badge_for_fetch = encryption_badge.clone();
    let call_button_for_fetch = call_button.clone();
    let voice_call_button_for_fetch = voice_call_button.clone();

    let ui_fetch = UiSend::new((
        sidebar_list_for_fetch,
        title_label_for_select,
        timeline_list_for_select,
        composer_entry_for_select,
        send_button_for_select,
        selected_room_for_fetch,
        all_rooms_for_fetch,
        known_for_fetch,
        encryption_badge_for_fetch,
        call_button_for_fetch,
        voice_call_button_for_fetch,
        search_entry_for_filter,
        app_state_for_select.clone(),
    ));

    tokio::spawn(async move {
        let result = app_state_for_select.fetch_rooms().await;

        // Start continuous sync after the initial room load
        app_state_for_select.start_sync().await;

        async_ui::on_ui(move || {
            let (
                sidebar_list_for_fetch,
                title_label_for_select,
                timeline_list_for_select,
                composer_entry_for_select,
                send_button_for_select,
                selected_room_for_fetch,
                all_rooms_for_fetch,
                known_for_fetch,
                encryption_badge_for_fetch,
                call_button_for_fetch,
                voice_call_button_for_fetch,
                search_entry_for_filter,
                app_state_for_select,
            ) = ui_fetch.into_inner();

            // Clear loading placeholder
            while let Some(row) = sidebar_list_for_fetch.row_at_index(0) {
                sidebar_list_for_fetch.remove(&row);
            }

            match result {
                Ok(rooms) => {
                    if let Ok(mut guard) = all_rooms_for_fetch.lock() {
                        *guard = rooms.clone();
                    }
                    populate_sidebar(
                        &sidebar_list_for_fetch,
                        &rooms,
                        &title_label_for_select,
                        &timeline_list_for_select,
                        &composer_entry_for_select,
                        &send_button_for_select,
                        &selected_room_for_fetch,
                        app_state_for_select.clone(),
                        known_for_fetch.clone(),
                        &encryption_badge_for_fetch,
                        &call_button_for_fetch,
                        &voice_call_button_for_fetch,
                    );

                    // Wire search filter
                    let list = sidebar_list_for_fetch.clone();
                    let rooms_ref = all_rooms_for_fetch.clone();
                    let title = title_label_for_select.clone();
                    let timeline = timeline_list_for_select.clone();
                    let composer = composer_entry_for_select.clone();
                    let send = send_button_for_select.clone();
                    let selected = selected_room_for_fetch.clone();
                    let state = app_state_for_select.clone();
                    let known = known_for_fetch.clone();
                    let enc_badge = encryption_badge_for_fetch.clone();
                    let call_btn = call_button_for_fetch.clone();
                    let voice_btn = voice_call_button_for_fetch.clone();
                    search_entry_for_filter.connect_search_changed(move |entry| {
                        let query = entry.text().to_lowercase();
                        let filtered: Vec<RoomSummary> = rooms_ref
                            .lock()
                            .map(|r| {
                                r.iter()
                                    .filter(|room| {
                                        query.is_empty()
                                            || room.name.to_lowercase().contains(&query)
                                            || room.room_id.to_lowercase().contains(&query)
                                    })
                                    .cloned()
                                    .collect()
                            })
                            .unwrap_or_default();
                        while let Some(row) = list.row_at_index(0) {
                            list.remove(&row);
                        }
                        populate_sidebar(
                            &list,
                            &filtered,
                            &title,
                            &timeline,
                            &composer,
                            &send,
                            &selected,
                            state.clone(),
                            known.clone(),
                            &enc_badge,
                            &call_btn,
                            &voice_btn,
                        );
                    });
                }
                Err(e) => {
                    let err_row = adw::ActionRow::builder()
                        .title("Failed to load rooms")
                        .subtitle(&e.to_string())
                        .sensitive(false)
                        .build();
                    sidebar_list_for_fetch.append(&err_row);
                }
            }
        });
    });


    // ── Sync event bridge (Phase 4) ─────────────────────────────────────────
    // Tokio only forwards SyncEvent values (Send). All widget updates run on the
    // GTK main thread via a glib channel.
    {
        let (event_tx, event_rx) = async_ui::ui_channel::<SyncEvent>();

        let sidebar_list = sidebar_list.clone();
        let title_label = title_label.clone();
        let timeline_list = timeline_list.clone();
        let composer_entry = composer_entry.clone();
        let send_button = send_button.clone();
        let selected_room = selected_room.clone();
        let all_rooms = all_rooms.clone();
        let known_event_ids = known_event_ids.clone();
        let search_entry = search_entry.clone();
        let encryption_badge = encryption_badge.clone();
        let call_button = call_button.clone();
        let voice_call_button = voice_call_button.clone();
        let status_banner = status_banner.clone();
        let window = window.clone();
        let app_state = app_state.clone();
        let app_state_sync = app_state.clone();

        async_ui::attach_ui_receiver(event_rx, move |event| {
            match event {
                SyncEvent::ConnectionLost { message } => {
                    status_banner.set_title(&format!("Offline — {message}"));
                    status_banner.set_revealed(true);
                }
                SyncEvent::ConnectionRestored => {
                    status_banner.set_revealed(false);
                }
                SyncEvent::RoomListUpdated => {
                    let selected_id = selected_room.lock().ok().and_then(|g| g.clone());
                    let app = app_state.clone();
                    let sidebar_list = sidebar_list.clone();
                    let title_label = title_label.clone();
                    let timeline_list = timeline_list.clone();
                    let composer_entry = composer_entry.clone();
                    let send_button = send_button.clone();
                    let selected_room = selected_room.clone();
                    let all_rooms = all_rooms.clone();
                    let known = known_event_ids.clone();
                    let search_entry = search_entry.clone();
                    let encryption_badge = encryption_badge.clone();
                    let call_button = call_button.clone();
                    let voice_call_button = voice_call_button.clone();

                    let ui = UiSend::new((
                        sidebar_list,
                        title_label,
                        timeline_list,
                        composer_entry,
                        send_button,
                        selected_room,
                        all_rooms,
                        known,
                        search_entry,
                        encryption_badge,
                        call_button,
                        voice_call_button,
                        app.clone(),
                    ));

                    tokio::spawn(async move {
                        let rooms_result = app.refresh_rooms().await;
                        let new_events = if let Some(ref rid) = selected_id {
                            match app.load_timeline(rid, 20).await {
                                Ok(events) => Some((rid.clone(), events)),
                                Err(e) => {
                                    tracing::warn!("Live timeline refresh failed: {e}");
                                    None
                                }
                            }
                        } else {
                            None
                        };

                        async_ui::on_ui(move || {
                            let (
                                sidebar_list,
                                title_label,
                                timeline_list,
                                composer_entry,
                                send_button,
                                selected_room,
                                all_rooms,
                                known,
                                search_entry,
                                encryption_badge,
                                call_button,
                                voice_call_button,
                                app_state,
                            ) = ui.into_inner();

                            if let Ok(rooms) = rooms_result {
                                if let Ok(mut guard) = all_rooms.lock() {
                                    *guard = rooms.clone();
                                }
                                let query = search_entry.text().to_lowercase();
                                let filtered: Vec<RoomSummary> = if query.is_empty() {
                                    rooms
                                } else {
                                    rooms
                                        .into_iter()
                                        .filter(|room| {
                                            room.name.to_lowercase().contains(&query)
                                                || room.room_id.to_lowercase().contains(&query)
                                        })
                                        .collect()
                                };
                                while let Some(row) = sidebar_list.row_at_index(0) {
                                    sidebar_list.remove(&row);
                                }
                                populate_sidebar(
                                    &sidebar_list,
                                    &filtered,
                                    &title_label,
                                    &timeline_list,
                                    &composer_entry,
                                    &send_button,
                                    &selected_room,
                                    app_state,
                                    known.clone(),
                                    &encryption_badge,
                                    &call_button,
                                    &voice_call_button,
                                );
                            }

                            if let Some((rid, events)) = new_events {
                                let still_selected = selected_room
                                    .lock()
                                    .ok()
                                    .and_then(|g| g.clone())
                                    .as_ref()
                                    == Some(&rid);
                                if still_selected {
                                    for ev in events {
                                        let is_new = known
                                            .lock()
                                            .map(|set| !set.contains(&ev.event_id))
                                            .unwrap_or(true);
                                        if is_new {
                                            if let Ok(mut set) = known.lock() {
                                                set.insert(ev.event_id.clone());
                                            }
                                            clear_timeline_placeholder(&timeline_list);
                                            timeline_list.append(&event_to_row(&ev));
                                        }
                                    }
                                }
                            }
                        });
                    });
                }
                SyncEvent::TimelineUpdated { room_id, event } => {
                    let sel = selected_room.lock().ok().and_then(|g| g.clone());
                    let is_active = sel.as_ref() == Some(&room_id);
                    if is_active {
                        let is_new = known_event_ids
                            .lock()
                            .map(|set| !set.contains(&event.event_id))
                            .unwrap_or(true);
                        if is_new {
                            if let Ok(mut set) = known_event_ids.lock() {
                                set.insert(event.event_id.clone());
                            }
                            clear_timeline_placeholder(&timeline_list);
                            timeline_list.append(&event_to_row(&event));
                        }
                    } else {
                        let preview = event.content.preview_text().to_string();
                        let sender = event.sender.clone();
                        app_state.notifications.show_message_notification(
                            &room_id, &sender, &preview,
                        );
                    }
                }
                SyncEvent::VerificationChanged(state) => {
                    use matrix_core::VerificationState;
                    match state {
                        VerificationState::Requested {
                            transaction_id,
                            other_user,
                            other_device,
                        } => {
                            verification::show_verification_request_dialog(
                                &window,
                                app_state.clone(),
                                other_user,
                                other_device,
                                transaction_id,
                            );
                        }
                        VerificationState::ShowEmojis {
                            transaction_id,
                            emojis,
                        } => {
                            let other_user = app_state
                                .client
                                .try_lock()
                                .ok()
                                .and_then(|g| g.as_ref().and_then(|c| c.user_id()))
                                .unwrap_or_default();
                            verification::show_sas_dialog(
                                &window,
                                app_state.clone(),
                                other_user,
                                transaction_id,
                                emojis,
                            );
                        }
                        _ => {}
                    }
                }
                SyncEvent::CallStateChanged {
                    room_id,
                    has_active_call,
                } => {
                    if has_active_call {
                        let app = app_state.clone();
                        let win = window.clone();
                        let rid = room_id.clone();
                        let dialog = adw::AlertDialog::builder()
                            .heading("Incoming video call")
                            .body(&format!("Incoming call in {rid}"))
                            .build();
                        dialog.add_response("decline", "Decline");
                        dialog.add_response("accept", "Accept");
                        dialog.set_response_appearance(
                            "accept",
                            adw::ResponseAppearance::Suggested,
                        );
                        dialog.set_default_response(Some("accept"));
                        dialog.set_close_response("decline");
                        dialog.connect_response(None, move |dialog, response| {
                            dialog.close();
                            if response == "accept" {
                                let app = app.clone();
                                let rid = rid.clone();
                                tokio::spawn(async move {
                                    match app.accept_incoming_call(&rid, true).await {
                                        Ok(controller) => {
                                            async_ui::on_ui(move || {
                                                let w = open_call_window(controller);
                                                w.present();
                                            });
                                        }
                                        Err(e) => {
                                            tracing::error!("Accept call failed: {e}")
                                        }
                                    }
                                });
                            }
                        });
                        dialog.present(Some(&win));
                    }
                }
                SyncEvent::SyncError(_) => {}
            }
        });

        tokio::spawn(async move {
            let mut rx = loop {
                if let Some(r) = app_state_sync.subscribe_sync().await {
                    break r;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            };
            info!("UI subscribed to sync events");
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if event_tx.send(event).is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Sync event receiver lagged by {n} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Sync event channel closed");
                        break;
                    }
                }
            }
        });
    }
}


/// Fill the sidebar ListBox with room rows and wire selection handlers.
fn populate_sidebar(
    sidebar_list: &gtk4::ListBox,
    rooms: &[RoomSummary],
    title_label: &gtk4::Label,
    timeline_list: &gtk4::ListBox,
    composer_entry: &gtk4::Entry,
    send_button: &gtk4::Button,
    selected_room: &Arc<std::sync::Mutex<Option<String>>>,
    app_state: Arc<MatrixusApp>,
    known_event_ids: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    encryption_badge: &gtk4::Label,
    call_button: &gtk4::Button,
    voice_call_button: &gtk4::Button,
) {
    if rooms.is_empty() {
        let empty = adw::ActionRow::builder()
            .title("No rooms yet")
            .subtitle("Join a room on your homeserver")
            .sensitive(false)
            .build();
        sidebar_list.append(&empty);
        return;
    }

    for room in rooms {
        let preview = room
            .last_event
            .as_ref()
            .map(|e| e.content.preview_text().to_string())
            .or_else(|| room.topic.clone())
            .unwrap_or_else(|| {
                if room.is_direct {
                    "Direct message".to_string()
                } else {
                    room.room_id.clone()
                }
            });

        let mut title = room.name.clone();
        if room.is_encrypted {
            title = format!("🔒 {title}");
        }
        if room.unread_notifications > 0 {
            title = format!("{title} ({})", room.unread_notifications);
        }

        let row = adw::ActionRow::builder()
            .title(&title)
            .subtitle(&preview)
            .activatable(true)
            .build();

        let room_id = room.room_id.clone();
        let room_name = room.name.clone();
        let room_encrypted = room.is_encrypted;
        let title_label = title_label.clone();
        let timeline_list = timeline_list.clone();
        let composer_entry = composer_entry.clone();
        let send_button = send_button.clone();
        let selected_room = selected_room.clone();
        let app_state = app_state.clone();
        let known = known_event_ids.clone();
        let encryption_badge = encryption_badge.clone();
        let call_button = call_button.clone();
        let voice_call_button = voice_call_button.clone();

        row.connect_activated(move |_row| {
            info!("Selected room: {room_name} ({room_id})");
            title_label.set_text(&room_name);
            if room_encrypted {
                encryption_badge.set_text("🔒 Encrypted");
                encryption_badge.set_tooltip_text(Some("This room uses end-to-end encryption"));
            } else {
                encryption_badge.set_text("Unencrypted");
                encryption_badge.set_tooltip_text(Some("Messages in this room are not encrypted"));
            }

            if let Ok(mut g) = selected_room.lock() {
                *g = Some(room_id.clone());
            }

            // Reset known event set for the new room
            if let Ok(mut set) = known.lock() {
                set.clear();
            }

            // Show loading state while history is fetched
            while let Some(child) = timeline_list.row_at_index(0) {
                timeline_list.remove(&child);
            }
            let loading = gtk4::Label::builder()
                .label("Loading messages…")
                .css_classes(["dim-label"])
                .margin_top(48)
                .halign(gtk4::Align::Center)
                .build();
            let loading_row = gtk4::ListBoxRow::new();
            loading_row.set_child(Some(&loading));
            loading_row.set_activatable(false);
            loading_row.set_selectable(false);
            timeline_list.append(&loading_row);

            composer_entry.set_sensitive(true);
            send_button.set_sensitive(true);
            call_button.set_sensitive(true);
            voice_call_button.set_sensitive(true);
            composer_entry.grab_focus();

            let app_state = app_state.clone();
            let room_id_open = room_id.clone();
            let room_id_tl = room_id.clone();
            let timeline_list = UiSend::new(timeline_list.clone());
            let known = known.clone();

            tokio::spawn(async move {
                app_state.open_room(&room_id_open).await;
                let result = app_state.load_timeline(&room_id_tl, 50).await;

                async_ui::on_ui(move || {
                    let timeline_list = timeline_list.into_inner();
                    while let Some(child) = timeline_list.row_at_index(0) {
                        timeline_list.remove(&child);
                    }

                    match result {
                        Ok(events) => {
                            if events.is_empty() {
                                let empty = gtk4::Label::builder()
                                    .label("No messages yet — say hello!")
                                    .css_classes(["dim-label"])
                                    .margin_top(48)
                                    .halign(gtk4::Align::Center)
                                    .build();
                                let row = gtk4::ListBoxRow::new();
                                row.set_child(Some(&empty));
                                row.set_activatable(false);
                                row.set_selectable(false);
                                timeline_list.append(&row);
                            } else {
                                if let Ok(mut set) = known.lock() {
                                    for ev in &events {
                                        set.insert(ev.event_id.clone());
                                    }
                                }
                                for event in &events {
                                    timeline_list.append(&event_to_row(event));
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to load timeline: {e}");
                            let err = gtk4::Label::builder()
                                .label(&format!("Failed to load messages:\n{e}"))
                                .css_classes(["error"])
                                .margin_top(48)
                                .halign(gtk4::Align::Center)
                                .justify(gtk4::Justification::Center)
                                .build();
                            let row = gtk4::ListBoxRow::new();
                            row.set_child(Some(&err));
                            row.set_activatable(false);
                            row.set_selectable(false);
                            timeline_list.append(&row);
                        }
                    }
                });
            });
        });

        sidebar_list.append(&row);
    }
}

/// Render a single timeline event as an ActionRow.
fn event_to_row(event: &matrix_core::TimelineEvent) -> adw::ActionRow {
    use matrix_core::EventContent;

    let sender_short = event
        .sender
        .split(':')
        .next()
        .unwrap_or(&event.sender)
        .trim_start_matches('@');

    let body = match &event.content {
        EventContent::Text { body, .. } => body.clone(),
        EventContent::Emote { body } => format!("* {sender_short} {body}"),
        EventContent::Notice { body } => body.clone(),
        EventContent::Image { filename, .. } => format!("📷 {filename}"),
        EventContent::Video { filename, .. } => format!("🎥 {filename}"),
        EventContent::Audio { filename, .. } => format!("🎵 {filename}"),
        EventContent::File { filename, .. } => format!("📎 {filename}"),
        EventContent::CallMember { active: true, .. } => "📞 Active call".into(),
        EventContent::CallMember { active: false, .. } => "📞 Call ended".into(),
        EventContent::Redacted => "🗑️ Message removed".into(),
    };

    adw::ActionRow::builder()
        .title(sender_short)
        .subtitle(&body)
        .build()
}

/// Remove the dim-label placeholder row if present at index 0.
fn clear_timeline_placeholder(timeline_list: &gtk4::ListBox) {
    if let Some(first) = timeline_list.row_at_index(0) {
        let is_placeholder = first
            .child()
            .and_then(|c| c.downcast::<gtk4::Label>().ok())
            .is_some();
        if is_placeholder {
            timeline_list.remove(&first);
        }
    }
}


/// Build a GTK ShortcutsWindow documenting primary keybindings.
fn build_shortcuts_window() -> gtk4::ShortcutsWindow {
    let window = gtk4::ShortcutsWindow::builder()
        .modal(true)
        .build();

    let section = gtk4::ShortcutsSection::builder()
        .section_name("shortcuts")
        .title("Shortcuts")
        .build();

    let general = gtk4::ShortcutsGroup::builder()
        .title("General")
        .build();
    general.add_shortcut(
        &gtk4::ShortcutsShortcut::builder()
            .title("Preferences")
            .accelerator("<Primary>comma")
            .build(),
    );
    general.add_shortcut(
        &gtk4::ShortcutsShortcut::builder()
            .title("Quit")
            .accelerator("<Primary>q")
            .build(),
    );
    general.add_shortcut(
        &gtk4::ShortcutsShortcut::builder()
            .title("Filter rooms")
            .accelerator("<Primary>k")
            .build(),
    );
    general.add_shortcut(
        &gtk4::ShortcutsShortcut::builder()
            .title("Focus message composer")
            .accelerator("<Primary>l")
            .build(),
    );

    let messaging = gtk4::ShortcutsGroup::builder()
        .title("Messaging")
        .build();
    messaging.add_shortcut(
        &gtk4::ShortcutsShortcut::builder()
            .title("Send message")
            .accelerator("Return")
            .build(),
    );

    section.add_group(&general);
    section.add_group(&messaging);
    window.add_section(&section);
    window
}
