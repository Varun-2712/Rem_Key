use std::time::{Duration, Instant};
use uuid::Uuid;
use pir_protocol::events::{ProtocolVersion, CapabilitySet, DisconnectReason};
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SessionState {
    #[default]
    Disconnected,
    Connecting,
    Negotiating,
    Connected,
    Degraded,
    Reconnecting,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: Uuid,
    pub state: SessionState,
    pub protocol_version: ProtocolVersion,
    pub negotiated_capabilities: CapabilitySet,
    pub local_capabilities: CapabilitySet,
    pub remote_capabilities: CapabilitySet,
    pub created_at: Instant,
    pub last_activity: Instant,
    pub heartbeat_interval: Duration,
    pub missed_heartbeats: u32,
    pub max_missed_heartbeats: u32,
}

impl Session {
    pub fn new(local_capabilities: CapabilitySet) -> Self {
        let now = Instant::now();
        Self {
            id: Uuid::new_v4(),
            state: SessionState::Connecting,
            protocol_version: ProtocolVersion::CURRENT,
            negotiated_capabilities: CapabilitySet::empty(),
            local_capabilities,
            remote_capabilities: CapabilitySet::empty(),
            created_at: now,
            last_activity: now,
            heartbeat_interval: Duration::from_secs(5),
            missed_heartbeats: 0,
            max_missed_heartbeats: 3,
        }
    }

    pub fn negotiate_capabilities(&mut self, remote: CapabilitySet) -> CapabilitySet {
        self.remote_capabilities = remote;
        self.negotiated_capabilities = self.local_capabilities & remote;
        self.state = SessionState::Connected;
        info!("Session {} negotiated capabilities: {:?}", self.id, self.negotiated_capabilities);
        self.negotiated_capabilities
    }

    pub fn set_protocol_version(&mut self, version: ProtocolVersion) {
        self.protocol_version = version;
    }

    pub fn update_activity(&mut self) {
        self.last_activity = Instant::now();
        self.missed_heartbeats = 0;
        if self.state == SessionState::Degraded {
            self.state = SessionState::Connected;
            info!("Session {} recovered from degraded state", self.id);
        }
    }

    /// Called once per heartbeat interval. Counts a missed heartbeat only if nothing was
    /// received from the peer during the last interval. Returns true if the session
    /// should now be torn down.
    pub fn check_liveness(&mut self) -> bool {
        if self.last_activity.elapsed() < self.heartbeat_interval {
            return false;
        }
        self.handle_heartbeat_timeout()
    }

    pub fn handle_heartbeat_timeout(&mut self) -> bool {
        self.missed_heartbeats += 1;
        if self.missed_heartbeats >= self.max_missed_heartbeats {
            warn!("Session {} missed {} heartbeats, disconnecting", self.id, self.missed_heartbeats);
            self.state = SessionState::Disconnected;
            true
        } else if self.missed_heartbeats == 1 {
            self.state = SessionState::Degraded;
            warn!("Session {} degraded (missed 1 heartbeat)", self.id);
            false
        } else {
            false
        }
    }

    pub fn start_reconnect(&mut self) {
        self.state = SessionState::Reconnecting;
        self.missed_heartbeats = 0;
    }

    pub fn complete_reconnect(&mut self) {
        self.state = SessionState::Connected;
        self.last_activity = Instant::now();
    }

    pub fn disconnect(&mut self, reason: DisconnectReason) {
        info!("Session {} disconnected: {:?}", self.id, reason);
        self.state = SessionState::Disconnected;
    }

    pub fn is_connected(&self) -> bool {
        matches!(self.state, SessionState::Connected | SessionState::Degraded)
    }

    pub fn supports(&self, capability: CapabilitySet) -> bool {
        self.negotiated_capabilities.contains(capability)
    }
}

#[derive(Debug, Clone)]
pub struct SessionManager {
    sessions: std::collections::HashMap<Uuid, Session>,
    default_capabilities: CapabilitySet,
}

impl SessionManager {
    pub fn new(default_capabilities: CapabilitySet) -> Self {
        Self {
            sessions: std::collections::HashMap::new(),
            default_capabilities,
        }
    }

    pub fn create_session(&mut self) -> Session {
        let session = Session::new(self.default_capabilities);
        let id = session.id;
        self.sessions.insert(id, session.clone());
        session
    }

    pub fn get_session(&self, id: Uuid) -> Option<&Session> {
        self.sessions.get(&id)
    }

    pub fn get_session_mut(&mut self, id: Uuid) -> Option<&mut Session> {
        self.sessions.get_mut(&id)
    }

    pub fn remove_session(&mut self, id: Uuid) -> Option<Session> {
        self.sessions.remove(&id)
    }

    pub fn cleanup_expired(&mut self, max_age: Duration) -> Vec<Uuid> {
        let now = Instant::now();
        let expired: Vec<_> = self.sessions
            .iter()
            .filter(|(_, s)| now.duration_since(s.last_activity) > max_age && s.state != SessionState::Disconnected)
            .map(|(id, _)| *id)
            .collect();
        
        for id in &expired {
            if let Some(session) = self.sessions.get_mut(id) {
                session.disconnect(DisconnectReason::Timeout);
            }
        }
        expired
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn connected_session() -> Session {
        let mut session = Session::new(CapabilitySet::all());
        session.negotiate_capabilities(CapabilitySet::all());
        session
    }

    fn go_silent(session: &mut Session) {
        session.last_activity = Instant::now() - session.heartbeat_interval * 2;
    }

    #[test]
    fn active_session_never_misses_heartbeats() {
        let mut session = connected_session();
        for _ in 0..10 {
            session.update_activity();
            assert!(!session.check_liveness());
        }
        assert_eq!(session.state, SessionState::Connected);
    }

    #[test]
    fn silent_session_degrades_then_disconnects() {
        let mut session = connected_session();
        go_silent(&mut session);

        assert!(!session.check_liveness());
        assert_eq!(session.state, SessionState::Degraded);
        assert!(!session.check_liveness());
        assert!(session.check_liveness());
        assert_eq!(session.state, SessionState::Disconnected);
    }

    #[test]
    fn activity_recovers_degraded_session() {
        let mut session = connected_session();
        go_silent(&mut session);
        session.check_liveness();
        assert_eq!(session.state, SessionState::Degraded);

        session.update_activity();
        assert_eq!(session.state, SessionState::Connected);
        assert_eq!(session.missed_heartbeats, 0);
    }
}
