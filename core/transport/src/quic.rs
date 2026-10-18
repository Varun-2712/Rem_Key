use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use quinn::{Endpoint, ClientConfig, ServerConfig, Connection, SendStream, RecvStream};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rcgen::{CertificateParams, DistinguishedName, KeyPair, SanType};
use uuid::Uuid;
use pir_protocol::{InputEvent, codec::{encode_event, decode_event, FrameFlags, MAX_FRAME_SIZE}};
use crate::transport::InputTransport;
use tracing::{debug, warn, error};

pub struct QuicClientTransport {
    endpoint: Endpoint,
    connection: Option<Connection>,
    reliable_tx: Option<SendStream>,
    reliable_rx: Option<RecvStream>,
    session_id: Uuid,
}

impl QuicClientTransport {
    pub async fn new(endpoint: Endpoint, connection: Connection) -> anyhow::Result<Self> {
        let (reliable_tx, reliable_rx) = connection.open_bi().await?;
        
        Ok(Self {
            endpoint,
            connection: Some(connection),
            reliable_tx: Some(reliable_tx),
            reliable_rx: Some(reliable_rx),
            session_id: Uuid::new_v4(),
        })
    }
}

#[async_trait::async_trait]
impl InputTransport for QuicClientTransport {
    async fn connect(addr: SocketAddr, server_name: &str) -> Result<Self, anyhow::Error>
    where
        Self: Sized,
    {
        let cert = generate_self_signed_cert()?;
        let mut roots = rustls::RootCertStore::empty();
        roots.add(CertificateDer::from(cert.cert.clone()))?;

        let client_config = ClientConfig::new(Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth()
        ));

        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(client_config);

        let connection = endpoint.connect(addr, server_name)?.await?;
        debug!("Connected to {}", addr);

        Self::new(endpoint, connection).await
    }

    async fn send_reliable(&mut self, event: &InputEvent) -> Result<(), anyhow::Error> {
        let flags = FrameFlags::from_bits_retain(match event {
            pir_protocol::InputEvent::KeyDown { .. } | pir_protocol::InputEvent::KeyUp { .. } => FrameFlags::RELIABLE | FrameFlags::PRIORITY,
            pir_protocol::InputEvent::Text { .. } => FrameFlags::RELIABLE,
            _ => FrameFlags::RELIABLE,
        });
        
        let data = encode_event(event, flags)?;
        
        if let Some(tx) = &mut self.reliable_tx {
            tx.write_all(&data).await?;
            tx.flush().await?;
        }
        Ok(())
    }

    async fn send_unreliable(&mut self, event: &InputEvent) -> Result<(), anyhow::Error> {
        let flags = FrameFlags::empty();
        let data = encode_event(event, flags)?;
        
        if let Some(conn) = &self.connection {
            conn.send_datagram(data.into())?;
        }
        Ok(())
    }

    async fn recv(&mut self) -> Result<InputEvent, anyhow::Error> {
        if let Some(rx) = &mut self.reliable_rx {
            let mut buf = vec![0u8; MAX_FRAME_SIZE];
            let n = rx.read(&mut buf).await?.ok_or_else(|| anyhow::anyhow!("Stream closed"))?;
            decode_event(&buf[..n]).map_err(|e| anyhow::anyhow!(e))
        } else {
            anyhow::bail!("No reliable receive stream")
        }
    }

    async fn close(&mut self) -> Result<(), anyhow::Error> {
        if let Some(tx) = self.reliable_tx.take() {
            let _ = tx.finish().await;
        }
        if let Some(conn) = self.connection.take() {
            conn.close(0u32.into(), b"closing");
        }
        self.endpoint.wait_idle().await;
        Ok(())
    }

    fn local_addr(&self) -> Option<SocketAddr> {
        self.connection.as_ref().map(|c| c.local_addr())
    }

    fn remote_addr(&self) -> Option<SocketAddr> {
        self.connection.as_ref().map(|c| c.remote_addr())
    }

    fn session_id(&self) -> Uuid {
        self.session_id
    }
}

pub struct QuicServerTransport {
    endpoint: Endpoint,
    incoming: quinn::Incoming,
}

impl QuicServerTransport {
    pub async fn bind(addr: SocketAddr) -> anyhow::Result<Self> {
        let cert = generate_self_signed_cert()?;
        let server_config = ServerConfig::with_single_cert(
            vec![cert.cert.clone()],
            cert.key.clone_key().into()
        )?;

        let mut endpoint = Endpoint::server(server_config, addr)?;
        let incoming = endpoint.accept().await.ok_or_else(|| anyhow::anyhow!("Failed to create incoming stream"))?;

        Ok(Self { endpoint, incoming })
    }

    pub async fn accept(&mut self) -> anyhow::Result<QuicServerConnection> {
        let connecting = self.incoming.accept().await.ok_or_else(|| anyhow::anyhow!("No incoming connection"))?;
        let connection = connecting.await?;
        debug!("Accepted connection from {}", connection.remote_addr());

        let (reliable_tx, reliable_rx) = connection.accept_bi().await?;
        
        Ok(QuicServerConnection {
            connection,
            reliable_tx,
            reliable_rx,
            session_id: Uuid::new_v4(),
        })
    }
}

pub struct QuicServerConnection {
    connection: Connection,
    reliable_tx: SendStream,
    reliable_rx: RecvStream,
    session_id: Uuid,
}

#[async_trait::async_trait]
impl InputTransport for QuicServerConnection {
    async fn connect(_addr: SocketAddr, _server_name: &str) -> Result<Self, anyhow::Error>
    where
        Self: Sized,
    {
        anyhow::bail!("ServerConnection cannot be created via connect()");
    }

    async fn send_reliable(&mut self, event: &InputEvent) -> Result<(), anyhow::Error> {
        let flags = FrameFlags::from_bits_retain(match event {
            pir_protocol::InputEvent::KeyDown { .. } | pir_protocol::InputEvent::KeyUp { .. } => FrameFlags::RELIABLE | FrameFlags::PRIORITY,
            pir_protocol::InputEvent::Text { .. } => FrameFlags::RELIABLE,
            _ => FrameFlags::RELIABLE,
        });
        
        let data = encode_event(event, flags)?;
        self.reliable_tx.write_all(&data).await?;
        self.reliable_tx.flush().await?;
        Ok(())
    }

    async fn send_unreliable(&mut self, event: &InputEvent) -> Result<(), anyhow::Error> {
        let data = encode_event(event, FrameFlags::empty())?;
        self.connection.send_datagram(data.into())?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<InputEvent, anyhow::Error> {
        let mut buf = vec![0u8; MAX_FRAME_SIZE];
        let n = self.reliable_rx.read(&mut buf).await?.ok_or_else(|| anyhow::anyhow!("Stream closed"))?;
        decode_event(&buf[..n]).map_err(|e| anyhow::anyhow!(e))
    }

    async fn close(&mut self) -> Result<(), anyhow::Error> {
        let _ = self.reliable_tx.finish().await;
        self.connection.close(0u32.into(), b"closing");
        Ok(())
    }

    fn local_addr(&self) -> Option<SocketAddr> {
        Some(self.connection.local_addr())
    }

    fn remote_addr(&self) -> Option<SocketAddr> {
        Some(self.connection.remote_addr())
    }

    fn session_id(&self) -> Uuid {
        self.session_id
    }
}

struct CertBundle {
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
}

fn generate_self_signed_cert() -> anyhow::Result<CertBundle> {
    let mut params = CertificateParams::new(vec!["localhost".to_string()])?;
    params.distinguished_name = DistinguishedName::new();
    params.is_ca = false;
    
    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;
    
    Ok(CertBundle {
        cert: CertificateDer::from(cert.der().to_vec()),
        key: PrivateKeyDer::from(key_pair.serialize_der()),
    })
}