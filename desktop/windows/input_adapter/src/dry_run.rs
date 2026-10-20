use anyhow::Result;
use pir_protocol::events::{ButtonState, KeyCode, MouseButton};
use tracing::info;
use crate::InputAdapter;

/// Logs what would be injected instead of touching the OS. Used on platforms without a
/// native adapter yet, and to exercise the agent end to end without moving the real cursor.
#[derive(Debug, Default)]
pub struct DryRunAdapter;

impl DryRunAdapter {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }
}

impl InputAdapter for DryRunAdapter {
    fn inject_mouse_move(&mut self, delta_x: i32, delta_y: i32) -> Result<()> {
        info!(target: "inject", "mouse move dx={delta_x} dy={delta_y}");
        Ok(())
    }

    fn inject_mouse_button(&mut self, button: MouseButton, state: ButtonState) -> Result<()> {
        info!(target: "inject", "mouse {button:?} {state:?}");
        Ok(())
    }

    fn inject_scroll(&mut self, delta_x: i32, delta_y: i32) -> Result<()> {
        info!(target: "inject", "scroll dx={delta_x} dy={delta_y}");
        Ok(())
    }

    fn inject_key(&mut self, key: KeyCode, state: ButtonState) -> Result<()> {
        info!(target: "inject", "key {key:?} {state:?}");
        Ok(())
    }

    fn inject_text(&mut self, text: &str) -> Result<()> {
        info!(target: "inject", "text {text:?}");
        Ok(())
    }
}
