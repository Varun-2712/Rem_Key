use bincode;
use crate::events::{InputEvent, ProtocolVersion, CapabilitySet, EventMetadata, SessionEventType, DisconnectReason};
use thiserror::Error;
use uuid::Uuid;

pub const MAGIC_BYTES: &[u8; 4] = b"PIR1";
pub const MAX_FRAME_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Frame {
    pub magic: [u8; 4],
    pub version: ProtocolVersion,
    pub flags: FrameFlags,
    pub payload: Vec<u8>,
}

bitflags::bitflags! {
    #[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
    pub struct FrameFlags: u8 {
        const RELIABLE = 0b0000_0001;
        const COMPRESSED = 0b0000_0010;
        const CONTROL = 0b0000_0100;
        const PRIORITY = 0b0000_1000;
    }
}

#[derive(Debug, Error)]
pub enum CodecError {
    #[error("Invalid magic bytes: expected PIR1, got {0:02x?}")]
    InvalidMagic([u8; 4]),
    #[error("Protocol version mismatch: expected {expected}, got {actual}")]
    VersionMismatch { expected: ProtocolVersion, actual: ProtocolVersion },
    #[error("Frame too large: {size} bytes (max {max})")]
    FrameTooLarge { size: usize, max: usize },
    #[error("Bincode error: {0}")]
    Bincode(#[from] bincode::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub fn encode_event(event: &InputEvent, flags: FrameFlags) -> Result<Vec<u8>, CodecError> {
    let payload = bincode::serialize(event)?;
    
    if payload.len() > MAX_FRAME_SIZE {
        return Err(CodecError::FrameTooLarge { size: payload.len(), max: MAX_FRAME_SIZE });
    }

    let frame = Frame {
        magic: *MAGIC_BYTES,
        version: ProtocolVersion::CURRENT,
        flags,
        payload,
    };
    
    Ok(bincode::serialize(&frame)?)
}

pub fn decode_event(data: &[u8]) -> Result<InputEvent, CodecError> {
    let frame: Frame = bincode::deserialize(data)?;
    
    if frame.magic != *MAGIC_BYTES {
        return Err(CodecError::InvalidMagic(frame.magic));
    }
    
    if frame.version != ProtocolVersion::CURRENT {
        return Err(CodecError::VersionMismatch {
            expected: ProtocolVersion::CURRENT,
            actual: frame.version,
        });
    }
    
    let event: InputEvent = bincode::deserialize(&frame.payload)?;
    Ok(event)
}

pub fn encode_hello(capabilities: CapabilitySet) -> Result<Vec<u8>, CodecError> {
    let event = InputEvent::SessionEvent {
        metadata: EventMetadata::new(Uuid::nil(), 0),
        event: SessionEventType::Hello {
            version: ProtocolVersion::CURRENT,
            capabilities,
        },
    };
    encode_event(&event, FrameFlags::RELIABLE | FrameFlags::CONTROL)
}

pub fn encode_hello_ack(capabilities: CapabilitySet, session_id: Uuid) -> Result<Vec<u8>, CodecError> {
    let event = InputEvent::SessionEvent {
        metadata: EventMetadata::new(session_id, 0),
        event: SessionEventType::HelloAck {
            version: ProtocolVersion::CURRENT,
            capabilities,
        },
    };
    encode_event(&event, FrameFlags::RELIABLE | FrameFlags::CONTROL)
}

pub fn encode_heartbeat(session_id: Uuid, seq: u64) -> Result<Vec<u8>, CodecError> {
    let event = InputEvent::SessionEvent {
        metadata: EventMetadata::new(session_id, seq),
        event: SessionEventType::Heartbeat,
    };
    encode_event(&event, FrameFlags::CONTROL)
}

pub fn encode_disconnect(session_id: Uuid, seq: u64, reason: DisconnectReason) -> Result<Vec<u8>, CodecError> {
    let event = InputEvent::SessionEvent {
        metadata: EventMetadata::new(session_id, seq),
        event: SessionEventType::Disconnect { reason },
    };
    encode_event(&event, FrameFlags::RELIABLE | FrameFlags::CONTROL)
}

pub fn flags_for_event(event: &InputEvent) -> FrameFlags {
    use crate::events::InputEvent::*;
    match event {
        MouseMove { .. } | Scroll { .. } => FrameFlags::empty(),
        MouseButton { .. } => FrameFlags::RELIABLE | FrameFlags::PRIORITY,
        KeyDown { .. } | KeyUp { .. } | ModifierState { .. } => FrameFlags::RELIABLE | FrameFlags::PRIORITY,
        Text { .. } | CompositionStart { .. } | CompositionUpdate { .. } | CompositionCommit { .. } => FrameFlags::RELIABLE,
        ClipboardRequest { .. } | ClipboardResponse { .. } => FrameFlags::RELIABLE,
        ConsumerKey { .. } => FrameFlags::RELIABLE,
        SessionEvent { .. } => FrameFlags::RELIABLE | FrameFlags::CONTROL,
        DiagnosticEvent { .. } => FrameFlags::empty(),
    }
}