# Phone Input Relay — Milestone Tracker

## Project Overview
Transform Android phone into wireless input device (touchpad, keyboard, voice) for Windows via QUIC transport. Semantic events generated on phone → transported via QUIC → injected via Windows `SendInput`.

---

## ✅ MVP 1 — Core Proof (COMPLETED)

### Core Infrastructure
- [x] Cargo workspace with 8 crates (6 core + Windows agent and input adapter)
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
- [x] Devices are identified by a persistent self-signed TLS certificate; peers refer to
  each other by its SHA-256 fingerprint (`StoredIdentity`, `identity.json`)
- [x] Mutual TLS: the handshake proves each side holds the key behind its fingerprint;
  the phone pins the agent's fingerprint, the agent checks the phone's against the trust store
- [x] `PairingInfo` QR payload (JSON, version 2):
  ```json
  {
    "version": 2,
    "name": "Desktop name",
    "endpoints": ["192.168.1.50:8080"],
    "cert_fingerprint": "hex sha-256 of the agent certificate",
    "secret": "one-time 128-bit secret (hex)"
  }
  ```
- [x] `PairingWindow`: time-limited (default 120 s), single-use secret; closes after 5 wrong attempts
- [x] `TrustStore` keyed by fingerprint, persisted as `trusted_devices.json`
- [x] Secrets written atomically with owner-only (0600) permissions (`fsutil::write_private`)
- [x] Agent CLI: `--pair` (terminal QR), `--list-devices`, `--forget <prefix>`, `--data-dir`

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
- [x] QR code display on desktop (terminal) — scanning on phone pending the app
- [x] Mutual authentication (mutual TLS with pinned certificate fingerprints)
- [x] Trust persistence (file-based `TrustStore`)

### Connection Reliability
- [ ] Heartbeat + timeout detection (implemented in session, needs integration)
- [ ] Reconnection with session resume (`SessionEvent::Reconnect { session_id }`)
- [x] Input state reset on ALL disconnect paths

### Capability Negotiation
- [x] Hello/HelloAck exchange during session establishment
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

### Android App (`phone/android`)
- [ ] Gradle project with Jetpack Compose
- [ ] UniFFI bindings generation
- [ ] Touchpad UI (`pointerInput` + `detectDragGestures`)
- [ ] Keyboard UI
- [ ] QUIC client integration

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
| Crypto | rustls (ring) mutual TLS, pinned SHA-256 cert fingerprints | Audited, no separate key scheme to maintain |
| Speech | Google ML Kit | On-device, free, offline |
| FFI | UniFFI | Generates Kotlin/Swift from UDL |
| Logging | tracing | Structured, spans, platform outputs |

---

## Known Issues / TODOs

### Code Quality
- [x] Fix clippy warnings across workspace
- [ ] Add comprehensive unit tests (protocol, state, transport)
- [ ] Add integration tests (E2E)
- [ ] Document public APIs

### Architecture
- [ ] Verify `reset_input_state()` called on ALL error paths in agent
- [ ] Add QUIC stream prioritization for key events
- [x] Certificate validation (pinned fingerprints from QR pairing)
- [ ] `InputStateManager::active_drag` is never set outside tests; wire drag tracking into the input path or remove it
- [ ] `HeartbeatManager` is not used by the agent; the server only answers heartbeats
- [ ] `mdns-sd` is declared in the workspace but no crate uses it yet (MVP 2 discovery)

### Build
- [ ] Windows cross-compilation setup
- [ ] Android Gradle + UniFFI plugin config
- [ ] CI/CD pipeline (GitHub Actions with Windows/Linux runners)

---

## Next Immediate Actions

1. **Install Windows target**: `rustup target add x86_64-pc-windows-gnu`
2. **Build desktop crates**: `cargo build --target x86_64-pc-windows-gnu -p pir_agent -p pir_input_adapter`
3. **Create Android project** with Gradle + UniFFI plugin
4. **Generate UniFFI bindings** for Kotlin
5. **Run MVP 0 E2E test**: Android → QUIC → Windows cursor movement
