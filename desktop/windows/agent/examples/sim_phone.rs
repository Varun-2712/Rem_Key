//! Stands in for the phone app: pairs with or connects to a running agent and sends one of
//! each kind of input, so the whole pipeline can be checked without a phone.
//!
//!     pir-agent --pair                       # prints "Pairing data: {...}"
//!     cargo run -p pir_agent --example sim_phone -- pair '<pairing data JSON>'
//!     cargo run -p pir_agent --example sim_phone -- connect 127.0.0.1:8080 <agent fingerprint>
//!
//! The simulated phone keeps its identity in `--data-dir` (default: a `sim_phone` folder in
//! the system temp directory), so after pairing once it can `connect` again.

use std::path::PathBuf;
use std::time::{Duration, Instant};
use anyhow::{bail, Context};
use pir_agent::client;
use pir_input::coalescer::{TouchCoalescer, TouchPoint};
use pir_input::state::InputStateManager;
use pir_pairing::pairing::PairingInfo;
use pir_pairing::parse_fingerprint;
use pir_protocol::events::{EventMetadata, InputEvent, KeyCode, ModifierState, SessionEventType};
use pir_session::capabilities::get_default_phone_capabilities;
use pir_transport::InputTransport;
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let data_dir = match args.iter().position(|a| a == "--data-dir") {
        Some(i) => {
            args.remove(i);
            PathBuf::from(args.remove(i))
        }
        None => std::env::temp_dir().join("sim_phone"),
    };
    let identity = client::load_identity(&data_dir)?;

    let mut conn = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["pair", json] => {
            let info = PairingInfo::from_json(json).context("parsing pairing data")?;
            let conn = client::pair(&info, &identity, "Simulated phone").await?;
            println!("paired with {}", info.name);
            conn
        }
        ["connect", addr, fingerprint] => client::connect(addr.parse()?, parse_fingerprint(fingerprint)?, &identity).await?,
        _ => bail!("usage: sim_phone [--data-dir DIR] pair <json> | connect <addr> <fingerprint>"),
    };
    println!("connected to {}", conn.remote_addr().map(|a| a.to_string()).unwrap_or_default());

    let (version, capabilities) = client::hello(&mut conn, get_default_phone_capabilities()).await?;
    println!("handshake: protocol {version}, capabilities {capabilities:?}");

    let session = Uuid::new_v4();
    let mut state = InputStateManager::new(session);

    // A one-finger drag, 120 Hz touch samples, 100 px right and 50 px down.
    let mut coalescer = TouchCoalescer::new(session, 60);
    let start = Instant::now();
    let mut moves = 0;
    for i in 0..=50 {
        let at = start + Duration::from_millis(i * 8);
        let touch = TouchPoint { x: i as f32 * 2.0, y: i as f32, pressure: 1.0, timestamp: at, pointer_id: 0 };
        tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await;
        if let Some(event) = coalescer.process_touches(&[touch]) {
            conn.send_unreliable(&event).await?;
            moves += 1;
        }
    }
    coalescer.process_touches(&[]);
    if let Some(event) = coalescer.flush() {
        conn.send_unreliable(&event).await?;
        moves += 1;
    }
    println!("sent {moves} coalesced mouse moves from 51 touch samples");

    for event in state.handle_key_down(KeyCode::KeyA, ModifierState::default())
        .into_iter()
        .chain(state.handle_key_up(KeyCode::KeyA, ModifierState::default()))
        .chain(state.handle_scroll(0, -2))
        .chain(state.handle_text("hello 👋".into()))
        .chain(state.handle_composition_start())
        .chain(["wor", "word", "wor", "world"].into_iter().flat_map(|t| state_update(&mut state, t)).collect::<Vec<_>>())
        .collect::<Vec<_>>()
    {
        conn.send_reliable(&event).await?;
    }
    for event in state.handle_composition_commit() {
        conn.send_reliable(&event).await?;
    }
    println!("sent key, scroll, text and composition events");

    conn.send_reliable(&InputEvent::SessionEvent { metadata: EventMetadata::new(session, 0), event: SessionEventType::Heartbeat }).await?;
    println!("heartbeat reply: {:?}", client::next_session_event(&mut conn).await?);

    // Simulate the phone dying mid-shortcut: Shift is held and the connection just drops.
    let shift = ModifierState { shift: true, ..Default::default() };
    for event in state.handle_key_down(KeyCode::ShiftLeft, shift) {
        conn.send_reliable(&event).await?;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    println!("dropping connection with Shift held");
    drop(conn);
    Ok(())
}

fn state_update(state: &mut InputStateManager, text: &str) -> Vec<InputEvent> {
    state.handle_composition_update(text.to_string())
}
