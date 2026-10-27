use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pir_input::state::InputStateManager;
use pir_pairing::identity::StoredIdentity;
use pir_protocol::events::{
    DisconnectReason, EventMetadata, InputEvent, ProtocolVersion, SessionEventType,
};
use pir_session::capabilities::get_default_phone_capabilities;
use pir_transport::quic::{CertFingerprint, QuicClientTransport, TlsIdentity};
use pir_transport::InputTransport;
use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

use crate::commands::{Command, PointerButton, SpecialKey};
use crate::errors::RelayError;
use crate::pairing::{parse_endpoints, parse_fingerprint, DesktopTarget, PairingCode};

/// Commands waiting for the connection task. Small enough that a stalled network shows up
/// as dropped pointer deltas quickly instead of a burst of stale movement later.
const COMMAND_QUEUE_LEN: usize = 256;
/// Most commands drained per wakeup, so pointer deltas get merged without starving recv.
const MAX_BATCH: usize = 64;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
/// How long dropping the client waits for the session to release held input and close.
const GRACEFUL_SHUTDOWN: Duration = Duration::from_secs(2);
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
    /// The current or last connection task, so dropping the client can let it finish.
    session: Mutex<Option<tokio::task::JoinHandle<()>>>,
    identity: Arc<TlsIdentity>,
}

#[uniffi::export]
impl RelayClient {
    /// `data_dir` is app-private storage. The phone's certificate and key are created there
    /// on first use and reused afterwards: desktops recognise the phone by that certificate,
    /// so losing it means pairing again.
    #[uniffi::constructor]
    pub fn new(data_dir: String) -> Result<Arc<Self>, RelayError> {
        let identity = load_identity(Path::new(&data_dir))
            .map_err(|e| RelayError::Identity { message: format!("{e:#}") })?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("pir-relay")
            .enable_all()
            .build()
            .expect("failed to start the relay runtime");
        Ok(Arc::new(Self { runtime: Some(runtime), commands: Mutex::new(None), session: Mutex::new(None), identity: Arc::new(identity) }))
    }

    /// This phone's certificate fingerprint, hex. The agent's `--list-devices` shows it.
    pub fn device_fingerprint(&self) -> String {
        pir_pairing::fingerprint_hex(&self.identity.fingerprint())
    }

    /// Start connecting to a desktop this phone is already paired with. Returns immediately;
    /// progress is reported through `listener`.
    pub fn connect(&self, target: DesktopTarget, listener: Box<dyn RelayListener>) -> Result<(), RelayError> {
        self.start(&target, None, listener)
    }

    /// Like `connect`, but first presents the pairing code's secret so the desktop trusts this
    /// phone from now on. `device_name` is how the phone is listed on the desktop.
    pub fn pair(&self, code: PairingCode, device_name: String, listener: Box<dyn RelayListener>) -> Result<(), RelayError> {
        let pairing = Pairing { secret: code.secret, device_name };
        self.start(&code.desktop, Some(pairing), listener)
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
    fn start(
        &self,
        target: &DesktopTarget,
        pairing: Option<Pairing>,
        listener: Box<dyn RelayListener>,
    ) -> Result<(), RelayError> {
        let peer = Peer { endpoints: parse_endpoints(&target.endpoints)?, fingerprint: parse_fingerprint(&target.fingerprint)? };

        let mut commands = self.commands.lock().unwrap();
        if commands.as_ref().is_some_and(|tx| !tx.is_closed()) {
            return Err(RelayError::AlreadyConnected);
        }
        let (tx, rx) = mpsc::channel(COMMAND_QUEUE_LEN);
        *commands = Some(tx);
        let task = self.runtime().spawn(run_session(peer, self.identity.clone(), pairing, rx, Arc::from(listener)));
        *self.session.lock().unwrap() = Some(task);
        Ok(())
    }

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
    /// Must not run on an async runtime thread (it blocks); Kotlin calls it from `close()`.
    fn drop(&mut self) {
        if let Some(tx) = self.commands.get_mut().unwrap().take() {
            let _ = tx.try_send(Command::Disconnect);
        }
        let Some(runtime) = self.runtime.take() else { return };
        // Shutting the runtime down cancels tasks at their next await, which would cut off the
        // key releases and the Disconnect; without the Disconnect the desktop keeps the old
        // session open and won't take this phone back until it times out.
        if let Some(session) = self.session.get_mut().unwrap().take() {
            let _ = runtime.block_on(tokio::time::timeout(GRACEFUL_SHUTDOWN, session));
        }
        runtime.shutdown_timeout(Duration::from_millis(100));
    }
}

fn load_identity(dir: &Path) -> anyhow::Result<TlsIdentity> {
    std::fs::create_dir_all(dir)?;
    let stored = StoredIdentity::load_or_create(dir, || TlsIdentity::generate().map(|id| id.to_der()))?;
    Ok(TlsIdentity::from_der(stored.cert_der, stored.key_der))
}

/// Where a desktop is and the certificate it must present.
struct Peer {
    endpoints: Vec<SocketAddr>,
    fingerprint: CertFingerprint,
}

/// Sent once, before Hello, to a desktop that doesn't know this phone yet.
struct Pairing {
    secret: String,
    device_name: String,
}

async fn run_session(
    peer: Peer,
    identity: Arc<TlsIdentity>,
    pairing: Option<Pairing>,
    mut commands: mpsc::Receiver<Command>,
    listener: Arc<dyn RelayListener>,
) {
    listener.on_state_changed(ConnectionState::Connecting);
    let final_state = match establish(&peer, &identity, pairing.as_ref()).await {
        Ok((mut conn, addr, session_id)) => {
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
        warn!("relay session ended: {message}");
    }
    listener.on_state_changed(final_state);
}

/// Connects to the first endpoint that answers, pairs if asked to, then says Hello.
async fn establish(
    peer: &Peer,
    identity: &TlsIdentity,
    pairing: Option<&Pairing>,
) -> anyhow::Result<(QuicClientTransport, SocketAddr, Uuid)> {
    let mut last_error = None;
    for &addr in &peer.endpoints {
        match QuicClientTransport::connect(addr, SERVER_NAME, peer.fingerprint, identity).await {
            Ok(mut conn) => {
                if let Some(pairing) = pairing {
                    request_pairing(&mut conn, pairing).await?;
                }
                let session_id = hello(&mut conn).await?;
                return Ok((conn, addr, session_id));
            }
            Err(e) => {
                warn!("could not reach {addr}: {e:#}");
                last_error = Some(e);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("no desktop address")))
}

async fn request_pairing(conn: &mut QuicClientTransport, pairing: &Pairing) -> anyhow::Result<()> {
    conn.send_reliable(&InputEvent::SessionEvent {
        metadata: EventMetadata::new(Uuid::nil(), 0),
        event: SessionEventType::PairRequest { secret: pairing.secret.clone(), device_name: pairing.device_name.clone() },
    })
    .await?;
    match next_session_event(conn).await? {
        SessionEventType::PairResult { accepted: true } => Ok(()),
        SessionEventType::PairResult { accepted: false } => anyhow::bail!("the desktop refused the pairing"),
        SessionEventType::Disconnect { reason } => Err(refused(reason)),
        other => anyhow::bail!("unexpected reply to pairing: {other:?}"),
    }
}

async fn hello(conn: &mut QuicClientTransport) -> anyhow::Result<Uuid> {
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
            Ok(if metadata.session_id.is_nil() { Uuid::new_v4() } else { metadata.session_id })
        }
        Ok(Ok(InputEvent::SessionEvent { event: SessionEventType::Disconnect { reason }, .. })) => Err(refused(reason)),
        Ok(Ok(other)) => anyhow::bail!("unexpected handshake reply: {other:?}"),
        Ok(Err(e)) => Err(e),
        Err(_) => anyhow::bail!("desktop did not answer the handshake"),
    }
}

async fn next_session_event(conn: &mut QuicClientTransport) -> anyhow::Result<SessionEventType> {
    loop {
        match tokio::time::timeout(HANDSHAKE_TIMEOUT, conn.recv()).await {
            Ok(Ok(InputEvent::SessionEvent { event, .. })) => return Ok(event),
            Ok(Ok(_)) => continue,
            Ok(Err(e)) => return Err(e),
            Err(_) => anyhow::bail!("desktop did not answer"),
        }
    }
}

fn refused(reason: DisconnectReason) -> anyhow::Error {
    match reason {
        // The agent only admits phones in its trust store, or ones presenting a live pairing secret.
        DisconnectReason::AuthFailed => anyhow::anyhow!("the desktop doesn't trust this phone; pair it again"),
        reason => anyhow::anyhow!("desktop refused the connection: {reason:?}"),
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

}
