use std::net::SocketAddr;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct QuicConfig {
    pub bind_addr: SocketAddr,
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
    pub keep_alive_interval: Duration,
    pub max_frame_size: usize,
    pub max_concurrent_streams: u32,
}

impl Default for QuicConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:8080".parse().unwrap(),
            connect_timeout: Duration::from_secs(10),
            // A phone that vanishes mid-keypress is detected (and its keys released by the
            // agent) after this long. Keep-alives every 2 s keep a healthy idle link up.
            idle_timeout: Duration::from_secs(10),
            keep_alive_interval: Duration::from_secs(2),
            max_frame_size: 64 * 1024,
            max_concurrent_streams: 100,
        }
    }
}

impl QuicConfig {
    pub fn with_bind_addr(mut self, addr: SocketAddr) -> Self {
        self.bind_addr = addr;
        self
    }

    pub fn with_idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }
}