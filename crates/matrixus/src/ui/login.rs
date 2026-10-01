//! Login page — homeserver, username and password.

use std::rc::Rc;
use std::sync::Arc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use tracing::{error, info};

use crate::app::MatrixusApp;
use crate::ui::async_ui::{self, UiSend};

/// Callback invoked on the GTK main thread after a successful login.
pub type OnLoginSuccess = Rc<dyn Fn()>;

pub fn build_login_page(
    window: &adw::ApplicationWindow,
    app_state: Arc<MatrixusApp>,
    on_success: OnLoginSuccess,
) {
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&gtk4::Label::new(Some("Sign in"))));
    header.set_show_end_title_buttons(true);

    let homeserver_entry = adw::EntryRow::builder()
        .title("Homeserver")
        .text("https://matrix.org")
        .build();
    let user_entry = adw::EntryRow::builder().title("Username").build();
    let password_entry = adw::PasswordEntryRow::builder()
        .title("Password")
        .build();

    let form = adw::PreferencesGroup::new();
    form.set_title("Matrix account");
    form.set_description(Some("Enter your homeserver and credentials to sign in."));
    form.add(&homeserver_entry);
    form.add(&user_entry);
    form.add(&password_entry);

    let status_label = gtk4::Label::builder()
        .label("")
        .wrap(true)
        .css_classes(["error"])
        .halign(gtk4::Align::Center)
        .margin_top(8)
        .build();
    status_label.set_visible(false);

    let spinner = gtk4::Spinner::new();
    spinner.set_visible(false);

    let login_button = gtk4::Button::builder()
        .label("Sign in")
        .css_classes(["suggested-action", "pill"])
        .halign(gtk4::Align::Center)
        .margin_top(16)
        .build();
    login_button.set_size_request(180, -1);

    let button_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
    button_box.set_halign(gtk4::Align::Center);
    button_box.append(&spinner);
    button_box.append(&login_button);

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    content.set_valign(gtk4::Align::Center);
    content.set_halign(gtk4::Align::Center);
    content.set_margin_start(24);
    content.set_margin_end(24);
    content.set_size_request(360, -1);
    content.append(&form);
    content.append(&status_label);
    content.append(&button_box);

    let clamp = adw::Clamp::builder()
        .maximum_size(420)
        .child(&content)
        .build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&clamp));
    window.set_content(Some(&toolbar));
    window.set_title(Some("Sign in — Matrixus"));
    window.set_default_size(480, 420);

    let do_login = {
        let homeserver_entry = homeserver_entry.clone();
        let user_entry = user_entry.clone();
        let password_entry = password_entry.clone();
        let status_label = status_label.clone();
        let spinner = spinner.clone();
        let login_button = login_button.clone();
        let app_state = app_state.clone();
        let on_success = on_success.clone();

        Rc::new(move || {
            let homeserver = homeserver_entry.text().trim().to_string();
            let user = user_entry.text().trim().to_string();
            let password = password_entry.text().to_string();

            if homeserver.is_empty() || user.is_empty() || password.is_empty() {
                status_label.set_label("Please fill in all fields.");
                status_label.set_visible(true);
                return;
            }

            let homeserver = if homeserver.starts_with("http://")
                || homeserver.starts_with("https://")
            {
                homeserver
            } else {
                format!("https://{homeserver}")
            };

            status_label.set_visible(false);
            spinner.set_visible(true);
            spinner.start();
            login_button.set_sensitive(false);
            login_button.set_label("Signing in…");

            let app_state = app_state.clone();
            let ui = UiSend::new((
                status_label.clone(),
                spinner.clone(),
                login_button.clone(),
                UiSend::new(on_success.clone()),
            ));

            async_ui::spawn_tokio_then_ui(
                async move { app_state.login(&homeserver, &user, &password).await },
                move |result| {
                    let (status_label, spinner, login_button, on_success) = ui.into_inner();
                    let on_success = on_success.into_inner();
                    spinner.stop();
                    spinner.set_visible(false);
                    login_button.set_sensitive(true);
                    login_button.set_label("Sign in");
                    match result {
                        Ok(()) => {
                            info!("Login UI: success, transitioning to main window");
                            on_success();
                        }
                        Err(e) => {
                            error!("Login UI: failed — {e}");
                            status_label.set_label(&format!("Sign-in failed: {e}"));
                            status_label.set_visible(true);
                        }
                    }
                },
            );
        })
    };

    login_button.connect_clicked({
        let do_login = do_login.clone();
        move |_| do_login()
    });
    password_entry.connect_activate({
        let do_login = do_login.clone();
        move |_| do_login()
    });
    user_entry.connect_activate({
        let do_login = do_login.clone();
        move |_| do_login()
    });
    homeserver_entry.connect_activate({
        let do_login = do_login.clone();
        move |_| do_login()
    });
}
