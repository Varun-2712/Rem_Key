mod dispatch;

use clap::Parser;
use dispatch::Dispatcher;
use pir_diagnostics::metrics::MetricsCollector;
use pir_input_adapter::{InputAdapter, PlatformAdapter, DryRunAdapter};
use pir_protocol::events::{DisconnectReason, EventMetadata, InputEvent, SessionEventType};
use pir_protocol::version::negotiate_version;
use pir_session::capabilities::{get_default_desktop_capabilities, negotiate_capabilities};
use pir_session::heartbeat::heartbeat_ack;
use pir_transport::quic::{QuicServerConnection, QuicServerTransport};
use pir_transport::InputTransport;
use tracing::{error, info, warn};
use anyhow::Result;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(name = "pir-agent", version, about = "Phone Input Relay Desktop Agent")]
struct Args {
    #[arg(short, long, default_value = "0.0.0.0:8080")]
    bind: String,

    /// Log the input that would be injected instead of injecting it.
    #[arg(long)]
    dry_run: bool,

    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(if args.verbose { "debug" } else { "info" })
        .init();

    if args.dry_run || cfg!(not(windows)) {
        if cfg!(not(windows)) {
            warn!("No native input adapter for this platform yet; running in dry-run mode");
        }
        run(args, DryRunAdapter::new()?).await
    } else {
        run(args, PlatformAdapter::new()?).await
    }
}

async fn run<A: InputAdapter>(args: Args, adapter: A) -> Result<()> {
    let bind_addr: std::net::SocketAddr = args.bind.parse()?;
    let mut server = QuicServerTransport::bind(bind_addr).await?;
    info!("Listening on {}", server.local_addr()?);
    info!("Certificate fingerprint: {}", hex(&server.cert_fingerprint()));
    warn!("Pairing is not enforced yet: any device that knows the fingerprint can connect");

    let metrics = Arc::new(MetricsCollector::new());
    let metrics_clone = metrics.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.tick().await;
        loop {
            interval.tick().await;
            metrics_clone.print_summary();
        }
    });

    let mut dispatcher = Dispatcher::new(adapter, Uuid::nil());
    loop {
        match server.accept().await {
            Ok(mut conn) => {
                let remote = conn.remote_addr().map(|a| a.to_string()).unwrap_or_else(|| "unknown".into());
                info!("Client connected from {}", remote);
                dispatcher.reset_session(conn.session_id());

                let reason = serve_connection(&mut conn, &mut dispatcher, &metrics).await;
                // Whatever ended the session, nothing may stay pressed.
                dispatcher.release_all();
                let _ = conn.close().await;
                info!("Client {} disconnected: {}", remote, reason);
            }
            Err(e) => error!("Accept error: {}", e),
        }
    }
}

/// Runs one session until it ends, returning why it ended.
async fn serve_connection<A: InputAdapter>(
    conn: &mut QuicServerConnection,
    dispatcher: &mut Dispatcher<A>,
    metrics: &MetricsCollector,
) -> String {
    let session_id = conn.session_id();
    loop {
        let event = match conn.recv().await {
            Ok(event) => event,
            Err(e) => return format!("connection error: {e}"),
        };
        metrics.record_packet_received(0);

        if let InputEvent::SessionEvent { event: session_event, .. } = &event {
            match session_event {
                SessionEventType::Hello { version, capabilities } => {
                    let reply = match negotiate_version(*version, pir_protocol::events::ProtocolVersion::CURRENT) {
                        Ok(negotiated) => {
                            let caps = negotiate_capabilities(*capabilities, get_default_desktop_capabilities());
                            info!("Hello from phone: protocol {} capabilities {:?}", negotiated, caps);
                            Ok(InputEvent::SessionEvent {
                                metadata: EventMetadata::new(session_id, 0),
                                event: SessionEventType::HelloAck { version: negotiated, capabilities: caps },
                            })
                        }
                        Err(e) => Err(e),
                    };
                    match reply {
                        Ok(ack) => {
                            if let Err(e) = conn.send_reliable(&ack).await {
                                return format!("failed to send HelloAck: {e}");
                            }
                        }
                        Err(e) => {
                            let _ = conn.send_reliable(&InputEvent::SessionEvent {
                                metadata: EventMetadata::new(session_id, 0),
                                event: SessionEventType::Disconnect { reason: DisconnectReason::ProtocolMismatch },
                            }).await;
                            return format!("protocol mismatch: {e}");
                        }
                    }
                }
                SessionEventType::Heartbeat => {
                    if let Err(e) = conn.send_reliable(&heartbeat_ack(session_id)).await {
                        return format!("failed to send heartbeat ack: {e}");
                    }
                }
                SessionEventType::Disconnect { reason } => return format!("peer disconnected ({reason:?})"),
                SessionEventType::HelloAck { .. } | SessionEventType::HeartbeatAck | SessionEventType::Reconnect { .. } => {}
            }
            continue;
        }

        let start = std::time::Instant::now();
        match dispatcher.apply(&event) {
            Ok(true) => {
                metrics.record_injection_time(start.elapsed().as_micros() as u64);
                metrics.record_event_sent();
            }
            Ok(false) => {}
            Err(e) => error!("Event handling error: {}", e),
        }
        let state = dispatcher.state();
        metrics.update_state(
            state.get_held_keys().len() as u32,
            state.get_held_buttons().len() as u32,
            state.has_active_composition(),
        );
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
