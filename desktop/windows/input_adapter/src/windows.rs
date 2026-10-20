use anyhow::{bail, Result};
use pir_protocol::events::{ButtonState, KeyCode, MouseButton};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::{XBUTTON1, XBUTTON2};
use crate::InputAdapter;

/// Notch size Windows uses for one wheel click.
const WHEEL_DELTA: i32 = 120;

pub struct WindowsInputAdapter;

impl WindowsInputAdapter {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}

impl InputAdapter for WindowsInputAdapter {
    fn inject_mouse_move(&mut self, delta_x: i32, delta_y: i32) -> Result<()> {
        // Relative move; Windows applies the user's pointer speed settings on top.
        send(&[mouse_input(delta_x, delta_y, 0, MOUSEEVENTF_MOVE)], "mouse move")
    }

    fn inject_mouse_button(&mut self, button: MouseButton, state: ButtonState) -> Result<()> {
        let pressed = state == ButtonState::Pressed;
        let (flags, data) = match button {
            MouseButton::Left => (if pressed { MOUSEEVENTF_LEFTDOWN } else { MOUSEEVENTF_LEFTUP }, 0),
            MouseButton::Right => (if pressed { MOUSEEVENTF_RIGHTDOWN } else { MOUSEEVENTF_RIGHTUP }, 0),
            MouseButton::Middle => (if pressed { MOUSEEVENTF_MIDDLEDOWN } else { MOUSEEVENTF_MIDDLEUP }, 0),
            MouseButton::Back => (if pressed { MOUSEEVENTF_XDOWN } else { MOUSEEVENTF_XUP }, XBUTTON1 as i32),
            MouseButton::Forward => (if pressed { MOUSEEVENTF_XDOWN } else { MOUSEEVENTF_XUP }, XBUTTON2 as i32),
        };
        send(&[mouse_input(0, 0, data, flags)], "mouse button")
    }

    fn inject_scroll(&mut self, delta_x: i32, delta_y: i32) -> Result<()> {
        let mut inputs = Vec::with_capacity(2);
        if delta_y != 0 {
            inputs.push(mouse_input(0, 0, delta_y.saturating_mul(WHEEL_DELTA), MOUSEEVENTF_WHEEL));
        }
        if delta_x != 0 {
            inputs.push(mouse_input(0, 0, delta_x.saturating_mul(WHEEL_DELTA), MOUSEEVENTF_HWHEEL));
        }
        send(&inputs, "scroll")
    }

    fn inject_key(&mut self, key: KeyCode, state: ButtonState) -> Result<()> {
        let Some((vk, extended)) = virtual_key(key) else {
            bail!("No Windows virtual key for {:?}", key);
        };
        let scan = unsafe { MapVirtualKeyW(vk.0 as u32, MAPVK_VK_TO_VSC) } as u16;

        let mut flags = KEYBD_EVENT_FLAGS(0);
        if state == ButtonState::Released {
            flags |= KEYEVENTF_KEYUP;
        }
        // Scan codes are what games and remote-desktop clients read. Keys without one
        // (media/volume) are sent by virtual key only.
        if scan != 0 {
            flags |= KEYEVENTF_SCANCODE;
        }
        // Without this, arrows/Home/Insert/... arrive as their numpad twins.
        if extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        send(&[keyboard_input(vk, scan, flags)], "key")
    }

    fn inject_text(&mut self, text: &str) -> Result<()> {
        let mut inputs = Vec::with_capacity(text.len() * 2);
        for ch in text.chars() {
            match ch {
                // Many apps ignore a Unicode newline; press Enter instead.
                '\r' => {}
                '\n' => {
                    inputs.push(keyboard_input(VK_RETURN, 0, KEYBD_EVENT_FLAGS(0)));
                    inputs.push(keyboard_input(VK_RETURN, 0, KEYEVENTF_KEYUP));
                }
                // One input per UTF-16 unit, so characters outside the BMP (emoji) are
                // sent as a surrogate pair.
                _ => {
                    let mut units = [0u16; 2];
                    for &unit in ch.encode_utf16(&mut units).iter() {
                        inputs.push(keyboard_input(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE));
                        inputs.push(keyboard_input(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                    }
                }
            }
        }
        send(&inputs, "text")
    }
}

fn mouse_input(dx: i32, dy: i32, data: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            // mouseData is a DWORD, but holds a signed wheel delta.
            mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

fn keyboard_input(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

fn send(inputs: &[INPUT], what: &str) -> Result<()> {
    if inputs.is_empty() {
        return Ok(());
    }
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        // Typically UIPI: the foreground window belongs to a more privileged process.
        bail!("SendInput injected {}/{} events for {}: {}", sent, inputs.len(), what, std::io::Error::last_os_error());
    }
    Ok(())
}

/// Maps a key to its virtual key and whether it is an "extended" key on a PC keyboard.
fn virtual_key(key: KeyCode) -> Option<(VIRTUAL_KEY, bool)> {
    use KeyCode::*;
    let normal = |vk: VIRTUAL_KEY| Some((vk, false));
    let extended = |vk: VIRTUAL_KEY| Some((vk, true));
    let ascii = |c: u8| Some((VIRTUAL_KEY(c as u16), false));

    match key {
        KeyA => ascii(b'A'), KeyB => ascii(b'B'), KeyC => ascii(b'C'), KeyD => ascii(b'D'),
        KeyE => ascii(b'E'), KeyF => ascii(b'F'), KeyG => ascii(b'G'), KeyH => ascii(b'H'),
        KeyI => ascii(b'I'), KeyJ => ascii(b'J'), KeyK => ascii(b'K'), KeyL => ascii(b'L'),
        KeyM => ascii(b'M'), KeyN => ascii(b'N'), KeyO => ascii(b'O'), KeyP => ascii(b'P'),
        KeyQ => ascii(b'Q'), KeyR => ascii(b'R'), KeyS => ascii(b'S'), KeyT => ascii(b'T'),
        KeyU => ascii(b'U'), KeyV => ascii(b'V'), KeyW => ascii(b'W'), KeyX => ascii(b'X'),
        KeyY => ascii(b'Y'), KeyZ => ascii(b'Z'),
        Digit0 => ascii(b'0'), Digit1 => ascii(b'1'), Digit2 => ascii(b'2'), Digit3 => ascii(b'3'),
        Digit4 => ascii(b'4'), Digit5 => ascii(b'5'), Digit6 => ascii(b'6'), Digit7 => ascii(b'7'),
        Digit8 => ascii(b'8'), Digit9 => ascii(b'9'),

        F1 => normal(VK_F1), F2 => normal(VK_F2), F3 => normal(VK_F3), F4 => normal(VK_F4),
        F5 => normal(VK_F5), F6 => normal(VK_F6), F7 => normal(VK_F7), F8 => normal(VK_F8),
        F9 => normal(VK_F9), F10 => normal(VK_F10), F11 => normal(VK_F11), F12 => normal(VK_F12),

        ControlLeft => normal(VK_LCONTROL), ControlRight => extended(VK_RCONTROL),
        ShiftLeft => normal(VK_LSHIFT), ShiftRight => normal(VK_RSHIFT),
        AltLeft => normal(VK_LMENU), AltRight => extended(VK_RMENU),
        MetaLeft => extended(VK_LWIN), MetaRight => extended(VK_RWIN),

        ArrowUp => extended(VK_UP), ArrowDown => extended(VK_DOWN),
        ArrowLeft => extended(VK_LEFT), ArrowRight => extended(VK_RIGHT),
        Home => extended(VK_HOME), End => extended(VK_END),
        PageUp => extended(VK_PRIOR), PageDown => extended(VK_NEXT),
        Insert => extended(VK_INSERT), Delete => extended(VK_DELETE),

        Escape => normal(VK_ESCAPE), Tab => normal(VK_TAB), Space => normal(VK_SPACE),
        Enter => normal(VK_RETURN), Backspace => normal(VK_BACK),
        CapsLock => normal(VK_CAPITAL), NumLock => extended(VK_NUMLOCK), ScrollLock => normal(VK_SCROLL),
        PrintScreen => extended(VK_SNAPSHOT), Pause => normal(VK_PAUSE),

        Numpad0 => normal(VK_NUMPAD0), Numpad1 => normal(VK_NUMPAD1), Numpad2 => normal(VK_NUMPAD2),
        Numpad3 => normal(VK_NUMPAD3), Numpad4 => normal(VK_NUMPAD4), Numpad5 => normal(VK_NUMPAD5),
        Numpad6 => normal(VK_NUMPAD6), Numpad7 => normal(VK_NUMPAD7), Numpad8 => normal(VK_NUMPAD8),
        Numpad9 => normal(VK_NUMPAD9),
        NumpadAdd => normal(VK_ADD), NumpadSubtract => normal(VK_SUBTRACT),
        NumpadMultiply => normal(VK_MULTIPLY), NumpadDivide => extended(VK_DIVIDE),
        NumpadEnter => extended(VK_RETURN), NumpadDecimal => normal(VK_DECIMAL),
        NumpadComma => normal(VK_SEPARATOR), NumpadEqual => normal(VK_OEM_NEC_EQUAL),

        Semicolon => normal(VK_OEM_1), Equal => normal(VK_OEM_PLUS), Comma => normal(VK_OEM_COMMA),
        Minus => normal(VK_OEM_MINUS), Period => normal(VK_OEM_PERIOD), Slash => normal(VK_OEM_2),
        Backquote => normal(VK_OEM_3), BracketLeft => normal(VK_OEM_4), BracketRight => normal(VK_OEM_6),
        Backslash => normal(VK_OEM_5), Quote => normal(VK_OEM_7),

        MediaPlayPause => extended(VK_MEDIA_PLAY_PAUSE), MediaStop => extended(VK_MEDIA_STOP),
        MediaTrackNext => extended(VK_MEDIA_NEXT_TRACK), MediaTrackPrevious => extended(VK_MEDIA_PREV_TRACK),
        VolumeUp => extended(VK_VOLUME_UP), VolumeDown => extended(VK_VOLUME_DOWN),
        VolumeMute => extended(VK_VOLUME_MUTE),

        // Raw virtual-key passthrough; anything wider than a VK is not a valid key.
        Unknown(code) => u8::try_from(code).ok().map(|c| (VIRTUAL_KEY(c as u16), false)),
    }
}
