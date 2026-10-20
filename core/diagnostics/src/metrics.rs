use std::sync::atomic::{AtomicU64, AtomicU32, Ordering};
use std::time::Instant;
use pir_protocol::events::DiagnosticPayload;
use tracing::info;

pub struct MetricsCollector {
    // Connection
    pub rtt_samples: parking_lot::Mutex<Vec<f64>>,
    pub jitter_samples: parking_lot::Mutex<Vec<f64>>,
    pub packet_loss_count: AtomicU64,
    pub packets_sent: AtomicU64,
    pub packets_received: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,

    // Input
    pub touch_events_received: AtomicU64,
    pub events_sent: AtomicU64,
    pub events_coalesced: AtomicU64,

    // Desktop
    pub decode_time_us: AtomicU64,
    pub injection_time_us: AtomicU64,

    // State
    pub held_keys: AtomicU32,
    pub held_buttons: AtomicU32,
    pub active_composition: AtomicU32,

    session_start: Instant,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            rtt_samples: parking_lot::Mutex::new(Vec::with_capacity(1000)),
            jitter_samples: parking_lot::Mutex::new(Vec::with_capacity(1000)),
            packet_loss_count: AtomicU64::new(0),
            packets_sent: AtomicU64::new(0),
            packets_received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            touch_events_received: AtomicU64::new(0),
            events_sent: AtomicU64::new(0),
            events_coalesced: AtomicU64::new(0),
            decode_time_us: AtomicU64::new(0),
            injection_time_us: AtomicU64::new(0),
            held_keys: AtomicU32::new(0),
            held_buttons: AtomicU32::new(0),
            active_composition: AtomicU32::new(0),
            session_start: Instant::now(),
        }
    }

    pub fn record_rtt(&self, rtt_ms: f64) {
        let mut samples = self.rtt_samples.lock();
        samples.push(rtt_ms);
        if samples.len() > 1000 { samples.remove(0); }
    }

    pub fn record_jitter(&self, jitter_ms: f64) {
        let mut samples = self.jitter_samples.lock();
        samples.push(jitter_ms);
        if samples.len() > 1000 { samples.remove(0); }
    }

    pub fn record_packet_sent(&self, bytes: usize) {
        self.packets_sent.fetch_add(1, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn record_packet_received(&self, bytes: usize) {
        self.packets_received.fetch_add(1, Ordering::Relaxed);
        self.bytes_received.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn record_packet_loss(&self) {
        self.packet_loss_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_touch_event(&self) {
        self.touch_events_received.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_event_sent(&self) {
        self.events_sent.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_event_coalesced(&self) {
        self.events_coalesced.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_decode_time(&self, us: u64) {
        self.decode_time_us.store(us, Ordering::Relaxed);
    }

    pub fn record_injection_time(&self, us: u64) {
        self.injection_time_us.store(us, Ordering::Relaxed);
    }

    pub fn update_state(&self, held_keys: u32, held_buttons: u32, has_composition: bool) {
        self.held_keys.store(held_keys, Ordering::Relaxed);
        self.held_buttons.store(held_buttons, Ordering::Relaxed);
        self.active_composition.store(if has_composition { 1 } else { 0 }, Ordering::Relaxed);
    }

    fn average(samples: &[f64]) -> f64 {
        if samples.is_empty() { 0.0 } else { samples.iter().sum::<f64>() / samples.len() as f64 }
    }

    pub fn latency(&self) -> DiagnosticPayload {
        DiagnosticPayload::Latency {
            rtt_ms: Self::average(&self.rtt_samples.lock()),
            jitter_ms: Self::average(&self.jitter_samples.lock()),
        }
    }

    pub fn get_diagnostics(&self) -> DiagnosticPayload {
        let uptime = self.session_start.elapsed().as_secs_f64().max(f64::EPSILON);
        let touch_hz = self.touch_events_received.load(Ordering::Relaxed) as f64 / uptime;
        let sent_hz = self.events_sent.load(Ordering::Relaxed) as f64 / uptime;
        let coalesced_pct = if self.touch_events_received.load(Ordering::Relaxed) > 0 {
            self.events_coalesced.load(Ordering::Relaxed) as f64 / self.touch_events_received.load(Ordering::Relaxed) as f64 * 100.0
        } else { 0.0 };

        DiagnosticPayload::InputStats {
            touch_hz,
            sent_hz,
            coalesced_pct,
        }
    }

    pub fn print_summary(&self) {
        let diag = self.get_diagnostics();
        let sent = self.packets_sent.load(Ordering::Relaxed);
        let recv = self.packets_received.load(Ordering::Relaxed);
        let loss = self.packet_loss_count.load(Ordering::Relaxed);
        let loss_pct = if sent > 0 { loss as f64 / sent as f64 * 100.0 } else { 0.0 };

        let (rtt_ms, jitter_ms) = match self.latency() {
            DiagnosticPayload::Latency { rtt_ms, jitter_ms } => (rtt_ms, jitter_ms),
            _ => (0.0, 0.0),
        };
        let (touch_hz, sent_hz, coalesced_pct) = match diag { 
            DiagnosticPayload::InputStats { touch_hz, sent_hz, coalesced_pct } => (touch_hz, sent_hz, coalesced_pct),
            _ => (0.0, 0.0, 0.0),
        };

        let bytes_sent_kb = self.bytes_sent.load(Ordering::Relaxed) / 1024;
        let bytes_recv_kb = self.bytes_received.load(Ordering::Relaxed) / 1024;
        let decode_us = self.decode_time_us.load(Ordering::Relaxed);
        let inject_us = self.injection_time_us.load(Ordering::Relaxed);
        let keys = self.held_keys.load(Ordering::Relaxed);
        let buttons = self.held_buttons.load(Ordering::Relaxed);
        let composition = self.active_composition.load(Ordering::Relaxed) == 1;

        info!(
            "=== Diagnostics Summary ===\n\
            Connection: RTT={:.1}ms Jitter={:.1}ms Loss={:.2}%\n\
            Throughput: Sent={} pkts ({} KB), Recv={} pkts ({} KB)\n\
            Input: Touch={:.0}Hz Sent={:.0}Hz Coalesced={:.1}%\n\
            Desktop: Decode={}us Inject={}us\n\
            State: Keys={} Buttons={} Composition={}",
            rtt_ms, jitter_ms, loss_pct,
            sent, bytes_sent_kb, recv, bytes_recv_kb,
            touch_hz, sent_hz, coalesced_pct,
            decode_us, inject_us,
            keys, buttons, composition
        );
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}