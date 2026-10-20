//! UniFFI surface for the phone apps. Kotlin (and later Swift) sees one long-lived
//! `RelayClient` with coarse commands; the wire protocol, QUIC and input-state tracking
//! stay in Rust so there is only one implementation of them.

mod client;
mod commands;
mod errors;

pub use client::{ConnectionState, RelayClient, RelayListener};
pub use commands::{PointerButton, SpecialKey};
pub use errors::RelayError;

uniffi::setup_scaffolding!();
