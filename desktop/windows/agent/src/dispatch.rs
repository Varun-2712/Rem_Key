use anyhow::Result;
use pir_input::composition::{CompositionTracker, TextEdit};
use pir_input::state::InputStateManager;
use pir_input_adapter::InputAdapter;
use pir_protocol::events::{ButtonState, ConsumerKeyCode, InputEvent, KeyCode};
use tracing::debug;
use uuid::Uuid;

/// Applies input events from one phone session to the desktop, mirroring what is held
/// down so it can all be released if the session ends abruptly.
pub struct Dispatcher<A: InputAdapter> {
    adapter: A,
    state: InputStateManager,
    composition: CompositionTracker,
}

impl<A: InputAdapter> Dispatcher<A> {
    pub fn new(adapter: A, session_id: Uuid) -> Self {
        Self {
            adapter,
            state: InputStateManager::new(session_id),
            composition: CompositionTracker::new(),
        }
    }

    /// Start a new session, keeping the adapter.
    pub fn reset_session(&mut self, session_id: Uuid) {
        self.release_all();
        self.state = InputStateManager::new(session_id);
    }

    pub fn state(&self) -> &InputStateManager {
        &self.state
    }

    #[cfg(test)]
    pub fn adapter(&self) -> &A {
        &self.adapter
    }

    /// Returns whether the event was an input event (as opposed to control/diagnostic).
    pub fn apply(&mut self, event: &InputEvent) -> Result<bool> {
        match event {
            InputEvent::MouseMove { delta_x, delta_y, .. } => {
                self.adapter.inject_mouse_move(*delta_x, *delta_y)?;
            }
            InputEvent::MouseButton { button, state, .. } => {
                self.state.handle_mouse_button(*button, *state == ButtonState::Pressed);
                self.adapter.inject_mouse_button(*button, *state)?;
            }
            InputEvent::Scroll { delta_x, delta_y, .. } => {
                self.adapter.inject_scroll(*delta_x, *delta_y)?;
            }
            InputEvent::KeyDown { key, modifiers, .. } => {
                self.state.handle_key_down(*key, *modifiers);
                self.adapter.inject_key(*key, ButtonState::Pressed)?;
            }
            InputEvent::KeyUp { key, modifiers, .. } => {
                self.state.handle_key_up(*key, *modifiers);
                self.adapter.inject_key(*key, ButtonState::Released)?;
            }
            InputEvent::ModifierState { modifiers, .. } => {
                // Informational: modifiers are injected as the KeyDown/KeyUp of the keys themselves.
                self.state.handle_modifier_state(*modifiers);
            }
            InputEvent::Text { text, .. } => {
                self.adapter.inject_text(text)?;
            }
            InputEvent::CompositionStart { .. } => {
                self.state.handle_composition_start();
                self.composition.start();
            }
            InputEvent::CompositionUpdate { text, .. } => {
                self.state.handle_composition_update(text.clone());
                let edit = self.composition.update(text);
                self.apply_edit(edit)?;
            }
            InputEvent::CompositionCommit { text, .. } => {
                self.state.handle_composition_commit();
                let edit = self.composition.commit(text);
                self.apply_edit(edit)?;
            }
            InputEvent::ConsumerKey { key, state, .. } => {
                self.adapter.inject_key(consumer_key(*key), *state)?;
            }
            InputEvent::ClipboardRequest { .. }
            | InputEvent::ClipboardResponse { .. }
            | InputEvent::SessionEvent { .. }
            | InputEvent::DiagnosticEvent { .. } => {
                debug!("Not an input event: {:?}", event);
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn apply_edit(&mut self, edit: TextEdit) -> Result<()> {
        self.adapter.tap_key(KeyCode::Backspace, edit.backspaces)?;
        if !edit.insert.is_empty() {
            self.adapter.inject_text(&edit.insert)?;
        }
        Ok(())
    }

    /// Releases every key and button the phone is holding. Must run on every way a
    /// session can end, or the desktop is left with stuck keys.
    pub fn release_all(&mut self) {
        for event in self.state.reset_input_state() {
            if let Err(e) = self.apply(&event) {
                tracing::error!("Failed to release input during reset: {}", e);
            }
        }
        self.composition.reset();
    }
}

fn consumer_key(key: ConsumerKeyCode) -> KeyCode {
    match key {
        ConsumerKeyCode::PlayPause => KeyCode::MediaPlayPause,
        ConsumerKeyCode::Stop => KeyCode::MediaStop,
        ConsumerKeyCode::NextTrack => KeyCode::MediaTrackNext,
        ConsumerKeyCode::PreviousTrack => KeyCode::MediaTrackPrevious,
        ConsumerKeyCode::VolumeUp => KeyCode::VolumeUp,
        ConsumerKeyCode::VolumeDown => KeyCode::VolumeDown,
        ConsumerKeyCode::Mute => KeyCode::VolumeMute,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pir_protocol::events::{EventMetadata, ModifierState, MouseButton};

    #[derive(Debug, Default)]
    struct Recorder(Vec<String>);

    impl InputAdapter for Recorder {
        fn inject_mouse_move(&mut self, dx: i32, dy: i32) -> Result<()> { self.0.push(format!("move {dx},{dy}")); Ok(()) }
        fn inject_mouse_button(&mut self, b: MouseButton, s: ButtonState) -> Result<()> { self.0.push(format!("{b:?} {s:?}")); Ok(()) }
        fn inject_scroll(&mut self, dx: i32, dy: i32) -> Result<()> { self.0.push(format!("scroll {dx},{dy}")); Ok(()) }
        fn inject_key(&mut self, k: KeyCode, s: ButtonState) -> Result<()> { self.0.push(format!("{k:?} {s:?}")); Ok(()) }
        fn inject_text(&mut self, t: &str) -> Result<()> { self.0.push(format!("text {t}")); Ok(()) }
    }

    fn meta() -> EventMetadata {
        EventMetadata::new(Uuid::nil(), 0)
    }

    fn dispatcher() -> Dispatcher<Recorder> {
        Dispatcher::new(Recorder::default(), Uuid::nil())
    }

    #[test]
    fn composition_types_each_character_once() {
        let mut d = dispatcher();
        d.apply(&InputEvent::CompositionStart { metadata: meta() }).unwrap();
        for text in ["h", "he", "hel", "help"] {
            d.apply(&InputEvent::CompositionUpdate { metadata: meta(), text: text.into() }).unwrap();
        }
        d.apply(&InputEvent::CompositionCommit { metadata: meta(), text: "hello".into() }).unwrap();

        let typed = &d.adapter().0;
        assert_eq!(typed, &["text h", "text e", "text l", "text p", "Backspace Pressed", "Backspace Released", "text lo"]);
    }

    #[test]
    fn release_all_lifts_held_keys_and_buttons() {
        let mut d = dispatcher();
        let shift = ModifierState { shift: true, ..Default::default() };
        d.apply(&InputEvent::KeyDown { metadata: meta(), key: KeyCode::ShiftLeft, modifiers: shift }).unwrap();
        d.apply(&InputEvent::MouseButton { metadata: meta(), button: MouseButton::Left, state: ButtonState::Pressed }).unwrap();

        d.release_all();

        let log = &d.adapter().0;
        assert!(log.contains(&"ShiftLeft Released".to_string()), "{log:?}");
        assert!(log.contains(&"Left Released".to_string()), "{log:?}");
        assert!(d.state().get_held_keys().is_empty());
        assert!(d.state().get_held_buttons().is_empty());
    }

    #[test]
    fn control_events_are_not_injected() {
        let mut d = dispatcher();
        let heartbeat = InputEvent::SessionEvent { metadata: meta(), event: pir_protocol::events::SessionEventType::Heartbeat };
        assert!(!d.apply(&heartbeat).unwrap());
        assert!(d.adapter().0.is_empty());
    }
}
