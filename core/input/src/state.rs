use std::collections::HashSet;
use pir_protocol::events::{InputEvent, EventMetadata, KeyCode, ModifierState, MouseButton, ButtonState, ConsumerKeyCode};
use uuid::Uuid;
use tracing::warn;

#[derive(Debug, Default)]
pub struct InputStateManager {
    held_keys: HashSet<KeyCode>,
    held_mouse_buttons: HashSet<MouseButton>,
    modifiers: ModifierState,
    active_drag: Option<DragState>,
    active_composition: Option<CompositionState>,
    sequence_number: u64,
    session_id: Uuid,
}

#[derive(Debug, Clone)]
struct DragState {
    button: MouseButton,
}

#[derive(Debug, Clone)]
struct CompositionState {
    text: String,
    cursor_position: usize,
}

impl InputStateManager {
    pub fn new(session_id: Uuid) -> Self {
        Self { session_id, ..Default::default() }
    }

    pub fn handle_key_down(&mut self, key: KeyCode, modifiers: ModifierState) -> Vec<InputEvent> {
        self.held_keys.insert(key);
        self.modifiers = modifiers;
        self.sequence_number += 1;
        vec![InputEvent::KeyDown {
            metadata: EventMetadata::new(self.session_id, self.sequence_number),
            key,
            modifiers,
        }]
    }

    pub fn handle_key_up(&mut self, key: KeyCode, modifiers: ModifierState) -> Vec<InputEvent> {
        self.held_keys.remove(&key);
        self.modifiers = modifiers;
        self.sequence_number += 1;
        vec![InputEvent::KeyUp {
            metadata: EventMetadata::new(self.session_id, self.sequence_number),
            key,
            modifiers,
        }]
    }

    pub fn handle_mouse_button(&mut self, button: MouseButton, pressed: bool) -> Vec<InputEvent> {
        if pressed {
            self.held_mouse_buttons.insert(button);
        } else {
            self.held_mouse_buttons.remove(&button);
        }
        self.sequence_number += 1;
        vec![InputEvent::MouseButton {
            metadata: EventMetadata::new(self.session_id, self.sequence_number),
            button,
            state: if pressed { ButtonState::Pressed } else { ButtonState::Released },
        }]
    }

    pub fn handle_mouse_move(&mut self, delta_x: i32, delta_y: i32) -> Vec<InputEvent> {
        self.sequence_number += 1;
        vec![InputEvent::MouseMove {
            metadata: EventMetadata::new(self.session_id, self.sequence_number),
            delta_x,
            delta_y,
        }]
    }

    pub fn handle_scroll(&mut self, delta_x: i32, delta_y: i32) -> Vec<InputEvent> {
        self.sequence_number += 1;
        vec![InputEvent::Scroll {
            metadata: EventMetadata::new(self.session_id, self.sequence_number),
            delta_x,
            delta_y,
        }]
    }

    pub fn handle_text(&mut self, text: String) -> Vec<InputEvent> {
        self.sequence_number += 1;
        vec![InputEvent::Text {
            metadata: EventMetadata::new(self.session_id, self.sequence_number),
            text,
        }]
    }

    pub fn handle_composition_start(&mut self) -> Vec<InputEvent> {
        self.active_composition = Some(CompositionState { text: String::new(), cursor_position: 0 });
        self.sequence_number += 1;
        vec![InputEvent::CompositionStart {
            metadata: EventMetadata::new(self.session_id, self.sequence_number),
        }]
    }

    pub fn handle_composition_update(&mut self, text: String) -> Vec<InputEvent> {
        if let Some(comp) = &mut self.active_composition {
            comp.text = text.clone();
            comp.cursor_position = text.len();
        }
        self.sequence_number += 1;
        vec![InputEvent::CompositionUpdate {
            metadata: EventMetadata::new(self.session_id, self.sequence_number),
            text,
        }]
    }

    pub fn handle_composition_commit(&mut self) -> Vec<InputEvent> {
        let text = self.active_composition.take().map(|c| c.text).unwrap_or_default();
        self.sequence_number += 1;
        vec![InputEvent::CompositionCommit {
            metadata: EventMetadata::new(self.session_id, self.sequence_number),
            text,
        }]
    }

    pub fn handle_modifier_state(&mut self, modifiers: ModifierState) -> Vec<InputEvent> {
        self.modifiers = modifiers;
        self.sequence_number += 1;
        vec![InputEvent::ModifierState {
            metadata: EventMetadata::new(self.session_id, self.sequence_number),
            modifiers,
        }]
    }

    pub fn handle_consumer_key(&mut self, key: ConsumerKeyCode, state: ButtonState) -> Vec<InputEvent> {
        self.sequence_number += 1;
        vec![InputEvent::ConsumerKey {
            metadata: EventMetadata::new(self.session_id, self.sequence_number),
            key,
            state,
        }]
    }

    /// CRITICAL: Must be called on EVERY disconnect/timeout/reset
    pub fn reset_input_state(&mut self) -> Vec<InputEvent> {
        let mut release_events = Vec::new();

        for key in self.held_keys.drain() {
            self.sequence_number += 1;
            release_events.push(InputEvent::KeyUp {
                metadata: EventMetadata::new(self.session_id, self.sequence_number),
                key,
                modifiers: ModifierState::default(),
            });
        }

        let released_buttons: Vec<MouseButton> = self.held_mouse_buttons.drain().collect();
        for &button in &released_buttons {
            self.sequence_number += 1;
            release_events.push(InputEvent::MouseButton {
                metadata: EventMetadata::new(self.session_id, self.sequence_number),
                button,
                state: ButtonState::Released,
            });
        }

        let had_modifiers = self.modifiers != ModifierState::default();
        if had_modifiers {
            self.modifiers = ModifierState::default();
            self.sequence_number += 1;
            release_events.push(InputEvent::ModifierState {
                metadata: EventMetadata::new(self.session_id, self.sequence_number),
                modifiers: ModifierState::default(),
            });
        }

        // The drag button is normally in held_mouse_buttons too; release it only once.
        let had_drag = self.active_drag.is_some();
        if let Some(drag) = self.active_drag.take().filter(|d| !released_buttons.contains(&d.button)) {
            self.sequence_number += 1;
            release_events.push(InputEvent::MouseButton {
                metadata: EventMetadata::new(self.session_id, self.sequence_number),
                button: drag.button,
                state: ButtonState::Released,
            });
        }

        let had_composition = self.active_composition.is_some();
        if let Some(composition) = self.active_composition.take() {
            self.sequence_number += 1;
            release_events.push(InputEvent::CompositionCommit {
                metadata: EventMetadata::new(self.session_id, self.sequence_number),
                text: composition.text,
            });
        }

        if !release_events.is_empty() {
            warn!("Reset input state: released {} keys, {} buttons, modifiers_cleared={}, drag={}, composition={}",
                release_events.iter().filter(|e| matches!(e, InputEvent::KeyUp {..})).count(),
                release_events.iter().filter(|e| matches!(e, InputEvent::MouseButton {..})).count(),
                had_modifiers,
                had_drag,
                had_composition
            );
        }

        release_events
    }

    pub fn get_held_keys(&self) -> &HashSet<KeyCode> { &self.held_keys }
    pub fn get_held_buttons(&self) -> &HashSet<MouseButton> { &self.held_mouse_buttons }
    pub fn get_modifiers(&self) -> ModifierState { self.modifiers }
    pub fn has_active_composition(&self) -> bool { self.active_composition.is_some() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_carries_the_composed_text() {
        let mut state = InputStateManager::new(Uuid::nil());
        state.handle_composition_start();
        state.handle_composition_update("hello".into());
        let events = state.handle_composition_commit();
        assert!(matches!(&events[..], [InputEvent::CompositionCommit { text, .. }] if text == "hello"));
        assert!(!state.has_active_composition());
    }

    #[test]
    fn reset_releases_everything_once() {
        let mut state = InputStateManager::new(Uuid::nil());
        let shift = ModifierState { shift: true, ..Default::default() };
        state.handle_key_down(KeyCode::ShiftLeft, shift);
        state.handle_key_down(KeyCode::KeyA, shift);
        state.handle_mouse_button(MouseButton::Left, true);
        state.active_drag = Some(DragState { button: MouseButton::Left });

        let events = state.reset_input_state();
        let key_ups = events.iter().filter(|e| matches!(e, InputEvent::KeyUp { .. })).count();
        let left_releases = events.iter().filter(|e| matches!(e,
            InputEvent::MouseButton { button: MouseButton::Left, state: ButtonState::Released, .. })).count();
        assert_eq!(key_ups, 2);
        assert_eq!(left_releases, 1);
        assert!(state.get_held_keys().is_empty());
        assert_eq!(state.get_modifiers(), ModifierState::default());
        assert!(state.reset_input_state().is_empty());
    }
}