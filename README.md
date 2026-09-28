# Phone Input Relay --- Architecture Review and Revised Proposal

**Document purpose:** Review the current HLD proposal for the
phone-as-input-device system, identify weaknesses in the proposed
components and architecture, and present a revised architecture.

**Source reviewed:** `HLD_phone_input_relay_v2.md`

------------------------------------------------------------------------

# 1. Executive Summary

The current HLD has a strong central idea:

> **Emulate an input peripheral, not a screen.**

The phone should generate semantic input events, send them to the
computer, and let the computer's native operating-system APIs translate
those events into actual mouse, keyboard, and text input.

The current HLD already proposes:

-   SwiftUI for iOS
-   Jetpack Compose for Android
-   Rust as a shared core
-   UniFFI for Swift/Kotlin ↔ Rust
-   QUIC for LAN transport
-   mDNS/Bonjour/NSD for discovery
-   QR/public-key pairing
-   native input adapters such as Windows `SendInput` and macOS
    `CGEvent`
-   Bluetooth HID as an optional transport
-   separate text and physical-key input paths
-   explicit cleanup of held keys and mouse buttons on disconnect
-   Windows + Android as the first implementation phase

These are useful foundations.

However, the current proposal mixes several different responsibilities
together and leaves some important architectural boundaries implicit.
The biggest issues are:

1.  **Discovery is mixed into the input architecture instead of being
    treated as a control-plane function.**
2.  **QUIC is treated too directly as the transport architecture rather
    than as one implementation of a transport abstraction.**
3.  **The voice-input requirement is not represented as a first-class
    subsystem.**
4.  **Text input and speech input need a common semantic text layer.**
5.  **There is no explicit capability-negotiation layer.**
6.  **Input-state recovery deserves its own component because a stuck
    modifier or mouse button is a core failure mode.**
7.  **High-frequency touch events should be coalesced before
    transmission.**
8.  **The connection state machine needs pairing, authentication,
    negotiation, and degraded states.**
9.  **Clipboard should be treated as a privileged feature rather than
    simply another event.**
10. **The security model should be supported by an explicit threat
    model.**

The revised proposal below keeps the original design principle but
separates the system into a **data plane**, **control plane**, and
**platform integration layer**.

------------------------------------------------------------------------

# 2. Current Proposal Being Reviewed

The current HLD describes the product as a phone-controlled touchpad and
keyboard, with the phone sending compact input events to a desktop agent
rather than streaming the desktop screen back.

The original logical architecture is:

``` text
                         PHONE
              ┌─────────────────────────────┐
              │ Native UI                   │
              │                             │
              │ Touchpad   On-screen        │
              │           Keyboard          │
              │      │          │           │
              │      └────┬─────┘           │
              │           ▼                 │
              │     Input Event Engine      │
              │           │                 │
              │     Shared Rust Core        │
              │           │                 │
              │       QUIC / Bluetooth HID  │
              └────────────┬────────────────┘
                           │
                    encrypted connection
                           │
                           ▼
                     DESKTOP AGENT
              ┌─────────────────────────────┐
              │ Shared Rust Core            │
              │       │                     │
              │       ▼                     │
              │ Platform Input Adapter      │
              │                             │
              │ Windows / macOS / Linux     │
              └─────────────────────────────┘
```

The current HLD also proposes the following layer split:

  Layer                Current proposal
  -------------------- ---------------------------
  UI                   SwiftUI / Jetpack Compose
  Native bridge        UniFFI
  Core                 Rust
  Transport            QUIC
  Discovery            mDNS + QR + manual IP
  Injection            Native OS APIs
  Storage              OS secure storage
  Optional transport   Bluetooth HID

The current HLD's keyboard design deliberately separates normal text
input from physical key semantics. This is a strong design decision and
should remain.

``` text
Phone keyboard
      ↓
Unicode text / editing operation
      ↓
Protocol TextEvent
      ↓
Desktop text-input adapter
      ↓
Target application
```

The current protocol also distinguishes reliable events from
high-frequency movement events:

``` text
MouseMove  → QUIC datagram
Scroll     → QUIC datagram
KeyDown    → reliable QUIC stream
KeyUp      → reliable QUIC stream
Text       → reliable QUIC stream
Clipboard  → reliable QUIC stream
Pairing    → reliable stream
Heartbeat  → datagram/stream
```

The current HLD recommends Windows + Android as the first implementation
phase, followed by macOS + iOS, then Bluetooth HID and Linux.

------------------------------------------------------------------------

# 3. Main Architectural Weaknesses

## 3.1 Discovery is mixed with the data path

### Current proposal

The current design contains discovery components near the input flow and
represents discovery as a major part of the connection architecture.

### Weakness

Discovery and input transport have fundamentally different
responsibilities.

Discovery answers:

> Which devices are available?

The input data path answers:

> How do trusted input events reach the computer?

If these are coupled, later changes to discovery can unnecessarily
affect the input path.

Examples:

-   mDNS may not work across some network boundaries.
-   Manual IP entry may be needed.
-   A previously paired device should not need to rediscover trust every
    time.
-   Bluetooth discovery is different from LAN discovery.
-   A trusted device can potentially connect using a known endpoint
    without a fresh discovery cycle.

### Revised proposal

Separate the **control plane** from the **data plane**.

``` text
CONTROL PLANE

Discovery
   ↓
Device identification
   ↓
Pairing
   ↓
Authentication
   ↓
Capability negotiation
   ↓
Session establishment


DATA PLANE

Semantic input events
   ↓
Transport
   ↓
Desktop Agent
   ↓
Platform Input Adapter
   ↓
Operating System
```

Discovery becomes replaceable without changing the input protocol.

------------------------------------------------------------------------

## 3.2 "Desktop Daemon" is too narrow as a concept

### Current proposal

The diagram refers to a "Desktop Daemon."

### Weakness

The desktop component is not merely a background daemon. It is
responsible for:

-   session management
-   authentication
-   protocol decoding
-   event validation
-   input-state tracking
-   platform-specific injection
-   diagnostics
-   reconnect handling
-   capability negotiation

On Windows and macOS, it may also have a tray/menu-bar UI and permission
onboarding.

### Revised proposal

Use the term:

> **Desktop Agent**

The internal structure should be explicit:

``` text
Desktop Agent
│
├── Session Manager
├── Protocol Decoder
├── Capability Manager
├── Input State Manager
├── Semantic Event Dispatcher
├── Platform Input Adapter
└── Diagnostics
```

------------------------------------------------------------------------

## 3.3 QUIC is useful, but it should not define the architecture

### Current proposal

The HLD uses QUIC as the primary LAN transport and Bluetooth HID as an
optional transport.

### Weakness

The logical architecture should not depend directly on QUIC.

The product requirement is:

> Deliver semantic input events to a connected computer.

QUIC is one way of doing that.

If the protocol layer directly assumes QUIC, Bluetooth HID and future
transports become awkward exceptions.

### Revised proposal

Introduce a transport interface.

``` text
Semantic Input Event
        │
        ▼
Transport Interface
        │
        ├── QUIC Transport
        │
        ├── Bluetooth HID Transport
        │
        └── Future Transport
```

The event model remains independent of the transport.

------------------------------------------------------------------------

## 3.4 Voice input is missing as a first-class subsystem

### Current proposal

The current HLD focuses on:

-   touchpad
-   keyboard
-   text
-   clipboard
-   consumer keys

### Weakness

The intended product requirement also involves speaking into the phone
and transferring the resulting text to the computer.

That requires a separate pipeline:

``` text
Microphone
   ↓
Audio Capture
   ↓
Voice Activity Detection
   ↓
Speech Recognition
   ↓
Partial / Final Transcript
   ↓
Semantic Text Input
   ↓
Desktop
```

This should not be implemented as an unrelated feature bolted onto the
keyboard system.

### Revised proposal

Add a **Speech Input Engine** to the phone.

``` text
                  PHONE
                     │
          ┌──────────┼──────────┐
          │          │          │
       Touchpad   Keyboard     Voice
          │          │          │
          ▼          ▼          ▼
       Gesture    Keyboard    Speech
        Engine     Engine     Engine
          │          │          │
          └──────────┼──────────┘
                     ▼
              Semantic Events
```

------------------------------------------------------------------------

## 3.5 Text input should be the common abstraction for keyboard and voice

### Current proposal

The HLD correctly distinguishes `Text` from physical `Key` events.

### Weakness

The same semantic text system should also receive text generated by
speech recognition.

Otherwise the architecture becomes:

``` text
Keyboard → TextEvent
Voice    → Special voice protocol
```

That creates unnecessary coupling.

### Revised proposal

Both keyboard and speech should feed a common text-input layer.

``` text
Keyboard ───────┐
                │
Voice ──────────┼──→ Text Input Engine ─→ TextEvent / CompositionEvent
                │
Clipboard ──────┘
```

This means the desktop agent does not need to know whether:

-   the user typed the text,
-   the user spoke the text,
-   the user selected a snippet.

It receives semantic text.

------------------------------------------------------------------------

## 3.6 Partial speech recognition requires composition semantics

Voice input can produce:

``` text
Partial:
"Book me a"

Partial:
"Book me a flight"

Final:
"Book me a flight to Delhi"
```

If every partial result is blindly injected as normal text, the
application could receive repeated text.

### Revised proposal

Use composition semantics:

``` text
CompositionStart
       ↓
CompositionUpdate
       ↓
CompositionUpdate
       ↓
CompositionCommit
```

The existing `Composition` concept in the current protocol can be
extended for this purpose.

------------------------------------------------------------------------

## 3.7 Mouse movement should be coalesced

### Current proposal

The HLD targets 60--120 Hz touchpad events and proposes sending mouse
movement as unreliable datagrams.

### Weakness

A phone touch surface can generate more events than the network needs.

Sending every raw touch event can increase:

-   packet count
-   CPU usage
-   radio usage
-   network overhead
-   battery consumption

### Revised proposal

Add an event coalescing stage:

``` text
Raw touch events
       ↓
Gesture classifier
       ↓
Velocity estimation
       ↓
Movement coalescer
       ↓
60–120 Hz scheduler
       ↓
MouseMove event
```

Multiple small movements can become one semantic movement event.

For example:

``` text
+1 +1 +1 +1 +1 +1
        ↓
       +6
```

This keeps the system responsive while reducing unnecessary traffic.

------------------------------------------------------------------------

## 3.8 The connection state machine is incomplete

### Current proposal

``` text
Disconnected
     ↓
Discovered
     ↓
Connecting
     ↓
Connected
     ↓
Reconnecting
     ↓
Disconnected
```

### Weakness

The system has more states than connection establishment alone.

There are important transitions involving:

-   pairing
-   authentication
-   capability negotiation
-   degraded network conditions
-   transport failure

### Revised proposal

``` text
Disconnected
     ↓
Discovering
     ↓
Discovered
     ↓
Pairing
     ↓
Authenticating
     ↓
Negotiating
     ↓
Connected
     ↓
Degraded
     ↓
Reconnecting
     ↓
Disconnected
```

------------------------------------------------------------------------

## 3.9 Capability negotiation is missing

Different versions of the phone app and desktop agent may support
different capabilities.

For example:

``` text
Phone:
✓ mouse
✓ keyboard
✓ text
✓ voice
✓ clipboard
```

while an older desktop agent might support only:

``` text
✓ mouse
✓ keyboard
✓ text
```

### Revised proposal

During session establishment:

``` text
Phone
  ↓
ClientHello
  ↓
Desktop
  ↓
ServerHello
  ↓
Capability negotiation
  ↓
Session established
```

Capabilities can include:

``` text
mouse
keyboard
text
composition
clipboard
voice
bluetooth
diagnostics
```

The phone UI can then adapt to what the desktop actually supports.

------------------------------------------------------------------------

## 3.10 Input state should be a dedicated subsystem

The current HLD correctly requires releasing all held keys and mouse
buttons when the connection is lost.

However, this is important enough to become an explicit component.

### Revised proposal

``` text
Input State Manager
│
├── Held keys
├── Held mouse buttons
├── Modifier state
├── Active drag
└── Active text composition
```

On:

-   disconnect
-   timeout
-   transport switch
-   session reset
-   agent shutdown

the system performs:

``` text
RESET_INPUT_STATE

→ release all keys
→ release all mouse buttons
→ clear modifiers
→ cancel active drag
→ terminate composition
```

This should be a hard protocol invariant.

------------------------------------------------------------------------

## 3.11 Clipboard should be treated as a privileged feature

### Current proposal

Clipboard is represented as another protocol event.

### Weakness

Clipboard contents can contain highly sensitive information.

It should therefore have an explicit permission model.

### Revised proposal

``` text
Clipboard
   ↓
Permission check
   ↓
User-approved operation
   ↓
Encrypted transport
   ↓
Desktop clipboard adapter
```

Possible permissions:

``` text
One-time
This session
Always allow
Disabled
```

Clipboard should not automatically become active merely because a device
is paired.

------------------------------------------------------------------------

## 3.12 Bluetooth HID should remain a separate mode

The current proposal correctly treats Bluetooth HID as an additional
transport.

The architectural distinction should be made even stronger.

### Network mode

``` text
Phone
  ↓
Semantic Events
  ↓
QUIC
  ↓
Desktop Agent
  ↓
Native OS APIs
```

### Bluetooth HID mode

``` text
Phone
  ↓
HID Reports
  ↓
Computer Bluetooth HID Stack
```

Bluetooth HID cannot naturally expose all of the richer semantics of the
network protocol.

Therefore:

> Bluetooth HID should be a transport/mode with a reduced capability
> set, not the foundation of the complete protocol.

------------------------------------------------------------------------

# 4. Revised Architecture Proposal

The revised architecture separates the system into three major planes:

1.  **Input/Data Plane**
2.  **Control Plane**
3.  **Platform Integration Layer**

## 4.1 High-level architecture

``` mermaid
flowchart TB
    subgraph PHONE["Phone"]
        UI["Native UI<br/>SwiftUI / Jetpack Compose"]

        TP["Touchpad Engine"]
        KB["Keyboard Engine"]
        VOICE["Speech Input Engine"]

        UI --> TP
        UI --> KB
        UI --> VOICE

        TP --> EVENTS["Semantic Event Engine"]
        KB --> EVENTS
        VOICE --> TEXT["Text Input Engine"]

        TEXT --> EVENTS
    end

    EVENTS --> CORE["Shared Rust Core<br/>Protocol + Session + State"]

    subgraph TRANSPORT["Transport Layer"]
        QUIC["QUIC Transport"]
        HID["Bluetooth HID Transport"]
    end

    CORE --> QUIC
    CORE --> HID

    QUIC --> AGENT["Desktop Agent"]

    subgraph DESKTOP["Desktop Agent"]
        SESSION["Session Manager"]
        DECODER["Protocol Decoder"]
        STATE["Input State Manager"]
        DISPATCH["Semantic Event Dispatcher"]

        SESSION --> DECODER
        DECODER --> STATE
        STATE --> DISPATCH
    end

    AGENT --> SESSION

    DISPATCH --> ADAPTER["Platform Input Adapter"]

    ADAPTER --> WIN["Windows<br/>SendInput"]
    ADAPTER --> MAC["macOS<br/>CGEvent"]
    ADAPTER --> LINUX["Linux<br/>X11 / Wayland"]
```

------------------------------------------------------------------------

# 5. Revised Control Plane

The control plane should be independent of normal input traffic.

``` mermaid
flowchart LR
    PHONE["Phone"] --> DISC["Discovery"]
    DISC --> FOUND["Desktop Found"]
    FOUND --> PAIR["QR Pairing"]
    PAIR --> AUTH["Mutual Authentication"]
    AUTH --> CAPS["Capability Negotiation"]
    CAPS --> SESSION["Session Establishment"]
    SESSION --> DATA["Input Data Plane"]
```

## Responsibilities

### Discovery

Find candidate desktop agents.

Possible mechanisms:

-   mDNS
-   Bonjour
-   Android NSD
-   manual IP
-   QR endpoint information

### Pairing

Establish initial trust.

### Authentication

Verify that both devices belong to an already trusted relationship.

### Capability negotiation

Determine which features are supported by both sides.

### Session establishment

Create the active input session.

------------------------------------------------------------------------

# 6. Revised Phone Architecture

``` mermaid
flowchart TB
    UI["Native Phone UI"]

    UI --> TOUCH["Touchpad"]
    UI --> KEY["Keyboard"]
    UI --> MIC["Microphone"]

    TOUCH --> GESTURE["Gesture Engine"]
    GESTURE --> MOUSE["Mouse Events"]

    KEY --> KEYENGINE["Keyboard Engine"]
    KEYENGINE --> KEYEVENT["Key Events"]

    MIC --> AUDIO["Audio Capture"]
    AUDIO --> VAD["Voice Activity Detection"]
    VAD --> STT["Speech Recognition"]
    STT --> COMPOSE["Text / Composition"]

    MOUSE --> EVENTS["Semantic Event Layer"]
    KEYEVENT --> EVENTS
    COMPOSE --> EVENTS

    EVENTS --> CORE["Shared Rust Core"]
```

The phone should therefore produce semantic events rather than
operating-system-specific events.

------------------------------------------------------------------------

# 7. Revised Semantic Event Model

The event model should remain transport-independent.

Conceptually:

``` text
InputEvent
│
├── MouseMove
├── MouseButton
├── Scroll
│
├── KeyDown
├── KeyUp
├── ModifierState
│
├── Text
├── CompositionStart
├── CompositionUpdate
├── CompositionCommit
│
├── ClipboardRequest
├── ClipboardResponse
│
├── ConsumerKey
│
├── SessionEvent
└── DiagnosticEvent
```

Every event should support metadata such as:

``` text
event_id
sequence_number
timestamp
session_id
```

This enables:

-   debugging
-   latency measurement
-   duplicate detection
-   ordering checks
-   diagnostics

------------------------------------------------------------------------

# 8. Revised Input Data Flow

``` mermaid
sequenceDiagram
    participant P as Phone
    participant C as Shared Core
    participant T as Transport
    participant A as Desktop Agent
    participant O as OS Adapter
    participant OS as Operating System

    P->>C: Semantic InputEvent
    C->>T: Encoded event
    T->>A: Event
    A->>A: Decode + validate
    A->>A: Update input state
    A->>O: Semantic event
    O->>OS: Native input API
```

The important boundary is:

> The shared protocol carries semantic intent. The platform adapter
> decides how that intent is represented to the operating system.

------------------------------------------------------------------------

# 9. Revised Voice Input Architecture

If voice is a core requirement, the architecture should explicitly
support it.

``` mermaid
flowchart LR
    MIC["Phone Microphone"]
    CAPTURE["Audio Capture"]
    VAD["Voice Activity Detection"]
    STT["Speech Recognition"]
    PARTIAL["Partial Transcript"]
    FINAL["Final Transcript"]
    TEXT["Text Input Engine"]
    COMPOSE["Composition Events"]
    QUIC["Transport"]
    DESKTOP["Desktop Agent"]
    OS["Native Text Input"]

    MIC --> CAPTURE
    CAPTURE --> VAD
    VAD --> STT
    STT --> PARTIAL
    STT --> FINAL

    PARTIAL --> TEXT
    FINAL --> TEXT

    TEXT --> COMPOSE
    COMPOSE --> QUIC
    QUIC --> DESKTOP
    DESKTOP --> OS
```

This makes speech another source of semantic text rather than a separate
remote-control system.

------------------------------------------------------------------------

# 10. Revised Mouse Pipeline

``` mermaid
flowchart LR
    TOUCH["Raw Touch Events"]
    GESTURE["Gesture Classifier"]
    VELOCITY["Velocity Estimation"]
    ACCEL["Acceleration Curve"]
    COALESCE["Movement Coalescer"]
    SCHEDULE["60–120 Hz Scheduler"]
    EVENT["MouseMove Event"]
    TRANSPORT["Transport"]

    TOUCH --> GESTURE
    GESTURE --> VELOCITY
    VELOCITY --> ACCEL
    ACCEL --> COALESCE
    COALESCE --> SCHEDULE
    SCHEDULE --> EVENT
    EVENT --> TRANSPORT
```

This keeps the high-frequency touch processing local to the phone and
avoids turning every physical touch event into a network packet.

------------------------------------------------------------------------

# 11. Revised Desktop Agent

``` mermaid
flowchart TB
    SESSION["Session Manager"]
    DECODER["Protocol Decoder"]
    VALIDATOR["Event Validator"]
    STATE["Input State Manager"]
    DISPATCH["Semantic Event Dispatcher"]

    SESSION --> DECODER
    DECODER --> VALIDATOR
    VALIDATOR --> STATE
    STATE --> DISPATCH

    DISPATCH --> MOUSE["Mouse Adapter"]
    DISPATCH --> KEY["Keyboard Adapter"]
    DISPATCH --> TEXT["Text Adapter"]
    DISPATCH --> CLIP["Clipboard Adapter"]

    MOUSE --> PLATFORM["Platform Adapter Layer"]
    KEY --> PLATFORM
    TEXT --> PLATFORM
    CLIP --> PLATFORM

    PLATFORM --> WINDOWS["Windows"]
    PLATFORM --> MACOS["macOS"]
    PLATFORM --> LINUX["Linux"]
```

------------------------------------------------------------------------

# 12. Revised Platform Adapter Boundary

The shared Rust layer should not know how Windows, macOS, or Linux
performs injection.

Instead:

``` text
                    Semantic Event
                          │
                          ▼
                 Platform Adapter
                          │
             ┌────────────┼────────────┐
             ▼            ▼            ▼
          Windows       macOS        Linux
         SendInput      CGEvent     X11/Wayland
```

The shared layer should know:

``` text
MouseMove
KeyDown
KeyUp
Text
Composition
```

The platform layer should know:

``` text
How does this OS represent that operation?
```

------------------------------------------------------------------------

# 13. Revised Connection State Machine

``` mermaid
stateDiagram-v2
    [*] --> Disconnected

    Disconnected --> Discovering
    Discovering --> Discovered
    Discovered --> Pairing
    Pairing --> Authenticating
    Authenticating --> Negotiating
    Negotiating --> Connected

    Connected --> Degraded
    Degraded --> Connected

    Connected --> Reconnecting
    Degraded --> Reconnecting

    Reconnecting --> Connected
    Reconnecting --> Disconnected

    Connected --> Disconnected
```

Every transition into `Disconnected` must trigger:

``` text
RESET_INPUT_STATE
```

------------------------------------------------------------------------

# 14. Revised Input Safety Model

``` mermaid
flowchart TB
    FAILURE["Disconnect / Timeout / Crash / Session Reset"]

    FAILURE --> RESET["Reset Input State"]

    RESET --> KEYS["Release Held Keys"]
    RESET --> BUTTONS["Release Mouse Buttons"]
    RESET --> MODIFIERS["Clear Modifiers"]
    RESET --> DRAG["Cancel Active Drag"]
    RESET --> COMPOSITION["Terminate Composition"]
```

This should be treated as a mandatory invariant rather than an optional
recovery mechanism.

------------------------------------------------------------------------

# 15. Revised Capability Negotiation

``` mermaid
sequenceDiagram
    participant P as Phone
    participant D as Desktop

    P->>D: ClientHello
    D->>P: ServerHello

    P->>D: Supported capabilities
    D->>P: Supported capabilities

    P->>D: Select compatible features
    D->>P: Confirm session capabilities

    Note over P,D: Session starts with negotiated capabilities
```

Example:

``` text
Phone capabilities:
- mouse
- keyboard
- text
- composition
- voice
- clipboard

Desktop capabilities:
- mouse
- keyboard
- text
- composition

Negotiated:
- mouse
- keyboard
- text
- composition
```

The UI can then hide unsupported features automatically.

------------------------------------------------------------------------

# 16. Revised Transport Architecture

``` mermaid
flowchart TB
    EVENTS["Semantic Input Events"]

    EVENTS --> INTERFACE["InputTransport Interface"]

    INTERFACE --> QUIC["QUIC Transport"]
    INTERFACE --> HID["Bluetooth HID Transport"]

    QUIC --> AGENT["Desktop Agent"]
    HID --> COMPUTER["Computer HID Stack"]
```

The logical event model remains independent of the transport.

------------------------------------------------------------------------

# 17. Revised Security Architecture

``` mermaid
flowchart LR
    PHONE["Phone Identity<br/>Private/Public Key"]
    QR["QR Pairing"]
    DESKTOP["Desktop Identity<br/>Private/Public Key"]

    PHONE --> QR
    QR --> DESKTOP

    PHONE --> AUTH["Mutual Authentication"]
    DESKTOP --> AUTH

    AUTH --> SESSION["Authenticated Session"]
    SESSION --> ENCRYPTED["Encrypted Input Channel"]
```

The system should not rely on:

``` text
IP address + password
```

as its identity mechanism.

Trust should be based on device identity and explicit pairing.

------------------------------------------------------------------------

# 18. Revised Repository Structure

``` text
phone-input-relay/
│
├── core/
│   ├── protocol/
│   │   ├── events.rs
│   │   ├── codec.rs
│   │   └── version.rs
│   │
│   ├── transport/
│   │   ├── transport.rs
│   │   └── quic.rs
│   │
│   ├── session/
│   │   ├── state.rs
│   │   ├── heartbeat.rs
│   │   └── capabilities.rs
│   │
│   ├── pairing/
│   │   ├── identity.rs
│   │   ├── pairing.rs
│   │   └── trust.rs
│   │
│   ├── input/
│   │   ├── event.rs
│   │   ├── state.rs
│   │   └── coalescer.rs
│   │
│   └── diagnostics/
│       └── metrics.rs
│
├── phone/
│   ├── android/
│   │   ├── ui/
│   │   ├── touchpad/
│   │   ├── keyboard/
│   │   ├── voice/
│   │   └── bridge/
│   │
│   └── ios/
│       ├── ui/
│       ├── touchpad/
│       ├── keyboard/
│       ├── voice/
│       └── bridge/
│
├── desktop/
│   ├── windows/
│   │   ├── agent/
│   │   └── input_adapter/
│   │
│   ├── macos/
│   │   ├── agent/
│   │   └── input_adapter/
│   │
│   └── linux/
│       ├── agent/
│       └── input_adapter/
│
├── tests/
│   ├── protocol/
│   ├── state/
│   ├── transport/
│   ├── latency/
│   └── integration/
│
├── docs/
│   ├── architecture/
│   ├── security/
│   └── protocol/
│
└── README.md
```

------------------------------------------------------------------------

# 19. Revised MVP Plan

The original HLD correctly recommends starting with Windows + Android.

The revised implementation should narrow the first milestone further.

## MVP 0 --- Core proof

``` text
Android
   ↓
QUIC
   ↓
Rust Windows Agent
   ↓
SendInput
```

Support only:

``` text
MouseMove
MouseButton
KeyDown
KeyUp
Text
```

No Bluetooth.

No iOS.

No macOS.

No Linux.

No clipboard.

No advanced voice pipeline.

The objective is to prove the core event path.

------------------------------------------------------------------------

## MVP 1 --- Reliable product foundation

Add:

``` text
mDNS discovery
QR pairing
Public-key identity
Authentication
Capability negotiation
Heartbeat
Reconnect
Input-state reset
Latency instrumentation
```

Then the system becomes:

``` text
Android
   ↓
Discovery
   ↓
Pairing
   ↓
Authentication
   ↓
QUIC
   ↓
Windows Agent
   ↓
SendInput
```

------------------------------------------------------------------------

## MVP 2 --- Voice

Add:

``` text
Android
   ↓
Microphone
   ↓
Speech Recognition
   ↓
Text / Composition
   ↓
QUIC
   ↓
Windows Agent
   ↓
Native text input
```

This is the point at which the "speak on phone → text appears on
computer" workflow becomes a first-class capability.

------------------------------------------------------------------------

## Phase 3 --- macOS + iOS

Reuse:

``` text
Protocol
Session
Pairing
Authentication
Transport
Input model
Diagnostics
```

Replace only platform-specific layers:

``` text
Android UI → SwiftUI
Windows injection → macOS CGEvent
```

------------------------------------------------------------------------

## Phase 4 --- Bluetooth HID

Add:

``` text
Android
   ↓
Bluetooth HID
   ↓
Computer
```

while keeping it behind the same transport abstraction.

------------------------------------------------------------------------

## Phase 5 --- Linux

Start with:

``` text
X11
```

and then validate:

``` text
Wayland
```

as a separate platform integration.

------------------------------------------------------------------------

# 20. Testing Strategy

The revised architecture should explicitly test four layers.

## Protocol tests

``` text
Encoding
Decoding
Version mismatch
Malformed events
Sequence handling
Duplicate events
```

## State tests

``` text
KeyDown → KeyUp
Disconnect during KeyDown
Disconnect during drag
Modifier held during timeout
Reconnect
Duplicate event
Out-of-order event
Composition interrupted
```

## Platform tests

``` text
Windows SendInput
macOS CGEvent
Linux X11
Linux Wayland
```

## End-to-end tests

``` text
Phone
  ↓
Network
  ↓
Transport
  ↓
Desktop Agent
  ↓
Platform Adapter
  ↓
Operating System
```

------------------------------------------------------------------------

# 21. Observability Proposal

The diagnostics screen should expose at least:

``` text
Connection
────────────────────
Status       Connected
Transport   QUIC
RTT         7.8 ms
Jitter      1.4 ms
Packet loss 0.0%

Input
────────────────────
Touch input     143 Hz
Sent events      72 Hz
Coalesced        48%
Dropped           0

Desktop
────────────────────
Decode           0.3 ms
Injection        0.7 ms

State
────────────────────
Held keys          0
Held buttons       0
Modifiers          0
Composition    inactive
```

The goal is to distinguish:

``` text
Phone processing
Network latency
Desktop processing
OS injection latency
```

rather than treating all latency as "network latency."

------------------------------------------------------------------------

# 22. Final Proposed Architecture

``` mermaid
flowchart TB
    subgraph PHONE["PHONE"]
        TOUCH["Touchpad"]
        KEY["Keyboard"]
        VOICE["Voice"]

        TOUCH --> INPUT["Semantic Input Engine"]
        KEY --> INPUT
        VOICE --> INPUT
    end

    INPUT --> CORE["Shared Rust Core"]

    subgraph CONTROL["CONTROL PLANE"]
        DISC["Discovery"]
        PAIR["Pairing"]
        AUTH["Authentication"]
        CAPS["Capability Negotiation"]
        DISC --> PAIR --> AUTH --> CAPS
    end

    CORE <--> CAPS

    subgraph TRANSPORT["DATA PLANE"]
        QUIC["QUIC"]
        HID["Bluetooth HID"]
    end

    CORE --> QUIC
    CORE --> HID

    QUIC --> AGENT["Desktop Agent"]

    subgraph AGENT_INTERNAL["DESKTOP AGENT"]
        SESSION["Session Manager"]
        DECODE["Decoder + Validator"]
        STATE["Input State Manager"]
        DISPATCH["Semantic Dispatcher"]

        SESSION --> DECODE --> STATE --> DISPATCH
    end

    AGENT --> SESSION

    DISPATCH --> ADAPTER["Platform Adapter"]

    ADAPTER --> WIN["Windows"]
    ADAPTER --> MAC["macOS"]
    ADAPTER --> LINUX["Linux"]
```

------------------------------------------------------------------------

# 23. Architectural Principles of the Revised Proposal

The revised system should follow these principles:

### 1. Semantic input over platform-specific input

The phone produces:

``` text
MouseMove
KeyDown
Text
Composition
```

rather than Windows/macOS-specific events.

### 2. Control plane separated from data plane

Discovery and pairing should not be mixed with high-frequency input
traffic.

### 3. Transport independence

QUIC and Bluetooth HID are implementations of transport, not definitions
of the protocol.

### 4. Text is a first-class semantic operation

Keyboard text and speech text should converge on the same text-input
system.

### 5. OS-specific logic stays at the edge

Windows, macOS, and Linux differences belong in platform adapters.

### 6. Input state is explicitly managed

Disconnects must never leave stuck keys, modifiers, buttons, drags, or
compositions.

### 7. Capability negotiation is part of session establishment

The phone and desktop should agree on supported features before the
session begins.

### 8. High-frequency events are optimized locally

Touch events should be classified, accelerated, coalesced, and scheduled
before transmission.

### 9. Security is based on device identity

Pairing should establish a trusted relationship between phone and
desktop.

### 10. The architecture should be transport- and platform-extensible

Adding iOS, macOS, Linux, Bluetooth, or another transport should not
require rewriting the semantic event model.

------------------------------------------------------------------------

# 24. Final Assessment

The original proposal has a sound foundation, particularly the decision
to:

-   use a shared Rust core,
-   separate semantic text from physical key events,
-   use native OS input APIs,
-   treat Bluetooth HID as an additional mode,
-   use QR/public-key pairing,
-   explicitly clear input state on disconnect,
-   start with Android + Windows.

The revised proposal does not replace those decisions.

Instead, it makes their boundaries explicit.

The resulting architecture can be summarized as:

``` text
                  PHONE
                    │
       ┌────────────┼────────────┐
       │            │            │
    Touchpad     Keyboard       Voice
       │            │            │
       └────────────┼────────────┘
                    ▼
             Semantic Events
                    │
                    ▼
             Shared Rust Core
                    │
          ┌─────────┴─────────┐
          │                   │
        QUIC             Bluetooth HID
          │
          ▼
     Desktop Agent
          │
     Input State
     + Dispatcher
          │
          ▼
   Platform Adapter
          │
    ┌─────┼─────┐
    ▼     ▼     ▼
 Windows macOS Linux
```

**The main architectural change is therefore not replacing the original
stack. It is introducing cleaner boundaries between input generation,
semantic events, transport, session management, security, and
OS-specific input injection.**
