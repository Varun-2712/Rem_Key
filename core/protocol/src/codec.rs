use bincode::{Decode, Encode};
use crate::events::{InputEvent, ProtocolVersion, CapabilitySet};
use thiserror::Error;

pub const MAGIC_BYTES: &[u8; 4] = b"PIR1";
pub const MAX_FRAME_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone, Encode, Decode)]
pub struct Frame {
    pub magic: [u8; 4],
    pub version: ProtocolVersion,
    pub flags: FrameFlags,
    pub payload: Vec<u8>,
}

bitflags::bitflags! {
    #[derive(Encode, Decode, Clone, Copy, Debug, PartialEq, Eq)]
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
    Bincode(#[from] bincode::error::EncodeError),
    #[error("Bincode decode error: {0}")]
    BincodeDecode(#[from] bincode::error::DecodeError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub fn encode_event(event: &InputEvent, flags: FrameFlags) -> Result<Vec<u8>, CodecError> {
    let payload = bincode::encode_to_vec(event, bincode::config::standard())?;
    
    if payload.len() > MAX_FRAME_SIZE {
        return Err(CodecError::FrameTooLarge { size: payload.len(), max: MAX_FRAME_SIZE });
    }

    let frame = Frame {
        magic: *MAGIC_BYTES,
        version: ProtocolVersion::CURRENT,
        flags,
        payload,
    };
    
    Ok(bincode::encode_to_vec(&frame, bincode::config::standard())?)
}

pub fn decode_event(data: &[u8]) -> Result<InputEvent, CodecError> {
    let frame: Frame = bincode::decode_from_slice(data, bincode::config::standard())?.0;
    
    if frame.magic != *MAGIC_BYTES {
        return Err(CodecError::InvalidMagic(frame.magic));
    }
    
    if frame.version != ProtocolVersion::CURRENT {
        return Err(CodecError::VersionMismatch {
            expected: ProtocolVersion::CURRENT,
            actual: frame.version,
        });
    }
    
    let event: InputEvent = bincode::decode_from_slice(&frame.payload, bincode::config::standard())?.0;
    Ok(event)
}

pub fn encode_hello(capabilities: CapabilitySet) -> Result<Vec<u8>, CodecError> {
    let event = InputEvent::SessionEvent {
        metadata: crate::events::EventMetadata::new(Uuid::nil(), 0),
        event: crate::events::SessionEventType::Hello {
            version: ProtocolVersion::CURRENT,
            capabilities,
        },
    };
    encode_event(&event, FrameFlags::RELIABLE | FrameFlags::CONTROL)
}

pub fn encode_hello_ack(capabilities: CapabilitySet, session_id: Uuid) -> Result<Vec<u8>, CodecError> {
    let event = InputEvent::SessionEvent {
        metadata: crate::events::EventMetadata::new(session_id, 0),
        event: crate::events::SessionEventType::HelloAck {
            version: ProtocolVersion::CURRENT,
            capabilities,
        },
    };
    encode_event(&event, FrameFlags::RELIABLE | FrameFlags::CONTROL)
}

pub fn encode_heartbeat(session_id: Uuid, seq: u64) -> Result<Vec<u8>, CodecError> {
    let event = InputEvent::SessionEvent {
        metadata: crate::events::EventMetadata::new(session_id, seq),
        event: crate::events::SessionEventType::Heartbeat,
    };
    encode_event(&event, FrameFlags::CONTROL)
}

pub fn encode_disconnect(session_id: Uuid, seq: u64, reason: crate::events::DisconnectReason) -> Result<Vec<u8>, CodecError> {
    let event = InputEvent::SessionEvent {
        metadata: crate::events::EventMetadata::new(session_id, seq),
        event: crate::events::SessionEventType::Disconnect { reason },
    };
    encode_event(&event, FrameFlags::RELIABLE | FrameFlags::CONTROL)
}

pub fn flags_for_event(event: &InputEvent) -> FrameFlags {
    use crate::events::InputEvent::*;
    match event {
        MouseMove { .. } | Scroll { .. } => FrameFlags::empty(),
        KeyDown { .. } | KeyUp { .. } | ModifierState { .. } => FrameFlags::RELIABLE | FrameFlags::PRIORITY,
        Text { .. } | CompositionStart { .. } | CompositionUpdate { .. } | CompositionCommit { .. } => FrameFlags::RELIABLE,
        ClipboardRequest { .. } | ClipboardResponse { .. } => FrameFlags::RELIABLE,
        ConsumerKey { .. } => FrameFlags::RELIABLE,
        SessionEvent { .. } => FrameFlags::RELIABLE | FrameFlags::CONTROL,
        DiagnosticEvent { .. } => FrameFlags::empty(),
    }
}