use std::sync::{Arc, Mutex};
use std::time::Duration;
use anyhow::Result;
use pir_agent::client;
use pir_agent::server::{Agent, AgentConfig};
use pir_input_adapter::InputAdapter;
use pir_pairing::pairing::PairingInfo;
use pir_pairing::Fingerprint;
use pir_protocol::events::{ButtonState, EventMetadata, InputEvent, KeyCode, ModifierState, MouseButton, SessionEventType};
use pir_session::capabilities::get_default_phone_capabilities;
use pir_transport::quic::{QuicClientTransport, TlsIdentity};
use pir_transport::InputTransport;
use uuid::Uuid;

/// Records injected input so tests can see what reached the "desktop".
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<String>>>);

impl Recorder {
    fn log(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
}

impl InputAdapter for Recorder {
    fn inject_mouse_move(&mut self, dx: i32, dy: i32) -> Result<()> { self.0.lock().unwrap().push(format!("move {dx},{dy}")); Ok(()) }
    fn inject_mouse_button(&mut self, b: MouseButton, s: ButtonState) -> Result<()> { self.0.lock().unwrap().push(format!("{b:?} {s:?}")); Ok(()) }
    fn inject_scroll(&mut self, dx: i32, dy: i32) -> Result<()> { self.0.lock().unwrap().push(format!("scroll {dx},{dy}")); Ok(()) }
    fn inject_key(&mut self, k: KeyCode, s: ButtonState) -> Result<()> { self.0.lock().unwrap().push(format!("{k:?} {s:?}")); Ok(()) }
    fn inject_text(&mut self, t: &str) -> Result<()> { self.0.lock().unwrap().push(format!("text {t}")); Ok(()) }
}

struct Harness {
    _dir: tempfile::TempDir,
    addr: std::net::SocketAddr,
    fingerprint: Fingerprint,
    pairing: Option<PairingInfo>,
    injected: Recorder,
}

async fn start_agent(pairing: Option<Duration>) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        data_dir: dir.path().to_path_buf(),
        name: "test desk".into(),
        advertise: None,
    };
    let mut agent = Agent::start(&config).await.unwrap();
    let addr = agent.local_addr().unwrap();
    let fingerprint = agent.cert_fingerprint();
    let pairing = pairing.map(|d| agent.open_pairing(d));
    let injected = Recorder::default();
    tokio::spawn(agent.run(injected.clone()));
    Harness { _dir: dir, addr, fingerprint, pairing, injected }
}

fn key(key: KeyCode) -> InputEvent {
    InputEvent::KeyDown { metadata: EventMetadata::new(Uuid::nil(), 1), key, modifiers: ModifierState::default() }
}

/// Sends a key and waits until it is injected (or not, within a short time).
async fn key_reaches_desktop(conn: &mut QuicClientTransport, h: &Harness, k: KeyCode) -> bool {
    let expected = format!("{k:?} Pressed");
    if conn.send_reliable(&key(k)).await.is_err() {
        return false;
    }
    for _ in 0..50 {
        if h.injected.log().contains(&expected) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

/// The agent's answer to an unauthenticated phone, or None if it just hung up.
async fn rejection(conn: &mut QuicClientTransport) -> Option<SessionEventType> {
    client::next_session_event(conn).await.ok()
}

#[tokio::test]
async fn paired_phone_can_type_and_reconnect() {
    let h = start_agent(Some(Duration::from_secs(60))).await;
    let info = h.pairing.clone().unwrap();
    assert_eq!(info.endpoints, vec![h.addr.to_string()]);

    let phone = TlsIdentity::generate().unwrap();
    let mut conn = client::pair(&info, &phone, "Pixel").await.unwrap();
    client::hello(&mut conn, get_default_phone_capabilities()).await.unwrap();
    assert!(key_reaches_desktop(&mut conn, &h, KeyCode::KeyA).await);
    conn.close().await.unwrap();

    // Later, without the QR code: the stored trust is enough.
    let mut conn = client::connect(h.addr, h.fingerprint, &phone).await.unwrap();
    client::hello(&mut conn, get_default_phone_capabilities()).await.unwrap();
    assert!(key_reaches_desktop(&mut conn, &h, KeyCode::KeyB).await);
}

#[tokio::test]
async fn unpaired_phone_is_rejected() {
    let h = start_agent(None).await;
    let stranger = TlsIdentity::generate().unwrap();
    let mut conn = client::connect(h.addr, h.fingerprint, &stranger).await.unwrap();

    assert!(!key_reaches_desktop(&mut conn, &h, KeyCode::KeyA).await);
    assert!(matches!(rejection(&mut conn).await, Some(SessionEventType::Disconnect { .. }) | None));
    assert!(h.injected.log().is_empty());
}

#[tokio::test]
async fn unpaired_phone_cannot_type_even_while_pairing_is_open() {
    let h = start_agent(Some(Duration::from_secs(60))).await;
    let stranger = TlsIdentity::generate().unwrap();
    let mut conn = client::connect(h.addr, h.fingerprint, &stranger).await.unwrap();
    assert!(!key_reaches_desktop(&mut conn, &h, KeyCode::KeyA).await);
    assert!(h.injected.log().is_empty());
}

#[tokio::test]
async fn wrong_secret_is_refused() {
    let h = start_agent(Some(Duration::from_secs(60))).await;
    let mut info = h.pairing.clone().unwrap();
    info.secret = "0".repeat(32);

    let Err(err) = client::pair(&info, &TlsIdentity::generate().unwrap(), "evil").await else {
        panic!("pairing with a wrong secret succeeded");
    };
    assert!(err.to_string().contains("refused"), "{err}");
}

#[tokio::test]
async fn expired_pairing_is_refused() {
    let h = start_agent(Some(Duration::from_millis(1))).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    let result = client::pair(h.pairing.as_ref().unwrap(), &TlsIdentity::generate().unwrap(), "late").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn pairing_secret_works_for_one_phone_only() {
    let h = start_agent(Some(Duration::from_secs(60))).await;
    let info = h.pairing.clone().unwrap();
    client::pair(&info, &TlsIdentity::generate().unwrap(), "first").await.unwrap();
    assert!(client::pair(&info, &TlsIdentity::generate().unwrap(), "second").await.is_err());
}

#[tokio::test]
async fn phone_pinning_the_wrong_agent_cannot_connect() {
    let h = start_agent(Some(Duration::from_secs(60))).await;
    let mut info = h.pairing.clone().unwrap();
    info.cert_fingerprint = "ab".repeat(32);
    assert!(client::pair(&info, &TlsIdentity::generate().unwrap(), "x").await.is_err());
}

#[tokio::test]
async fn identity_and_trust_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        data_dir: dir.path().to_path_buf(),
        name: "desk".into(),
        advertise: None,
    };
    let phone = TlsIdentity::generate().unwrap();

    let mut first = Agent::start(&config).await.unwrap();
    let fingerprint = first.cert_fingerprint();
    let info = first.open_pairing(Duration::from_secs(60));
    let handle = tokio::spawn(first.run(Recorder::default()));
    client::pair(&info, &phone, "Pixel").await.unwrap().close().await.unwrap();
    handle.abort();
    let _ = handle.await;

    let second = Agent::start(&config).await.unwrap();
    assert_eq!(second.cert_fingerprint(), fingerprint, "agent identity must be stable");
    assert!(second.trust().is_trusted(&phone.fingerprint()));
}
