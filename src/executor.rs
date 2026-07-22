// 战备执行器 — Windows SendInput API 键盘模拟
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::sync::LazyLock;
use std::thread;
use std::time::Duration;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, GetAsyncKeyState, INPUT, INPUT_KEYBOARD, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE,
    VK_LMENU, VK_LSHIFT, VK_RMENU, VK_RSHIFT,
};

use crate::config::Config;
use crate::stratagems::Stratagem;

type ScanData = (u16, bool);

static SCANCODE_MAP: LazyLock<HashMap<&'static str, ScanData>> = LazyLock::new(|| {
    let mut m: HashMap<&'static str, ScanData> = HashMap::new();
    m.insert("↑", (0x48, true));
    m.insert("↓", (0x50, true));
    m.insert("←", (0x4B, true));
    m.insert("→", (0x4D, true));
    m.insert("ctrl", (0x1D, false));
    m.insert("left ctrl", (0x1D, false));
    m.insert("lctrl", (0x1D, false));
    m.insert("right ctrl", (0x1D, true));
    m.insert("rctrl", (0x1D, true));
    m.insert("alt", (0x38, false));
    m.insert("left alt", (0x38, false));
    m.insert("lalt", (0x38, false));
    m.insert("right alt", (0x38, true));
    m.insert("ralt", (0x38, true));
    m.insert("shift", (0x2A, false));
    m.insert("left shift", (0x2A, false));
    m.insert("lshift", (0x2A, false));
    m.insert("right shift", (0x36, false));
    m.insert("rshift", (0x36, false));
    m.insert("esc", (0x01, false));
    m.insert("escape", (0x01, false));
    m.insert("up", (0x48, true));
    m.insert("down", (0x50, true));
    m.insert("left", (0x4B, true));
    m.insert("right", (0x4D, true));
    m.insert("f1", (0x3B, false)); m.insert("f2", (0x3C, false));
    m.insert("f3", (0x3D, false)); m.insert("f4", (0x3E, false));
    m.insert("f5", (0x3F, false)); m.insert("f6", (0x40, false));
    m.insert("f7", (0x41, false)); m.insert("f8", (0x42, false));
    m.insert("f9", (0x43, false)); m.insert("f10", (0x44, false));
    m.insert("f11", (0x57, false)); m.insert("f12", (0x58, false));
    m.insert("space", (0x39, false));
    m.insert("enter", (0x1C, false));
    m.insert("tab", (0x0F, false));
    m.insert("backspace", (0x0E, false));
    m.insert("insert", (0x52, true));
    m.insert("delete", (0x53, true));
    m.insert("home", (0x47, true));
    m.insert("end", (0x4F, true));
    m.insert("pageup", (0x49, true));
    m.insert("pagedown", (0x51, true));
    m.insert("capslock", (0x3A, false));
    let digits: [(u16, &str); 10] = [
        (0x02, "1"), (0x03, "2"), (0x04, "3"), (0x05, "4"), (0x06, "5"),
        (0x07, "6"), (0x08, "7"), (0x09, "8"), (0x0A, "9"), (0x0B, "0"),
    ];
    for (sc, name) in digits {
        m.insert(name, (sc, false));
    }
    let letters: [(u16, char); 26] = [
        (0x10, 'q'), (0x11, 'w'), (0x12, 'e'), (0x13, 'r'), (0x14, 't'),
        (0x15, 'y'), (0x16, 'u'), (0x17, 'i'), (0x18, 'o'), (0x19, 'p'),
        (0x1E, 'a'), (0x1F, 's'), (0x20, 'd'), (0x21, 'f'), (0x22, 'g'),
        (0x23, 'h'), (0x24, 'j'), (0x25, 'k'), (0x26, 'l'),
        (0x2C, 'z'), (0x2D, 'x'), (0x2E, 'c'), (0x2F, 'v'), (0x30, 'b'),
        (0x31, 'n'), (0x32, 'm'),
    ];
    for (sc, c) in letters {
        // leak string to get &'static str
        m.insert(Box::leak(format!("{c}").into_boxed_str()), (sc, false));
    }
    m
});

fn lookup_scancode(key: &str) -> Result<ScanData, String> {
    let low = key.to_lowercase();
    SCANCODE_MAP
        .get(low.as_str())
        .copied()
        .or_else(|| {
            match low.as_str() {
                "escape" => SCANCODE_MAP.get("esc"),
                "control" => SCANCODE_MAP.get("ctrl"),
                "return" => SCANCODE_MAP.get("enter"),
                _ => None,
            }
            .copied()
        })
        .ok_or_else(|| format!("未知按键: {key}"))
}

fn send_key_event(scan_code: u16, extended: bool, key_up: bool) {
    let mut flags = KEYEVENTF_SCANCODE;
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }

    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(0),
                wScan: scan_code,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };

    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

fn press_key(key: &str) -> Result<(), String> {
    let (sc, ext) = lookup_scancode(key)?;
    send_key_event(sc, ext, false);
    Ok(())
}

fn release_key(key: &str) -> Result<(), String> {
    let (sc, ext) = lookup_scancode(key)?;
    send_key_event(sc, ext, true);
    Ok(())
}

fn is_key_down(vk: u16) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000u16) != 0 }
}

/// 执行战备指令序列
pub fn execute_stratagem(s: &Stratagem, config: &Config) {
    let delay = config.key_delay;
    let stratagem_key = config.stratagem_key.clone();
    let key_bindings = config.key_bindings.clone();

    // 按下战备激活键
    let _ = press_key(&stratagem_key);

    // 依次输入方向指令
    for dir in s.command.iter() {
        let mapped = key_bindings.get(*dir).cloned().unwrap_or_else(|| dir.to_string());
        thread::sleep(Duration::from_secs_f64(delay));
        let _ = press_key(&mapped);
        thread::sleep(Duration::from_secs_f64(delay));
        let _ = release_key(&mapped);
    }

    // 释放激活键
    let _ = release_key(&stratagem_key);

    // 临时释放被按住的修饰键
    let modifiers: [(u16, &str); 4] = [
        (VK_LSHIFT.0, "left shift"),
        (VK_RSHIFT.0, "right shift"),
        (VK_LMENU.0, "left alt"),
        (VK_RMENU.0, "right alt"),
    ];
    let mut released: Vec<String> = Vec::new();
    for (vk, name) in &modifiers {
        if is_key_down(*vk) {
            let (sc, ext) = lookup_scancode(name).unwrap();
            send_key_event(sc, ext, true);
            released.push(name.to_string());
        }
    }

    // 立即恢复
    thread::sleep(Duration::from_millis(5));
    for name in &released {
        let (sc, ext) = lookup_scancode(name).unwrap();
        send_key_event(sc, ext, false);
    }
}
