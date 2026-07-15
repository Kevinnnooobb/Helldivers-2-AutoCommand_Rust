// 全局热键监听 — Windows Low-Level Keyboard Hook
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW,
    TranslateMessage, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN,
};

static HOTKEY_STATE: OnceLock<Mutex<HotkeyState>> = OnceLock::new();

struct HotkeyState {
    map: Arc<Mutex<HashMap<String, usize>>>,
    tx: Sender<usize>,
}

pub fn vk_to_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 => format!("{}", vk - 0x30),
        0x41..=0x5A => format!("{}", (vk as u8 - 0x41 + b'a') as char),
        0x70 => "f1".into(), 0x71 => "f2".into(), 0x72 => "f3".into(),
        0x73 => "f4".into(), 0x74 => "f5".into(), 0x75 => "f6".into(),
        0x76 => "f7".into(), 0x77 => "f8".into(), 0x78 => "f9".into(),
        0x79 => "f10".into(), 0x7A => "f11".into(), 0x7B => "f12".into(),
        0x60 => "numpad0".into(), 0x61 => "numpad1".into(), 0x62 => "numpad2".into(),
        0x63 => "numpad3".into(), 0x64 => "numpad4".into(), 0x65 => "numpad5".into(),
        0x66 => "numpad6".into(), 0x67 => "numpad7".into(), 0x68 => "numpad8".into(),
        0x69 => "numpad9".into(),
        0x20 => "space".into(), 0x0D => "enter".into(), 0x09 => "tab".into(),
        0x1B => "esc".into(), 0x08 => "backspace".into(),
        _ => format!("vk({vk})"),
    }
}

pub fn start(
    hotkey_map: Arc<Mutex<HashMap<String, usize>>>,
    tx: Sender<usize>,
    running: Arc<AtomicBool>,
) {
    let state = HotkeyState {
        map: hotkey_map,
        tx,
    };
    let _ = HOTKEY_STATE.set(Mutex::new(state));

    thread::spawn(move || {
        let hook: HHOOK = unsafe {
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0)
        }.expect("SetWindowsHookExW failed");

        let mut msg = MSG::default();
        while running.load(Ordering::Relaxed) {
            let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            if ret.0 == 0 || ret.0 == -1 {
                break;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(hook); }
    });
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> windows::Win32::Foundation::LRESULT {
    if code >= 0 && wparam.0 == WM_KEYDOWN as usize {
        let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk = kbd.vkCode;

        let ctrl = (GetAsyncKeyState(0xA2) as u16 & 0x8000u16) != 0
            || (GetAsyncKeyState(0xA3) as u16 & 0x8000u16) != 0;
        let alt = (GetAsyncKeyState(0xA4) as u16 & 0x8000u16) != 0
            || (GetAsyncKeyState(0xA5) as u16 & 0x8000u16) != 0;

        if !ctrl && !alt {
            let key_name = vk_to_name(vk);
            if let Some(state_lock) = HOTKEY_STATE.get() {
                if let Ok(data) = state_lock.lock() {
                    if let Ok(map) = data.map.lock() {
                        if let Some(&slot) = map.get(&key_name) {
                            let _ = data.tx.send(slot);
                        }
                    }
                }
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}
