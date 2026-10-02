//! Create group / create space dialogs.

use std::rc::Rc;
use std::sync::Arc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::app::MatrixusApp;
use crate::ui::async_ui;

/// Kind of room the dialog creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateKind {
    Group,
    Space,
}

impl CreateKind {
    fn title(self) -> &'static str {
        match self {
            CreateKind::Group => "Create group",
            CreateKind::Space => "Create space",
        }
    }

    fn action_label(self) -> &'static str {
        match self {
            CreateKind::Group => "Create group",
            CreateKind::Space => "Create space",
        }
    }
}

/// Show a dialog to create a group chat or a Matrix Space.
///
/// On success, `on_created` is called on the GTK main thread with the new room ID.
/// The callback stays on the UI thread (never moved into Tokio).
pub fn show_create_room_dialog(
    parent: &impl IsA<gtk4::Window>,
    app_state: Arc<MatrixusApp>,
    kind: CreateKind,
    on_created: Rc<dyn Fn(String)>,
) {
    let parent_win = parent.upcast_ref::<gtk4::Window>().clone();

    let dialog = adw::Dialog::builder()
        .title(kind.title())
        .content_width(420)
        .build();

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    toolbar.add_top_bar(&header);

    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();

    let name_row = adw::EntryRow::builder().title("Name").build();
    name_row.set_input_purpose(gtk4::InputPurpose::Name);

    let topic_row = adw::EntryRow::builder()
        .title("Topic (optional)")
        .build();

    let invite_row = adw::EntryRow::builder()
        .title("Invite users (optional)")
        .build();
    invite_row.set_tooltip_text(Some(
        "Comma-separated Matrix IDs, e.g. @alice:matrix.org, @bob:example.com",
    ));

    group.add(&name_row);
    group.add(&topic_row);
    group.add(&invite_row);

    let encrypt_row = adw::SwitchRow::builder()
        .title("Enable end-to-end encryption")
        .subtitle("Recommended for private group chats")
        .active(true)
        .build();

    let public_row = adw::SwitchRow::builder()
        .title("Public")
        .subtitle("Anyone who knows the address can join")
        .active(false)
        .build();

    match kind {
        CreateKind::Group => {
            group.add(&encrypt_row);
            group.add(&public_row);
        }
        CreateKind::Space => {
            public_row.set_subtitle("Public spaces are discoverable and joinable");
            group.add(&public_row);
        }
    }

    page.add(&group);
    toolbar.set_content(Some(&page));

    let status = gtk4::Label::builder()
        .label("")
        .css_classes(["caption"])
        .wrap(true)
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(8)
        .halign(gtk4::Align::Start)
        .build();

    let create_btn = gtk4::Button::builder()
        .label(kind.action_label())
        .css_classes(["suggested-action"])
        .margin_top(8)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();

    let bottom = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    bottom.append(&status);
    bottom.append(&create_btn);

    let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    outer.append(&toolbar);
    outer.append(&bottom);
    dialog.set_child(Some(&outer));

    {
        let dialog = dialog.clone();
        let name_row = name_row.clone();
        let topic_row = topic_row.clone();
        let invite_row = invite_row.clone();
        let encrypt_row = encrypt_row.clone();
        let public_row = public_row.clone();
        let status = status.clone();
        let create_btn = create_btn.clone();
        let app_state = app_state.clone();
        let on_created = on_created.clone();

        create_btn.connect_clicked(move |btn| {
            let name = name_row.text().to_string();
            if name.trim().is_empty() {
                status.set_label("Name is required.");
                status.add_css_class("error");
                return;
            }

            let topic = {
                let t = topic_row.text().trim().to_string();
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            };

            let invite: Vec<String> = invite_row
                .text()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            let encrypted = encrypt_row.is_active();
            let is_public = public_row.is_active();
            let _ = encrypted; // group helper is private+encrypted for now

            btn.set_sensitive(false);
            status.set_label("Creating…");
            status.remove_css_class("error");

            let app = app_state.clone();
            let status = status.clone();
            let btn = btn.clone();
            let dialog = dialog.clone();
            let on_created = on_created.clone();

            // Result is delivered back to the UI thread via a channel so GTK
            // widgets and the callback never need to be Send.
            let (tx, rx) = async_ui::ui_channel::<Result<String, String>>();
            async_ui::attach_ui_receiver(rx, move |result| {
                match result {
                    Ok(room_id) => {
                        dialog.close();
                        on_created(room_id);
                    }
                    Err(e) => {
                        status.set_label(&format!("Failed: {e}"));
                        status.add_css_class("error");
                        btn.set_sensitive(true);
                    }
                }
            });

            tokio::spawn(async move {
                let result = match kind {
                    CreateKind::Group => app
                        .create_group(&name, topic.as_deref(), &invite)
                        .await
                        .map_err(|e| e.to_string()),
                    CreateKind::Space => app
                        .create_space(&name, topic.as_deref(), &invite, is_public)
                        .await
                        .map_err(|e| e.to_string()),
                };
                let _ = tx.send(result);
            });
        });
    }

    dialog.present(Some(&parent_win));
}
