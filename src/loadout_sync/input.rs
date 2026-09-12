// Loadout Sync 输入控制 —— 唯一的鼠标/滚轮注入入口。
//
// 与 executor.rs 严格分离：executor 只负责「战备指令键盘序列」，
// 这里只负责 Loadout UI 自动化所需的鼠标移动 / 左键 / 滚轮，互不混用。
//
// 所有注入都走 SendInput，并且保证异常路径下不会留下按下的键：
// 任何状态结束时控制器都会调用 release_all()。
#![allow(non_snake_case)]

use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{GetLastError, ERROR_ACCESS_DENIED};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEINPUT,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SetCursorPos, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use crate::loadout_sync::error::LoadoutSyncError;
use crate::loadout_sync::types::ScreenPoint;

/// 鼠标/滚轮注入接口 —— 状态机只依赖这个 trait，测试用 mock 实现。
pub trait SyncInput {
    fn move_to(&mut self, point: ScreenPoint) -> Result<(), LoadoutSyncError>;
    fn click_left(&mut self) -> Result<(), LoadoutSyncError>;
    fn scroll(&mut self, notches: i32) -> Result<(), LoadoutSyncError>;
    /// 释放所有可能被按下的鼠标键与修饰键（异常安全网）。
    fn release_all(&mut self);
}

/// 虚拟桌面范围缓存（绝对坐标归一化用）。
#[derive(Debug, Clone, Copy)]
struct DesktopBounds {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl DesktopBounds {
    fn query() -> Self {
        unsafe {
            let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let w = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1);
            let h = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1);
            Self { x, y, w, h }
        }
    }

    /// 物理像素 → SendInput 绝对坐标（0..65535，跨多显示器需 VIRTUALDESK）。
    fn normalize(&self, p: ScreenPoint) -> (i32, i32) {
        let nx = ((p.x - self.x) as i64 * 65_535 / (self.w.max(2) - 1) as i64).clamp(0, 65_535);
        let ny = ((p.y - self.y) as i64 * 65_535 / (self.h.max(2) - 1) as i64).clamp(0, 65_535);
        (nx as i32, ny as i32)
    }
}

pub struct LoadoutInputController {
    desktop: DesktopBounds,
}

impl Default for LoadoutInputController {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadoutInputController {
    pub fn new() -> Self {
        Self {
            desktop: DesktopBounds::query(),
        }
    }

    pub fn mouse_down(&self) -> Result<(), LoadoutSyncError> {
        send_mouse(0, 0, 0, MOUSEEVENTF_LEFTDOWN)
    }

    pub fn mouse_up(&self) -> Result<(), LoadoutSyncError> {
        send_mouse(0, 0, 0, MOUSEEVENTF_LEFTUP)
    }
}

impl SyncInput for LoadoutInputController {
    fn move_to(&mut self, point: ScreenPoint) -> Result<(), LoadoutSyncError> {
        let (nx, ny) = self.desktop.normalize(point);
        let flags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
        send_mouse(nx, ny, 0, flags)?;
        // 少数游戏在 SetCursorPos/SendInput 相对移动上更可靠，双重保证：
        // 若注入后的光标位置与目标偏差过大，则用 SetCursorPos 兜底。
        unsafe {
            let mut cur = windows::Win32::Foundation::POINT::default();
            if GetCursorPos(&mut cur).is_ok() {
                let dx = (cur.x - point.x).abs();
                let dy = (cur.y - point.y).abs();
                if dx > 2 || dy > 2 {
                    let _ = SetCursorPos(point.x, point.y);
                }
            }
        }
        Ok(())
    }

    fn click_left(&mut self) -> Result<(), LoadoutSyncError> {
        self.mouse_down()?;
        // 20ms 按住时间：避免游戏把点击当成瞬时抖动而忽略
        thread::sleep(Duration::from_millis(20));
        self.mouse_up()?;
        Ok(())
    }

    fn scroll(&mut self, notches: i32) -> Result<(), LoadoutSyncError> {
        let delta = notches * crate::loadout_sync::config::WHEEL_UNIT;
        send_mouse(0, 0, delta, MOUSEEVENTF_WHEEL)
    }

    fn release_all(&mut self) {
        // 鼠标：左/右键抬起（未按下时发送也无副作用）
        let _ = send_mouse(0, 0, 0, MOUSEEVENTF_LEFTUP);
        let _ = send_mouse(0, 0, 0, MOUSEEVENTF_RIGHTUP);
        // 键盘：Shift/Ctrl/Alt 抬起（Loadout Sync 自身不按键，但用户可能在按下修饰键时触发快捷键）
        for (vk, scan) in [
            (0x10u16, 0x2Au16), // shift
            (0x11u16, 0x1Du16), // ctrl
            (0x12u16, 0x38u16), // alt
        ] {
            let _ = send_key(scan, vk, true);
        }
    }
}

fn send_mouse(
    dx: i32,
    dy: i32,
    data: i32,
    flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS,
) -> Result<(), LoadoutSyncError> {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: data as u32,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inserted = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if inserted == 0 {
        return Err(inject_error("SendInput(鼠标)"));
    }
    Ok(())
}

fn send_key(scan: u16, vk: u16, key_up: bool) -> Result<(), LoadoutSyncError> {
    let mut flags = KEYEVENTF_SCANCODE;
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inserted = unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
    if inserted == 0 {
        return Err(inject_error("SendInput(键盘释放)"));
    }
    Ok(())
}

/// 注入失败时区分「权限不足（UIPI 拦截）」与其它错误，避免静默失败。
fn inject_error(what: &str) -> LoadoutSyncError {
    let err = unsafe { GetLastError() };
    if err == ERROR_ACCESS_DENIED {
        LoadoutSyncError::PermissionError
    } else {
        LoadoutSyncError::InputError {
            detail: format!("{what} 被系统拒绝 (GetLastError={})", err.0),
        }
    }
}
