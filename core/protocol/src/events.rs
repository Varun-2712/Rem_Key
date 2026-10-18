use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use bitflags::bitflags;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct EventMetadata {
    pub event_id: Uuid,
    pub sequence_number: u64,
    pub timestamp: DateTime<Utc>,
    pub session_id: Uuid,
}

impl EventMetadata {
    pub fn new(session_id: Uuid, sequence_number: u64) -> Self {
        Self {
            event_id: Uuid::new_v4(),
            sequence_number,
            timestamp: Utc::now(),
            session_id,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ButtonState {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum KeyCode {
    // Letters
    KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ,
    KeyK, KeyL, KeyM, KeyN, KeyO, KeyP, KeyQ, KeyR, KeyS, KeyT,
    KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,

    // Numbers
    Digit0, Digit1, Digit2, Digit3, Digit4,
    Digit5, Digit6, Digit7, Digit8, Digit9,

    // Function keys
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,

    // Modifiers
    ControlLeft, ControlRight,
    ShiftLeft, ShiftRight,
    AltLeft, AltRight,
    MetaLeft, MetaRight,

    // Navigation
    ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
    Home, End, PageUp, PageDown,
    Insert, Delete,

    // Special
    Escape, Tab, Space, Enter, Backspace,
    CapsLock, NumLock, ScrollLock,
    PrintScreen, Pause,

    // Numpad
    Numpad0, Numpad1, Numpad2, Numpad3, Numpad4,
    Numpad5, Numpad6, Numpad7, Numpad8, Numpad9,
    NumpadAdd, NumpadSubtract, NumpadMultiply, NumpadDivide,
    NumpadEnter, NumpadDecimal, NumpadComma, NumpadEqual,

    // Punctuation
    Semicolon, Equal, Comma, Minus, Period, Slash,
    Backquote, BracketLeft, BracketRight, Backslash, Quote,

    // Media/Consumer
    MediaPlayPause, MediaStop, MediaTrackNext, MediaTrackPrevious,
    VolumeUp, VolumeDown, VolumeMute,

    // Unknown fallback
    Unknown(u32),
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModifierState {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

impl ModifierState {
    pub fn from_key_code(key: KeyCode) -> Self {
        match key {
            KeyCode::ControlLeft | KeyCode::ControlRight => Self { control: true, ..Default::default() },
            KeyCode::ShiftLeft | KeyCode::ShiftRight => Self { shift: true, ..Default::default() },
            KeyCode::AltLeft | KeyCode::AltRight => Self { alt: true, ..Default::default() },
            KeyCode::MetaLeft | KeyCode::MetaRight => Self { meta: true, ..Default::default() },
            _ => Default::default(),
        }
    }
}

bitflags! {
    #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct CapabilitySet: u32 {
        const MOUSE = 0x0001;
        const KEYBOARD = 0x0002;
        const TEXT = 0x0004;
        const COMPOSITION = 0x0008;
        const CLIPBOARD = 0x0010;
        const VOICE = 0x0020;
        const CONSUMER_KEYS = 0x0040;
        const DIAGNOSTICS = 0x0080;
        const BLUETOOTH_HID = 0x0100;
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ClipboardFormat {
    Text,
    Image,
    Html,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ConsumerKeyCode {
    PlayPause,
    Stop,
    NextTrack,
    PreviousTrack,
    VolumeUp,
    VolumeDown,
    Mute,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SessionEventType {
    Hello { version: ProtocolVersion, capabilities: CapabilitySet },
    HelloAck { version: ProtocolVersion, capabilities: CapabilitySet },
    Heartbeat,
    HeartbeatAck,
    Disconnect { reason: DisconnectReason },
    Reconnect { session_id: Uuid },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum DisconnectReason {
    Normal,
    Timeout,
    AuthFailed,
    ProtocolMismatch,
    TransportError,
    PeerReset,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DiagnosticPayload {
    Latency { rtt_ms: f64, jitter_ms: f64 },
    Throughput { bytes_sent: u64, bytes_recv: u64 },
    InputStats { touch_hz: f64, sent_hz: f64, coalesced_pct: f64 },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InputEvent {
    MouseMove { metadata: EventMetadata, delta_x: i32, delta_y: i32 },
    MouseButton { metadata: EventMetadata, button: MouseButton, state: ButtonState },
    Scroll { metadata: EventMetadata, delta_x: i32, delta_y: i32 },

    KeyDown { metadata: EventMetadata, key: KeyCode, modifiers: ModifierState },
    KeyUp { metadata: EventMetadata, key: KeyCode, modifiers: ModifierState },
    ModifierState { metadata: EventMetadata, modifiers: ModifierState },

    Text { metadata: EventMetadata, text: String },
    CompositionStart { metadata: EventMetadata },
    CompositionUpdate { metadata: EventMetadata, text: String },
    CompositionCommit { metadata: EventMetadata, text: String },

    ClipboardRequest { metadata: EventMetadata, format: ClipboardFormat },
    ClipboardResponse { metadata: EventMetadata, data: Vec<u8> },

    ConsumerKey { metadata: EventMetadata, key: ConsumerKeyCode, state: ButtonState },

    SessionEvent { metadata: EventMetadata, event: SessionEventType },

    DiagnosticEvent { metadata: EventMetadata, payload: DiagnosticPayload },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl ProtocolVersion {
    pub const CURRENT: Self = Self { major: 1, minor: 0, patch: 0 };
    pub const MIN_COMPATIBLE: Self = Self { major: 1, minor: 0, patch: 0 };
}

impl std::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl PartialOrd for ProtocolVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ProtocolVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch))
    }
}

impl From<ProtocolVersion> for u32 {
    fn from(v: ProtocolVersion) -> Self {
        ((v.major as u32) << 24) | ((v.minor as u32) << 16) | (v.patch as u32)
    }
}