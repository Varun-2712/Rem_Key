use async_trait::async_trait;
use pir_protocol::events::InputEvent;
use pir_protocol::codec::CodecError;
use std::net::SocketAddr;
use uuid::Uuid;

#[async_trait]
pub trait InputTransport: Send + Sync {
    async fn connect(addr: SocketAddr, server_name: &str) -> Result<Self, anyhow::Error>
    where
        Self: Sized;

    async fn send_reliable(&mut self, event: &InputEvent) -> Result<(), anyhow::Error>;
    async fn send_unreliable(&mut self, event: &InputEvent) -> Result<(), anyhow::Error>;
    async fn recv(&mut self) -> Result<InputEvent, anyhow::Error>;
    async fn close(&mut self) -> Result<(), anyhow::Error>;
    
    fn local_addr(&self) -> Option<SocketAddr>;
    fn remote_addr(&self) -> Option<SocketAddr>;
    fn session_id(&self) -> Uuid;
}