//! Bridges NativeCallEngine PeerEvents ↔ Matrix room VoIP events,
//! and handles incoming SyncEvent::VoipSignalling.

use std::sync::Arc;

use matrix_call::{
    MatrixVoipEvent, MediaCapture, MediaConstraints, NativeCallEngine, PeerEvent,
};
use matrix_core::{MatrixClient};
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, error, info, warn};

/// Owns the active native call engine and forwards signalling both ways.
pub struct VoipBridge {
    client: Arc<Mutex<Option<MatrixClient>>>,
    /// Currently active engine (one call at a time for now)
    active: Arc<Mutex<Option<ActiveCall>>>,
}

struct ActiveCall {
    room_id: String,
    engine: Arc<NativeCallEngine>,
}

impl VoipBridge {
    pub fn new(client: Arc<Mutex<Option<MatrixClient>>>) -> Self {
        Self {
            client,
            active: Arc::new(Mutex::new(None)),
        }
    }

    /// Register an engine for an outbound or accepted call and spawn the
    /// PeerEvent → Matrix sender task.
    pub async fn attach_engine(
        &self,
        room_id: String,
        engine: Arc<NativeCallEngine>,
        _outbound: bool,
    ) {
        {
            let mut guard = self.active.lock().await;
            *guard = Some(ActiveCall {
                room_id: room_id.clone(),
                engine: engine.clone(),
            });
        }

        // Acquire local media (mic / camera) for this call
        let is_video = engine.session_snapshot().await.is_video_call;
        let constraints = if is_video {
            MediaConstraints::video_call()
        } else {
            MediaConstraints::voice_only()
        };
        let capture = MediaCapture::new(constraints);
        let streams = capture.acquire().await;
        info!(
            "VoipBridge: acquired {} local media stream(s) for {room_id}",
            streams.len()
        );

        // Forward PeerEvent → m.call.* room events
        let client = self.client.clone();
        let engine_for_task = engine.clone();
        let room_for_task = room_id.clone();
        let mut rx = engine.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        if let Some(matrix_ev) = engine_for_task.to_matrix_event(&ev, 60_000) {
                            if let Err(e) =
                                send_matrix_voip(&client, &room_for_task, &matrix_ev).await
                            {
                                error!("Failed to send {:?}: {e}", matrix_ev.event_type());
                            }
                        }
                        match &ev {
                            PeerEvent::StateChanged { state, .. } => {
                                debug!("VoipBridge state → {state:?}");
                            }
                            PeerEvent::Ended { reason, .. } => {
                                info!("VoipBridge call ended: {reason:?}");
                                break;
                            }
                            PeerEvent::Error { message, .. } => {
                                warn!("VoipBridge peer error: {message}");
                            }
                            _ => {}
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("VoipBridge lagged {n} peer events");
                    }
                }
            }
        });
    }

    /// Clear active call (after hangup).
    pub async fn clear(&self) {
        let mut guard = self.active.lock().await;
        *guard = None;
    }

    /// Handle an incoming SyncEvent::VoipSignalling from the homeserver.
    pub async fn on_sync_voip(
        &self,
        room_id: &str,
        sender: &str,
        event_type: &str,
        content: serde_json::Value,
    ) -> Option<IncomingVoipAction> {
        let own_user = {
            let guard = self.client.lock().await;
            guard
                .as_ref()
                .and_then(|c| c.user_id())
                .unwrap_or_default()
        };
        // Ignore our own echo
        if sender == own_user {
            return None;
        }

        let parsed = MatrixVoipEvent::from_type_and_content(event_type, content)?;
        info!(
            "VoipBridge: incoming {} from {sender} in {room_id}",
            parsed.event_type()
        );

        match &parsed {
            MatrixVoipEvent::Invite(invite) => {
                // New incoming call — UI should show Accept/Decline
                Some(IncomingVoipAction::Ring {
                    room_id: room_id.to_string(),
                    call_id: invite.call_id.clone(),
                    is_video: invite
                        .offer
                        .sdp
                        .contains("m=video"),
                    sender: sender.to_string(),
                    invite: invite.clone(),
                })
            }
            MatrixVoipEvent::Answer(answer) => {
                let guard = self.active.lock().await;
                if let Some(active) = guard.as_ref() {
                    if active.room_id == room_id {
                        let engine = active.engine.clone();
                        drop(guard);
                        let answer = answer.clone();
                        tokio::spawn(async move {
                            if let Err(e) = engine.apply_answer(answer).await {
                                error!("apply_answer failed: {e}");
                            }
                        });
                    }
                }
                None
            }
            MatrixVoipEvent::Candidates(cands) => {
                let guard = self.active.lock().await;
                if let Some(active) = guard.as_ref() {
                    if active.room_id == room_id {
                        let engine = active.engine.clone();
                        drop(guard);
                        let cands = cands.clone();
                        tokio::spawn(async move {
                            if let Err(e) = engine.add_remote_candidates(cands).await {
                                error!("add_remote_candidates failed: {e}");
                            }
                        });
                    }
                }
                None
            }
            MatrixVoipEvent::Hangup(h) | MatrixVoipEvent::Reject(h) => {
                let guard = self.active.lock().await;
                if let Some(active) = guard.as_ref() {
                    if active.room_id == room_id && active.engine.call_id() == h.call_id {
                        let engine = active.engine.clone();
                        drop(guard);
                        let reason = h.reason.clone();
                        tokio::spawn(async move {
                            let _ = engine.hangup(reason).await;
                        });
                        return Some(IncomingVoipAction::RemoteEnded {
                            room_id: room_id.to_string(),
                            call_id: h.call_id.clone(),
                        });
                    }
                }
                // Incoming ring cancelled before accept
                Some(IncomingVoipAction::RemoteEnded {
                    room_id: room_id.to_string(),
                    call_id: h.call_id.clone(),
                })
            }
        }
    }
}

/// Actions the UI should take in response to remote VoIP events
#[derive(Debug, Clone)]
pub enum IncomingVoipAction {
    Ring {
        room_id: String,
        call_id: String,
        is_video: bool,
        sender: String,
        invite: matrix_call::CallInvite,
    },
    RemoteEnded {
        room_id: String,
        call_id: String,
    },
}

async fn send_matrix_voip(
    client: &Arc<Mutex<Option<MatrixClient>>>,
    room_id: &str,
    event: &MatrixVoipEvent,
) -> anyhow::Result<()> {
    let content = event
        .to_content_value()
        .map_err(|e| anyhow::anyhow!("serialize: {e}"))?;
    let guard = client.lock().await;
    let client = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Not logged in"))?;
    client
        .send_voip_event(room_id, event.event_type(), content)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}
