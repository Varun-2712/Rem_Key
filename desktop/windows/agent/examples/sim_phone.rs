//! Stands in for the phone app: connects to a running agent and sends one of each kind of
//! input, so the whole pipeline can be checked without a phone.
//!
//!     cargo run -p pir_agent --example sim_phone -- 127.0.0.1:8080 <fingerprint from agent log>

use std::time::{Duration, Instant};
use pir_input::coalescer::{TouchCoalescer, TouchPoint};
use pir_input::state::InputStateManager;
use pir_protocol::events::{EventMetadata, InputEvent, KeyCode, ModifierState, ProtocolVersion, SessionEventType};
use pir_session::capabilities::get_default_phone_capabilities;
use pir_transport::quic::QuicClientTransport;
use pir_transport::InputTransport;
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let addr = args.next().unwrap_or_else(|| "127.0.0.1:8080".into()).parse()?;
    let fingerprint = parse_fingerprint(&args.next().ok_or_else(|| anyhow::anyhow!("usage: sim_phone <addr> <fingerprint>"))?)?;

    let mut conn = QuicClientTransport::connect(addr, "localhost", fingerprint).await?;
    println!("connected to {addr}");

    conn.send_reliable(&InputEvent::SessionEvent {
        metadata: EventMetadata::new(Uuid::nil(), 0),
        event: SessionEventType::Hello { version: ProtocolVersion::CURRENT, capabilities: get_default_phone_capabilities() },
    }).await?;
    let ack = expect_session_event(&mut conn).await?;
    println!("handshake: {ack:?}");

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
    println!("heartbeat reply: {:?}", expect_session_event(&mut conn).await?);

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

async fn expect_session_event(conn: &mut QuicClientTransport) -> anyhow::Result<SessionEventType> {
    match tokio::time::timeout(Duration::from_secs(5), conn.recv()).await?? {
        InputEvent::SessionEvent { event, .. } => Ok(event),
        other => anyhow::bail!("expected a session event, got {other:?}"),
    }
}

fn parse_fingerprint(hex: &str) -> anyhow::Result<[u8; 32]> {
    anyhow::ensure!(hex.len() == 64, "fingerprint must be 64 hex characters");
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)?;
    }
    Ok(out)
}
