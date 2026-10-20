use std::collections::HashSet;
use std::time::{Duration, Instant};
use anyhow::{anyhow, bail, Result};
use core_graphics::display::{CGDisplay, CGPoint, CGRect};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton, EventField, ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use pir_protocol::events::{ButtonState, KeyCode, MouseButton};
use crate::InputAdapter;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

/// `CGEventKeyboardSetUnicodeString` only takes a short string per event.
const MAX_UTF16_PER_EVENT: usize = 20;

/// macOS default double-click interval and a small movement tolerance.
const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);
const DOUBLE_CLICK_DISTANCE: f64 = 4.0;

/// Injects input with Quartz events. Requires the Accessibility permission for the
/// process (or the terminal it runs in).
pub struct MacInputAdapter {
    source: CGEventSource,
    held_buttons: HashSet<MouseButton>,
    held_modifiers: HashSet<KeyCode>,
    clicks: ClickTracker,
}

impl MacInputAdapter {
    pub fn new() -> Result<Self> {
        if !unsafe { AXIsProcessTrusted() } {
            bail!(
                "macOS has not allowed this app to control the computer. Open System Settings → \
                 Privacy & Security → Accessibility, enable the app you run pir-agent from \
                 (e.g. Terminal), then restart it. Use --dry-run to run without injecting."
            );
        }
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| anyhow!("could not create a Quartz event source"))?;
        Ok(Self { source, held_buttons: HashSet::new(), held_modifiers: HashSet::new(), clicks: ClickTracker::default() })
    }

    fn cursor(&self) -> Result<CGPoint> {
        Ok(CGEvent::new(self.source.clone()).map_err(|_| anyhow!("could not read cursor position"))?.location())
    }

    fn flags(&self) -> CGEventFlags {
        self.held_modifiers.iter().filter_map(|k| modifier_flag(*k)).fold(CGEventFlags::empty(), |a, f| a | f)
    }

    fn post(&self, event: CGEvent) {
        event.post(CGEventTapLocation::HID);
    }

    fn mouse_event(&self, kind: CGEventType, at: CGPoint, button: MouseButton) -> Result<CGEvent> {
        let event = CGEvent::new_mouse_event(self.source.clone(), kind, at, cg_button(button))
            .map_err(|_| anyhow!("could not create mouse event"))?;
        event.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, button_number(button));
        event.set_flags(self.flags());
        Ok(event)
    }

    fn key_event(&self, keycode: u16, down: bool) -> Result<CGEvent> {
        CGEvent::new_keyboard_event(self.source.clone(), keycode, down).map_err(|_| anyhow!("could not create key event"))
    }

    fn tap(&self, keycode: u16) -> Result<()> {
        for down in [true, false] {
            let event = self.key_event(keycode, down)?;
            event.set_flags(self.flags());
            self.post(event);
        }
        Ok(())
    }
}

impl InputAdapter for MacInputAdapter {
    fn inject_mouse_move(&mut self, delta_x: i32, delta_y: i32) -> Result<()> {
        let from = self.cursor()?;
        let displays: Vec<Rect> = CGDisplay::active_displays()
            .map_err(|e| anyhow!("listing displays failed: {e}"))?
            .into_iter()
            .map(|id| Rect::from(CGDisplay::new(id).bounds()))
            .collect();
        let (x, y) = clamp_to_displays((from.x + delta_x as f64, from.y + delta_y as f64), (from.x, from.y), &displays);

        // While a button is held, apps expect "dragged" events, or drag-and-drop breaks.
        let (kind, button) = match drag_button(&self.held_buttons) {
            Some(MouseButton::Left) => (CGEventType::LeftMouseDragged, MouseButton::Left),
            Some(MouseButton::Right) => (CGEventType::RightMouseDragged, MouseButton::Right),
            Some(other) => (CGEventType::OtherMouseDragged, other),
            None => (CGEventType::MouseMoved, MouseButton::Left),
        };
        let event = self.mouse_event(kind, CGPoint::new(x, y), button)?;
        // Raw deltas, for apps (games, 3D tools) that read them instead of the position.
        event.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_X, delta_x as i64);
        event.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y, delta_y as i64);
        self.post(event);
        Ok(())
    }

    fn inject_mouse_button(&mut self, button: MouseButton, state: ButtonState) -> Result<()> {
        let at = self.cursor()?;
        let pressed = state == ButtonState::Pressed;
        let kind = match (button, pressed) {
            (MouseButton::Left, true) => CGEventType::LeftMouseDown,
            (MouseButton::Left, false) => CGEventType::LeftMouseUp,
            (MouseButton::Right, true) => CGEventType::RightMouseDown,
            (MouseButton::Right, false) => CGEventType::RightMouseUp,
            (_, true) => CGEventType::OtherMouseDown,
            (_, false) => CGEventType::OtherMouseUp,
        };
        let clicks = if pressed {
            self.held_buttons.insert(button);
            self.clicks.press(button, Instant::now(), (at.x, at.y))
        } else {
            self.held_buttons.remove(&button);
            self.clicks.current(button)
        };
        let event = self.mouse_event(kind, at, button)?;
        // Without the click count, two quick clicks never become a double click.
        event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, clicks);
        self.post(event);
        Ok(())
    }

    fn inject_scroll(&mut self, delta_x: i32, delta_y: i32) -> Result<()> {
        // Quartz: positive wheel1 scrolls up, positive wheel2 scrolls left; the trait's
        // positive delta_x means right, hence the negation.
        let event = CGEvent::new_scroll_event(self.source.clone(), ScrollEventUnit::LINE, 2, delta_y, -delta_x, 0)
            .map_err(|_| anyhow!("could not create scroll event"))?;
        event.set_flags(self.flags());
        self.post(event);
        Ok(())
    }

    fn inject_key(&mut self, key: KeyCode, state: ButtonState) -> Result<()> {
        let keycode = mac_keycode(key).ok_or_else(|| media_key_error(key))?;
        let down = state == ButtonState::Pressed;
        let is_modifier = modifier_flag(key).is_some();
        if is_modifier {
            if down {
                self.held_modifiers.insert(key);
            } else {
                self.held_modifiers.remove(&key);
            }
        }

        let event = self.key_event(keycode, down)?;
        if is_modifier {
            // Modifier keys are FlagsChanged events carrying the new modifier state.
            event.set_type(CGEventType::FlagsChanged);
        }
        let mut flags = self.flags();
        if is_numpad(key) {
            flags |= CGEventFlags::CGEventFlagNumericPad;
        }
        event.set_flags(flags);
        self.post(event);
        Ok(())
    }

    fn inject_text(&mut self, text: &str) -> Result<()> {
        for piece in split_text(text) {
            match piece {
                TextPiece::Newline => self.tap(KVK_RETURN)?,
                TextPiece::Chars(units) => {
                    for down in [true, false] {
                        let event = self.key_event(0, down)?;
                        // Typed as characters, independent of the keyboard layout.
                        event.set_string_from_utf16_unchecked(&units);
                        event.set_flags(CGEventFlags::empty());
                        self.post(event);
                    }
                }
            }
        }
        Ok(())
    }
}

fn cg_button(button: MouseButton) -> CGMouseButton {
    match button {
        MouseButton::Left => CGMouseButton::Left,
        MouseButton::Right => CGMouseButton::Right,
        _ => CGMouseButton::Center,
    }
}

fn button_number(button: MouseButton) -> i64 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Right => 1,
        MouseButton::Middle => 2,
        MouseButton::Back => 3,
        MouseButton::Forward => 4,
    }
}

/// The button a move should be reported as dragging, left taking precedence.
fn drag_button(held: &HashSet<MouseButton>) -> Option<MouseButton> {
    [MouseButton::Left, MouseButton::Right, MouseButton::Middle, MouseButton::Back, MouseButton::Forward]
        .into_iter()
        .find(|b| held.contains(b))
}

fn media_key_error(key: KeyCode) -> anyhow::Error {
    anyhow!(
        "{key:?} has no macOS key code; media and volume keys need system-defined events, \
         which this adapter does not send yet"
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl From<CGRect> for Rect {
    fn from(r: CGRect) -> Self {
        Self { x: r.origin.x, y: r.origin.y, w: r.size.width, h: r.size.height }
    }
}

impl Rect {
    fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    fn clamp(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (x.clamp(self.x, self.x + self.w - 1.0), y.clamp(self.y, self.y + self.h - 1.0))
    }
}

/// Keeps the cursor on a screen: a target off every display is pulled back onto the
/// display the cursor is moving from, so it slides along that screen's edge.
fn clamp_to_displays(target: (f64, f64), from: (f64, f64), displays: &[Rect]) -> (f64, f64) {
    if displays.is_empty() || displays.iter().any(|d| d.contains(target)) {
        return target;
    }
    let home = displays.iter().find(|d| d.contains(from)).unwrap_or(&displays[0]);
    home.clamp(target)
}

/// Counts consecutive clicks of one button for the click-state field.
#[derive(Debug, Default)]
struct ClickTracker {
    last: Option<(MouseButton, Instant, (f64, f64), i64)>,
}

impl ClickTracker {
    fn press(&mut self, button: MouseButton, at: Instant, pos: (f64, f64)) -> i64 {
        let count = match self.last {
            Some((b, t, p, n))
                if b == button
                    && at.saturating_duration_since(t) <= DOUBLE_CLICK_INTERVAL
                    && ((p.0 - pos.0).powi(2) + (p.1 - pos.1).powi(2)).sqrt() <= DOUBLE_CLICK_DISTANCE =>
            {
                n + 1
            }
            _ => 1,
        };
        self.last = Some((button, at, pos, count));
        count
    }

    /// The count to report on release: the same as the press it ends.
    fn current(&self, button: MouseButton) -> i64 {
        match self.last {
            Some((b, _, _, n)) if b == button => n,
            _ => 1,
        }
    }
}

#[derive(Debug, PartialEq)]
enum TextPiece {
    Chars(Vec<u16>),
    Newline,
}

/// Splits text into events of at most `MAX_UTF16_PER_EVENT` UTF-16 units without splitting
/// a character, with newlines as separate Return presses.
fn split_text(text: &str) -> Vec<TextPiece> {
    let mut pieces = Vec::new();
    let mut current: Vec<u16> = Vec::new();
    for ch in text.chars() {
        match ch {
            '\r' => {}
            '\n' => {
                if !current.is_empty() {
                    pieces.push(TextPiece::Chars(std::mem::take(&mut current)));
                }
                pieces.push(TextPiece::Newline);
            }
            _ => {
                let mut buf = [0u16; 2];
                let units = ch.encode_utf16(&mut buf);
                if current.len() + units.len() > MAX_UTF16_PER_EVENT {
                    pieces.push(TextPiece::Chars(std::mem::take(&mut current)));
                }
                current.extend_from_slice(units);
            }
        }
    }
    if !current.is_empty() {
        pieces.push(TextPiece::Chars(current));
    }
    pieces
}

fn modifier_flag(key: KeyCode) -> Option<CGEventFlags> {
    use KeyCode::*;
    match key {
        ShiftLeft | ShiftRight => Some(CGEventFlags::CGEventFlagShift),
        ControlLeft | ControlRight => Some(CGEventFlags::CGEventFlagControl),
        AltLeft | AltRight => Some(CGEventFlags::CGEventFlagAlternate),
        MetaLeft | MetaRight => Some(CGEventFlags::CGEventFlagCommand),
        _ => None,
    }
}

fn is_numpad(key: KeyCode) -> bool {
    use KeyCode::*;
    matches!(
        key,
        Numpad0 | Numpad1 | Numpad2 | Numpad3 | Numpad4 | Numpad5 | Numpad6 | Numpad7 | Numpad8 | Numpad9
            | NumpadAdd | NumpadSubtract | NumpadMultiply | NumpadDivide | NumpadEnter | NumpadDecimal
            | NumpadComma | NumpadEqual | ArrowUp | ArrowDown | ArrowLeft | ArrowRight
    )
}

const KVK_RETURN: u16 = 0x24;

/// macOS virtual key codes (`kVK_*` from Carbon's Events.h). These are positions on an
/// ANSI keyboard, like scan codes; the active layout decides the character.
fn mac_keycode(key: KeyCode) -> Option<u16> {
    use KeyCode::*;
    Some(match key {
        KeyA => 0x00, KeyS => 0x01, KeyD => 0x02, KeyF => 0x03, KeyH => 0x04, KeyG => 0x05,
        KeyZ => 0x06, KeyX => 0x07, KeyC => 0x08, KeyV => 0x09, KeyB => 0x0B, KeyQ => 0x0C,
        KeyW => 0x0D, KeyE => 0x0E, KeyR => 0x0F, KeyY => 0x10, KeyT => 0x11, KeyO => 0x1F,
        KeyU => 0x20, KeyI => 0x22, KeyP => 0x23, KeyL => 0x25, KeyJ => 0x26, KeyK => 0x28,
        KeyN => 0x2D, KeyM => 0x2E,

        Digit1 => 0x12, Digit2 => 0x13, Digit3 => 0x14, Digit4 => 0x15, Digit6 => 0x16,
        Digit5 => 0x17, Digit9 => 0x19, Digit7 => 0x1A, Digit8 => 0x1C, Digit0 => 0x1D,

        Equal => 0x18, Minus => 0x1B, BracketRight => 0x1E, BracketLeft => 0x21, Quote => 0x27,
        Semicolon => 0x29, Backslash => 0x2A, Comma => 0x2B, Slash => 0x2C, Period => 0x2F,
        Backquote => 0x32,

        Enter => KVK_RETURN, Tab => 0x30, Space => 0x31, Backspace => 0x33, Escape => 0x35,
        CapsLock => 0x39,

        MetaRight => 0x36, MetaLeft => 0x37, ShiftLeft => 0x38, AltLeft => 0x3A, ControlLeft => 0x3B,
        ShiftRight => 0x3C, AltRight => 0x3D, ControlRight => 0x3E,

        F1 => 0x7A, F2 => 0x78, F3 => 0x63, F4 => 0x76, F5 => 0x60, F6 => 0x61, F7 => 0x62,
        F8 => 0x64, F9 => 0x65, F10 => 0x6D, F11 => 0x67, F12 => 0x6F,

        // Mac keyboards put Help where PC keyboards have Insert, and F13–F15 where they
        // have Print Screen / Scroll Lock / Pause.
        Insert => 0x72, Home => 0x73, PageUp => 0x74, Delete => 0x75, End => 0x77, PageDown => 0x79,
        PrintScreen => 0x69, ScrollLock => 0x6B, Pause => 0x71,
        ArrowLeft => 0x7B, ArrowRight => 0x7C, ArrowDown => 0x7D, ArrowUp => 0x7E,

        NumpadDecimal => 0x41, NumpadMultiply => 0x43, NumpadAdd => 0x45, NumLock => 0x47,
        NumpadDivide => 0x4B, NumpadEnter => 0x4C, NumpadSubtract => 0x4E, NumpadEqual => 0x51,
        Numpad0 => 0x52, Numpad1 => 0x53, Numpad2 => 0x54, Numpad3 => 0x55, Numpad4 => 0x56,
        Numpad5 => 0x57, Numpad6 => 0x58, Numpad7 => 0x59, Numpad8 => 0x5B, Numpad9 => 0x5C,
        NumpadComma => 0x5F,

        // Volume/media "keys" on Macs are system-defined events, not key codes.
        MediaPlayPause | MediaStop | MediaTrackNext | MediaTrackPrevious | VolumeUp | VolumeDown | VolumeMute => {
            return None
        }

        Unknown(code) => return u16::try_from(code).ok().filter(|c| *c <= 0x7F),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every KeyCode except Unknown, so new variants can't be forgotten in the table.
    fn all_keys() -> Vec<KeyCode> {
        use KeyCode::*;
        vec![
            KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO, KeyP,
            KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
            Digit0, Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9,
            F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
            ControlLeft, ControlRight, ShiftLeft, ShiftRight, AltLeft, AltRight, MetaLeft, MetaRight,
            ArrowUp, ArrowDown, ArrowLeft, ArrowRight, Home, End, PageUp, PageDown, Insert, Delete,
            Escape, Tab, Space, Enter, Backspace, CapsLock, NumLock, ScrollLock, PrintScreen, Pause,
            Numpad0, Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8, Numpad9,
            NumpadAdd, NumpadSubtract, NumpadMultiply, NumpadDivide, NumpadEnter, NumpadDecimal,
            NumpadComma, NumpadEqual,
            Semicolon, Equal, Comma, Minus, Period, Slash, Backquote, BracketLeft, BracketRight, Backslash, Quote,
            MediaPlayPause, MediaStop, MediaTrackNext, MediaTrackPrevious, VolumeUp, VolumeDown, VolumeMute,
        ]
    }

    #[test]
    fn every_regular_key_has_a_unique_code() {
        let media = [KeyCode::MediaPlayPause, KeyCode::MediaStop, KeyCode::MediaTrackNext,
            KeyCode::MediaTrackPrevious, KeyCode::VolumeUp, KeyCode::VolumeDown, KeyCode::VolumeMute];
        let mut seen = std::collections::HashMap::new();
        for key in all_keys() {
            match mac_keycode(key) {
                Some(code) => {
                    assert!(code <= 0x7F, "{key:?} -> {code:#x}");
                    if let Some(other) = seen.insert(code, key) {
                        panic!("{key:?} and {other:?} share key code {code:#x}");
                    }
                }
                None => assert!(media.contains(&key), "{key:?} is unmapped"),
            }
        }
    }

    #[test]
    fn known_codes() {
        assert_eq!(mac_keycode(KeyCode::KeyA), Some(0x00));
        assert_eq!(mac_keycode(KeyCode::KeyC), Some(0x08));
        assert_eq!(mac_keycode(KeyCode::MetaLeft), Some(0x37));
        assert_eq!(mac_keycode(KeyCode::Enter), Some(0x24));
        assert_eq!(mac_keycode(KeyCode::ArrowUp), Some(0x7E));
        assert_eq!(mac_keycode(KeyCode::Unknown(0x31)), Some(0x31));
        assert_eq!(mac_keycode(KeyCode::Unknown(0x1234)), None);
    }

    #[test]
    fn meta_is_command() {
        assert_eq!(modifier_flag(KeyCode::MetaLeft), Some(CGEventFlags::CGEventFlagCommand));
        assert_eq!(modifier_flag(KeyCode::AltRight), Some(CGEventFlags::CGEventFlagAlternate));
        assert_eq!(modifier_flag(KeyCode::KeyA), None);
    }

    const MAIN: Rect = Rect { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 };
    const RIGHT: Rect = Rect { x: 1440.0, y: -200.0, w: 1920.0, h: 1080.0 };

    #[test]
    fn moves_freely_across_displays() {
        assert_eq!(clamp_to_displays((1500.0, -100.0), (1430.0, 100.0), &[MAIN, RIGHT]), (1500.0, -100.0));
    }

    #[test]
    fn stops_at_the_screen_edge() {
        assert_eq!(clamp_to_displays((-50.0, 100.0), (10.0, 100.0), &[MAIN, RIGHT]), (0.0, 100.0));
        // Above the main display, where the taller right display doesn't reach.
        assert_eq!(clamp_to_displays((500.0, -30.0), (500.0, 5.0), &[MAIN, RIGHT]), (500.0, 0.0));
        assert_eq!(clamp_to_displays((2000.0, 2000.0), (1500.0, 800.0), &[MAIN, RIGHT]), (2000.0, 879.0));
    }

    #[test]
    fn counts_double_and_triple_clicks() {
        let mut c = ClickTracker::default();
        let t = Instant::now();
        assert_eq!(c.press(MouseButton::Left, t, (10.0, 10.0)), 1);
        assert_eq!(c.current(MouseButton::Left), 1);
        assert_eq!(c.press(MouseButton::Left, t + Duration::from_millis(200), (11.0, 10.0)), 2);
        assert_eq!(c.press(MouseButton::Left, t + Duration::from_millis(400), (11.0, 11.0)), 3);
    }

    #[test]
    fn slow_far_or_other_button_clicks_start_over() {
        let mut c = ClickTracker::default();
        let t = Instant::now();
        c.press(MouseButton::Left, t, (10.0, 10.0));
        assert_eq!(c.press(MouseButton::Left, t + Duration::from_millis(900), (10.0, 10.0)), 1);
        assert_eq!(c.press(MouseButton::Left, t + Duration::from_millis(1000), (50.0, 10.0)), 1);
        assert_eq!(c.press(MouseButton::Right, t + Duration::from_millis(1100), (50.0, 10.0)), 1);
    }

    #[test]
    fn left_drag_wins_over_other_buttons() {
        let held: HashSet<_> = [MouseButton::Right, MouseButton::Left].into_iter().collect();
        assert_eq!(drag_button(&held), Some(MouseButton::Left));
        assert_eq!(drag_button(&HashSet::new()), None);
    }

    #[test]
    fn text_is_chunked_without_splitting_characters() {
        let text = format!("{}👋", "a".repeat(19));
        let pieces = split_text(&text);
        // 19 + 2 units would exceed 20, so the emoji starts a new event.
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[1], TextPiece::Chars("👋".encode_utf16().collect()));
        for piece in &pieces {
            if let TextPiece::Chars(units) = piece {
                assert!(units.len() <= MAX_UTF16_PER_EVENT);
            }
        }
    }

    #[test]
    fn newlines_press_return() {
        assert_eq!(
            split_text("hi\r\nyo"),
            vec![TextPiece::Chars("hi".encode_utf16().collect()), TextPiece::Newline, TextPiece::Chars("yo".encode_utf16().collect())]
        );
    }
}
