use std::path::PathBuf;
use std::time::Duration;
use anyhow::Result;
use clap::Parser;
use pir_agent::server::{Agent, AgentConfig};
use pir_input_adapter::{DryRunAdapter, PlatformAdapter};
use pir_pairing::fsutil::default_data_dir;
use pir_pairing::trust::TrustStore;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "pir-agent", version, about = "Phone Input Relay Desktop Agent")]
struct Args {
    #[arg(short, long, default_value = "0.0.0.0:8080")]
    bind: String,

    /// Where the agent keeps its identity and paired devices.
    #[arg(long)]
    data_dir: Option<PathBuf>,

    /// Name shown on the phone when pairing.
    #[arg(long)]
    name: Option<String>,

    /// Address to put in the pairing QR code, if the detected LAN address is wrong.
    #[arg(long)]
    advertise: Option<std::net::SocketAddr>,

    /// Show a QR code and accept one new phone for --pair-seconds.
    #[arg(long)]
    pair: bool,

    #[arg(long, default_value_t = 120)]
    pair_seconds: u64,

    /// List paired devices and exit.
    #[arg(long)]
    list_devices: bool,

    /// Unpair the device whose fingerprint starts with this (8+ hex characters) and exit.
    #[arg(long, value_name = "FINGERPRINT_PREFIX")]
    forget: Option<String>,

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

    let data_dir = match &args.data_dir {
        Some(dir) => dir.clone(),
        None => default_data_dir()?,
    };

    if args.list_devices || args.forget.is_some() {
        return manage_devices(&args, &data_dir);
    }

    let config = AgentConfig {
        bind: args.bind.parse()?,
        data_dir,
        name: args.name.clone().unwrap_or_else(default_name),
        advertise: args.advertise,
    };
    let mut agent = Agent::start(&config).await?;
    info!("Listening on {} (data in {})", agent.local_addr()?, config.data_dir.display());
    info!("Certificate fingerprint: {}", pir_pairing::fingerprint_hex(&agent.cert_fingerprint()));
    info!("{} paired device(s)", agent.trust().list().count());

    if args.pair {
        let info = agent.open_pairing(Duration::from_secs(args.pair_seconds));
        let json = info.to_json()?;
        println!("\nScan with the phone app to pair (valid {} s, one phone):\n", args.pair_seconds);
        println!("{}", render_qr(&json)?);
        println!("Pairing data: {json}\n");
    } else if agent.trust().list().next().is_none() {
        warn!("No paired devices yet. Run with --pair to pair a phone.");
    }

    let metrics = agent.metrics();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        interval.tick().await;
        loop {
            interval.tick().await;
            metrics.print_summary();
        }
    });

    if args.dry_run || cfg!(not(windows)) {
        if cfg!(not(windows)) {
            warn!("No native input adapter for this platform yet; running in dry-run mode");
        }
        agent.run(DryRunAdapter::new()?).await
    } else {
        agent.run(PlatformAdapter::new()?).await
    }
}

fn manage_devices(args: &Args, data_dir: &std::path::Path) -> Result<()> {
    let mut trust = TrustStore::open(data_dir.join(TrustStore::FILE_NAME))?;
    if let Some(prefix) = &args.forget {
        let device = trust.remove_by_prefix(prefix)?;
        println!("Unpaired {} ({})", device.name, device.fingerprint);
        return Ok(());
    }
    let mut any = false;
    for device in trust.list() {
        any = true;
        println!("{}  {}  paired {}", &device.fingerprint[..16], device.name, device.paired_at.format("%Y-%m-%d %H:%M"));
    }
    if !any {
        println!("No paired devices.");
    }
    Ok(())
}

fn render_qr(data: &str) -> Result<String> {
    use qrcode::render::unicode::Dense1x2;
    let code = qrcode::QrCode::new(data.as_bytes())?;
    Ok(code.render::<Dense1x2>().quiet_zone(true).build())
}

fn default_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "Desktop".to_string())
}
