use anyhow::Result;
use pir_protocol::events::{ButtonState, KeyCode, MouseButton};

/// Turns semantic input into native OS input. One implementation per platform.
pub trait InputAdapter {
    fn inject_mouse_move(&mut self, delta_x: i32, delta_y: i32) -> Result<()>;
    fn inject_mouse_button(&mut self, button: MouseButton, state: ButtonState) -> Result<()>;
    /// Scroll by whole wheel notches; positive `delta_y` scrolls up, positive `delta_x` right.
    fn inject_scroll(&mut self, delta_x: i32, delta_y: i32) -> Result<()>;
    fn inject_key(&mut self, key: KeyCode, state: ButtonState) -> Result<()>;
    /// Types `text` as characters, independent of keyboard layout.
    fn inject_text(&mut self, text: &str) -> Result<()>;

    fn tap_key(&mut self, key: KeyCode, times: usize) -> Result<()> {
        for _ in 0..times {
            self.inject_key(key, ButtonState::Pressed)?;
            self.inject_key(key, ButtonState::Released)?;
        }
        Ok(())
    }
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use crate::windows::WindowsInputAdapter as PlatformAdapter;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use crate::macos::MacInputAdapter as PlatformAdapter;

mod dry_run;
pub use dry_run::DryRunAdapter;
#[cfg(not(any(windows, target_os = "macos")))]
pub use dry_run::DryRunAdapter as PlatformAdapter;

/// Whether `PlatformAdapter` injects real input on this platform (otherwise it only logs).
pub const HAS_NATIVE_ADAPTER: bool = cfg!(any(windows, target_os = "macos"));
