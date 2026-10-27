//! Pairs `RelayClient` with the real desktop agent and checks it is recognised afterwards.

use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use pir_agent::server::{Agent, AgentConfig};
use pir_input_adapter::InputAdapter;
use pir_mobile::{parse_pairing_code, ConnectionState, PairingCode, RelayClient, RelayListener, SpecialKey};
use pir_protocol::events::{ButtonState, KeyCode, MouseButton};

const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<String>>>);

impl InputAdapter for Recorder {
    fn inject_mouse_move(&mut self, _: i32, _: i32) -> Result<()> { Ok(()) }
    fn inject_mouse_button(&mut self, _: MouseButton, _: ButtonState) -> Result<()> { Ok(()) }
    fn inject_scroll(&mut self, _: i32, _: i32) -> Result<()> { Ok(()) }
    fn inject_key(&mut self, k: KeyCode, s: ButtonState) -> Result<()> { self.0.lock().unwrap().push(format!("{k:?} {s:?}")); Ok(()) }
    fn inject_text(&mut self, t: &str) -> Result<()> { self.0.lock().unwrap().push(format!("text {t}")); Ok(()) }
}

impl Recorder {
    async fn wait_for(&self, entry: &str) -> bool {
        for _ in 0..200 {
            if self.0.lock().unwrap().iter().any(|e| e == entry) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    }
}

struct ChannelListener(std_mpsc::Sender<ConnectionState>);

impl RelayListener for ChannelListener {
    fn on_state_changed(&self, state: ConnectionState) {
        let _ = self.0.send(state);
    }
}

/// Waits (off the async runtime) for the first state that isn't Connecting.
async fn settled(states: std_mpsc::Receiver<ConnectionState>) -> ConnectionState {
    tokio::task::spawn_blocking(move || loop {
        match states.recv_timeout(TIMEOUT).expect("no state change") {
            ConnectionState::Connecting => continue,
            state => return state,
        }
    })
    .await
    .unwrap()
}

fn listener() -> (Box<dyn RelayListener>, std_mpsc::Receiver<ConnectionState>) {
    let (tx, rx) = std_mpsc::channel();
    (Box::new(ChannelListener(tx)), rx)
}

struct Desk {
    _dir: tempfile::TempDir,
    code: PairingCode,
    injected: Recorder,
}

async fn start_agent() -> Desk {
    let dir = tempfile::tempdir().unwrap();
    let config = AgentConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        data_dir: dir.path().to_path_buf(),
        name: "test desk".into(),
        advertise: None,
    };
    let mut agent = Agent::start(&config).await.unwrap();
    // Exactly what the QR code holds.
    let qr = agent.open_pairing(Duration::from_secs(60)).to_json().unwrap();
    let code = parse_pairing_code(qr).unwrap();
    let injected = Recorder::default();
    tokio::spawn(agent.run(injected.clone()));
    Desk { _dir: dir, code, injected }
}

async fn drop_client(client: Arc<RelayClient>) {
    tokio::task::spawn_blocking(move || drop(client)).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn pairs_then_reconnects_without_the_secret() {
    let desk = start_agent().await;
    let phone_dir = tempfile::tempdir().unwrap();
    let data_dir = phone_dir.path().to_string_lossy().into_owned();

    let client = RelayClient::new(data_dir.clone()).unwrap();
    let fingerprint = client.device_fingerprint();
    let (l, states) = listener();
    client.pair(desk.code.clone(), "Pixel".into(), l).unwrap();
    assert_eq!(settled(states).await, ConnectionState::Connected);
    client.commit_text("paired".into()).unwrap();
    assert!(desk.injected.wait_for("text paired").await);
    client.disconnect();
    drop_client(client).await;

    // A new client over the same storage is the same phone, so the desktop lets it in.
    let client = RelayClient::new(data_dir).unwrap();
    assert_eq!(client.device_fingerprint(), fingerprint);
    let (l, states) = listener();
    client.connect(desk.code.desktop.clone(), l).unwrap();
    assert_eq!(settled(states).await, ConnectionState::Connected);
    client.special_key(SpecialKey::Tab, true).unwrap();
    assert!(desk.injected.wait_for("Tab Pressed").await);
    client.disconnect();
    drop_client(client).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn unpaired_phone_is_told_to_pair() {
    let desk = start_agent().await;
    let phone_dir = tempfile::tempdir().unwrap();
    let client = RelayClient::new(phone_dir.path().to_string_lossy().into_owned()).unwrap();
    let (l, states) = listener();
    client.connect(desk.code.desktop.clone(), l).unwrap();
    match settled(states).await {
        ConnectionState::Failed { message } => assert!(message.contains("pair"), "{message}"),
        other => panic!("expected failure, got {other:?}"),
    }
    drop_client(client).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_secret_is_refused() {
    let desk = start_agent().await;
    let phone_dir = tempfile::tempdir().unwrap();
    let client = RelayClient::new(phone_dir.path().to_string_lossy().into_owned()).unwrap();
    let mut code = desk.code.clone();
    code.secret = "0".repeat(32);
    let (l, states) = listener();
    client.pair(code, "Pixel".into(), l).unwrap();
    assert!(matches!(settled(states).await, ConnectionState::Failed { .. }));
    drop_client(client).await;
}
