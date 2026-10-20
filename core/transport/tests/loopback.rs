use std::time::Duration;
use pir_protocol::events::{EventMetadata, InputEvent, KeyCode, ModifierState};
use pir_transport::quic::{QuicClientTransport, QuicServerTransport};
use pir_transport::InputTransport;
use uuid::Uuid;

const TIMEOUT: Duration = Duration::from_secs(5);

fn key_down(seq: u64) -> InputEvent {
    InputEvent::KeyDown {
        metadata: EventMetadata::new(Uuid::nil(), seq),
        key: KeyCode::KeyA,
        modifiers: ModifierState::default(),
    }
}

fn mouse_move(seq: u64) -> InputEvent {
    InputEvent::MouseMove { metadata: EventMetadata::new(Uuid::nil(), seq), delta_x: 3, delta_y: -2 }
}

fn seq(event: &InputEvent) -> u64 {
    match event {
        InputEvent::KeyDown { metadata, .. } | InputEvent::MouseMove { metadata, .. } => metadata.sequence_number,
        other => panic!("unexpected event {other:?}"),
    }
}

async fn start_server() -> (QuicServerTransport, std::net::SocketAddr) {
    let server = QuicServerTransport::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let addr = server.local_addr().unwrap();
    (server, addr)
}

#[tokio::test]
async fn reliable_events_arrive_in_order_with_boundaries_intact() {
    let (mut server, addr) = start_server().await;
    let fingerprint = server.cert_fingerprint();
    let accept = tokio::spawn(async move { server.accept().await.unwrap() });

    let mut client = QuicClientTransport::connect(addr, "localhost", fingerprint).await.unwrap();
    // Back-to-back writes are what used to get merged into one read.
    for i in 0..500 {
        client.send_reliable(&key_down(i)).await.unwrap();
    }

    let mut conn = tokio::time::timeout(TIMEOUT, accept).await.unwrap().unwrap();
    for i in 0..500 {
        let event = tokio::time::timeout(TIMEOUT, conn.recv()).await.unwrap().unwrap();
        assert_eq!(seq(&event), i);
    }

    // And the other direction.
    conn.send_reliable(&key_down(999)).await.unwrap();
    let reply = tokio::time::timeout(TIMEOUT, client.recv()).await.unwrap().unwrap();
    assert_eq!(seq(&reply), 999);
}

#[tokio::test]
async fn unreliable_events_are_received() {
    let (mut server, addr) = start_server().await;
    let fingerprint = server.cert_fingerprint();
    let accept = tokio::spawn(async move { server.accept().await.unwrap() });

    let mut client = QuicClientTransport::connect(addr, "localhost", fingerprint).await.unwrap();
    // The server only sees the stream once something is written to it.
    client.send_reliable(&key_down(0)).await.unwrap();
    let mut conn = tokio::time::timeout(TIMEOUT, accept).await.unwrap().unwrap();
    assert_eq!(seq(&conn.recv().await.unwrap()), 0);

    for i in 1..=20 {
        client.send_unreliable(&mouse_move(i)).await.unwrap();
    }
    // Datagrams may be lost in principle, but never on loopback in practice.
    let mut received = 0;
    while received < 20 {
        let event = tokio::time::timeout(TIMEOUT, conn.recv()).await.unwrap().unwrap();
        assert!(matches!(event, InputEvent::MouseMove { delta_x: 3, delta_y: -2, .. }));
        received += 1;
    }
}

#[tokio::test]
async fn rejects_server_with_unexpected_certificate() {
    let (mut server, addr) = start_server().await;
    let mut wrong = server.cert_fingerprint();
    wrong[0] ^= 0xff;
    tokio::spawn(async move { let _ = server.accept().await; });

    let result = QuicClientTransport::connect(addr, "localhost", wrong).await;
    assert!(result.is_err(), "connection to an unpinned certificate must fail");
}
