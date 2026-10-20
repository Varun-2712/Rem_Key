use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use anyhow::Result;
use pir_diagnostics::metrics::MetricsCollector;
use pir_input_adapter::InputAdapter;
use pir_pairing::identity::StoredIdentity;
use pir_pairing::pairing::{PairingInfo, PairingWindow};
use pir_pairing::trust::TrustStore;
use pir_pairing::{fingerprint_hex, Fingerprint};
use pir_protocol::events::{DisconnectReason, EventMetadata, InputEvent, ProtocolVersion, SessionEventType};
use pir_protocol::version::negotiate_version;
use pir_session::capabilities::{get_default_desktop_capabilities, negotiate_capabilities};
use pir_session::heartbeat::heartbeat_ack;
use pir_transport::config::QuicConfig;
use pir_transport::quic::{QuicServerConnection, QuicServerTransport, TlsIdentity};
use pir_transport::InputTransport;
use tracing::{error, info, warn};
use uuid::Uuid;
use crate::dispatch::Dispatcher;

/// How long an unpaired phone has to send its pairing request after connecting.
const PAIR_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub struct AgentConfig {
    pub bind: SocketAddr,
    /// Holds the agent's identity and the paired devices.
    pub data_dir: PathBuf,
    /// Shown on the phone when pairing.
    pub name: String,
    /// Address to put in the pairing QR code instead of the detected LAN address.
    pub advertise: Option<SocketAddr>,
}

pub struct Agent {
    server: QuicServerTransport,
    trust: TrustStore,
    pairing: Option<PairingWindow>,
    name: String,
    endpoints: Vec<String>,
    metrics: Arc<MetricsCollector>,
}

impl Agent {
    pub async fn start(config: &AgentConfig) -> Result<Self> {
        let stored = StoredIdentity::load_or_create(&config.data_dir, || TlsIdentity::generate().map(|id| id.to_der()))?;
        let identity = TlsIdentity::from_der(stored.cert_der, stored.key_der);
        let server = QuicServerTransport::bind_with_identity(&QuicConfig::default().with_bind_addr(config.bind), identity).await?;
        let trust = TrustStore::open(config.data_dir.join(TrustStore::FILE_NAME))?;

        let local = server.local_addr()?;
        let endpoints = match config.advertise {
            Some(addr) => vec![addr.to_string()],
            None if local.ip().is_unspecified() => lan_ip()
                .into_iter()
                .map(|ip| SocketAddr::new(ip, local.port()).to_string())
                .collect(),
            None => vec![local.to_string()],
        };

        Ok(Self { server, trust, pairing: None, name: config.name.clone(), endpoints, metrics: Arc::new(MetricsCollector::new()) })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.server.local_addr()
    }

    pub fn cert_fingerprint(&self) -> Fingerprint {
        self.server.cert_fingerprint()
    }

    pub fn trust(&self) -> &TrustStore {
        &self.trust
    }

    pub fn metrics(&self) -> Arc<MetricsCollector> {
        self.metrics.clone()
    }

    /// Opens a pairing window and returns what the QR code should contain.
    pub fn open_pairing(&mut self, duration: Duration) -> PairingInfo {
        let window = PairingWindow::open(duration);
        let info = PairingInfo {
            version: PairingInfo::VERSION,
            name: self.name.clone(),
            endpoints: self.endpoints.clone(),
            cert_fingerprint: fingerprint_hex(&self.cert_fingerprint()),
            secret: window.secret().to_string(),
        };
        self.pairing = Some(window);
        info
    }

    pub async fn run<A: InputAdapter>(mut self, adapter: A) -> Result<()> {
        let mut dispatcher = Dispatcher::new(adapter, Uuid::nil());
        loop {
            let mut conn = match self.server.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    error!("Accept error: {}", e);
                    continue;
                }
            };
            let remote = conn.remote_addr().map(|a| a.to_string()).unwrap_or_else(|| "unknown".into());

            let device = match self.authenticate(&mut conn).await {
                Ok(device) => device,
                Err(reason) => {
                    warn!("Rejected {}: {}", remote, reason);
                    let _ = conn.send_reliable(&disconnect(conn.session_id(), DisconnectReason::AuthFailed)).await;
                    let _ = conn.close().await;
                    continue;
                }
            };

            info!("{} connected from {}", device, remote);
            dispatcher.reset_session(conn.session_id());
            let reason = serve_connection(&mut conn, &mut dispatcher, &self.metrics).await;
            // Whatever ended the session, nothing may stay pressed.
            dispatcher.release_all();
            let _ = conn.close().await;
            info!("{} disconnected: {}", device, reason);
        }
    }

    /// Admits a trusted device, or pairs a new one if pairing is open and it presents
    /// the current secret. Returns the device's name.
    async fn authenticate(&mut self, conn: &mut QuicServerConnection) -> Result<String, String> {
        // Mutual TLS has already proven the phone holds the key for this fingerprint.
        let fingerprint = conn.peer_fingerprint().ok_or("no client certificate")?;
        if let Some(device) = self.trust.get(&fingerprint) {
            return Ok(device.name.clone());
        }

        let window = self.pairing.as_mut().filter(|w| w.is_open(Instant::now()))
            .ok_or_else(|| format!("unknown device {} and pairing is closed", short(&fingerprint)))?;

        let first = tokio::time::timeout(PAIR_REQUEST_TIMEOUT, conn.recv()).await
            .map_err(|_| "no pairing request".to_string())?
            .map_err(|e| e.to_string())?;
        let InputEvent::SessionEvent { event: SessionEventType::PairRequest { secret, device_name }, .. } = first else {
            return Err(format!("unknown device {} did not ask to pair", short(&fingerprint)));
        };

        if let Err(e) = window.redeem(&secret, Instant::now()) {
            let _ = conn.send_reliable(&pair_result(conn.session_id(), false)).await;
            return Err(format!("pairing refused: {e}"));
        }
        let device = self.trust.add(&fingerprint, &device_name).map_err(|e| format!("saving paired device: {e}"))?;
        info!("Paired new device {} ({})", device.name, short(&fingerprint));
        conn.send_reliable(&pair_result(conn.session_id(), true)).await.map_err(|e| e.to_string())?;
        Ok(device.name)
    }
}

/// Runs one authenticated session until it ends, returning why it ended.
async fn serve_connection<A: InputAdapter>(
    conn: &mut QuicServerConnection,
    dispatcher: &mut Dispatcher<A>,
    metrics: &MetricsCollector,
) -> String {
    let session_id = conn.session_id();
    loop {
        let event = match conn.recv().await {
            Ok(event) => event,
            Err(e) => return format!("connection error: {e}"),
        };
        metrics.record_packet_received(0);

        if let InputEvent::SessionEvent { event: session_event, .. } = &event {
            let reply = match session_event {
                SessionEventType::Hello { version, capabilities } => match negotiate_version(*version, ProtocolVersion::CURRENT) {
                    Ok(negotiated) => {
                        let caps = negotiate_capabilities(*capabilities, get_default_desktop_capabilities());
                        info!("Hello from phone: protocol {} capabilities {:?}", negotiated, caps);
                        InputEvent::SessionEvent {
                            metadata: EventMetadata::new(session_id, 0),
                            event: SessionEventType::HelloAck { version: negotiated, capabilities: caps },
                        }
                    }
                    Err(e) => {
                        let _ = conn.send_reliable(&disconnect(session_id, DisconnectReason::ProtocolMismatch)).await;
                        return format!("protocol mismatch: {e}");
                    }
                },
                SessionEventType::Heartbeat => heartbeat_ack(session_id),
                // Already paired: confirm, so a phone retrying a pairing doesn't hang.
                SessionEventType::PairRequest { .. } => pair_result(session_id, true),
                SessionEventType::Disconnect { reason } => return format!("peer disconnected ({reason:?})"),
                SessionEventType::HelloAck { .. }
                | SessionEventType::HeartbeatAck
                | SessionEventType::Reconnect { .. }
                | SessionEventType::PairResult { .. } => continue,
            };
            if let Err(e) = conn.send_reliable(&reply).await {
                return format!("failed to reply: {e}");
            }
            continue;
        }

        let start = Instant::now();
        match dispatcher.apply(&event) {
            Ok(true) => {
                metrics.record_injection_time(start.elapsed().as_micros() as u64);
                metrics.record_event_sent();
            }
            Ok(false) => {}
            Err(e) => error!("Event handling error: {}", e),
        }
        let state = dispatcher.state();
        metrics.update_state(
            state.get_held_keys().len() as u32,
            state.get_held_buttons().len() as u32,
            state.has_active_composition(),
        );
    }
}

fn disconnect(session_id: Uuid, reason: DisconnectReason) -> InputEvent {
    InputEvent::SessionEvent { metadata: EventMetadata::new(session_id, 0), event: SessionEventType::Disconnect { reason } }
}

fn pair_result(session_id: Uuid, accepted: bool) -> InputEvent {
    InputEvent::SessionEvent { metadata: EventMetadata::new(session_id, 0), event: SessionEventType::PairResult { accepted } }
}

fn short(fingerprint: &Fingerprint) -> String {
    fingerprint_hex(fingerprint)[..12].to_string()
}

/// The address other devices on the LAN most likely reach us at. Connecting a UDP socket
/// only picks a route; nothing is sent.
fn lan_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    socket.local_addr().ok().map(|a| a.ip()).filter(|ip| !ip.is_unspecified())
}
