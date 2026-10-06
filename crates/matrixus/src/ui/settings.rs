//! Settings / Preferences page (Phase 7).
//!
//! Compiled only when the `gui` feature is enabled.

use std::sync::{Arc, Mutex};

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::settings::{AppSettings, ThemePreference};

/// Open a preferences window bound to shared settings.
pub fn show_settings_window(
    parent: &impl IsA<gtk4::Window>,
    settings: Arc<Mutex<AppSettings>>,
    on_changed: Arc<dyn Fn(AppSettings) + Send + Sync>,
) {
    let parent_win = parent.upcast_ref::<gtk4::Window>().clone();

    let window = adw::PreferencesDialog::builder()
        .search_enabled(true)
        .title("Preferences")
        .build();

    // ── General ─────────────────────────────────────────────────────────────
    let general = adw::PreferencesPage::builder()
        .title("General")
        .icon_name("preferences-system-symbolic")
        .build();

    let appearance = adw::PreferencesGroup::builder()
        .title("Appearance")
        .build();

    let theme_row = adw::ComboRow::builder()
        .title("Color scheme")
        .subtitle("Follow the system theme or force light/dark")
        .build();
    let theme_model = gtk4::StringList::new(&["System", "Light", "Dark"]);
    theme_row.set_model(Some(&theme_model));
    {
        let s = settings.lock().ok();
        let idx = match s.as_ref().map(|g| g.theme) {
            Some(ThemePreference::System) | None => 0,
            Some(ThemePreference::Light) => 1,
            Some(ThemePreference::Dark) => 2,
        };
        theme_row.set_selected(idx);
    }
    appearance.add(&theme_row);

    let messaging = adw::PreferencesGroup::builder()
        .title("Messaging")
        .build();

    let enter_row = adw::SwitchRow::builder()
        .title("Enter to send")
        .subtitle("Press Enter to send messages; use Shift+Enter for a new line")
        .build();
    if let Ok(s) = settings.lock() {
        enter_row.set_active(s.enter_to_send);
    }
    messaging.add(&enter_row);

    general.add(&appearance);
    general.add(&messaging);

    // ── Notifications ───────────────────────────────────────────────────────
    let notif_page = adw::PreferencesPage::builder()
        .title("Notifications")
        .icon_name("preferences-system-notifications-symbolic")
        .build();

    let notif_group = adw::PreferencesGroup::builder()
        .title("Desktop notifications")
        .build();

    let msg_notif_row = adw::SwitchRow::builder()
        .title("Message notifications")
        .subtitle("Show a notification when a new message arrives")
        .build();
    if let Ok(s) = settings.lock() {
        msg_notif_row.set_active(s.notifications_enabled);
    }
    notif_group.add(&msg_notif_row);

    let call_notif_row = adw::SwitchRow::builder()
        .title("Call notifications")
        .subtitle("Alert when an incoming call is detected")
        .build();
    if let Ok(s) = settings.lock() {
        call_notif_row.set_active(s.call_notifications_enabled);
    }
    notif_group.add(&call_notif_row);

    notif_page.add(&notif_group);

    // ── Desktop ─────────────────────────────────────────────────────────────
    let desktop_page = adw::PreferencesPage::builder()
        .title("Desktop")
        .icon_name("computer-symbolic")
        .build();

    let window_group = adw::PreferencesGroup::builder()
        .title("Window")
        .build();

    let tray_row = adw::SwitchRow::builder()
        .title("Close to background")
        .subtitle("Hide the main window on close instead of quitting (tray-style)")
        .build();
    if let Ok(s) = settings.lock() {
        tray_row.set_active(s.close_to_tray);
    }
    window_group.add(&tray_row);

    let calls_group = adw::PreferencesGroup::builder()
        .title("Calls")
        .build();

    let call_url_row = adw::EntryRow::builder()
        .title("STUN / Call server (optional)")
        .build();
    if let Ok(s) = settings.lock() {
        call_url_row.set_text(&s.wire_call_url);
    }
    calls_group.add(&call_url_row);

    desktop_page.add(&window_group);
    desktop_page.add(&calls_group);

    // ── About ───────────────────────────────────────────────────────────────
    let about_page = adw::PreferencesPage::builder()
        .title("About")
        .icon_name("help-about-symbolic")
        .build();

    let about_group = adw::PreferencesGroup::builder()
        .title("Matrixus")
        .description("Native Linux Matrix client · Apache-2.0")
        .build();

    let version_row = adw::ActionRow::builder()
        .title("Version")
        .subtitle(env!("CARGO_PKG_VERSION"))
        .build();
    about_group.add(&version_row);

    let repo_row = adw::ActionRow::builder()
        .title("Source")
        .subtitle("https://github.com/dev-lukaszglowacki/matrixus")
        .build();
    about_group.add(&repo_row);

    about_page.add(&about_group);

    window.add(&general);
    window.add(&notif_page);
    window.add(&desktop_page);
    window.add(&about_page);

    // Persist helpers
    let persist = {
        let settings = settings.clone();
        let on_changed = on_changed.clone();
        move |mutate: Box<dyn FnOnce(&mut AppSettings)>| {
            if let Ok(mut s) = settings.lock() {
                mutate(&mut s);
                s.save();
                on_changed(s.clone());
            }
        }
    };

    {
        let persist = persist.clone();
        theme_row.connect_selected_notify(move |row| {
            let theme = match row.selected() {
                1 => ThemePreference::Light,
                2 => ThemePreference::Dark,
                _ => ThemePreference::System,
            };
            persist(Box::new(move |s| s.theme = theme));
        });
    }
    {
        let persist = persist.clone();
        enter_row.connect_active_notify(move |row| {
            let v = row.is_active();
            persist(Box::new(move |s| s.enter_to_send = v));
        });
    }
    {
        let persist = persist.clone();
        msg_notif_row.connect_active_notify(move |row| {
            let v = row.is_active();
            persist(Box::new(move |s| s.notifications_enabled = v));
        });
    }
    {
        let persist = persist.clone();
        call_notif_row.connect_active_notify(move |row| {
            let v = row.is_active();
            persist(Box::new(move |s| s.call_notifications_enabled = v));
        });
    }
    {
        let persist = persist.clone();
        tray_row.connect_active_notify(move |row| {
            let v = row.is_active();
            persist(Box::new(move |s| s.close_to_tray = v));
        });
    }
    {
        let persist = persist.clone();
        call_url_row.connect_changed(move |row| {
            let v = row.text().to_string();
            persist(Box::new(move |s| s.wire_call_url = v));
        });
    }

    window.present(Some(&parent_win));
}

/// Apply theme preference to the global Adw style manager.
pub fn apply_theme(theme: ThemePreference) {
    let manager = adw::StyleManager::default();
    match theme {
        ThemePreference::System => {
            manager.set_color_scheme(adw::ColorScheme::Default);
        }
        ThemePreference::Light => {
            manager.set_color_scheme(adw::ColorScheme::ForceLight);
        }
        ThemePreference::Dark => {
            manager.set_color_scheme(adw::ColorScheme::ForceDark);
        }
    }
}
