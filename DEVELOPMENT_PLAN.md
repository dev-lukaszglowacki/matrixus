# Matrixus — GUI Development Plan

**Repository:** https://github.com/dev-lukaszglowacki/matrixus  
**Stack:** Rust · matrix-rust-sdk 0.19 · GTK4 · Libadwaita · MatrixRTC  
**Last updated:** 2026-10-01

---

## Current State

| Crate            | Role                                      | Status                                      |
|------------------|-------------------------------------------|---------------------------------------------|
| `matrix-core`    | SDK wrapper, login, E2EE, sync, SQLite    | Solid foundation                            |
| `matrix-call`    | MatrixRTC state machine + Element Call widget bridge | Scaffolded                         |
| `matrixus` | GTK4 / Libadwaita UI, notifications, portals | Phases 1–7 GUI complete |

**What works today**
- Login page + session restore (`FileSessionStore`)
- Real room list with search, unread badges, encrypted indicator
- Timeline history load + send messages (optimistic UI)
- Continuous background sync with live sidebar/timeline updates
- Offline / reconnect status banner
- Encryption status badge, security dialog, SAS device verification
- MatrixRTC calls via Element Call widget (WebKitGTK) + portal media controls

**What is missing**
- Full StatusNotifierItem system tray applet
- Broader automated UI tests / packaging CI

---

## Phase Overview

### Phase 0 – Project Hygiene
- Confirm `cargo build -p matrixus --features gui`
- Convenience `RUST_LOG` / Makefile targets
- Keep Flatpak manifest (GNOME 50) in sync

### Phase 1 – Authentication & Session Flow  ← **DONE**
**Goal:** User can log in; session is restored on next start.

- [x] Create `ui/login.rs` — Adw login page (homeserver, user, password)
- [x] Wire `MatrixusApp::login` and `try_auto_login` to the UI
- [x] Show loading / “Signing in…” state during login
- [x] On success → transition to main window
- [x] Start sync service after successful login (Phase 4)
- Session already persisted via `FileSessionStore`

### Phase 2 – Real Room List & Navigation  ← **DONE**
**Goal:** Sidebar shows the user’s actual rooms.

- [x] Drive `SidebarState` from `MatrixClient::list_rooms()` (via `fetch_rooms` + one-shot sync)
- [x] Room selection updates `MainWindowState` and header / timeline placeholder
- [x] Search / filter and basic unread badges (+ 🔒 for encrypted rooms)

### Phase 3 – Timeline & Messaging  ← **DONE**
**Goal:** View and send real messages.

- [x] `MatrixClient::fetch_timeline` via `/messages` (backward, limit 50)
- [x] Map SDK events → `TimelineEvent` / GTK ActionRows
- [x] Composer calls `send_message` (network) with optimistic UI
- [x] Enter-to-send; load history on room select

### Phase 4 – Sync Loop Integration  ← **DONE**
**Goal:** Live updates without blocking the UI thread.

- [x] Run `SyncService` on a Tokio task (`MatrixusApp::start_sync`)
- [x] Bridge events to GTK main loop (broadcast channel + `glib::MainContext::invoke`)
- [x] Update sidebar previews, unread counts, and active timeline on `RoomListUpdated`
- [x] Offline / reconnect banner (`adw::Banner` driven by `ConnectionLost` / `ConnectionRestored`)
- [x] Deduplicate live timeline events via known event-id set

### Phase 5 – Encryption & Verification UI  ← **DONE**
**Goal:** Basic E2EE trust UX.

- [x] Room encryption status badge in header (🔒 Encrypted / Unencrypted)
- [x] Device trust levels via `list_own_devices` / `DeviceTrustLevel`
- [x] SAS emoji verification dialog (`ui/verification.rs`)
- [x] Incoming verification request dialog
- [x] Security dialog with crypto status + device list + Verify actions
- [x] Key backup / cross-signing status indicator on security button

### Phase 6 – Calls & Media  ← **DONE**
**Goal:** Usable 1:1 / group calls via Element Call widget.

- [x] Complete `CallViewController` + WebKitGTK Element Call host (`open_call_window`)
- [x] Incoming-call dialog with Accept / Decline → opens call window
- [x] `ashpd` Camera + ScreenCast portal helpers (`PortalService`)
- [x] Call controls: mute mic, toggle camera, screen share, hang-up
- [x] Header voice / video call buttons for the selected room

### Phase 7 – Polish & Desktop Integration  ← **DONE**
- [x] Real FreeDesktop / GNotification for messages and calls
- [x] Close-to-background (tray-style) preference
- [x] Keyboard shortcuts (`Ctrl+,` Preferences, `Ctrl+Q` Quit, `Ctrl+K` search, `Ctrl+L` composer)
- [x] Accessibility roles/tooltips on primary controls
- [x] System / light / dark theme via Adw StyleManager
- [x] Settings page (notifications, messaging, desktop, Element Call URL)
- [x] Flatpak finish-args + AppStream / .desktop polish

---

## Milestones

| Milestone | Deliverable                              | Effort   |
|-----------|------------------------------------------|----------|
| M1        | Login + auto-restore + empty real room list | ~1 week |
| M2        | Working timeline + send text messages    | +1 week |
| M3        | Live sync + notifications                | +1 week |
| M4        | Encryption indicators + basic verification | +1 week |
| M5        | MatrixRTC calls via WebKit widget        | +1–2 weeks |

---

## Technical Guidelines

- Prefer `adw::` widgets and GNOME HIG (`NavigationSplitView`, `ToolbarView`, `ActionRow`, …).
- Keep pure presentation state (`SidebarState`, `TimelineState`, `ComposerState`) separate from widgets.
- Use `Arc<MatrixusApp>` + channels / `glib::MainContext::invoke` for all async → UI communication. Never block the GTK main loop.
- Feature-gate GUI code under the `gui` Cargo feature so crates remain usable headless.
- Build & run:  
  `cargo run -p matrixus --features gui`  
  (requires `gtk4-devel`, `libadwaita-devel`, `webkitgtk6.0-devel`)

---

## File Layout (relevant)

```
crates/matrixus/src/
├── app.rs                 # MatrixusApp coordinator
├── main.rs                # Entry point (try_auto_login → GUI)
├── desktop/               # Notifications, portals
└── ui/
    ├── gtk_app.rs         # Application shell, window construction
    ├── login.rs           # Phase 1 — login page
    ├── window.rs          # MainWindowState
    ├── sidebar.rs         # SidebarState
    ├── timeline.rs        # TimelineState
    ├── composer.rs        # ComposerState
    └── call/              # Call UI + widget host
```
