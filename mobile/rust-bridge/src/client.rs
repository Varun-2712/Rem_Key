use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pir_input::state::InputStateManager;
use pir_protocol::events::{
    DisconnectReason, EventMetadata, InputEvent, ProtocolVersion, SessionEventType,
};
use pir_session::capabilities::get_default_phone_capabilities;
use pir_transport::quic::{CertFingerprint, QuicClientTransport};
use pir_transport::InputTransport;
use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

use crate::commands::{Command, PointerButton, SpecialKey};
use crate::errors::RelayError;

/// Commands waiting for the connection task. Small enough that a stalled network shows up
/// as dropped pointer deltas quickly instead of a burst of stale movement later.
const COMMAND_QUEUE_LEN: usize = 256;
/// Most commands drained per wakeup, so pointer deltas get merged without starving recv.
const MAX_BATCH: usize = 64;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const GRACEFUL_SHUTDOWN: Duration = Duration::from_secs(1);
/// The agent's certificate is pinned by fingerprint, so the name only has to be valid SNI.
const SERVER_NAME: &str = "localhost";

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Failed { message: String },
}

/// Implemented on the Kotlin side. Called from a Rust worker thread, never the UI thread.
#[uniffi::export(callback_interface)]
pub trait RelayListener: Send + Sync {
    fn on_state_changed(&self, state: ConnectionState);
}

#[derive(uniffi::Object)]
pub struct RelayClient {
    runtime: Option<tokio::runtime::Runtime>,
    commands: Mutex<Option<mpsc::Sender<Command>>>,
}

#[uniffi::export]
impl RelayClient {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("pir-relay")
            .enable_all()
            .build()
            .expect("failed to start the relay runtime");
        Arc::new(Self { runtime: Some(runtime), commands: Mutex::new(None) })
    }

    /// Start connecting to a desktop agent. Returns immediately; progress is reported
    /// through `listener`. `fingerprint_hex` is the agent's certificate SHA-256 from pairing.
    pub fn connect(
        &self,
        host: String,
        port: u16,
        fingerprint_hex: String,
        listener: Box<dyn RelayListener>,
    ) -> Result<(), RelayError> {
        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .or_else(|_| format!("[{host}]:{port}").parse())
            .map_err(|_| RelayError::InvalidAddress { message: format!("{host}:{port}") })?;
        let fingerprint = parse_fingerprint(&fingerprint_hex)?;

        let mut commands = self.commands.lock().unwrap();
        if commands.as_ref().is_some_and(|tx| !tx.is_closed()) {
            return Err(RelayError::AlreadyConnected);
        }
        let (tx, rx) = mpsc::channel(COMMAND_QUEUE_LEN);
        *commands = Some(tx);
        self.runtime().spawn(run_session(addr, fingerprint, rx, Arc::from(listener)));
        Ok(())
    }

    /// Releases anything still held on the desktop, then closes the connection.
    pub fn disconnect(&self) {
        if let Some(tx) = self.commands.lock().unwrap().take() {
            self.runtime().spawn(async move {
                let _ = tx.send(Command::Disconnect).await;
            });
        }
    }

    /// Relative pointer movement in desktop pixels. Fractions accumulate across calls.
    /// Dropped rather than queued when the connection can't keep up.
    pub fn pointer_delta(&self, dx: f32, dy: f32) -> Result<(), RelayError> {
        self.enqueue_lossy(Command::PointerDelta { dx, dy })
    }

    /// Scroll in wheel notches: positive `dy` is up, positive `dx` is right. Fractions accumulate.
    pub fn scroll(&self, dx: f32, dy: f32) -> Result<(), RelayError> {
        self.enqueue_lossy(Command::Scroll { dx, dy })
    }

    pub fn mouse_button(&self, button: PointerButton, pressed: bool) -> Result<(), RelayError> {
        self.enqueue(Command::Button { button, pressed })
    }

    pub fn special_key(&self, key: SpecialKey, pressed: bool) -> Result<(), RelayError> {
        self.enqueue(Command::Key { key, pressed })
    }

    pub fn commit_text(&self, text: String) -> Result<(), RelayError> {
        self.enqueue(Command::Text(text))
    }
}

impl RelayClient {
    fn runtime(&self) -> &tokio::runtime::Runtime {
        self.runtime.as_ref().expect("runtime is only taken on drop")
    }

    fn sender(&self) -> Result<mpsc::Sender<Command>, RelayError> {
        self.commands.lock().unwrap().clone().ok_or(RelayError::NotConnected)
    }

    fn enqueue(&self, command: Command) -> Result<(), RelayError> {
        self.sender()?.try_send(command).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => RelayError::QueueFull,
            mpsc::error::TrySendError::Closed(_) => RelayError::NotConnected,
        })
    }

    fn enqueue_lossy(&self, command: Command) -> Result<(), RelayError> {
        match self.enqueue(command) {
            Err(RelayError::QueueFull) => Ok(()),
            other => other,
        }
    }
}

impl Drop for RelayClient {
    fn drop(&mut self) {
        // Give the session task a moment to release held keys before the runtime goes away.
        if let Some(tx) = self.commands.get_mut().unwrap().take() {
            let _ = tx.try_send(Command::Disconnect);
        }
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(GRACEFUL_SHUTDOWN);
        }
    }
}

fn parse_fingerprint(hex: &str) -> Result<CertFingerprint, RelayError> {
    let hex: String = hex.chars().filter(|c| !c.is_whitespace() && *c != ':').collect();
    if hex.len() != 64 {
        return Err(RelayError::InvalidFingerprint { message: "expected 64 hex characters".into() });
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| RelayError::InvalidFingerprint { message: "not hexadecimal".into() })?;
    }
    Ok(out)
}

async fn run_session(
    addr: SocketAddr,
    fingerprint: CertFingerprint,
    mut commands: mpsc::Receiver<Command>,
    listener: Arc<dyn RelayListener>,
) {
    listener.on_state_changed(ConnectionState::Connecting);
    let final_state = match establish(addr, fingerprint).await {
        Ok((mut conn, session_id)) => {
            info!("connected to {addr}, session {session_id}");
            listener.on_state_changed(ConnectionState::Connected);
            let mut session = Session::new(session_id);
            let result = session.run(&mut conn, &mut commands).await;
            let _ = conn.close().await;
            match result {
                Ok(()) => ConnectionState::Disconnected,
                Err(e) => ConnectionState::Failed { message: e.to_string() },
            }
        }
        Err(e) => ConnectionState::Failed { message: e.to_string() },
    };
    if let ConnectionState::Failed { message } = &final_state {
        warn!("relay session to {addr} ended: {message}");
    }
    listener.on_state_changed(final_state);
}

async fn establish(addr: SocketAddr, fingerprint: CertFingerprint) -> anyhow::Result<(QuicClientTransport, Uuid)> {
    let mut conn = QuicClientTransport::connect(addr, SERVER_NAME, fingerprint).await?;
    conn.send_reliable(&InputEvent::SessionEvent {
        metadata: EventMetadata::new(Uuid::nil(), 0),
        event: SessionEventType::Hello {
            version: ProtocolVersion::CURRENT,
            capabilities: get_default_phone_capabilities(),
        },
    })
    .await?;
    match tokio::time::timeout(HANDSHAKE_TIMEOUT, conn.recv()).await {
        Ok(Ok(InputEvent::SessionEvent { metadata, event: SessionEventType::HelloAck { .. } })) => {
            let session_id = if metadata.session_id.is_nil() { Uuid::new_v4() } else { metadata.session_id };
            Ok((conn, session_id))
        }
        Ok(Ok(InputEvent::SessionEvent { event: SessionEventType::Disconnect { reason }, .. })) => {
            anyhow::bail!("desktop refused the connection: {reason:?}")
        }
        Ok(Ok(other)) => anyhow::bail!("unexpected handshake reply: {other:?}"),
        Ok(Err(e)) => Err(e),
        Err(_) => anyhow::bail!("desktop did not answer the handshake"),
    }
}

/// The phone side of one connection: turns commands into protocol events and keeps
/// track of what is held so it can all be released on the way out.
struct Session {
    state: InputStateManager,
    pointer: Accumulator,
    scroll: Accumulator,
    session_id: Uuid,
}

impl Session {
    fn new(session_id: Uuid) -> Self {
        Self {
            state: InputStateManager::new(session_id),
            pointer: Accumulator::default(),
            scroll: Accumulator::default(),
            session_id,
        }
    }

    async fn run(
        &mut self,
        conn: &mut QuicClientTransport,
        commands: &mut mpsc::Receiver<Command>,
    ) -> anyhow::Result<()> {
        let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
        heartbeat.tick().await;
        loop {
            tokio::select! {
                first = commands.recv() => {
                    let Some(first) = first else { return self.shutdown(conn).await };
                    let mut batch = vec![first];
                    while batch.len() < MAX_BATCH {
                        match commands.try_recv() {
                            Ok(command) => batch.push(command),
                            Err(_) => break,
                        }
                    }
                    for command in batch {
                        if let Command::Disconnect = command {
                            return self.shutdown(conn).await;
                        }
                        self.apply(conn, command).await?;
                    }
                    self.flush_motion(conn, false).await?;
                }
                incoming = conn.recv() => {
                    if let InputEvent::SessionEvent { event: SessionEventType::Disconnect { reason }, .. } = incoming? {
                        info!("desktop closed the session: {reason:?}");
                        return Ok(());
                    }
                }
                _ = heartbeat.tick() => {
                    conn.send_reliable(&self.session_event(SessionEventType::Heartbeat)).await?;
                }
            }
        }
    }

    async fn apply(&mut self, conn: &mut QuicClientTransport, command: Command) -> anyhow::Result<()> {
        let events = match command {
            Command::PointerDelta { dx, dy } => {
                self.pointer.add(dx, dy);
                return Ok(());
            }
            Command::Scroll { dx, dy } => {
                self.scroll.add(dx, dy);
                return Ok(());
            }
            // Anything else must land after the motion that preceded it, and datagrams and
            // the stream are read independently, so that motion has to go on the stream.
            Command::Button { button, pressed } => {
                self.flush_motion(conn, true).await?;
                self.state.handle_mouse_button(button.into(), pressed)
            }
            Command::Key { key, pressed } => {
                self.flush_motion(conn, true).await?;
                let modifiers = self.state.get_modifiers();
                if pressed {
                    self.state.handle_key_down(key.into(), modifiers)
                } else {
                    self.state.handle_key_up(key.into(), modifiers)
                }
            }
            Command::Text(text) => {
                self.flush_motion(conn, true).await?;
                self.state.handle_text(text)
            }
            Command::Disconnect => unreachable!("handled by the run loop"),
        };
        for event in events {
            conn.send_reliable(&event).await?;
        }
        Ok(())
    }

    async fn flush_motion(&mut self, conn: &mut QuicClientTransport, ordered: bool) -> anyhow::Result<()> {
        if let Some((dx, dy)) = self.pointer.take_whole() {
            for event in self.state.handle_mouse_move(dx, dy) {
                if ordered {
                    conn.send_reliable(&event).await?;
                } else {
                    conn.send_unreliable(&event).await?;
                }
            }
        }
        if let Some((dx, dy)) = self.scroll.take_whole() {
            for event in self.state.handle_scroll(dx, dy) {
                conn.send_reliable(&event).await?;
            }
        }
        Ok(())
    }

    async fn shutdown(&mut self, conn: &mut QuicClientTransport) -> anyhow::Result<()> {
        for event in self.state.reset_input_state() {
            conn.send_reliable(&event).await?;
        }
        conn.send_reliable(&self.session_event(SessionEventType::Disconnect { reason: DisconnectReason::Normal }))
            .await?;
        Ok(())
    }

    fn session_event(&self, event: SessionEventType) -> InputEvent {
        InputEvent::SessionEvent { metadata: EventMetadata::new(self.session_id, 0), event }
    }
}

/// Sums fractional deltas and hands out the whole part, keeping the remainder, so slow
/// finger movement still adds up to motion instead of rounding to zero every frame.
#[derive(Default)]
struct Accumulator {
    x: f32,
    y: f32,
}

impl Accumulator {
    fn add(&mut self, dx: f32, dy: f32) {
        if dx.is_finite() && dy.is_finite() {
            self.x += dx;
            self.y += dy;
        }
    }

    fn take_whole(&mut self) -> Option<(i32, i32)> {
        let (wx, wy) = (self.x.trunc(), self.y.trunc());
        if wx == 0.0 && wy == 0.0 {
            return None;
        }
        self.x -= wx;
        self.y -= wy;
        Some((wx as i32, wy as i32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulator_keeps_fractions() {
        let mut acc = Accumulator::default();
        for _ in 0..3 {
            acc.add(0.4, -0.4);
        }
        assert_eq!(acc.take_whole(), Some((1, -1)));
        assert_eq!(acc.take_whole(), None);
        acc.add(0.8, 0.0);
        assert_eq!(acc.take_whole(), Some((1, 0)));
    }

    #[test]
    fn fingerprint_accepts_colons_and_rejects_bad_input() {
        let hex = "ab".repeat(32);
        assert_eq!(parse_fingerprint(&hex).unwrap(), [0xab; 32]);
        let colons = vec!["AB"; 32].join(":");
        assert_eq!(parse_fingerprint(&colons).unwrap(), [0xab; 32]);
        assert!(parse_fingerprint("abcd").is_err());
        assert!(parse_fingerprint(&"zz".repeat(32)).is_err());
    }
}
