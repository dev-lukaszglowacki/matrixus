//! Native WebRTC video & voice calling (Matrix m.call signalling)

pub mod call;
pub mod signaling;
pub mod peer;
pub mod media;
#[cfg(feature = "widget")]
pub mod widget;

pub use call::{CallError, CallParticipant, CallSession, CallState};
pub use media::{
    LocalMediaStream, MediaAcquireResult, MediaCapture, MediaConstraints, MediaSourceKind,
};
pub use peer::{NativeCallEngine, PeerConnectionConfig, PeerEvent};
pub use signaling::{
    CallAnswer, CallCandidate, CallHangup, CallInvite, CallOfferSdp, MatrixVoipEvent, VoipDirection,
};
