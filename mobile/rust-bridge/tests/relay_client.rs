//! Drives `RelayClient` the way the Kotlin app does, against a real QUIC server standing in
//! for the desktop agent.

use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use std::sync::Arc;

use pir_mobile::{ConnectionState, DesktopTarget, PointerButton, RelayClient, RelayError, RelayListener, SpecialKey};
use pir_protocol::events::{
    ButtonState, CapabilitySet, DisconnectReason, EventMetadata, InputEvent, KeyCode, MouseButton,
    ProtocolVersion, SessionEventType,
};
use pir_transport::quic::{QuicServerConnection, QuicServerTransport};
use pir_transport::InputTransport;
use uuid::Uuid;

const TIMEOUT: Duration = Duration::from_secs(5);

struct ChannelListener(std_mpsc::Sender<ConnectionState>);

impl RelayListener for ChannelListener {
    fn on_state_changed(&self, state: ConnectionState) {
        let _ = self.0.send(state);
    }
}

/// A client with a fresh identity. The directory must outlive it only until it is built.
fn new_client() -> Arc<RelayClient> {
    let dir = tempfile::tempdir().unwrap();
    RelayClient::new(dir.path().to_string_lossy().into_owned()).unwrap()
}

fn target(endpoint: impl Into<String>, fingerprint: String) -> DesktopTarget {
    DesktopTarget { name: "test desk".into(), endpoints: vec![endpoint.into()], fingerprint }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn wait_for(states: &std_mpsc::Receiver<ConnectionState>, want: impl Fn(&ConnectionState) -> bool) -> ConnectionState {
    loop {
        let state = states.recv_timeout(TIMEOUT).expect("no state change");
        if want(&state) {
            return state;
        }
    }
}

/// Accepts one phone and answers its Hello, like the agent does.
async fn accept_and_handshake(mut server: QuicServerTransport) -> QuicServerConnection {
    let mut conn = server.accept().await.unwrap();
    match tokio::time::timeout(TIMEOUT, conn.recv()).await.unwrap().unwrap() {
        InputEvent::SessionEvent { event: SessionEventType::Hello { .. }, .. } => {}
        other => panic!("expected Hello, got {other:?}"),
    }
    conn.send_reliable(&InputEvent::SessionEvent {
        metadata: EventMetadata::new(Uuid::new_v4(), 0),
        event: SessionEventType::HelloAck { version: ProtocolVersion::CURRENT, capabilities: CapabilitySet::all() },
    })
    .await
    .unwrap();
    conn
}

async fn next_input(conn: &mut QuicServerConnection) -> InputEvent {
    loop {
        match tokio::time::timeout(TIMEOUT, conn.recv()).await.unwrap().unwrap() {
            InputEvent::SessionEvent { event: SessionEventType::Heartbeat, .. } => continue,
            event => return event,
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn sends_input_and_releases_held_state_on_disconnect() {
    let server = QuicServerTransport::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let addr = server.local_addr().unwrap();
    let fingerprint = hex(&server.cert_fingerprint());
    let accept = tokio::spawn(accept_and_handshake(server));

    let client = new_client();
    let (tx, states) = std_mpsc::channel();
    client.connect(target(addr.to_string(), fingerprint), Box::new(ChannelListener(tx))).unwrap();
    let mut conn = accept.await.unwrap();
    tokio::task::spawn_blocking(move || {
        wait_for(&states, |s| *s == ConnectionState::Connected);
        states
    })
    .await
    .unwrap();

    // Sub-pixel deltas add up instead of rounding away.
    for _ in 0..10 {
        client.pointer_delta(0.5, -0.25).unwrap();
    }
    client.mouse_button(PointerButton::Left, true).unwrap();
    client.special_key(SpecialKey::Enter, true).unwrap();
    client.commit_text("hej 👋".into()).unwrap();

    let (mut dx, mut dy) = (0, 0);
    let first_non_move = loop {
        match next_input(&mut conn).await {
            InputEvent::MouseMove { delta_x, delta_y, .. } => {
                dx += delta_x;
                dy += delta_y;
            }
            other => break other,
        }
    };
    assert_eq!((dx, dy), (5, -2));
    assert!(matches!(
        first_non_move,
        InputEvent::MouseButton { button: MouseButton::Left, state: ButtonState::Pressed, .. }
    ));
    assert!(matches!(next_input(&mut conn).await, InputEvent::KeyDown { key: KeyCode::Enter, .. }));
    assert!(matches!(next_input(&mut conn).await, InputEvent::Text { text, .. } if text == "hej 👋"));

    // Leaving with Enter and the left button held must not leave them stuck on the desktop.
    client.disconnect();
    let mut released_enter = false;
    let mut released_left = false;
    loop {
        match next_input(&mut conn).await {
            InputEvent::KeyUp { key: KeyCode::Enter, .. } => released_enter = true,
            InputEvent::MouseButton { button: MouseButton::Left, state: ButtonState::Released, .. } => released_left = true,
            InputEvent::SessionEvent { event: SessionEventType::Disconnect { reason: DisconnectReason::Normal }, .. } => break,
            other => panic!("unexpected event during disconnect: {other:?}"),
        }
    }
    assert!(released_enter && released_left);
    assert!(matches!(client.pointer_delta(1.0, 1.0), Err(RelayError::NotConnected)));
    tokio::task::spawn_blocking(move || drop(client)).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_fingerprint_reports_failure() {
    let server = QuicServerTransport::bind("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let addr = server.local_addr().unwrap();
    let _accept = tokio::spawn(accept_and_handshake(server));

    let client = new_client();
    let (tx, states) = std_mpsc::channel();
    client.connect(target(addr.to_string(), "00".repeat(32)), Box::new(ChannelListener(tx))).unwrap();
    let state = tokio::task::spawn_blocking(move || {
        wait_for(&states, |s| !matches!(s, ConnectionState::Connecting))
    })
    .await
    .unwrap();
    assert!(matches!(state, ConnectionState::Failed { .. }), "got {state:?}");
    tokio::task::spawn_blocking(move || drop(client)).await.unwrap();
}

#[test]
fn rejects_bad_connection_details() {
    let client = new_client();
    let (tx, _states) = std_mpsc::channel();
    let fp = "ab".repeat(32);
    assert!(matches!(
        client.connect(target("not a host", fp), Box::new(ChannelListener(tx.clone()))),
        Err(RelayError::InvalidAddress { .. })
    ));
    assert!(matches!(
        client.connect(target("127.0.0.1:8080", "abcd".into()), Box::new(ChannelListener(tx))),
        Err(RelayError::InvalidFingerprint { .. })
    ));
}
