# Phone Input Relay — Milestone Tracker

## Project Overview
Transform Android phone into wireless input device (touchpad, keyboard, voice) for Windows via QUIC transport. Semantic events generated on phone → transported via QUIC → injected via Windows `SendInput`.

---

## ✅ MVP 1 — Core Proof (COMPLETED)

### Core Infrastructure
- [x] Cargo workspace with 6 crates
- [x] Workspace dependencies managed centrally
- [x] All core crates compile without errors

### `pir_protocol` — Protocol Definitions
- [x] `InputEvent` enum with all semantic event types:
  - Mouse: `MouseMove`, `MouseButton`, `Scroll`
  - Keyboard: `KeyDown`, `KeyUp`, `ModifierState`
  - Text: `Text`, `CompositionStart`, `CompositionUpdate`, `CompositionCommit`
  - Clipboard: `ClipboardRequest`, `ClipboardResponse`
  - Consumer: `ConsumerKey`
  - Session: `SessionEvent` (Hello, Heartbeat, Disconnect, Reconnect)
  - Diagnostics: `DiagnosticEvent`
- [x] `EventMetadata` (event_id, sequence_number, timestamp, session_id)
- [x] `KeyCode` enum (100+ keys including modifiers, navigation, media)
- [x] `CapabilitySet` bitflags (MOUSE, KEYBOARD, TEXT, COMPOSITION, CLIPBOARD, VOICE, CONSUMER_KEYS, DIAGNOSTICS, BLUETOOTH_HID)
- [x] `ProtocolVersion` with negotiation logic
- [x] Binary codec using bincode (Frame: MAGIC | VERSION | FLAGS | PAYLOAD)
- [x] FrameFlags: RELIABLE, COMPRESSED, CONTROL, PRIORITY
- [x] UniFFI UDL file for Kotlin/Swift bindings generation

### `pir_transport` — Transport Layer
- [x] `InputTransport` trait (async, Send+Sync)
  - `connect(addr, server_name)`
  - `send_reliable(event)` — for keys, text, control
  - `send_unreliable(event)` — for mouse movement, scroll
  - `recv()` — decode incoming events
  - `close()` — graceful shutdown
- [x] QUIC implementation using `quinn` crate
  - Client: `QuicClientTransport` with bidirectional reliable stream + datagrams
  - Server: `QuicServerTransport` + `QuicServerConnection`
  - Self-signed cert generation via `rcgen::generate_simple_self_signed`
  - Proper rustls/quinn crypto config via `QuicClientConfig`
- [x] Frame encoding/decoding integration with protocol codec

### `pir_session` — Session Management
- [x] `SessionState` enum: Disconnected, Connecting, Negotiating, Connected, Degraded, Reconnecting
- [x] `Session` struct with:
  - Protocol version negotiation
  - Capability negotiation (intersection of client/server caps)
  - Heartbeat interval (5s), missed heartbeat tracking (max 3)
  - Activity tracking, reconnection support
- [x] `SessionManager` for multi-session handling
- [x] `HeartbeatManager` background task
  - Sends heartbeat every interval
  - Handles timeout → Degraded → Disconnected transitions
  - Updates activity on incoming heartbeat/ack

### `pir_pairing` — Device Identity & Pairing
- [x] `DeviceIdentity` with Ed25519 keypair (signing + verifying)
  - `generate()` — new random identity
  - `from_bytes()` — restore from 32-byte seed
  - `public_key_b64()` — base64 encoded public key
  - `sign()` / `verify()` — message signing
  - `device_id()` — short human-readable ID
  - Zeroize on drop
- [x] `PairingInfo` QR code payload (JSON):
  ```json
  {
    "version": 1,
    "device_id": "pir-...",
    "public_key": "base64...",
    "endpoint": "quic://192.168.1.50:8080",
    "name": "Phone Name",
    "capabilities": ["mouse", "keyboard", "text", ...]
  }
  ```
- [x] `PairingManager` with trust store
  - `generate_pairing_qr()` — creates QR data
  - `verify_pairing()` — challenge-response auth
  - `add_trusted_device()` / `is_trusted()` / `get_trusted()`
- [x] `TrustedDevice` with serde serialization (custom pubkey serde)
- [x] `capability_to_string()` for capability negotiation

### `pir_input` — Input Processing Pipeline
- [x] `TouchCoalescer` — high-frequency touch processing:
  - Gesture classification (Pan/Pinch/Scroll/Tap)
  - Velocity estimation (per-pointer)
  - Acceleration curve (Windows-like)
  - Movement coalescing (merge small consecutive deltas)
  - 60-120Hz scheduler (configurable target rate)
  - `process_touches()` → `Option<InputEvent::MouseMove>`
  - `flush()` for final buffered movement
- [x] `InputStateManager` — explicit input state tracking:
  - `held_keys: HashSet<KeyCode>`
  - `held_mouse_buttons: HashSet<MouseButton>`
  - `modifiers: ModifierState`
  - `active_drag`, `active_composition`
  - Event handlers: `handle_key_down/up`, `handle_mouse_button/move/scroll`, `handle_text`, `handle_composition_*`, `handle_modifier_state`, `handle_consumer_key`
  - **CRITICAL**: `reset_input_state()` — releases ALL held keys, buttons, clears modifiers, cancels drag, terminates composition on disconnect/timeout/reset

### `pir_diagnostics` — Observability
- [x] `MetricsCollector` with atomic counters:
  - Connection: RTT samples, jitter, packet loss, bytes sent/received
  - Input: touch events received, events sent, events coalesced
  - Desktop: decode time, injection time
  - State: held keys, held buttons, active composition
- [x] `get_diagnostics()` → `DiagnosticPayload::InputStats`
- [x] `print_summary()` — structured logging output

---

## ⏳ MVP 2 — Reliable Foundation (PENDING)

### Discovery & Pairing
- [ ] mDNS discovery (`mdns-sd` crate) — service `_phone-input-relay._udp.local.`
- [ ] QR code display on desktop, scan on phone
- [ ] Mutual authentication (challenge-response with Ed25519)
- [ ] Trust persistence (file-based `TrustStore`)

### Connection Reliability
- [ ] Heartbeat + timeout detection (implemented in session, needs integration)
- [ ] Reconnection with session resume (`SessionEvent::Reconnect { session_id }`)
- [ ] Input state reset on ALL disconnect paths (implemented, needs integration)

### Capability Negotiation
- [ ] Hello/HelloAck exchange during session establishment
- [ ] UI adaptation based on negotiated capabilities

---

## ⏳ MVP 3 — Voice Input (PENDING)

### Android Voice Pipeline
- [ ] `SpeechRecognizer` integration (Google ML Kit on-device)
- [ ] Partial results → `CompositionUpdate`
- [ ] Final result → `CompositionCommit`
- [ ] Shared text engine (keyboard + voice converge)

---

## ⏳ Platform Expansion (PENDING)

### Windows Desktop Agent (`pir_agent`)
- [ ] Build with `x86_64-pc-windows-gnu` target
- [ ] `SendInput` adapter (implemented in `pir_input_adapter`)
- [ ] System tray UI for pairing/status
- [ ] Windows service / auto-start

### Android App (`mobile/`, see `mobile/README.md`)
- [x] `pir_mobile` UniFFI bridge: `RelayClient` with coarse commands, bounded queue, Hello handshake, heartbeats, held-input release on disconnect
- [x] Gradle project: KMP `shared` module + Jetpack Compose `androidApp`
- [x] UniFFI bindings generation and cargo-ndk build wired into Gradle
- [x] Touchpad UI (shared `TouchpadGestures`: move, tap/two-finger tap, natural two-finger scroll)
- [x] Keyboard UI (committed text + editing keys)
- [x] QUIC client integration (manual host/port/fingerprint until pairing lands)
- [ ] First build on a machine with the Android SDK/NDK, then on a real phone
- [ ] mTLS client identity once `feature/pairing-auth` merges

### Cross-Platform
- [ ] macOS (`CGEvent` adapter)
- [ ] Linux X11 / Wayland adapters
- [ ] iOS (SwiftUI)
- [ ] Bluetooth HID transport

---

## Technical Decisions Log

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Serialization | bincode | Compact, fast, Rust-native |
| QUIC | quinn | Pure Rust, active maintenance |
| Windows Input | windows-rs (SendInput) | Official Microsoft crate |
| Discovery | mdns-sd | Cross-platform, zero-config |
| Crypto | ed25519-dalek + rustls | Modern, audited, no C deps |
| Speech | Google ML Kit | On-device, free, offline |
| FFI | UniFFI | Generates Kotlin/Swift from UDL |
| Logging | tracing | Structured, spans, platform outputs |

---

## Known Issues / TODOs

### Code Quality
- [ ] Fix clippy warnings across workspace
- [ ] Add comprehensive unit tests (protocol, state, transport)
- [ ] Add integration tests (E2E)
- [ ] Document public APIs

### Architecture
- [ ] Verify `reset_input_state()` called on ALL error paths in agent
- [ ] Add QUIC stream prioritization for key events
- [ ] Implement proper certificate validation (TOFU for MVP 0, PKI for MVP 1)

### Build
- [ ] Windows cross-compilation setup
- [x] Android Gradle + UniFFI config (Exec tasks, no plugin)
- [ ] CI/CD pipeline (GitHub Actions with Windows/Linux runners)

---

## Next Immediate Actions

1. **Install Windows target**: `rustup target add x86_64-pc-windows-gnu`
2. **Build desktop crates**: `cargo build --target x86_64-pc-windows-gnu -p pir_agent -p pir_input_adapter`
3. **Create Android project** with Gradle + UniFFI plugin
4. **Generate UniFFI bindings** for Kotlin
5. **Run MVP 0 E2E test**: Android → QUIC → Windows cursor movement
