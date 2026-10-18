use std::collections::{HashSet, HashMap};
use pir_protocol::{InputEvent, EventMetadata, KeyCode, ModifierState, MouseButton, ButtonState};
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
    start_x: i32,
    start_y: i32,
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
        self.active_composition = None;
        self.sequence_number += 1;
        vec![InputEvent::CompositionCommit {
            metadata: EventMetadata::new(self.session_id, self.sequence_number),
            text: String::new(),
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

    pub fn handle_consumer_key(&mut self, key: pir_protocol::ConsumerKeyCode, state: ButtonState) -> Vec<InputEvent> {
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

        for button in self.held_mouse_buttons.drain() {
            self.sequence_number += 1;
            release_events.push(InputEvent::MouseButton {
                metadata: EventMetadata::new(self.session_id, self.sequence_number),
                button,
                state: ButtonState::Released,
            });
        }

        if self.modifiers != ModifierState::default() {
            self.modifiers = ModifierState::default();
            self.sequence_number += 1;
            release_events.push(InputEvent::ModifierState {
                metadata: EventMetadata::new(self.session_id, self.sequence_number),
                modifiers: ModifierState::default(),
            });
        }

        if let Some(drag) = self.active_drag.take() {
            self.sequence_number += 1;
            release_events.push(InputEvent::MouseButton {
                metadata: EventMetadata::new(self.session_id, self.sequence_number),
                button: drag.button,
                state: ButtonState::Released,
            });
        }

        if self.active_composition.is_some() {
            self.active_composition = None;
            self.sequence_number += 1;
            release_events.push(InputEvent::CompositionCommit {
                metadata: EventMetadata::new(self.session_id, self.sequence_number),
                text: String::new(),
            });
        }

        if !release_events.is_empty() {
            warn!("Reset input state: released {} keys, {} buttons, modifiers={:?}, drag={:?}, composition={:?}",
                release_events.iter().filter(|e| matches!(e, InputEvent::KeyUp {..})).count(),
                release_events.iter().filter(|e| matches!(e, InputEvent::MouseButton {..})).count(),
                self.modifiers,
                self.active_drag.is_some(),
                self.active_composition.is_some()
            );
        }

        release_events
    }

    pub fn get_held_keys(&self) -> &HashSet<KeyCode> { &self.held_keys }
    pub fn get_held_buttons(&self) -> &HashSet<MouseButton> { &self.held_mouse_buttons }
    pub fn get_modifiers(&self) -> ModifierState { self.modifiers }
    pub fn has_active_composition(&self) -> bool { self.active_composition.is_some() }
}