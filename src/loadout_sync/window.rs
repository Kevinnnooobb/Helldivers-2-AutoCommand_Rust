// 游戏窗口检测 —— 只做「找到窗口 / 读几何 / 判前台」，绝不抢焦点。
#![allow(non_snake_case)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;

use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible,
};

use crate::loadout_sync::error::LoadoutSyncError;
use crate::loadout_sync::types::{GameWindowInfo, ScreenRect};

/// 游戏进程名（不含扩展名，比较时忽略大小写）。
pub const GAME_PROCESS_NAME: &str = "helldivers2";
/// 兜底匹配：窗口标题包含该子串（进程名读取失败时的备用路径）。
pub const GAME_TITLE_HINT: &str = "HELLDIVERS";

/// 找到 HELLDIVERS 2 主窗口；返回客户区几何（虚拟桌面物理像素）。
///
/// 找不到 → GameNotFound；窗口最小化 / 客户区为空 → GameWindowInvalid。
pub fn find_game_window() -> Result<GameWindowInfo, LoadoutSyncError> {
    let hwnd = find_game_hwnd().ok_or(LoadoutSyncError::GameNotFound)?;
    window_info(hwnd)
}

/// 读取指定窗口的客户区几何与 DPI。
pub fn window_info(hwnd: HWND) -> Result<GameWindowInfo, LoadoutSyncError> {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            return Err(LoadoutSyncError::GameWindowInvalid {
                detail: "窗口处于最小化状态".into(),
            });
        }
        if !IsWindowVisible(hwnd).as_bool() {
            return Err(LoadoutSyncError::GameWindowInvalid {
                detail: "窗口不可见".into(),
            });
        }
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect).map_err(|e| LoadoutSyncError::GameWindowInvalid {
            detail: format!("GetClientRect 失败: {e}"),
        })?;
        let w = rect.right - rect.left;
        let h = rect.bottom - rect.top;
        if w <= 0 || h <= 0 {
            return Err(LoadoutSyncError::GameWindowInvalid {
                detail: format!("客户区尺寸异常 {w}x{h}"),
            });
        }
        // 客户区原点 → 虚拟桌面物理像素（进程为 Per-Monitor V2 DPI 感知）
        let mut origin = POINT { x: 0, y: 0 };
        let _ = ClientToScreen(hwnd, &mut origin);
        let dpi = {
            let d = GetDpiForWindow(hwnd);
            if d == 0 {
                96
            } else {
                d
            }
        };
        Ok(GameWindowInfo {
            hwnd: hwnd.0 as isize,
            client: ScreenRect {
                x: origin.x,
                y: origin.y,
                w,
                h,
            },
            dpi,
        })
    }
}

/// 目标窗口是否处于前台。自动化开始前必须满足，否则直接失败（不抢焦点）。
pub fn is_foreground(hwnd: isize) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        !fg.0.is_null() && fg.0 as isize == hwnd
    }
}

struct EnumCtx {
    best: Option<(HWND, i64)>,
}

/// 枚举顶层窗口，返回面积最大的 HELLDIVERS 2 渲染窗口。
fn find_game_hwnd() -> Option<HWND> {
    let mut ctx = EnumCtx { best: None };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut EnumCtx as isize));
    }
    ctx.best.map(|(h, _)| h)
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut EnumCtx);
    if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return BOOL::from(true);
    }
    if !is_game_window(hwnd) {
        return BOOL::from(true);
    }
    let mut rect = RECT::default();
    if GetClientRect(hwnd, &mut rect).is_err() {
        return BOOL::from(true);
    }
    let area = ((rect.right - rect.left) as i64) * ((rect.bottom - rect.top) as i64);
    if area <= 0 {
        return BOOL::from(true);
    }
    if ctx.best.map(|(_, a)| area > a).unwrap_or(true) {
        ctx.best = Some((hwnd, area));
    }
    BOOL::from(true)
}

fn is_game_window(hwnd: HWND) -> bool {
    if let Some(stem) = window_process_stem(hwnd) {
        if stem.eq_ignore_ascii_case(GAME_PROCESS_NAME) {
            return true;
        }
    }
    window_title(hwnd)
        .map(|t| {
            let up = t.to_uppercase();
            up.contains(GAME_TITLE_HINT)
        })
        .unwrap_or(false)
}

fn window_process_stem(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let res = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        res.ok()?;
        let path = OsString::from_wide(&buf[..len as usize]);
        let path = std::path::PathBuf::from(path);
        path.file_stem().map(|s| s.to_string_lossy().to_string())
    }
}

fn window_title(hwnd: HWND) -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
    unsafe {
        let mut buf = [0u16; 256];
        let len = GetWindowTextW(hwnd, &mut buf);
        if len <= 0 {
            return None;
        }
        Some(
            OsString::from_wide(&buf[..len as usize])
                .to_string_lossy()
                .to_string(),
        )
    }
}
