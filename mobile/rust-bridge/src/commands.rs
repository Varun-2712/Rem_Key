use pir_protocol::events::{KeyCode, MouseButton};

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PointerButton {
    Left,
    Right,
    Middle,
}

impl From<PointerButton> for MouseButton {
    fn from(button: PointerButton) -> Self {
        match button {
            PointerButton::Left => MouseButton::Left,
            PointerButton::Right => MouseButton::Right,
            PointerButton::Middle => MouseButton::Middle,
        }
    }
}

/// Keys the phone keyboard cannot express as committed text. Printable characters go
/// through `RelayClient::commit_text` so they don't depend on the desktop's keyboard layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SpecialKey {
    Enter,
    Backspace,
    Delete,
    Tab,
    Escape,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
}

impl From<SpecialKey> for KeyCode {
    fn from(key: SpecialKey) -> Self {
        match key {
            SpecialKey::Enter => KeyCode::Enter,
            SpecialKey::Backspace => KeyCode::Backspace,
            SpecialKey::Delete => KeyCode::Delete,
            SpecialKey::Tab => KeyCode::Tab,
            SpecialKey::Escape => KeyCode::Escape,
            SpecialKey::ArrowUp => KeyCode::ArrowUp,
            SpecialKey::ArrowDown => KeyCode::ArrowDown,
            SpecialKey::ArrowLeft => KeyCode::ArrowLeft,
            SpecialKey::ArrowRight => KeyCode::ArrowRight,
            SpecialKey::Home => KeyCode::Home,
            SpecialKey::End => KeyCode::End,
            SpecialKey::PageUp => KeyCode::PageUp,
            SpecialKey::PageDown => KeyCode::PageDown,
        }
    }
}

/// What the UI thread hands to the connection task.
#[derive(Debug)]
pub(crate) enum Command {
    PointerDelta { dx: f32, dy: f32 },
    Scroll { dx: f32, dy: f32 },
    Button { button: PointerButton, pressed: bool },
    Key { key: SpecialKey, pressed: bool },
    Text(String),
    Disconnect,
}
