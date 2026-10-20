use std::net::SocketAddr;
use std::sync::Arc;
use quinn::{Endpoint, ClientConfig, ServerConfig, Connection, SendStream, RecvStream, TransportConfig, crypto::rustls::QuicClientConfig};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;
use pir_protocol::events::InputEvent;
use pir_protocol::codec::{encode_event, decode_event, flags_for_event, FrameFlags, MAX_FRAME_SIZE};
use crate::config::QuicConfig;
use crate::transport::InputTransport;
use tracing::{debug, warn};

/// Frames on the reliable stream are prefixed with their length as a big-endian u32,
/// because a QUIC stream is a byte stream and does not preserve message boundaries.
const LENGTH_PREFIX_LEN: usize = 4;

/// Incoming events are buffered between the reader tasks and `recv()`.
const INCOMING_QUEUE_LEN: usize = 1024;

pub type CertFingerprint = [u8; 32];

/// SHA-256 of a DER certificate. The phone pins this value (delivered via the pairing QR)
/// instead of relying on a CA, since the desktop agent uses a self-signed certificate.
pub fn cert_fingerprint(cert: &CertificateDer<'_>) -> CertFingerprint {
    let digest = ring::digest::digest(&ring::digest::SHA256, cert.as_ref());
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_ref());
    out
}

fn crypto_provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn transport_config(config: &QuicConfig) -> anyhow::Result<Arc<TransportConfig>> {
    let mut transport = TransportConfig::default();
    transport.max_idle_timeout(Some(config.idle_timeout.try_into()?));
    transport.keep_alive_interval(Some(config.keep_alive_interval));
    Ok(Arc::new(transport))
}

/// One established QUIC connection, used by both the phone (client) and desktop (server) side.
///
/// Two background tasks read the reliable stream and incoming datagrams and feed a single
/// queue, so `recv()` returns events from either path in arrival order.
pub struct QuicConnection {
    endpoint: Option<Endpoint>,
    connection: Connection,
    reliable_tx: SendStream,
    incoming: mpsc::Receiver<anyhow::Result<InputEvent>>,
    readers: Vec<JoinHandle<()>>,
    session_id: Uuid,
}

pub type QuicClientTransport = QuicConnection;
pub type QuicServerConnection = QuicConnection;

impl QuicConnection {
    fn start(endpoint: Option<Endpoint>, connection: Connection, reliable_tx: SendStream, reliable_rx: RecvStream) -> Self {
        let (tx, incoming) = mpsc::channel(INCOMING_QUEUE_LEN);
        let readers = vec![
            tokio::spawn(read_stream_frames(reliable_rx, tx.clone())),
            tokio::spawn(read_datagrams(connection.clone(), tx)),
        ];
        Self { endpoint, connection, reliable_tx, incoming, readers, session_id: Uuid::new_v4() }
    }

    /// Connect to a desktop agent, accepting only a server certificate whose SHA-256
    /// matches `expected_fingerprint`.
    pub async fn connect(addr: SocketAddr, server_name: &str, expected_fingerprint: CertFingerprint) -> anyhow::Result<Self> {
        Self::connect_with_config(addr, server_name, expected_fingerprint, &QuicConfig::default()).await
    }

    pub async fn connect_with_config(
        addr: SocketAddr,
        server_name: &str,
        expected_fingerprint: CertFingerprint,
        config: &QuicConfig,
    ) -> anyhow::Result<Self> {
        let provider = crypto_provider();
        let verifier = Arc::new(PinnedCertVerifier {
            fingerprint: expected_fingerprint,
            algorithms: provider.signature_verification_algorithms,
        });
        let rustls_config = rustls::ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth();

        let mut client_config = ClientConfig::new(Arc::new(QuicClientConfig::try_from(rustls_config)?));
        client_config.transport_config(transport_config(config)?);

        let bind: SocketAddr = if addr.is_ipv6() { "[::]:0".parse()? } else { "0.0.0.0:0".parse()? };
        let mut endpoint = Endpoint::client(bind)?;
        endpoint.set_default_client_config(client_config);

        let connection = tokio::time::timeout(config.connect_timeout, endpoint.connect(addr, server_name)?)
            .await
            .map_err(|_| anyhow::anyhow!("Timed out connecting to {}", addr))??;
        debug!("Connected to {}", addr);

        let (reliable_tx, reliable_rx) = connection.open_bi().await?;
        Ok(Self::start(Some(endpoint), connection, reliable_tx, reliable_rx))
    }
}

#[async_trait::async_trait]
impl InputTransport for QuicConnection {
    async fn send_reliable(&mut self, event: &InputEvent) -> Result<(), anyhow::Error> {
        let data = encode_event(event, flags_for_event(event) | FrameFlags::RELIABLE)?;
        self.reliable_tx.write_all(&(data.len() as u32).to_be_bytes()).await?;
        self.reliable_tx.write_all(&data).await?;
        Ok(())
    }

    async fn send_unreliable(&mut self, event: &InputEvent) -> Result<(), anyhow::Error> {
        let data = encode_event(event, flags_for_event(event) - FrameFlags::RELIABLE)?;
        match self.connection.max_datagram_size() {
            Some(max) if data.len() <= max => {
                self.connection.send_datagram(data.into())?;
                Ok(())
            }
            // Peer has datagrams disabled, or the event is too big for one: don't drop it.
            _ => self.send_reliable(event).await,
        }
    }

    async fn recv(&mut self) -> Result<InputEvent, anyhow::Error> {
        self.incoming.recv().await.unwrap_or_else(|| Err(anyhow::anyhow!("Connection closed")))
    }

    async fn close(&mut self) -> Result<(), anyhow::Error> {
        let _ = self.reliable_tx.finish();
        self.connection.close(0u32.into(), b"closing");
        if let Some(endpoint) = &self.endpoint {
            endpoint.wait_idle().await;
        }
        Ok(())
    }

    fn local_addr(&self) -> Option<SocketAddr> {
        self.endpoint.as_ref().and_then(|e| e.local_addr().ok())
    }

    fn remote_addr(&self) -> Option<SocketAddr> {
        Some(self.connection.remote_address())
    }

    fn session_id(&self) -> Uuid {
        self.session_id
    }
}

impl Drop for QuicConnection {
    fn drop(&mut self) {
        for reader in &self.readers {
            reader.abort();
        }
    }
}

async fn read_stream_frames(mut rx: RecvStream, tx: mpsc::Sender<anyhow::Result<InputEvent>>) {
    let mut len_buf = [0u8; LENGTH_PREFIX_LEN];
    loop {
        let result = async {
            rx.read_exact(&mut len_buf).await?;
            let len = u32::from_be_bytes(len_buf) as usize;
            if len > MAX_FRAME_SIZE {
                anyhow::bail!("Frame too large: {} bytes (max {})", len, MAX_FRAME_SIZE);
            }
            let mut frame = vec![0u8; len];
            rx.read_exact(&mut frame).await?;
            decode_event(&frame).map_err(anyhow::Error::from)
        }
        .await;

        let fatal = result.is_err();
        if tx.send(result).await.is_err() || fatal {
            // Framing is lost after any error on the stream, so stop reading.
            break;
        }
    }
}

async fn read_datagrams(connection: Connection, tx: mpsc::Sender<anyhow::Result<InputEvent>>) {
    // A connection-level error is already reported through the stream reader.
    while let Ok(datagram) = connection.read_datagram().await {
        match decode_event(&datagram) {
            Ok(event) => {
                if tx.send(Ok(event)).await.is_err() {
                    break;
                }
            }
            // A bad datagram is dropped; it must not tear down the session.
            Err(e) => warn!("Dropping undecodable datagram: {}", e),
        }
    }
}

pub struct QuicServerTransport {
    endpoint: Endpoint,
    fingerprint: CertFingerprint,
}

impl QuicServerTransport {
    pub async fn bind(addr: SocketAddr) -> anyhow::Result<Self> {
        Self::bind_with_config(&QuicConfig::default().with_bind_addr(addr)).await
    }

    pub async fn bind_with_config(config: &QuicConfig) -> anyhow::Result<Self> {
        let cert = generate_self_signed_cert()?;
        let fingerprint = cert_fingerprint(&cert.cert);

        let rustls_config = rustls::ServerConfig::builder_with_provider(crypto_provider())
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_no_client_auth()
            .with_single_cert(vec![cert.cert], cert.key)?;
        let mut server_config = ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(rustls_config)?,
        ));
        server_config.transport_config(transport_config(config)?);

        let endpoint = Endpoint::server(server_config, config.bind_addr)?;
        Ok(Self { endpoint, fingerprint })
    }

    /// The value a client must pin; shown to the phone during pairing.
    pub fn cert_fingerprint(&self) -> CertFingerprint {
        self.fingerprint
    }

    pub fn local_addr(&self) -> anyhow::Result<SocketAddr> {
        Ok(self.endpoint.local_addr()?)
    }

    pub async fn accept(&mut self) -> anyhow::Result<QuicServerConnection> {
        let incoming = self.endpoint.accept().await.ok_or_else(|| anyhow::anyhow!("Endpoint closed"))?;
        let connection = incoming.await?;
        debug!("Accepted connection from {}", connection.remote_address());

        let (reliable_tx, reliable_rx) = connection.accept_bi().await?;
        Ok(QuicConnection::start(None, connection, reliable_tx, reliable_rx))
    }
}

/// Accepts exactly one certificate, identified by its SHA-256 fingerprint, but still
/// checks that the peer holds that certificate's private key.
#[derive(Debug)]
struct PinnedCertVerifier {
    fingerprint: CertFingerprint,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if cert_fingerprint(end_entity) == self.fingerprint {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::InvalidCertificate(rustls::CertificateError::ApplicationVerificationFailure))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

struct CertBundle {
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
}

fn generate_self_signed_cert() -> anyhow::Result<CertBundle> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])?;
    let key = PrivatePkcs8KeyDer::from(cert.serialize_private_key_der());
    Ok(CertBundle {
        cert: CertificateDer::from(cert.serialize_der()?),
        key: PrivateKeyDer::from(key),
    })
}
