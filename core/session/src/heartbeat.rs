use tokio::time::{interval, Duration, MissedTickBehavior};
use uuid::Uuid;
use pir_protocol::events::{InputEvent, EventMetadata, SessionEventType, DisconnectReason};
use crate::state::SessionManager;
use tracing::debug;

pub struct HeartbeatManager {
    session_id: Uuid,
    interval: Duration,
    session_manager: std::sync::Arc<tokio::sync::Mutex<SessionManager>>,
    outgoing_tx: tokio::sync::mpsc::UnboundedSender<InputEvent>,
}

impl HeartbeatManager {
    pub fn new(
        session_id: Uuid,
        interval: Duration,
        session_manager: std::sync::Arc<tokio::sync::Mutex<SessionManager>>,
        outgoing_tx: tokio::sync::mpsc::UnboundedSender<InputEvent>,
    ) -> Self {
        Self {
            session_id,
            interval,
            session_manager,
            outgoing_tx,
        }
    }

    /// Sends a heartbeat every interval and disconnects the session once the peer has been
    /// silent for too many intervals. Liveness is driven only by what the peer sends
    /// (see `handle_incoming_heartbeat`), never by our own heartbeats.
    pub async fn run(self) {
        let mut ticker = interval(self.interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // The first tick fires immediately; give the peer a full interval first.
        ticker.tick().await;
        let mut seq = 0u64;

        loop {
            ticker.tick().await;

            let should_disconnect = {
                let mut manager = self.session_manager.lock().await;
                match manager.get_session_mut(self.session_id) {
                    Some(session) if session.state != crate::state::SessionState::Disconnected => {
                        session.check_liveness()
                    }
                    _ => break,
                }
            };

            if should_disconnect {
                let _ = self.outgoing_tx.send(InputEvent::SessionEvent {
                    metadata: EventMetadata::new(self.session_id, seq),
                    event: SessionEventType::Disconnect {
                        reason: DisconnectReason::Timeout,
                    },
                });
                break;
            }

            seq += 1;
            let event = InputEvent::SessionEvent {
                metadata: EventMetadata::new(self.session_id, seq),
                event: SessionEventType::Heartbeat,
            };

            if self.outgoing_tx.send(event).is_err() {
                debug!("Heartbeat send failed, channel closed");
                break;
            }
        }

        debug!("Heartbeat manager for session {} stopped", self.session_id);
    }
}

/// Records that the peer is alive, and answers a heartbeat with a `HeartbeatAck`.
pub async fn handle_incoming_heartbeat(
    session_manager: std::sync::Arc<tokio::sync::Mutex<SessionManager>>,
    session_id: Uuid,
    is_ack: bool,
    outgoing_tx: &tokio::sync::mpsc::UnboundedSender<InputEvent>,
) {
    let mut manager = session_manager.lock().await;
    if let Some(session) = manager.get_session_mut(session_id) {
        session.update_activity();
        if !is_ack {
            let _ = outgoing_tx.send(heartbeat_ack(session_id));
        }
    }
}

pub fn heartbeat_ack(session_id: Uuid) -> InputEvent {
    InputEvent::SessionEvent {
        metadata: EventMetadata::new(session_id, 0),
        event: SessionEventType::HeartbeatAck,
    }
}
