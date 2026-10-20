use std::time::{Duration, Instant};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// What the agent shows as a QR code while pairing is open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingInfo {
    pub version: u32,
    /// Human-readable name of the desktop.
    pub name: String,
    /// `ip:port` addresses the phone can try, most likely first.
    pub endpoints: Vec<String>,
    /// Hex SHA-256 of the agent's certificate, which the phone pins.
    pub cert_fingerprint: String,
    /// One-time secret proving the phone saw this QR code.
    pub secret: String,
}

impl PairingInfo {
    pub const VERSION: u32 = 2;

    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let info: Self = serde_json::from_str(json)?;
        anyhow::ensure!(info.version == Self::VERSION, "unsupported pairing QR version {}", info.version);
        Ok(info)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PairingError {
    #[error("pairing is not open")]
    Closed,
    #[error("pairing window expired")]
    Expired,
    #[error("pairing secret already used")]
    AlreadyUsed,
    #[error("wrong pairing secret")]
    WrongSecret,
    #[error("too many wrong pairing secrets; pairing closed")]
    TooManyAttempts,
}

/// A time-limited, single-use pairing secret.
#[derive(Debug)]
pub struct PairingWindow {
    secret: String,
    expires_at: Instant,
    used: bool,
    failures: u32,
}

impl PairingWindow {
    pub const DEFAULT_DURATION: Duration = Duration::from_secs(120);
    /// The secret is 128 random bits, so guessing is hopeless anyway; this bounds noise
    /// from a misbehaving client.
    const MAX_FAILURES: u32 = 5;

    pub fn open(duration: Duration) -> Self {
        let mut bytes = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self { secret: hex::encode(bytes), expires_at: Instant::now() + duration, used: false, failures: 0 }
    }

    pub fn secret(&self) -> &str {
        &self.secret
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        self.expires_at.saturating_duration_since(now)
    }

    pub fn is_open(&self, now: Instant) -> bool {
        !self.used && self.failures < Self::MAX_FAILURES && now < self.expires_at
    }

    /// Consumes the secret if `presented` matches and the window is still open.
    pub fn redeem(&mut self, presented: &str, now: Instant) -> Result<(), PairingError> {
        if self.used {
            return Err(PairingError::AlreadyUsed);
        }
        if self.failures >= Self::MAX_FAILURES {
            return Err(PairingError::TooManyAttempts);
        }
        if now >= self.expires_at {
            return Err(PairingError::Expired);
        }
        if !constant_time_eq(presented.as_bytes(), self.secret.as_bytes()) {
            self.failures += 1;
            return Err(if self.failures >= Self::MAX_FAILURES { PairingError::TooManyAttempts } else { PairingError::WrongSecret });
        }
        self.used = true;
        Ok(())
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_works_once() {
        let mut w = PairingWindow::open(Duration::from_secs(60));
        let secret = w.secret().to_string();
        let now = Instant::now();
        assert_eq!(w.redeem(&secret, now), Ok(()));
        assert_eq!(w.redeem(&secret, now), Err(PairingError::AlreadyUsed));
        assert!(!w.is_open(now));
    }

    #[test]
    fn expired_window_rejects_correct_secret() {
        let mut w = PairingWindow::open(Duration::from_secs(60));
        let secret = w.secret().to_string();
        assert_eq!(w.redeem(&secret, Instant::now() + Duration::from_secs(61)), Err(PairingError::Expired));
    }

    #[test]
    fn wrong_secrets_close_the_window() {
        let mut w = PairingWindow::open(Duration::from_secs(60));
        let secret = w.secret().to_string();
        let now = Instant::now();
        for _ in 0..4 {
            assert_eq!(w.redeem("nope", now), Err(PairingError::WrongSecret));
        }
        assert_eq!(w.redeem("nope", now), Err(PairingError::TooManyAttempts));
        assert_eq!(w.redeem(&secret, now), Err(PairingError::TooManyAttempts));
    }

    #[test]
    fn secrets_are_random_and_long() {
        let a = PairingWindow::open(Duration::from_secs(1));
        let b = PairingWindow::open(Duration::from_secs(1));
        assert_ne!(a.secret(), b.secret());
        assert_eq!(a.secret().len(), 32);
    }

    #[test]
    fn qr_json_round_trips_and_checks_version() {
        let info = PairingInfo {
            version: PairingInfo::VERSION,
            name: "desk".into(),
            endpoints: vec!["192.168.1.5:8080".into()],
            cert_fingerprint: "ab".repeat(32),
            secret: "s".into(),
        };
        assert_eq!(PairingInfo::from_json(&info.to_json().unwrap()).unwrap(), info);
        let old = info.to_json().unwrap().replace("\"version\":2", "\"version\":1");
        assert!(PairingInfo::from_json(&old).is_err());
    }
}
