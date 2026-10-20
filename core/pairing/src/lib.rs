//! Device identity, pairing and trust.
//!
//! Every device (desktop agent and phone) is identified by a persistent self-signed TLS
//! certificate, and devices refer to each other by that certificate's SHA-256 fingerprint.
//! The QUIC transport uses mutual TLS, so the handshake itself proves that a peer holds the
//! private key behind its fingerprint; this crate decides which fingerprints to trust.
//!
//! Pairing: the agent shows a QR code with its endpoint, its fingerprint and a one-time
//! secret. The phone pins the fingerprint, connects with its own certificate and presents
//! the secret; the agent then adds the phone's fingerprint to its `TrustStore`.

pub mod capabilities;
pub mod fsutil;
pub mod identity;
pub mod pairing;
pub mod trust;

/// SHA-256 of a device's DER certificate.
pub type Fingerprint = [u8; 32];

pub fn fingerprint_hex(fingerprint: &Fingerprint) -> String {
    hex::encode(fingerprint)
}

pub fn parse_fingerprint(hex_str: &str) -> anyhow::Result<Fingerprint> {
    let bytes = hex::decode(hex_str.trim())?;
    bytes.try_into().map_err(|_| anyhow::anyhow!("fingerprint must be 32 bytes (64 hex characters)"))
}
