use tokio::time::{interval, Duration, MissedTickBehavior};
use uuid::Uuid;
use pir_protocol::events::{InputEvent, EventMetadata, SessionEventType, DisconnectReason};
use crate::state::SessionManager;
use tracing::{debug, warn};

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

    pub async fn run(mut self) {
        let mut ticker = interval(self.interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            ticker.tick().await;

            let should_disconnect = {
                let mut manager = self.session_manager.lock().await;
                if let Some(session) = manager.get_session_mut(self.session_id) {
                    if session.state == crate::state::SessionState::Disconnected {
                        break;
                    }
                    session.handle_heartbeat_timeout()
                } else {
                    break;
                }
            };

            if should_disconnect {
                let _ = self.outgoing_tx.send(InputEvent::SessionEvent {
                    metadata: EventMetadata::new(self.session_id, 0),
                    event: SessionEventType::Disconnect {
                        reason: DisconnectReason::Timeout,
                    },
                });
                break;
            }

            let seq = {
                let mut manager = self.session_manager.lock().await;
                if let Some(session) = manager.get_session_mut(self.session_id) {
                    session.update_activity();
                    0 // sequence number for heartbeat
                } else {
                    break;
                }
            };

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

pub async fn handle_incoming_heartbeat(
    session_manager: std::sync::Arc<tokio::sync::Mutex<SessionManager>>,
    session_id: Uuid,
    is_ack: bool,
) {
    let mut manager = session_manager.lock().await;
    if let Some(session) = manager.get_session_mut(session_id) {
        if is_ack {
            session.update_activity();
        } else {
            session.update_activity();
            // Send ack
        }
    }
}