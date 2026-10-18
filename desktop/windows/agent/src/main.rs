mod session;
mod decoder;
mod dispatcher;
mod diagnostics;

use clap::Parser;
use pir_transport::quic::QuicServerTransport;
use pir_input::state::InputStateManager;
use pir_input_adapter::WindowsInputAdapter;
use pir_protocol::{InputEvent, EventMetadata, SessionEventType, DisconnectReason};
use pir_diagnostics::MetricsCollector;
use tracing::{info, warn, error, debug};
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Parser, Debug)]
#[command(name = "pir-agent", version, about = "Phone Input Relay Desktop Agent")]
struct Args {
    #[arg(short, long, default_value = "0.0.0.0:8080")]
    bind: String,
    
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    
    tracing_subscriber::fmt()
        .with_env_filter(if args.verbose { "debug" } else { "info" })
        .init();

    let bind_addr: std::net::SocketAddr = args.bind.parse()?;
    info!("Starting Phone Input Relay Agent on {}", bind_addr);

    let mut server = QuicServerTransport::bind(bind_addr).await?;
    info!("QUIC server listening");

    let mut adapter = WindowsInputAdapter::new()?;
    let mut input_state = InputStateManager::new(uuid::Uuid::new_v4());
    let metrics = Arc::new(MetricsCollector::new());
    
    let metrics_clone = metrics.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            metrics_clone.print_summary();
        }
    });

    loop {
        match server.accept().await {
            Ok(mut conn) => {
                info!("Client connected from {}", conn.remote_addr().unwrap_or("unknown".parse().unwrap()));
                
                let session_id = conn.session_id();
                input_state = InputStateManager::new(session_id);
                
                let metrics = metrics.clone();
                
                loop {
                    match conn.recv().await {
                        Ok(event) => {
                            metrics.record_packet_received(0);
                            
                            if let Err(e) = handle_event(&event, &mut adapter, &mut input_state, &metrics) {
                                error!("Event handling error: {}", e);
                            }
                        }
                        Err(e) => {
                            warn!("Connection error: {}", e);
                            let release_events = input_state.reset_input_state();
                            for ev in release_events {
                                let _ = handle_event(&ev, &mut adapter, &mut input_state, &metrics);
                            }
                            break;
                        }
                    }
                }
                info!("Client disconnected");
            }
            Err(e) => {
                error!("Accept error: {}", e);
            }
        }
    }
}

fn handle_event(
    event: &InputEvent,
    adapter: &mut WindowsInputAdapter,
    state: &mut InputStateManager,
    metrics: &MetricsCollector,
) -> Result<()> {
    let start = std::time::Instant::now();
    
    match event {
        InputEvent::MouseMove { delta_x, delta_y, .. } => {
            adapter.inject_mouse_move(*delta_x, *delta_y)?;
            metrics.record_event_sent();
        }
        InputEvent::MouseButton { button, state: btn_state, .. } => {
            adapter.inject_mouse_button(*button, *btn_state)?;
            metrics.record_event_sent();
        }
        InputEvent::Scroll { delta_x, delta_y, .. } => {
            adapter.inject_scroll(*delta_x, *delta_y)?;
            metrics.record_event_sent();
        }
        InputEvent::KeyDown { key, modifiers, .. } => {
            adapter.inject_key(*key, pir_input_adapter::types::ButtonState::Pressed)?;
            state.handle_key_down(*key, *modifiers);
            metrics.record_event_sent();
        }
        InputEvent::KeyUp { key, modifiers, .. } => {
            adapter.inject_key(*key, pir_input_adapter::types::ButtonState::Released)?;
            state.handle_key_up(*key, *modifiers);
            metrics.record_event_sent();
        }
        InputEvent::Text { text, .. } => {
            adapter.inject_text(text)?;
            metrics.record_event_sent();
        }
        InputEvent::CompositionStart { .. } => {
            state.handle_composition_start();
        }
        InputEvent::CompositionUpdate { text, .. } => {
            adapter.inject_composition(text)?;
            state.handle_composition_update(text.clone());
        }
        InputEvent::CompositionCommit { .. } => {
            state.handle_composition_commit();
        }
        InputEvent::ModifierState { modifiers, .. } => {
            state.handle_modifier_state(*modifiers);
        }
        InputEvent::ConsumerKey { key, state: btn_state, .. } => {
            let key_code = match key {
                pir_protocol::ConsumerKeyCode::PlayPause => pir_input_adapter::types::KeyCode::MediaPlayPause,
                pir_protocol::ConsumerKeyCode::Stop => pir_input_adapter::types::KeyCode::MediaStop,
                pir_protocol::ConsumerKeyCode::NextTrack => pir_input_adapter::types::KeyCode::MediaTrackNext,
                pir_protocol::ConsumerKeyCode::PreviousTrack => pir_input_adapter::types::KeyCode::MediaTrackPrevious,
                pir_protocol::ConsumerKeyCode::VolumeUp => pir_input_adapter::types::KeyCode::VolumeUp,
                pir_protocol::ConsumerKeyCode::VolumeDown => pir_input_adapter::types::KeyCode::VolumeDown,
                pir_protocol::ConsumerKeyCode::Mute => pir_input_adapter::types::KeyCode::VolumeMute,
            };
            adapter.inject_key(key_code, match btn_state {
                pir_protocol::ButtonState::Pressed => pir_input_adapter::types::ButtonState::Pressed,
                pir_protocol::ButtonState::Released => pir_input_adapter::types::ButtonState::Released,
            })?;
            metrics.record_event_sent();
        }
        InputEvent::SessionEvent { event: SessionEventType::Disconnect { reason }, .. } => {
            info!("Received disconnect: {:?}", reason);
            let release_events = state.reset_input_state();
            for ev in release_events {
                handle_event(&ev, adapter, state, metrics)?;
            }
            return Err(anyhow::anyhow!("Session disconnected: {:?}", reason));
        }
        InputEvent::SessionEvent { event: SessionEventType::Heartbeat, .. } => {
            // Send heartbeat ack
        }
        _ => {}
    }
    
    metrics.record_decode_time(start.elapsed().as_micros() as u64);
    metrics.update_state(
        state.get_held_keys().len() as u32,
        state.get_held_buttons().len() as u32,
        state.has_active_composition()
    );
    
    Ok(())
}