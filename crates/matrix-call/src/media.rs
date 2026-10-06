//! Local media capture scaffolding for native WebRTC calls.
//!
//! Full PipeWire / GStreamer capture is feature-gated later; this module
//! defines the track abstraction and portal-backed request helpers the UI uses.

use serde::{Deserialize, Serialize};
use tracing::info;

/// Kind of local media source
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaSourceKind {
    Microphone,
    Camera,
    ScreenShare,
}

/// Capture constraints requested from the OS / portal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaConstraints {
    pub audio: bool,
    pub video: bool,
    pub screen: bool,
    /// Preferred camera width (hint)
    pub width: u32,
    pub height: u32,
    pub frame_rate: u32,
}

impl Default for MediaConstraints {
    fn default() -> Self {
        Self {
            audio: true,
            video: true,
            screen: false,
            width: 1280,
            height: 720,
            frame_rate: 30,
        }
    }
}

impl MediaConstraints {
    pub fn voice_only() -> Self {
        Self {
            audio: true,
            video: false,
            screen: false,
            ..Default::default()
        }
    }

    pub fn video_call() -> Self {
        Self::default()
    }
}

/// Handle to an acquired local media stream (opaque until wired to webrtc-rs tracks)
#[derive(Debug, Clone)]
pub struct LocalMediaStream {
    pub id: String,
    pub has_audio: bool,
    pub has_video: bool,
    pub is_screen: bool,
}

/// Result of requesting capture devices
#[derive(Debug, Clone)]
pub enum MediaAcquireResult {
    Ready(LocalMediaStream),
    Denied { kind: MediaSourceKind },
    Unavailable { kind: MediaSourceKind, reason: String },
}

/// Media acquisition service — portal permission + placeholder stream ids.
///
/// Real PCM/YUV frames will be fed into `TrackLocalStaticSample` when the
/// `native-webrtc` feature is fully wired with a capture backend.
pub struct MediaCapture {
    constraints: MediaConstraints,
}

impl MediaCapture {
    pub fn new(constraints: MediaConstraints) -> Self {
        Self { constraints }
    }

    pub fn constraints(&self) -> &MediaConstraints {
        &self.constraints
    }

    /// Request mic (+ optional camera) and return a logical stream handle.
    pub async fn acquire(&self) -> Vec<MediaAcquireResult> {
        let mut results = Vec::new();

        if self.constraints.audio {
            info!("MediaCapture: acquiring microphone");
            results.push(MediaAcquireResult::Ready(LocalMediaStream {
                id: format!("mic-{}", uuid::Uuid::new_v4()),
                has_audio: true,
                has_video: false,
                is_screen: false,
            }));
        }

        if self.constraints.video {
            info!("MediaCapture: acquiring camera ({}x{}@{})", 
                self.constraints.width, self.constraints.height, self.constraints.frame_rate);
            results.push(MediaAcquireResult::Ready(LocalMediaStream {
                id: format!("cam-{}", uuid::Uuid::new_v4()),
                has_audio: false,
                has_video: true,
                is_screen: false,
            }));
        }

        if self.constraints.screen {
            info!("MediaCapture: acquiring screen share");
            results.push(MediaAcquireResult::Ready(LocalMediaStream {
                id: format!("screen-{}", uuid::Uuid::new_v4()),
                has_audio: false,
                has_video: true,
                is_screen: true,
            }));
        }

        results
    }

    /// Stop all local tracks (placeholder)
    pub async fn release(&self, stream: &LocalMediaStream) {
        info!("MediaCapture: releasing stream {}", stream.id);
    }
}
