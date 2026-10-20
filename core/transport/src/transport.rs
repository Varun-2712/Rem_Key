use async_trait::async_trait;
use pir_protocol::events::InputEvent;
use std::net::SocketAddr;
use uuid::Uuid;

/// An established connection that carries `InputEvent`s in both directions.
/// How a connection is established (connect vs. accept, trust anchors) is transport-specific.
#[async_trait]
pub trait InputTransport: Send + Sync {
    async fn send_reliable(&mut self, event: &InputEvent) -> Result<(), anyhow::Error>;
    async fn send_unreliable(&mut self, event: &InputEvent) -> Result<(), anyhow::Error>;
    async fn recv(&mut self) -> Result<InputEvent, anyhow::Error>;
    async fn close(&mut self) -> Result<(), anyhow::Error>;

    fn local_addr(&self) -> Option<SocketAddr>;
    fn remote_addr(&self) -> Option<SocketAddr>;
    fn session_id(&self) -> Uuid;
}
