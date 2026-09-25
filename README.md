# Linux Desktop Matrix Client

A native, high-performance Linux desktop client for the [Matrix open standard](https://matrix.org/) built with Rust, GTK4 / Libadwaita, and modern **MatrixRTC** video calling.

## Key Features

- **Matrix Core Engine (`matrix-core`)**:
  - Built on `matrix-rust-sdk` (v0.19) with sliding sync (Matrix 2.0).
  - End-to-End Encryption (E2EE) powered by pure-Rust **Vodozemac** (Megolm & Olm).
  - Interactive SAS (Short Authentication String) emoji device verification.
  - Session persistence and encrypted SQLite state storage.
- **MatrixRTC Video Calling Engine (`matrix-call`)**:
  - Native MatrixRTC (MSC3401 & MSC4143) room state signaling.
  - Element Call widget integration via Matrix Widget API (MSC1236 & MSC2762) postMessage bridge.
  - Full support for 1:1 calls, group conferences, and screen sharing via Wayland `xdg-desktop-portal`.
- **Linux Desktop & GNOME Integration (`matrix-desktop`)**:
  - Follows GNOME Human Interface Guidelines (Libadwaita / Wayland native).
  - FreeDesktop notifications (`org.freedesktop.Notifications`) with incoming call `[Accept]` and `[Decline]` actions.
  - PipeWire audio/video capture and Wayland ScreenCast portal support.
  - Flatpak manifest targeting `org.gnome.Platform//50`.

## Architecture Overview

```text
matrixclient/
├── Cargo.toml                          # Workspace definition
├── crates/
│   ├── matrix-core/                    # Matrix SDK client, auth, sliding sync, E2EE, SQLite
│   │   └── src/
│   │       ├── client.rs               # High-level client API & message dispatch
│   │       ├── crypto.rs               # E2EE & SAS device verification
│   │       ├── error.rs                # Typed errors
│   │       ├── room.rs                 # Room and timeline event models
│   │       ├── session.rs              # Session persistence store
│   │       └── sync.rs                 # Sliding sync event stream
│   ├── matrix-call/                    # Video & voice calling engine
│   │   └── src/
│   │       ├── call.rs                 # Call session state machine & participant tracking
│   │       └── widget.rs               # Element Call Widget API postMessage protocol bridge
│   └── matrix-desktop/                 # Linux desktop application layer
│       └── src/
│           ├── app.rs                  # Application controller & lifecycle
│           ├── desktop/                # Notifications, Secret Service, Portals
│           └── ui/                     # GTK4 / Libadwaita view controllers & Call window
├── data/                               # Desktop entry and AppStream metadata
└── build-aux/flatpak/                  # Flatpak manifest (GNOME 50 runtime)
```

## Building & Testing

### Running Automated Tests
```bash
cargo test --workspace
```

### Host Compilation with GTK4 / Libadwaita
On Fedora Workstation:
```bash
sudo dnf install -y gtk4-devel libadwaita-devel webkitgtk6.0-devel
cargo build -p matrix-desktop --features gui
```

### Running with Flatpak
```bash
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user -y flathub org.gnome.Sdk//50 org.gnome.Platform//50
flatpak-builder --user --install --force-clean build build-aux/flatpak/com.example.MatrixClient.json
```
