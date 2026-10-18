use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::Foundation::*;
use crate::types::{MouseButton, ButtonState, KeyCode};
use anyhow::Result;
use tracing::{debug, error};

pub struct WindowsInputAdapter;

impl WindowsInputAdapter {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    pub fn inject_mouse_move(&self, delta_x: i32, delta_y: i32) -> Result<()> {
        let mut input = INPUT::default();
        input.r#type = INPUT_MOUSE;
        input.Anonymous.mi = MOUSEINPUT {
            dx: delta_x,
            dy: delta_y,
            dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_VIRTUALDESK,
            ..Default::default()
        };
        
        let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if result == 0 {
            error!("SendInput failed for mouse move: {}", std::io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn inject_mouse_button(&self, button: MouseButton, state: ButtonState) -> Result<()> {
        let (down_flag, up_flag) = button.to_win32_flags();
        let x_button = button.x_button_data();
        
        let mut input = INPUT::default();
        input.r#type = INPUT_MOUSE;
        input.Anonymous.mi = MOUSEINPUT {
            dwFlags: if state == ButtonState::Pressed { down_flag } else { up_flag },
            mouseData: x_button,
            ..Default::default()
        };
        
        let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if result == 0 {
            error!("SendInput failed for mouse button: {}", std::io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn inject_key(&self, key: KeyCode, state: ButtonState) -> Result<()> {
        let vk = key.to_virtual_key();
        let scan = unsafe { MapVirtualKeyW(vk.0 as u32, MAPVK_VK_TO_VSC) };
        
        let mut input = INPUT::default();
        input.r#type = INPUT_KEYBOARD;
        input.Anonymous.ki = KEYBDINPUT {
            wVk: vk,
            wScan: scan as u16,
            dwFlags: if state == ButtonState::Pressed { 
                KEYEVENTF_SCANCODE 
            } else { 
                KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP 
            },
            ..Default::default()
        };
        
        let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
        if result == 0 {
            error!("SendInput failed for key: {}", std::io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn inject_text(&self, text: &str) -> Result<()> {
        for ch in text.chars() {
            let mut input = INPUT::default();
            input.r#type = INPUT_KEYBOARD;
            input.Anonymous.ki = KEYBDINPUT {
                wScan: ch as u16,
                dwFlags: KEYEVENTF_UNICODE,
                ..Default::default()
            };
            
            let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
            if result == 0 {
                error!("SendInput failed for text (down): {}", std::io::Error::last_os_error());
            }
            
            input.Anonymous.ki.dwFlags = KEYEVENTF_UNICODE | KEYEVENTF_KEYUP;
            let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
            if result == 0 {
                error!("SendInput failed for text (up): {}", std::io::Error::last_os_error());
            }
        }
        Ok(())
    }

    pub fn inject_composition(&self, text: &str) -> Result<()> {
        self.inject_text(text)
    }

    pub fn inject_scroll(&self, delta_x: i32, delta_y: i32) -> Result<()> {
        if delta_y != 0 {
            let mut input = INPUT::default();
            input.r#type = INPUT_MOUSE;
            input.Anonymous.mi = MOUSEINPUT {
                mouseData: (delta_y * 120) as u32,
                dwFlags: MOUSEEVENTF_WHEEL,
                ..Default::default()
            };
            let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
            if result == 0 {
                error!("SendInput failed for scroll: {}", std::io::Error::last_os_error());
            }
        }
        
        if delta_x != 0 {
            let mut input = INPUT::default();
            input.r#type = INPUT_MOUSE;
            input.Anonymous.mi = MOUSEINPUT {
                mouseData: (delta_x * 120) as u32,
                dwFlags: MOUSEEVENTF_HWHEEL,
                ..Default::default()
            };
            let result = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
            if result == 0 {
                error!("SendInput failed for hscroll: {}", std::io::Error::last_os_error());
            }
        }
        Ok(())
    }
}