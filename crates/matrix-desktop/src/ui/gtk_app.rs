//! GTK4 / Libadwaita application shell and main window.
//!
//! Compiled only when the `gui` feature is enabled.

use std::sync::Arc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use tracing::info;

use crate::app::MatrixDesktopApp;

const APP_ID: &str = "com.example.MatrixClient";

/// Launch the GTK4 / Libadwaita desktop application.
///
/// Blocks until the application window is closed.
pub fn run(app_state: Arc<MatrixDesktopApp>) -> gtk4::glib::ExitCode {
    info!("Initializing libadwaita");
    libadwaita::init().expect("Failed to initialize libadwaita");

    let gtk_app = adw::Application::builder()
        .application_id(APP_ID)
        .build();

    let state = app_state.clone();
    gtk_app.connect_activate(move |app| {
        build_ui(app, state.clone());
    });

    info!("Starting GTK application main loop");
    // Empty args so cargo/test flags are not treated as GTK options.
    gtk_app.run_with_args(&[] as &[&str])
}

fn build_ui(app: &adw::Application, _app_state: Arc<MatrixDesktopApp>) {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Matrix Client")
        .default_width(1100)
        .default_height(700)
        .build();

    // ── Header bar ──────────────────────────────────────────────────────────
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&gtk4::Label::new(Some("Matrix Client"))));

    let menu_button = gtk4::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .tooltip_text("Menu")
        .build();
    header.pack_end(&menu_button);

    // ── Sidebar (room list) ─────────────────────────────────────────────────
    let sidebar_list = gtk4::ListBox::builder()
        .selection_mode(gtk4::SelectionMode::Single)
        .css_classes(["navigation-sidebar"])
        .build();

    // Placeholder rows so the window is not empty on first launch
    for (name, preview) in [
        ("Welcome", "Sign in to see your rooms"),
        ("#matrix:matrix.org", "Matrix HQ"),
        ("#element-web:matrix.org", "Element Web"),
    ] {
        let row = adw::ActionRow::builder()
            .title(name)
            .subtitle(preview)
            .activatable(true)
            .build();
        sidebar_list.append(&row);
    }

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
        .label("Select a room or sign in to start chatting")
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
        .build();

    let send_button = gtk4::Button::builder()
        .icon_name("mail-send-symbolic")
        .tooltip_text("Send")
        .css_classes(["suggested-action", "circular"])
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

    // Local preview: append typed text as a row (no network yet)
    let timeline_for_send = timeline_list.clone();
    send_button.connect_clicked(move |_| {
        let text = composer_entry.text();
        if text.is_empty() {
            return;
        }
        // Drop the empty-state placeholder if it is still the first row
        if let Some(first) = timeline_for_send.row_at_index(0) {
            if first
                .child()
                .and_then(|c| c.downcast::<gtk4::Label>().ok())
                .is_some()
            {
                timeline_for_send.remove(&first);
            }
        }
        let row = adw::ActionRow::builder()
            .title("You")
            .subtitle(text.as_str())
            .build();
        timeline_for_send.append(&row);
        composer_entry.set_text("");
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

    // ── Toolbar view (header + split) ───────────────────────────────────────
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&split));

    window.set_content(Some(&toolbar));
    window.present();

    info!("Main window presented");
}
