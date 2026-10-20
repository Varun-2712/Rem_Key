use bincode::Options;
use crate::events::{InputEvent, ProtocolVersion, CapabilitySet, EventMetadata, SessionEventType, DisconnectReason};
use thiserror::Error;
use uuid::Uuid;

pub const MAGIC_BYTES: &[u8; 4] = b"PIR1";
pub const MAX_FRAME_SIZE: usize = 64 * 1024;

/// Bincode settings shared by encode and decode. The size limit makes the decoder reject a
/// length field that claims more data than a frame can hold, instead of allocating it.
fn wire_format() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(MAX_FRAME_SIZE as u64)
}

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
    let payload = wire_format().serialize(event)?;

    let frame = Frame {
        magic: *MAGIC_BYTES,
        version: ProtocolVersion::CURRENT,
        flags,
        payload,
    };
    
    let data = wire_format().serialize(&frame)?;
    if data.len() > MAX_FRAME_SIZE {
        return Err(CodecError::FrameTooLarge { size: data.len(), max: MAX_FRAME_SIZE });
    }
    Ok(data)
}

pub fn decode_event(data: &[u8]) -> Result<InputEvent, CodecError> {
    if data.len() > MAX_FRAME_SIZE {
        return Err(CodecError::FrameTooLarge { size: data.len(), max: MAX_FRAME_SIZE });
    }
    let frame: Frame = wire_format().deserialize(data)?;
    
    if frame.magic != *MAGIC_BYTES {
        return Err(CodecError::InvalidMagic(frame.magic));
    }
    
    if !crate::version::is_compatible(frame.version, ProtocolVersion::CURRENT) {
        return Err(CodecError::VersionMismatch {
            expected: ProtocolVersion::CURRENT,
            actual: frame.version,
        });
    }
    
    let event: InputEvent = wire_format().deserialize(&frame.payload)?;
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{KeyCode, ModifierState};

    fn key_down() -> InputEvent {
        InputEvent::KeyDown {
            metadata: EventMetadata::new(Uuid::new_v4(), 7),
            key: KeyCode::KeyA,
            modifiers: ModifierState { shift: true, ..Default::default() },
        }
    }

    fn encode_frame(frame: &Frame) -> Vec<u8> {
        wire_format().serialize(frame).unwrap()
    }

    #[test]
    fn round_trips_event() {
        let event = key_down();
        let data = encode_event(&event, flags_for_event(&event)).unwrap();
        assert_eq!(decode_event(&data).unwrap(), event);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut frame: Frame = wire_format().deserialize(&encode_event(&key_down(), FrameFlags::RELIABLE).unwrap()).unwrap();
        frame.magic = *b"NOPE";
        assert!(matches!(decode_event(&encode_frame(&frame)), Err(CodecError::InvalidMagic(_))));
    }

    #[test]
    fn accepts_newer_minor_version_rejects_other_major() {
        let mut frame: Frame = wire_format().deserialize(&encode_event(&key_down(), FrameFlags::RELIABLE).unwrap()).unwrap();
        frame.version.minor += 1;
        assert!(decode_event(&encode_frame(&frame)).is_ok());

        frame.version.major += 1;
        assert!(matches!(decode_event(&encode_frame(&frame)), Err(CodecError::VersionMismatch { .. })));
    }

    #[test]
    fn rejects_huge_length_field_without_allocating() {
        // magic + version (3 x u16) + flags (u8), then a payload length claiming ~16 EiB.
        let mut data = MAGIC_BYTES.to_vec();
        data.extend_from_slice(&[1, 0, 0, 0, 0, 0]);
        data.push(0);
        data.extend_from_slice(&u64::MAX.to_le_bytes());
        assert!(matches!(decode_event(&data), Err(CodecError::Bincode(_))));
    }

    #[test]
    fn rejects_oversized_input() {
        assert!(matches!(decode_event(&vec![0u8; MAX_FRAME_SIZE + 1]), Err(CodecError::FrameTooLarge { .. })));
    }

    #[test]
    fn rejects_oversized_event() {
        let event = InputEvent::Text { metadata: EventMetadata::new(Uuid::nil(), 0), text: "x".repeat(MAX_FRAME_SIZE) };
        assert!(encode_event(&event, FrameFlags::RELIABLE).is_err());
    }
}
