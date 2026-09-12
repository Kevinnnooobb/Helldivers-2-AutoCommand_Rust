// Overlay 窗口行为（只操作本进程自己的窗口）
//
// 为什么不用 ViewportCommand::Visible(false) 来「隐藏」浮窗：
//   Windows 不会给隐藏窗口派发 WM_PAINT，winit 的 RedrawRequested 也随之停止，
//   于是 eframe 的 update() 不再被调用 —— 而全局热键动作正是在 update() 里消费的，
//   结果就是「隐藏后热键再也唤不出浮窗」（违反生命周期要求）。
//
// 因此这里用分层窗口（WS_EX_LAYERED + LWA_ALPHA）把浮窗设成完全透明并开启点击穿透：
//   * 视觉上等价于隐藏，游戏画面完全无遮挡；
//   * 窗口仍然是「可见」的，事件循环与热键派发持续工作；
//   * 恢复时把 alpha 设回配置的不透明度即可，无需重建窗口。
//
// 只做这些事：不改游戏文件/内存、不注入、不抢焦点（全程 SWP_NOACTIVATE）。
#![allow(non_snake_case)]

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::sync::Mutex;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, RECT};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClientRect, GetLayeredWindowAttributes, GetWindowLongW, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    SetLayeredWindowAttributes, SetWindowLongW, SetWindowPos, GWL_EXSTYLE, HWND_NOTOPMOST,
    HWND_TOPMOST, LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, WS_EX_LAYERED, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};

/// 本进程主窗口句柄缓存（窗口不会重建，缓存一次即可）
static OWN_HWND: Mutex<Option<isize>> = Mutex::new(None);

/// 窗口标题前缀（eframe 用 with_title 设置；无边框窗口依然带标题）
const TITLE_HINT: &str = "H2AC-RS";

/// 找到本进程的主窗口（缓存）。
pub fn own_window() -> Option<HWND> {
    if let Ok(guard) = OWN_HWND.lock() {
        if let Some(raw) = *guard {
            return Some(HWND(raw as *mut _));
        }
    }
    let pid = unsafe { GetCurrentProcessId() };
    let mut ctx = FindCtx {
        pid,
        best: None,
        best_area: 0,
    };
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut FindCtx as isize));
    }
    let hwnd = ctx.best?;
    if let Ok(mut guard) = OWN_HWND.lock() {
        *guard = Some(hwnd.0 as isize);
    }
    Some(hwnd)
}

struct FindCtx {
    pid: u32,
    best: Option<HWND>,
    best_area: i64,
}

unsafe extern "system" fn enum_proc(
    hwnd: HWND,
    lparam: LPARAM,
) -> windows::Win32::Foundation::BOOL {
    let ctx = &mut *(lparam.0 as *mut FindCtx);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid != ctx.pid || !IsWindowVisible(hwnd).as_bool() {
        return windows::Win32::Foundation::BOOL::from(true);
    }
    let mut rect = RECT::default();
    if GetClientRect(hwnd, &mut rect).is_err() {
        return windows::Win32::Foundation::BOOL::from(true);
    }
    let area = ((rect.right - rect.left) as i64) * ((rect.bottom - rect.top) as i64);
    if area <= 0 {
        return windows::Win32::Foundation::BOOL::from(true);
    }
    // 优先取带 H2AC-RS 标题的窗口，其余按面积最大者兜底
    let titled = window_title(hwnd)
        .map(|t| t.contains(TITLE_HINT))
        .unwrap_or(false);
    let better = match ctx.best {
        None => true,
        Some(_) => titled || area > ctx.best_area,
    };
    if better {
        ctx.best = Some(hwnd);
        ctx.best_area = area;
    }
    windows::Win32::Foundation::BOOL::from(true)
}

fn window_title(hwnd: HWND) -> Option<String> {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        let written = GetWindowTextW(hwnd, &mut buf);
        if written <= 0 {
            return None;
        }
        Some(
            OsString::from_wide(&buf[..written as usize])
                .to_string_lossy()
                .to_string(),
        )
    }
}

/// 应用分层窗口不透明度（0.0 = 完全透明 / 视觉隐藏）。
/// 返回 true 表示设置已被系统确认（可通过 GetLayeredWindowAttributes 读回）。
pub fn apply_alpha(hwnd: HWND, alpha: f32) -> bool {
    let value = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    unsafe {
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let new_ex = (ex as u32 | WS_EX_LAYERED.0) as i32;
        if new_ex != ex {
            SetWindowLongW(hwnd, GWL_EXSTYLE, new_ex);
        }
        if SetLayeredWindowAttributes(hwnd, COLORREF(0), value, LWA_ALPHA).is_err() {
            return false;
        }
        // 让新的扩展样式立即生效（不移动、不改尺寸、不抢焦点）
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
        );
        // 读回确认
        let mut got = 0u8;
        if GetLayeredWindowAttributes(hwnd, None, Some(&mut got), None).is_err() {
            return false;
        }
        (got as i32 - value as i32).abs() <= 1
    }
}

/// 读取当前分层窗口不透明度（未启用分层时返回 None）。
pub fn current_alpha(hwnd: HWND) -> Option<u8> {
    unsafe {
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_LAYERED.0 == 0 {
            return None;
        }
        let mut alpha = 0u8;
        if GetLayeredWindowAttributes(hwnd, None, Some(&mut alpha), None).is_err() {
            return None;
        }
        Some(alpha)
    }
}

/// 是否已开启点击穿透。
pub fn is_click_through(hwnd: HWND) -> bool {
    unsafe { (GetWindowLongW(hwnd, GWL_EXSTYLE) as u32) & WS_EX_TRANSPARENT.0 != 0 }
}

/// 开启 / 关闭点击穿透（透明隐藏时不应该拦住游戏里的鼠标操作）。
pub fn set_click_through(hwnd: HWND, enabled: bool) -> bool {
    unsafe {
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let updated = if enabled {
            ex | WS_EX_TRANSPARENT.0
        } else {
            ex & !WS_EX_TRANSPARENT.0
        };
        SetWindowLongW(hwnd, GWL_EXSTYLE, updated as i32);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
        );
        let now = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        (now & WS_EX_TRANSPARENT.0 != 0) == enabled
    }
}

/// 设置真正的 Windows 置顶（Z-order）。
///
/// 与 ViewportCommand::WindowLevel 的区别：这里显式使用 HWND_TOPMOST / HWND_NOTOPMOST，
/// 且**不带 SWP_NOZORDER**（带上就不会真正改变 Z-order）。
/// 同时带 SWP_NOACTIVATE：置顶 != 抢焦点，绝不调用 SetForegroundWindow。
pub fn set_topmost(hwnd: HWND, on: bool) -> bool {
    let insert_after = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
    let ok = unsafe {
        SetWindowPos(
            hwnd,
            insert_after,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .is_ok()
    };
    ok && is_topmost(hwnd) == on
}

/// 当前是否处于置顶状态（读 WS_EX_TOPMOST，不修改任何东西）。
pub fn is_topmost(hwnd: HWND) -> bool {
    unsafe { (GetWindowLongW(hwnd, GWL_EXSTYLE) as u32) & WS_EX_TOPMOST.0 != 0 }
}

/// 把浮窗移到屏幕坐标（物理像素）。不激活窗口。
pub fn move_to(hwnd: HWND, x: i32, y: i32) -> bool {
    unsafe {
        // 只移动：带 SWP_NOZORDER 时 hwndInsertAfter 被忽略，Z-order 不受影响
        // （置顶由 set_topmost 用 HWND_TOPMOST 单独完成）
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
        .is_ok()
    }
}

/// 当前窗口矩形（屏幕物理像素）。
pub fn window_rect(hwnd: HWND) -> Option<(i32, i32, i32, i32)> {
    unsafe {
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).ok()?;
        Some((rect.left, rect.top, rect.right, rect.bottom))
    }
}
