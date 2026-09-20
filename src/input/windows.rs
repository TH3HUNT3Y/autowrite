#[cfg(windows)]
mod native {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput,
        VIRTUAL_KEY, VK_BACK,
    };

    pub fn send_char(character: char) {
        let down = KEYBDINPUT {
            wVk: VIRTUAL_KEY(0),
            wScan: character as u16,
            dwFlags: KEYEVENTF_UNICODE,
            time: 0,
            dwExtraInfo: 0,
        };
        let up = KEYBDINPUT {
            dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
            ..down
        };
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: down },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: up },
            },
        ];
        unsafe {
            let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }

    pub fn send_backspace() {
        let down = KEYBDINPUT {
            wVk: VIRTUAL_KEY(VK_BACK.0),
            ..Default::default()
        };
        let up = KEYBDINPUT {
            wVk: VIRTUAL_KEY(VK_BACK.0),
            dwFlags: KEYEVENTF_KEYUP,
            ..Default::default()
        };
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: down },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: up },
            },
        ];
        unsafe {
            let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }

    pub fn send_revision_navigation(character_count: usize, word_length: usize, replacement: &str) {
        send_vk(0xA2, false);
        send_vk(0x24, false);
        send_vk(0x24, true);
        send_vk(0xA2, true);
        for _ in 0..character_count {
            send_vk(0x27, false);
            send_vk(0x27, true);
        }
        for _ in 0..word_length {
            send_backspace();
        }
        for character in replacement.chars() {
            send_char(character);
        }
        send_vk(0x23, false);
        send_vk(0x23, true);
    }

    fn send_vk(vk: u16, key_up: bool) {
        let input = KEYBDINPUT {
            wVk: VIRTUAL_KEY(vk),
            dwFlags: if key_up {
                KEYEVENTF_KEYUP
            } else {
                Default::default()
            },
            ..Default::default()
        };
        let inputs = [INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: input },
        }];
        unsafe {
            let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }
}

#[cfg(windows)]
pub use native::*;

#[cfg(windows)]
pub fn spawn_hotkeys(commands: crossbeam_channel::Sender<crate::typing::EngineCommand>) {
    use windows::Win32::UI::Input::KeyboardAndMouse::RegisterHotKey;
    use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};
    std::thread::spawn(move || unsafe {
        let _ = RegisterHotKey(None, 1, Default::default(), 0x75);
        let _ = RegisterHotKey(None, 2, Default::default(), 0x76);
        let _ = RegisterHotKey(None, 3, Default::default(), 0x77);
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            if message.message == WM_HOTKEY {
                match message.wParam.0 {
                    1 => {
                        let _ = commands.send(crate::typing::EngineCommand::StartLast);
                    }
                    2 => {
                        let _ = commands.send(crate::typing::EngineCommand::TogglePause);
                    }
                    3 => {
                        let _ = commands.send(crate::typing::EngineCommand::Stop);
                    }
                    _ => {}
                }
            }
        }
    });
}
