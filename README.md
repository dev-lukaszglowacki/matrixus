# Matrixus

A native Linux desktop client for the [Matrix](https://matrix.org/) open standard.

Built with **Rust**, **GTK4 / Libadwaita**, and the official [`matrix-rust-sdk`](https://github.com/matrix-org/matrix-rust-sdk). Targets GNOME / Wayland with MatrixRTC video calling via Element Call.

**App ID:** `com.matrixus.Matrixus`  
**Repository:** https://github.com/dev-lukaszglowacki/matrixus

---

## Status

GUI development phases **1–7 are complete**.

| Area | Status |
|------|--------|
| Login / session restore | ✅ |
| Room list (search, unread, 🔒) | ✅ |
| Timeline + send messages | ✅ |
| Live sync + offline banner | ✅ |
| Encryption / SAS verification UI | ✅ |
| MatrixRTC calls (Element Call) | ✅ |
| Settings, shortcuts, theme, notifications | ✅ |
| Flatpak / AppStream packaging metadata | ✅ |

Optional follow-ups: StatusNotifierItem tray applet, broader UI tests / CI packaging.

See [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md) for the detailed roadmap.

---

## Features

### Core (`matrix-core`)
- `matrix-rust-sdk` 0.19 with E2EE (Vodozemac) and SQLite store
- Password login, session persistence, auto-restore
- Room list, timeline fetch, text messaging
- Crypto status, device list, SAS verification helpers
- Background sync service with connection events

### Calls (`matrix-call`)
- MatrixRTC-oriented call session state machine
- Element Call URL builder and Widget API message types
- Mute / video / screen-share / hang-up session flags

### Desktop app (`matrixus`)
- GTK4 + Libadwaita UI (GNOME HIG)
- Login page, split view (sidebar + timeline + composer)
- Live sidebar/timeline updates and offline banner
- Security dialog and SAS emoji verification
- WebKitGTK Element Call window with media controls
- `ashpd` camera / ScreenCast portal helpers
- Preferences: theme (system/light/dark), notifications, close-to-background, Element Call URL
- Keyboard shortcuts, GNotifications, About dialog
- Config: `~/.config/matrixus/settings.json`  
  Session: `~/.local/share/matrixus/session.json`

---

## Architecture

```text
matrixus/                          # workspace root
├── Cargo.toml
├── crates/
│   ├── matrix-core/               # SDK, auth, rooms, timeline, E2EE, sync
│   ├── matrix-call/               # Call session + Element Call widget types
│   └── matrixus/                  # GTK4 / Libadwaita application (binary)
│       └── src/
│           ├── main.rs
│           ├── app.rs             # MatrixusApp coordinator
│           ├── settings.rs        # AppSettings persistence
│           ├── desktop/           # GNotifications, XDG portals
│           └── ui/
│               ├── gtk_app.rs     # Shell, shortcuts, main window
│               ├── login.rs
│               ├── settings.rs    # Preferences UI
│               ├── verification.rs
│               ├── sidebar.rs / timeline.rs / composer.rs
│               └── call/          # Call window + WebKit host
├── data/
│   ├── com.matrixus.Matrixus.desktop
│   └── com.matrixus.Matrixus.metainfo.xml
└── build-aux/flatpak/
    └── com.matrixus.Matrixus.json
```

---

## Building

### Dependencies (Fedora)

```bash
sudo dnf install -y gtk4-devel libadwaita-devel webkitgtk6.0-devel
```

### Run the GUI

```bash
cargo run -p matrixus --features gui
```

Optional environment:

```bash
export RUST_LOG=matrixus=info,matrix_core=info
export ELEMENT_CALL_URL=https://call.element.io   # default
```

### Tests

```bash
cargo test --workspace
```

### Flatpak

```bash
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user -y flathub org.gnome.Sdk//50 org.gnome.Platform//50
flatpak-builder --user --install --force-clean build build-aux/flatpak/com.matrixus.Matrixus.json
```

---

## Keyboard shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+,` | Preferences |
| `Ctrl+Q` | Quit |
| `Ctrl+K` / `Ctrl+F` | Focus room filter |
| `Ctrl+L` | Focus message composer |
| `Enter` | Send message (when enabled in settings) |

---

## License

Apache-2.0
