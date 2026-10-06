//! Native WebRTC peer connection engine for Matrix 1:1 VoIP.
//!
//! Uses webrtc-rs when the `native-webrtc` feature is enabled. Without the
//! feature, provides a signalling-only stub so the rest of the app compiles.

use std::sync::Arc;

use tokio::sync::{broadcast, Mutex};
use tracing::{debug, info};

use crate::call::{CallSession, CallState};
use crate::signaling::{
    voip_version, CallAnswer, CallCandidate, CallHangup, CallInvite, CallOfferSdp, IceCandidateJson,
    MatrixVoipEvent,
};

/// Configuration for ICE / STUN (TURN optional later)
#[derive(Debug, Clone)]
pub struct PeerConnectionConfig {
    pub stun_servers: Vec<String>,
    /// Local party id (device-scoped) for MSC2746
    pub party_id: String,
}

impl Default for PeerConnectionConfig {
    fn default() -> Self {
        Self {
            stun_servers: vec!["stun:stun.l.google.com:19302".into()],
            party_id: uuid::Uuid::new_v4().to_string(),
        }
    }
}

/// Events emitted by the native call engine toward the UI / Matrix sender
#[derive(Debug, Clone)]
pub enum PeerEvent {
    /// Local SDP offer ready — send as m.call.invite
    LocalOffer { call_id: String, sdp: String },
    /// Local SDP answer ready — send as m.call.answer
    LocalAnswer { call_id: String, sdp: String },
    /// Local ICE candidate — batch/send as m.call.candidates
    LocalIceCandidate {
        call_id: String,
        candidate: String,
        sdp_mid: Option<String>,
        sdp_m_line_index: Option<u16>,
    },
    /// Connection state changed
    StateChanged { call_id: String, state: CallState },
    /// Remote track became available (audio/video)
    RemoteTrack {
        call_id: String,
        kind: TrackKind,
        stream_id: String,
    },
    /// Fatal or recoverable error
    Error { call_id: String, message: String },
    /// Call ended locally or remotely
    Ended { call_id: String, reason: Option<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Audio,
    Video,
}

/// High-level native call controller: one active PeerConnection per call.
pub struct NativeCallEngine {
    config: PeerConnectionConfig,
    session: Arc<Mutex<CallSession>>,
    event_tx: broadcast::Sender<PeerEvent>,
    /// Accumulated remote ICE candidates received before remote description is set
    pending_remote_candidates: Arc<Mutex<Vec<IceCandidateJson>>>,
}

impl NativeCallEngine {
    pub fn new(session: CallSession, config: PeerConnectionConfig) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        Self {
            config,
            session: Arc::new(Mutex::new(session)),
            event_tx,
            pending_remote_candidates: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PeerEvent> {
        self.event_tx.subscribe()
    }

    pub async fn session_snapshot(&self) -> CallSession {
        self.session.lock().await.clone()
    }

    pub fn call_id(&self) -> String {
        // Blocking-ish read is fine for id
        self.session
            .try_lock()
            .map(|s| s.call_id.clone())
            .unwrap_or_default()
    }

    /// Start an outgoing call: create offer and emit LocalOffer.
    pub async fn place_call(&self) -> anyhow::Result<()> {
        {
            let mut s = self.session.lock().await;
            s.start_ringing().map_err(|e| anyhow::anyhow!("{e}"))?;
            let _ = s.start_connecting();
        }
        let call_id = self.session.lock().await.call_id.clone();
        let is_video = self.session.lock().await.is_video_call;

        info!(
            "Native WebRTC: placing {} call {call_id}",
            if is_video { "video" } else { "voice" }
        );

        #[cfg(feature = "native-webrtc")]
        {
            self.create_and_emit_offer(&call_id, is_video).await?;
        }
        #[cfg(not(feature = "native-webrtc"))]
        {
            // Stub SDP so UI/signalling path can be exercised without webrtc-rs
            let sdp = stub_sdp_offer(is_video);
            let _ = self.event_tx.send(PeerEvent::LocalOffer {
                call_id: call_id.clone(),
                sdp,
            });
            let _ = self.event_tx.send(PeerEvent::StateChanged {
                call_id,
                state: CallState::Connecting,
            });
        }
        Ok(())
    }

    /// Handle an incoming m.call.invite: set remote offer and create answer.
    pub async fn accept_invite(&self, invite: CallInvite) -> anyhow::Result<()> {
        {
            let mut s = self.session.lock().await;
            if s.call_id != invite.call_id && s.state == CallState::Idle {
                s.call_id = invite.call_id.clone();
            }
            let _ = s.start_ringing();
            let _ = s.start_connecting();
        }
        let call_id = invite.call_id.clone();
        info!("Native WebRTC: accepting invite for {call_id}");

        #[cfg(feature = "native-webrtc")]
        {
            self.set_remote_offer_and_answer(&call_id, &invite.offer.sdp)
                .await?;
        }
        #[cfg(not(feature = "native-webrtc"))]
        {
            let sdp = stub_sdp_answer();
            let _ = self.event_tx.send(PeerEvent::LocalAnswer {
                call_id: call_id.clone(),
                sdp,
            });
            let _ = self.event_tx.send(PeerEvent::StateChanged {
                call_id,
                state: CallState::Connecting,
            });
        }
        Ok(())
    }

    /// Apply remote answer (callee accepted our invite).
    pub async fn apply_answer(&self, answer: CallAnswer) -> anyhow::Result<()> {
        info!("Native WebRTC: applying remote answer for {}", answer.call_id);
        #[cfg(feature = "native-webrtc")]
        {
            self.set_remote_answer(&answer.call_id, &answer.answer.sdp)
                .await?;
        }
        {
            let mut s = self.session.lock().await;
            let _ = s.set_connected();
        }
        let _ = self.event_tx.send(PeerEvent::StateChanged {
            call_id: answer.call_id,
            state: CallState::Connected,
        });
        Ok(())
    }

    /// Add remote ICE candidates from m.call.candidates.
    pub async fn add_remote_candidates(&self, candidates: CallCandidate) -> anyhow::Result<()> {
        debug!(
            "Native WebRTC: {} remote ICE candidate(s) for {}",
            candidates.candidates.len(),
            candidates.call_id
        );
        #[cfg(feature = "native-webrtc")]
        {
            self.add_ice_candidates(&candidates.call_id, &candidates.candidates)
                .await?;
        }
        #[cfg(not(feature = "native-webrtc"))]
        {
            let mut pending = self.pending_remote_candidates.lock().await;
            pending.extend(candidates.candidates);
        }
        Ok(())
    }

    /// Hang up: transition state and emit Ended (caller sends m.call.hangup).
    pub async fn hangup(&self, reason: Option<String>) -> anyhow::Result<()> {
        let call_id = {
            let mut s = self.session.lock().await;
            s.hang_up();
            s.call_id.clone()
        };
        info!("Native WebRTC: hangup {call_id}");
        let _ = self.event_tx.send(PeerEvent::Ended {
            call_id,
            reason,
        });
        Ok(())
    }

    /// Build Matrix events from peer events for sending over the room.
    pub fn to_matrix_event(
        &self,
        event: &PeerEvent,
        lifetime_ms: u64,
    ) -> Option<MatrixVoipEvent> {
        let party_id = Some(self.config.party_id.clone());
        match event {
            PeerEvent::LocalOffer { call_id, sdp } => Some(MatrixVoipEvent::Invite(CallInvite {
                call_id: call_id.clone(),
                version: voip_version(),
                lifetime: lifetime_ms,
                offer: CallOfferSdp {
                    sdp_type: "offer".into(),
                    sdp: sdp.clone(),
                },
                party_id,
                invitee: None,
            })),
            PeerEvent::LocalAnswer { call_id, sdp } => Some(MatrixVoipEvent::Answer(CallAnswer {
                call_id: call_id.clone(),
                version: voip_version(),
                answer: CallOfferSdp {
                    sdp_type: "answer".into(),
                    sdp: sdp.clone(),
                },
                party_id,
            })),
            PeerEvent::LocalIceCandidate {
                call_id,
                candidate,
                sdp_mid,
                sdp_m_line_index,
            } => Some(MatrixVoipEvent::Candidates(CallCandidate {
                call_id: call_id.clone(),
                version: voip_version(),
                candidates: vec![IceCandidateJson {
                    candidate: candidate.clone(),
                    sdp_mid: sdp_mid.clone(),
                    sdp_m_line_index: *sdp_m_line_index,
                }],
                party_id,
            })),
            PeerEvent::Ended { call_id, reason } => Some(MatrixVoipEvent::Hangup(CallHangup {
                call_id: call_id.clone(),
                version: voip_version(),
                reason: reason.clone(),
                party_id,
            })),
            _ => None,
        }
    }

    // ── webrtc-rs integration (feature-gated) ──────────────────────────────

    #[cfg(feature = "native-webrtc")]
    async fn create_and_emit_offer(&self, call_id: &str, is_video: bool) -> anyhow::Result<()> {
        use webrtc::api::media_engine::{MIME_TYPE_OPUS, MIME_TYPE_VP8};
        use webrtc::api::APIBuilder;
        use webrtc::ice_transport::ice_server::RTCIceServer;
        use webrtc::peer_connection::configuration::RTCConfiguration;
        use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
        use webrtc::rtp_transceiver::rtp_codec::{
            RTCRtpCodecCapability, RTPCodecType,
        };
        use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
        use webrtc::track::track_local::TrackLocal;

        let api = APIBuilder::new().build();
        let config = RTCConfiguration {
            ice_servers: self
                .config
                .stun_servers
                .iter()
                .map(|s| RTCIceServer {
                    urls: vec![s.clone()],
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let pc = api.new_peer_connection(config).await?;

        // Audio track
        let audio_track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_OPUS.to_owned(),
                ..Default::default()
            },
            "audio".to_owned(),
            "matrixus".to_owned(),
        ));
        pc.add_track(audio_track as Arc<dyn TrackLocal + Send + Sync>)
            .await?;

        if is_video {
            let video_track = Arc::new(TrackLocalStaticSample::new(
                RTCRtpCodecCapability {
                    mime_type: MIME_TYPE_VP8.to_owned(),
                    ..Default::default()
                },
                "video".to_owned(),
                "matrixus".to_owned(),
            ));
            pc.add_track(video_track as Arc<dyn TrackLocal + Send + Sync>)
                .await?;
        }

        let offer = pc.create_offer(None).await?;
        pc.set_local_description(offer.clone()).await?;

        let _ = self.event_tx.send(PeerEvent::LocalOffer {
            call_id: call_id.to_string(),
            sdp: offer.sdp,
        });
        let _ = self.event_tx.send(PeerEvent::StateChanged {
            call_id: call_id.to_string(),
            state: CallState::Connecting,
        });

        // Keep PC alive for the session lifetime (simplified: store in engine later)
        // Full ICE candidate gathering + track wiring is the next iteration.
        let _ = pc;
        Ok(())
    }

    #[cfg(feature = "native-webrtc")]
    async fn set_remote_offer_and_answer(&self, call_id: &str, remote_sdp: &str) -> anyhow::Result<()> {
        use webrtc::api::APIBuilder;
        use webrtc::ice_transport::ice_server::RTCIceServer;
        use webrtc::peer_connection::configuration::RTCConfiguration;
        use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

        let api = APIBuilder::new().build();
        let config = RTCConfiguration {
            ice_servers: self
                .config
                .stun_servers
                .iter()
                .map(|s| RTCIceServer {
                    urls: vec![s.clone()],
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let pc = api.new_peer_connection(config).await?;
        let offer = RTCSessionDescription::offer(remote_sdp.to_string())?;
        pc.set_remote_description(offer).await?;
        let answer = pc.create_answer(None).await?;
        pc.set_local_description(answer.clone()).await?;

        let _ = self.event_tx.send(PeerEvent::LocalAnswer {
            call_id: call_id.to_string(),
            sdp: answer.sdp,
        });
        let _ = pc;
        Ok(())
    }

    #[cfg(feature = "native-webrtc")]
    async fn set_remote_answer(&self, _call_id: &str, remote_sdp: &str) -> anyhow::Result<()> {
        // Full implementation stores PC and sets remote description here.
        debug!("Remote answer SDP length={}", remote_sdp.len());
        Ok(())
    }

    #[cfg(feature = "native-webrtc")]
    async fn add_ice_candidates(
        &self,
        _call_id: &str,
        candidates: &[IceCandidateJson],
    ) -> anyhow::Result<()> {
        for c in candidates {
            debug!("ICE candidate: {}", c.candidate);
        }
        Ok(())
    }
}

#[cfg(not(feature = "native-webrtc"))]
fn stub_sdp_offer(is_video: bool) -> String {
    let video = if is_video {
        "m=video 9 UDP/TLS/RTP/SAVPF 96\r\na=rtpmap:96 VP8/90000\r\n"
    } else {
        ""
    };
    format!(
        "v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\n\
         m=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=rtpmap:111 opus/48000/2\r\n{video}"
    )
}

#[cfg(not(feature = "native-webrtc"))]
fn stub_sdp_answer() -> String {
    "v=0\r\no=- 0 0 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\n\
     m=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=rtpmap:111 opus/48000/2\r\n"
        .into()
}
