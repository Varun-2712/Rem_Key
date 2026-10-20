//! Phone-side helpers for pairing and connecting, used by the `sim_phone` example and the
//! agent's tests until the phone app has its own.

use std::path::Path;
use std::time::Duration;
use anyhow::{bail, Context, Result};
use pir_pairing::identity::StoredIdentity;
use pir_pairing::pairing::PairingInfo;
use pir_pairing::{parse_fingerprint, Fingerprint};
use pir_protocol::events::{CapabilitySet, EventMetadata, InputEvent, ProtocolVersion, SessionEventType};
use pir_transport::quic::{QuicClientTransport, TlsIdentity};
use pir_transport::InputTransport;
use uuid::Uuid;

const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// The phone's own identity, created on first use in `dir`.
pub fn load_identity(dir: &Path) -> Result<TlsIdentity> {
    let stored = StoredIdentity::load_or_create(dir, || TlsIdentity::generate().map(|id| id.to_der()))?;
    Ok(TlsIdentity::from_der(stored.cert_der, stored.key_der))
}

/// Pairs with the agent described by a scanned QR code, returning the open connection.
pub async fn pair(info: &PairingInfo, identity: &TlsIdentity, device_name: &str) -> Result<QuicClientTransport> {
    let fingerprint = parse_fingerprint(&info.cert_fingerprint)?;
    let mut last_error = None;
    for endpoint in &info.endpoints {
        let addr = match endpoint.parse() {
            Ok(addr) => addr,
            Err(e) => {
                last_error = Some(anyhow::anyhow!("bad endpoint {endpoint}: {e}"));
                continue;
            }
        };
        match connect(addr, fingerprint, identity).await {
            Ok(mut conn) => {
                conn.send_reliable(&session_event(SessionEventType::PairRequest {
                    secret: info.secret.clone(),
                    device_name: device_name.to_string(),
                })).await?;
                return match next_session_event(&mut conn).await? {
                    SessionEventType::PairResult { accepted: true } => Ok(conn),
                    SessionEventType::PairResult { accepted: false } => bail!("the desktop refused the pairing"),
                    SessionEventType::Disconnect { reason } => bail!("the desktop disconnected: {reason:?}"),
                    other => bail!("unexpected reply to pairing: {other:?}"),
                };
            }
            Err(e) => last_error = Some(e),
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("pairing info has no endpoints")))
}

pub async fn connect(addr: std::net::SocketAddr, fingerprint: Fingerprint, identity: &TlsIdentity) -> Result<QuicClientTransport> {
    QuicClientTransport::connect(addr, "localhost", fingerprint, identity)
        .await
        .with_context(|| format!("connecting to {addr}"))
}

/// Sends Hello and returns the agreed version and capabilities.
pub async fn hello(conn: &mut QuicClientTransport, capabilities: CapabilitySet) -> Result<(ProtocolVersion, CapabilitySet)> {
    conn.send_reliable(&session_event(SessionEventType::Hello { version: ProtocolVersion::CURRENT, capabilities })).await?;
    match next_session_event(conn).await? {
        SessionEventType::HelloAck { version, capabilities } => Ok((version, capabilities)),
        SessionEventType::Disconnect { reason } => bail!("the desktop disconnected: {reason:?}"),
        other => bail!("unexpected reply to Hello: {other:?}"),
    }
}

pub async fn next_session_event(conn: &mut QuicClientTransport) -> Result<SessionEventType> {
    loop {
        match tokio::time::timeout(REPLY_TIMEOUT, conn.recv()).await.context("no reply from the desktop")?? {
            InputEvent::SessionEvent { event, .. } => return Ok(event),
            _ => continue,
        }
    }
}

fn session_event(event: SessionEventType) -> InputEvent {
    InputEvent::SessionEvent { metadata: EventMetadata::new(Uuid::nil(), 0), event }
}
