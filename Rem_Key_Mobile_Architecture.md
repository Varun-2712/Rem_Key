# Rem_Key --- End-to-End Mobile & Desktop Architecture

> **Status:** Proposed architecture / implementation roadmap\
> **Targets:** Android first, iOS second; Windows desktop first,
> macOS/Linux later\
> **Stack:** Kotlin Multiplatform (KMP), Compose Multiplatform, Rust,
> UniFFI, Tokio, QUIC\
> **Proposed repository path:** `docs/MOBILE_ARCHITECTURE.md`

## 1. Executive summary

Rem_Key turns a phone into a wireless semantic input controller for a
desktop computer. The phone sends input commands (pointer deltas,
clicks, keys, text, clipboard, voice transcription, media commands),
**not screen pixels**. The desktop agent authenticates the client,
validates commands, tracks held-input state, and injects native OS
events.

**Ownership boundaries:**

-   **Compose UI / platform mobile code:** Touch sensing, gestures, text
    editing, voice recognition, clipboard access, permissions, haptics.
-   **KMP shared Kotlin:** Presentation state, device list, pairing UX
    coordination, settings, use cases, input-command mapping, reconnect
    policy at the app level.
-   **Rust mobile bridge:** UniFFI-compatible API, QUIC client, codec,
    cryptographic identity and protocol/session machinery.
-   **Rust desktop agent:** Listener, authentication and authorization,
    session arbitration, event routing, bounded input execution, OS
    adapter.
-   **Native desktop adapter:** Windows `SendInput` initially; macOS
    Quartz and Linux-specific backends later.

**Current vs proposed:** The uploaded repository already contains
`core/protocol`, `core/transport`, `core/session`, `core/pairing`,
`core/input`, `core/diagnostics`, and Windows desktop components.
`mobile/rust-bridge`, KMP modules, Android/iOS applications, and several
integration boundaries below are **proposals**, not claims of existing
code. The uploaded ZIP may predate merged PR #1; verify its changes
against latest `main`.

## 2. System context

``` mermaid
flowchart TD
    A[Android: Compose UI] --> K[KMP shared domain / presentation]
    I[iOS: Compose UI or SwiftUI] --> K
    K --> P[Platform RelayClient adapters]
    P --> U[Rust UniFFI mobile bridge]
    U --> R[Rust protocol + pairing + session + QUIC]
    R <-->|Authenticated QUIC over LAN| D[Rust desktop agent]
    D --> V[Validation + session authorization]
    V --> Q[Bounded input executor]
    Q --> W[Windows SendInput]
    Q -.-> M[Future macOS adapter]
    Q -.-> L[Future Linux adapter]
```

Do not route native Android or iOS permissions through Rust. Conversely,
do not implement a second wire protocol or QUIC stack in Kotlin.

### 2.1 Control plane vs data plane

  -----------------------------------------------------------------------
  Plane                   Examples                Delivery
  ----------------------- ----------------------- -----------------------
  Control                 Discovery, pairing,     Reliable, authenticated
                          capability negotiation, 
                          auth, session           
                          open/close, heartbeat   

  Data --- stateful       Key down/up, mouse      Reliable, ordered,
                          button down/up, text    bounded
                          commit, clipboard       
                          commands                

  Data --- disposable     Pointer deltas,         QUIC datagrams where
                          high-frequency          appropriate;
                          scroll/gesture updates  sequence/expiry
                                                  semantics

  Telemetry               RTT, send queue depth,  Aggregated and
                          dropped/coalesced       rate-limited
                          movements, reconnects   
  -----------------------------------------------------------------------

**Critical:** Relative pointer deltas are not intrinsically disposable:
dropping one changes total movement. Choose one of (a) reliable deltas,
(b) coalesced deltas that preserve accumulated displacement, or (c) a
datagram protocol with sequence-aware cumulative motion semantics. Never
silently discard relative deltas without defining the resulting
behavior.

## 3. Repository structure

The following tree preserves the existing Rust crates and proposes
mobile additions.

``` text
Rem_Key/
├── Cargo.toml                         # Existing Rust workspace
├── Cargo.lock
├── core/                              # Existing shared Rust crates
│   ├── protocol/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── events.rs
│   │       ├── codec.rs
│   │       ├── version.rs
│   │       └── protocol.udl
│   ├── transport/
│   │   └── src/{lib.rs,quic.rs,transport.rs,config.rs}
│   ├── session/
│   │   └── src/{lib.rs,state.rs,heartbeat.rs,capabilities.rs}
│   ├── pairing/
│   │   └── src/{lib.rs,identity.rs,pairing.rs,trust.rs,capabilities.rs}
│   ├── input/
│   │   └── src/{lib.rs,event.rs,state.rs,coalescer.rs}
│   └── diagnostics/
│       └── src/{lib.rs,metrics.rs}
├── desktop/
│   ├── windows/                       # Existing
│   │   ├── agent/
│   │   └── input_adapter/
│   ├── macos/                         # Proposed
│   └── linux/                         # Proposed
├── mobile/                             # Proposed
│   ├── rust-bridge/
│   │   ├── Cargo.toml                 # cdylib/staticlib as needed
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── client.rs
│   │   │   ├── commands.rs
│   │   │   ├── callbacks.rs
│   │   │   └── errors.rs
│   │   └── tests/
│   ├── shared/                         # Kotlin Multiplatform Gradle module
│   │   ├── build.gradle.kts
│   │   └── src/
│   │       ├── commonMain/kotlin/
│   │       │   └── dev/remkey/
│   │       │       ├── domain/
│   │       │       │   ├── model/
│   │       │       │   ├── repository/
│   │       │       │   └── usecase/
│   │       │       ├── data/
│   │       │       │   ├── DeviceRepositoryImpl.kt
│   │       │       │   └── SessionRepositoryImpl.kt
│   │       │       ├── presentation/
│   │       │       │   ├── DeviceListViewModel.kt
│   │       │       │   └── TouchpadViewModel.kt
│   │       │       └── relay/
│   │       │           ├── RelayClient.kt
│   │       │           ├── RelayCommand.kt
│   │       │           └── RelayConnectionState.kt
│   │       ├── androidMain/kotlin/
│   │       │   └── dev/remkey/platform/
│   │       │       ├── AndroidRelayClient.kt
│   │       │       ├── AndroidDiscovery.kt
│   │       │       └── AndroidSecretStore.kt
│   │       └── iosMain/kotlin/
│   │           └── dev/remkey/platform/
│   │               ├── IosRelayClient.kt
│   │               ├── IosDiscovery.kt
│   │               └── IosSecretStore.kt
│   ├── composeApp/                     # Optional shared Compose UI
│   │   └── src/commonMain/kotlin/
│   │       ├── screens/
│   │       │   ├── DeviceListScreen.kt
│   │       │   ├── PairingScreen.kt
│   │       │   ├── TouchpadScreen.kt
│   │       │   ├── KeyboardScreen.kt
│   │       │   └── SettingsScreen.kt
│   │       └── components/
│   ├── androidApp/
│   │   ├── build.gradle.kts
│   │   └── src/main/
│   │       ├── AndroidManifest.xml
│   │       └── kotlin/dev/remkey/
│   │           ├── MainActivity.kt
│   │           ├── service/RelayForegroundService.kt
│   │           └── platform/{VoiceInput.kt,Clipboard.kt,Haptics.kt}
│   ├── iosApp/
│   │   ├── RemKey.xcodeproj
│   │   └── RemKey/
│   │       ├── RemKeyApp.swift
│   │       └── Platform/{SpeechInput.swift,Clipboard.swift}
│   └── bindings/
│       ├── android/                    # Generated UniFFI Kotlin + .so artifacts
│       └── ios/                        # Generated Swift bindings + XCFramework
├── tests/
│   ├── protocol-conformance/
│   └── end-to-end/
├── docs/
│   ├── Architecture_HMD.md            # Existing
│   ├── MILESTONE.md                    # Existing
│   └── MOBILE_ARCHITECTURE.md         # This document
└── .github/workflows/                 # Proposed CI
```

**Gradle layout:** A single KMP `shared` module plus Android app is
enough for the first milestone. Split `composeApp`, feature modules, and
iOS wrapper only when complexity warrants it. `iosApp` is an Xcode
target; it is not necessarily a Gradle module.

**Cargo integration:** The current root workspace excludes `desktop` and
lists only the six `core` members. Explicitly add `mobile/rust-bridge`
to workspace members when ready and check target-specific dependency
resolution. Build the Windows desktop agent with its own manifest until
workspace organization is consolidated.

## 4. Core contracts

### 4.1 KMP-facing API

``` kotlin
interface RelayClient {
    val state: StateFlow<RelayConnectionState>
    suspend fun connect(target: PairedDevice)
    suspend fun send(command: RelayCommand)
    suspend fun disconnect()
}

sealed interface RelayCommand {
    data class PointerDelta(val dx: Float, val dy: Float) : RelayCommand
    data class Scroll(val dx: Float, val dy: Float) : RelayCommand
    data class MouseButton(val button: Button, val pressed: Boolean) : RelayCommand
    data class Key(val key: Key, val pressed: Boolean) : RelayCommand
    data class CommitText(val text: String) : RelayCommand
    data class Media(val action: MediaAction) : RelayCommand
}
```

These are **domain-level examples**, not guaranteed 1:1 matches with
existing Rust event variants. Map them explicitly to `pir_protocol` and
add protocol versioning before modifying the wire schema.

### 4.2 Rust UniFFI boundary

Expose a **long-lived** Rust client object with high-level methods:

``` rust
// Conceptual API; exact UniFFI annotations, error types,
// callback interfaces and runtime integration need implementation.
pub struct RelayClient {
    // Owns a connection task, command channel, and state notifier.
}

impl RelayClient {
    pub async fn connect(&self, target: PairedTarget) -> Result<(), RelayError> {
        todo!()
    }

    pub fn enqueue_pointer_delta(&self, dx: f64, dy: f64)
        -> Result<(), RelayError> {
        todo!()
    }

    pub async fn disconnect(&self) -> Result<(), RelayError> {
        todo!()
    }
}
```

-   Avoid an FFI crossing for every raw touch sample; aggregate in
    Kotlin and/or Rust.
-   A bounded Rust channel decouples UI dispatch from QUIC I/O.
-   Use UniFFI-supported records/enums/errors; avoid exposing Tokio
    internals.
-   Deliver connection changes via supported callbacks or observable
    adapter state.
-   Test bridge cancellation, lifecycle, and object destruction.

### 4.3 Platform binding details

**Android:** Compile Rust `cdylib` for supported Android ABIs
(`arm64-v8a` first), package `.so` libraries in the APK/AAB, and
integrate generated UniFFI Kotlin/JNA bindings according to the chosen
UniFFI generation mode. Check Android runtime compatibility and
library-loading behavior.

**iOS:** Build Rust for iOS device/simulator targets, produce an
XCFramework with appropriate Swift bindings, and expose it to the iOS
application. If KMP common code needs to call the Rust core, implement
an `iosMain` adapter over a Kotlin/Native-accessible Objective-C/C
bridge or Swift-facing wrapper. **Do not assume generated Swift UniFFI
APIs are directly callable from Kotlin common code.** This integration
is a specific proof-of-concept milestone.

## 5. Input flows

### 5.1 Pointer movement

``` mermaid
sequenceDiagram
    participant UI as Compose Touchpad
    participant G as Gesture Controller
    participant K as KMP RelayClient
    participant R as Rust Mobile Core
    participant D as Desktop Agent
    participant OS as Native OS
    UI->>G: Touch movement
    G->>G: Relative delta + sensitivity + coalescing
    G->>K: PointerDelta(dx,dy)
    K->>R: UniFFI command
    R->>R: Session metadata + serialize
    R->>D: QUIC event
    D->>D: Authenticate, validate, order, enqueue
    D->>OS: Inject pointer movement
```

**Movement policy:** Begin with reliable ordered movement or an
explicitly cumulative datagram scheme; optimize only after measuring
perceived smoothness. Keep key/button transitions reliably ordered and
do not allow coalescing to cross a click boundary.

### 5.2 Keyboard

-   Distinguish **physical key transitions** (`KeyDown`, `KeyUp`) from
    **committed Unicode text**.
-   Compose text fields and Android IME should send committed text; key
    combinations use explicit key transitions.
-   Handle composition/IME and non-ASCII characters without assuming US
    keyboard layout.
-   Desktop agent tracks pressed state **per authenticated session** and
    releases held keys/buttons when that session ends.
-   Never replay stale key-down events after reconnect.

### 5.3 Clipboard

MVP: explicit user action to **send clipboard text to desktop**. Add
desktop-to-phone clipboard later after permissions and UX are designed.
Set size limits, handle MIME types, prevent synchronization loops, and
avoid automatic background clipboard harvesting.

### 5.4 Voice

Android SpeechRecognizer / iOS Speech framework → committed text →
`CommitText` → Rust protocol → desktop text injection. Speech
recognition is native to each mobile OS; raw microphone audio does not
need to traverse QUIC for the MVP. Handle recognition permissions and
transient results separately from committed text.

### 5.5 Media keys

Map UI actions to semantic consumer-key commands (play/pause,
next/previous, volume). Desktop adapters translate to OS-native key
events; not all platforms expose identical media-key semantics.

## 6. Discovery, pairing and trust

### 6.1 Discovery

1.  Desktop advertises a service on the LAN using mDNS/DNS-SD, or
    displays a QR code with endpoint information.
2.  Phone discovers candidate desktops and shows friendly names.
3.  Discovery information is **untrusted** until authenticated pairing.
4.  User selects a desktop and completes a verified pairing ceremony.

Android NSD and iOS Bonjour/network permissions are platform-specific.
Provide manual IP entry as a development fallback.

### 6.2 Trust model

-   Persist a **desktop identity** and a device-specific pairing record.
-   Bind TLS server identity to the trusted pairing, using validated
    certificate/public-key pinning or an equivalent authenticated
    identity scheme.
-   Authenticate the phone to the desktop, not merely the desktop to the
    phone.
-   Bind the authenticated peer to session authorization and permitted
    capabilities.
-   Protect mobile credentials using Android Keystore / iOS Keychain
    where appropriate. Rust-generated private keys require an explicit
    secure-storage/signing design.
-   Pairing and revocation must be enforceable by the desktop agent, not
    merely shown in the UI.
-   Restrict listening interfaces and permissions; never expose
    unauthenticated input injection.
-   Plan identity rotation, revocation, replay protection, and rate
    limits.

### 6.3 Connection state machine

``` mermaid
stateDiagram-v2
    [*] --> Disconnected
    Disconnected --> Discovering
    Discovering --> Pairing: Untrusted desktop
    Discovering --> Connecting: Trusted desktop
    Pairing --> Connecting: Verified and saved
    Connecting --> Connected: Auth + capabilities OK
    Connecting --> Failed: Timeout / rejected
    Connected --> Reconnecting: Network lost
    Reconnecting --> Connected: New authorized session
    Reconnecting --> Failed: Retry budget exhausted
    Connected --> Disconnected: User disconnect
    Failed --> Disconnected
```

A failed identity check must **not** trigger silent trust-on-first-use
or an automatic certificate replacement. Reconnection creates a fresh
input state and reauthorizes the session.

## 7. Desktop agent design

``` mermaid
flowchart LR
    A[QUIC listener] --> B[Per-connection task]
    B --> C[Authenticated session]
    C --> D[Event validation]
    D --> E[Bounded priority-aware dispatcher]
    E --> F[Single input executor]
    F --> G[OS adapter]
    C --> H[Per-session held input state]
    H --> F
```

**Concurrency:** Spawn a connection task for each accepted peer, but
keep native input ownership centralized. A single executor can own the
OS adapter, receiving validated commands through bounded channels.
Decide whether only one controlling device is active or multiple devices
can share input. For the MVP, use **one active controller**; additional
paired phones may connect but cannot inject until granted control.

**Backpressure:** Never allow an unbounded task/future queue. Coalesce
safe movement updates; do not silently drop key-up or button-up events.
Apply explicit overflow handling, including terminating unsafe sessions
if correctness cannot be maintained.

**Cleanup:** On disconnect, timeout, cancellation, revocation, or app
shutdown, release held keys/buttons associated with that session. Test
modifier and mouse-button cleanup against the actual OS adapter, not
only the in-memory state manager.

## 8. Android and iOS lifecycle

### Android

-   Use lifecycle-aware Compose and ViewModels; a Composable must not
    own the transport.
-   Keep a session repository / service as the single connection owner.
-   For background operation, use a foreground service **only** when
    justified and compliant with current Android service-type and
    notification requirements.
-   Handle network changes, screen rotation, battery behavior, and
    microphone permission.
-   Start with active foreground usage.

### iOS

-   Keep native permissions, clipboard, speech and Bonjour integration
    in iOS-specific code.
-   Do **not** assume a persistent background QUIC connection is
    permitted; iOS background execution is constrained.
-   Support foreground sessions first and treat background suspension as
    a normal disconnect/recovery scenario.
-   Validate real-device behavior and iOS framework/binding packaging
    early.

## 9. Error handling, diagnostics and performance

**Metrics:** connection establishment latency, authentication failures,
RTT, queue depth, input dispatch latency, end-to-end event latency (with
clock-sync caveats), dropped/coalesced movements, heartbeat timeouts,
reconnect attempts, session cleanup failures.

**Error taxonomy:** discovery error, trust failure, pairing rejected,
protocol mismatch, network timeout, backpressure, permission denied,
native input injection failure.

**Observability:** Rust `tracing` on desktop/mobile; Kotlin structured
logs with session correlation IDs. Do not log clipboard contents,
dictated text, raw keys, or secrets. Avoid recording sensitive event
payloads in crash reports.

**Performance targets are hypotheses, not measured facts:** prototype at
approximately 60 UI gesture updates/second, instrument latency
distributions and tune coalescing/batching from device measurements.
Test under LAN jitter, packet loss, disconnects, and CPU pressure.

## 10. Test strategy

  -----------------------------------------------------------------------
  Level                   Test                    Expected property
  ----------------------- ----------------------- -----------------------
  Rust protocol unit      Encode/decode           Compatibility and
                          round-trip and version  bounded decoding
                          mismatch                

  Rust transport          Fragmented/coalesced    Correct framing and
  integration             frames, datagrams,      recovery
                          disconnects             

  Security integration    Unknown peer, revoked   No unauthorized
                          peer, wrong certificate injection

  Session unit            Held key/button,        Cleanup and state
                          heartbeat timeout,      isolation
                          reconnect               

  FFI tests               Rust-to-Kotlin/Swift    Stable ABI and
                          conversion and errors   cancellation

  KMP unit                Gesture mapping, state  Deterministic domain
                          reducer, reconnect      behavior
                          policy                  

  Android UI              Touch gestures, IME,    Correct semantic
                          navigation              commands

  iOS UI                  Gestures, permissions,  Correct native
                          speech                  integration

  End-to-end              Android → QUIC →        Real cursor/key
                          Windows adapter         behavior

  Chaos tests             Network drop, suspend,  No stuck inputs or
                          queue overflow          unsafe replay
  -----------------------------------------------------------------------

Test with a fake input adapter in CI. Run OS-native injection smoke
tests on appropriate desktop hosts; tests passing on Linux do not prove
Windows `SendInput` correctness.

## 11. Delivery roadmap

### Phase 0 --- Baseline and contracts

-   Sync latest `main` after PR #1.
-   Confirm protocol wire format, QUIC framing, datagram semantics, TLS
    trust behavior, and session authorization.
-   Publish a versioned event contract and transport conformance tests.
-   Define single-active-controller policy.

**Exit:** Desktop Rust client and agent exchange authenticated events
reliably.

### Phase 1 --- Android vertical slice

-   Add Rust UniFFI mobile bridge.
-   Compile for Android ARM64; package library.
-   Build minimal Android Compose app.
-   Connect to known desktop endpoint.
-   Send one movement event and display connection state.

**Exit:** Real Android phone moves Windows cursor over authenticated
QUIC.

### Phase 2 --- Usable Android MVP

-   Touchpad, click, scroll, sensitivity.
-   Keyboard/text/shortcuts.
-   Discovery, QR pairing, trust persistence.
-   Reconnect, safe release, diagnostics.

**Exit:** Phone replaces basic mouse/keyboard for foreground use.

### Phase 3 --- iOS feasibility and KMP consolidation

-   Prove Rust XCFramework + Swift bindings.
-   Prove KMP `iosMain` adapter to the bridge.
-   Share presentation/domain logic and optional Compose UI.
-   Implement iOS discovery, permissions, speech/clipboard.

**Exit:** iPhone connects to same desktop protocol without a second QUIC
implementation.

### Phase 4 --- Extended functionality

-   Clipboard (explicit opt-in), voice-to-text, media controls.
-   Better gestures and accessibility.
-   Device switching and multi-controller arbitration if needed.
-   Desktop macOS/Linux adapters.

**Exit:** Cross-platform feature parity and tested OS-specific behavior.

## 12. Contributor ownership and review boundaries

  ----------------------------------------------------------------------------
  Subsystem                    Suggested owner         Shared contract
  ---------------------------- ----------------------- -----------------------
  Rust                         Rust contributor        Event schema, session
  protocol/transport/session                           states, errors

  Desktop agent/input adapter  Desktop contributor     Injection API,
                                                       permissions, cleanup

  KMP shared logic             Mobile contributor      `RelayClient`,
                                                       `RelayCommand`, UI
                                                       states

  Android native features      Mobile contributor      Platform adapters and
                                                       permissions

  iOS native integration       Mobile/iOS contributor  FFI bridge, native APIs

  Security and end-to-end      Joint review            Pairing/auth guarantees
  testing                                              and acceptance tests
  ----------------------------------------------------------------------------

Use small PRs, each with tests and a demonstrated vertical slice. Avoid
one large PR that simultaneously changes wire protocol, cryptography,
Android UI, and desktop injection.

## 13. Architecture decision records (ADRs)

-   **ADR-001:** QUIC + existing Rust codec are the single network
    implementation.
-   **ADR-002:** Kotlin Multiplatform shares domain/presentation logic;
    platform APIs remain native.
-   **ADR-003:** UniFFI exposes coarse-grained commands, not raw Tokio
    or QUIC handles.
-   **ADR-004:** Desktop enforces authentication and authorization
    before input injection.
-   **ADR-005:** One active controlling device for the MVP.
-   **ADR-006:** Reliable ordered delivery for stateful key/button
    events; pointer transport must define loss semantics.
-   **ADR-007:** Foreground-first mobile lifecycle, with background
    support separately evaluated.
-   **ADR-008:** Android first; validate iOS Rust/KMP interoperability
    before promising full UI parity.

## 14. Open decisions before implementation

1.  Does the merged PR #1 provide mutual client authentication or only
    server certificate pinning?
2.  What exact frame/version schema is now on `main`?
3.  How are cumulative pointer deltas and datagram loss handled?
4.  Is device identity managed by Rust or signed using platform-backed
    keys?
5.  Will iOS use Compose Multiplatform or SwiftUI for its first release?
6.  Can the desktop accept multiple paired connections while permitting
    only one active controller?
7.  Which Android ABI/minimum SDK and iOS deployment target are
    supported?
8.  What permissions and capability negotiation are required for
    clipboard, voice, and media keys?

------------------------------------------------------------------------

**Implementation principle:** First prove a secure, reliable
Android-to-desktop input event. Then grow UI and features without
breaking the shared protocol and session invariants.
