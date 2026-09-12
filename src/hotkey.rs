// 全局热键监听 — Windows Low-Level Keyboard Hook
#![allow(non_snake_case)]

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, HHOOK, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// 全局热键触发的动作
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    /// 执行指定槽位的战备
    Slot(usize),
    /// 开关监听状态
    ToggleListening,
    /// 紧凑模式 Overlay：显示 / 隐藏浮窗。
    /// **这是唯一允许改变浮窗可见性的动作**。
    ToggleCompactOverlay,
    /// 自动装配：正常模式 / 紧凑模式 / 浮窗隐藏时都可用，只负责启动 Loadout Sync。
    /// 绝不切换浮窗状态（开始时会临时隐藏并在结束后恢复原状态）。
    AutoLoadout,
    /// 取消正在执行的自动装配（独立热键，不碰浮窗状态）
    CancelLoadoutSync,
}

/// 钩子线程与 UI 线程共享的状态（回调只能访问全局数据）
#[derive(Clone)]
struct HotkeyShared {
    map: Arc<Mutex<HashMap<String, HotkeyAction>>>,
    tx: Sender<HotkeyAction>,
}

static HOTKEY_STATE: Mutex<Option<HotkeyShared>> = Mutex::new(None);

/// 热键监听器的生命周期句柄（start/stop 成对使用，可反复重建）
pub struct HotkeyListener {
    join: Option<JoinHandle<()>>,
    id_rx: Option<Receiver<u32>>,
    thread_id: Option<u32>,
}

/// Windows VK → 配置中使用的规范按键名。
/// 标点键统一存成未按 Shift 的基础字符，与键盘钩子看到的物理键保持一致。
pub fn vk_to_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 => format!("{}", vk - 0x30),
        0x41..=0x5A => format!("{}", (vk as u8 - 0x41 + b'a') as char),
        0x70 => "f1".into(),
        0x71 => "f2".into(),
        0x72 => "f3".into(),
        0x73 => "f4".into(),
        0x74 => "f5".into(),
        0x75 => "f6".into(),
        0x76 => "f7".into(),
        0x77 => "f8".into(),
        0x78 => "f9".into(),
        0x79 => "f10".into(),
        0x7A => "f11".into(),
        0x7B => "f12".into(),
        0x7C => "f13".into(),
        0x7D => "f14".into(),
        0x7E => "f15".into(),
        0x7F => "f16".into(),
        0x80 => "f17".into(),
        0x81 => "f18".into(),
        0x82 => "f19".into(),
        0x83 => "f20".into(),
        0x84 => "f21".into(),
        0x85 => "f22".into(),
        0x86 => "f23".into(),
        0x87 => "f24".into(),
        0x60 => "numpad0".into(),
        0x61 => "numpad1".into(),
        0x62 => "numpad2".into(),
        0x63 => "numpad3".into(),
        0x64 => "numpad4".into(),
        0x65 => "numpad5".into(),
        0x66 => "numpad6".into(),
        0x67 => "numpad7".into(),
        0x68 => "numpad8".into(),
        0x69 => "numpad9".into(),
        0x21 => "pageup".into(),
        0x22 => "pagedown".into(),
        0x23 => "end".into(),
        0x24 => "home".into(),
        0x25 => "left".into(),
        0x26 => "up".into(),
        0x27 => "right".into(),
        0x28 => "down".into(),
        0x2D => "insert".into(),
        0x2E => "delete".into(),
        0x6B => "+".into(),
        0x6D => "-".into(),
        0x6E => ".".into(),
        0x6F => "/".into(),
        0x20 => "space".into(),
        0x0D => "enter".into(),
        0x09 => "tab".into(),
        0x1B => "esc".into(),
        0x08 => "backspace".into(),
        0xBA => ";".into(),
        0xBB => "=".into(),
        0xBC => ",".into(),
        0xBD => "-".into(),
        0xBE => ".".into(),
        0xBF => "/".into(),
        0xC0 => "`".into(),
        0xDB => "[".into(),
        0xDC => "\\".into(),
        0xDD => "]".into(),
        0xDE => "'".into(),
        _ => format!("vk({vk})"),
    }
}

/// 按键自动重复过滤。
///
/// 按住热键时 Windows 会持续发送 WM_KEYDOWN：浮窗热键若不过滤，按住 Ctrl+Shift+F7
/// 会疯狂「显示/隐藏」，槽位热键也会被反复触发。这里按 vk 记录按下状态，
/// 只放行「真正的新按下」。
#[derive(Debug)]
pub struct KeyRepeatFilter {
    down: [bool; 256],
}

impl Default for KeyRepeatFilter {
    fn default() -> Self {
        Self { down: [false; 256] }
    }
}

impl KeyRepeatFilter {
    /// 返回 true 表示这是一次新的按下（不是按住后的自动重复）。
    pub fn on_keydown(&mut self, vk: u32) -> bool {
        let idx = (vk & 0xFF) as usize;
        if self.down[idx] {
            return false;
        }
        self.down[idx] = true;
        true
    }

    pub fn on_keyup(&mut self, vk: u32) {
        self.down[(vk & 0xFF) as usize] = false;
    }

    /// 清零（钩子重建时调用，避免状态残留）
    pub fn reset(&mut self) {
        self.down = [false; 256];
    }
}

static REPEAT_FILTER: Mutex<KeyRepeatFilter> = Mutex::new(KeyRepeatFilter { down: [false; 256] });

/// 当前按下的修饰键集合（低级键盘钩子里用 GetAsyncKeyState 读取）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
}

impl Modifiers {
    pub fn any(self) -> bool {
        self.ctrl || self.shift || self.alt || self.win
    }
}

fn key_down(vk: i32) -> bool {
    (unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000u16) != 0
}

/// 修饰键 + 主键 → 规范组合名（顺序固定 ctrl+shift+alt+win+key）。
pub fn combo_name(mods: Modifiers, key: &str) -> String {
    let mut parts: Vec<&str> = Vec::with_capacity(5);
    if mods.ctrl {
        parts.push("ctrl");
    }
    if mods.shift {
        parts.push("shift");
    }
    if mods.alt {
        parts.push("alt");
    }
    if mods.win {
        parts.push("win");
    }
    let mut out = parts.join("+");
    if !out.is_empty() {
        out.push('+');
    }
    out.push_str(key);
    out
}

/// 配置里的快捷键文本 → 规范组合名。
///
/// 支持 "Ctrl + Shift + F7"、"ctrl+shift+f7"、"F7"、"," 等写法；
/// 键名部分复用原有的别名归一化（comma → ","、pageup → "pageup" …）。
pub fn normalize_hotkey(raw: &str) -> String {
    let text = raw.trim();
    if text.is_empty() {
        return String::new();
    }
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut win = false;
    let mut key = String::new();
    for part in text.split('+') {
        let token = part.trim();
        if token.is_empty() {
            continue;
        }
        match token.to_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            "alt" => alt = true,
            "win" | "super" | "meta" => win = true,
            other => {
                // 主键：只保留最后一段（"+"/"=" 这类键名本身含加号时除外）
                if key.is_empty() {
                    key = normalize_key_name(other);
                } else {
                    key = format!("{key}+{}", normalize_key_name(other));
                }
            }
        }
    }
    if key.is_empty() {
        // 例如用户只写了 "Ctrl+Shift"：视为无效绑定
        return String::new();
    }
    combo_name(
        Modifiers {
            ctrl,
            shift,
            alt,
            win,
        },
        &key,
    )
}

/// 把用户手输的键名（Comma/Period/Slash 等）归一到 vk_to_name 使用的规范名。/// 把用户手输的键名（Comma/Period/Slash 等）归一到 vk_to_name 使用的规范名。
pub fn normalize_key_name(raw: &str) -> String {
    let key = raw.trim().to_lowercase();
    match key.as_str() {
        "comma" => ",".into(),
        "period" | "dot" => ".".into(),
        "slash" | "divide" => "/".into(),
        "semicolon" | "colon" => ";".into(),
        "minus" | "dash" => "-".into(),
        "plus" => "=".into(),
        "equals" | "equal" => "=".into(),
        "openbracket" | "leftbracket" => "[".into(),
        "closebracket" | "rightbracket" => "]".into(),
        "backslash" => "\\".into(),
        "backtick" | "grave" => "`".into(),
        "quote" | "apostrophe" => "'".into(),
        _ => key,
    }
}

impl HotkeyListener {
    /// 安装钩子并启动消息泵线程；线程 id 经内部通道回传，供 stop() 唤醒。
    pub fn start(
        hotkey_map: Arc<Mutex<HashMap<String, HotkeyAction>>>,
        tx: Sender<HotkeyAction>,
    ) -> Self {
        if let Ok(mut slot) = HOTKEY_STATE.lock() {
            *slot = Some(HotkeyShared {
                map: hotkey_map,
                tx,
            });
        }
        if let Ok(mut filter) = REPEAT_FILTER.lock() {
            filter.reset();
        }

        let (id_tx, id_rx) = std::sync::mpsc::channel();
        let join = thread::spawn(move || hook_thread(id_tx));
        Self {
            join: Some(join),
            id_rx: Some(id_rx),
            thread_id: None,
        }
    }

    /// 钩子是否已成功安装（安装失败时不发送线程 id；应用照常运行，只记录错误）。
    pub fn installed(&self) -> bool {
        self.thread_id.is_some()
    }

    /// 每帧调用一次：收集钩子线程 id（用于 stop 时唤醒）。
    pub fn poll(&mut self) {
        if self.thread_id.is_none() {
            if let Some(rx) = &self.id_rx {
                if let Ok(id) = rx.try_recv() {
                    self.thread_id = Some(id);
                }
            }
        }
    }

    /// 唤醒（WM_QUIT）、清理共享状态并回收钩子线程。
    pub fn stop(&mut self) {
        if self.thread_id.is_none() {
            if let Some(rx) = &self.id_rx {
                // 钩子线程在进入消息循环前就会发送 id；短暂等待兜底极端时序
                if let Ok(id) = rx.recv_timeout(std::time::Duration::from_millis(100)) {
                    self.thread_id = Some(id);
                }
            }
        }
        if let Some(id) = self.thread_id {
            unsafe {
                let _ = PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        if let Ok(mut slot) = HOTKEY_STATE.lock() {
            *slot = None;
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// 热更新热键映射（监听运行中修改 slot_hotkeys / listen_hotkey 后调用；未监听时为 no-op）。
pub fn update_map(new_map: &HashMap<String, HotkeyAction>) {
    if let Some(shared) = HOTKEY_STATE.lock().ok().and_then(|s| s.as_ref().cloned()) {
        if let Ok(mut map) = shared.map.lock() {
            map.clear();
            map.extend(new_map.iter().map(|(k, v)| (k.clone(), *v)));
        }
    }
}

/// 钩子线程主体：安装钩子 → 报告线程 id → 消息泵 → 卸载钩子。
fn hook_thread(id_tx: Sender<u32>) {
    let hook: HHOOK = match unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) } {
        Ok(h) => h,
        Err(e) => {
            // 安装失败：不发送 id，调用方判定启动失败
            eprintln!("SetWindowsHookExW failed: {e}");
            return;
        }
    };

    // 强制创建线程消息队列，保证 stop() 的 PostThreadMessage 不会因队列未创建而丢失
    let mut msg = MSG::default();
    unsafe {
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
    }
    let _ = id_tx.send(unsafe { GetCurrentThreadId() });

    loop {
        let ret = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if ret.0 == 0 || ret.0 == -1 {
            break; // WM_QUIT 或错误
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(hook);
    }
}

unsafe extern "system" fn hook_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    if code >= 0 {
        let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let vk = kbd.vkCode;
        let msg = wparam.0;
        // 回调运行在系统挂钩线程里：panic 绝不能逃逸出去 ——
        // 一旦逃逸，钩子线程会死掉，系统会静默摘除 WH_KEYBOARD_LL 钩子，
        // 表现为「所有全局热键突然全部失效，且没有任何报错」。
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle_key_event(msg, vk);
        }));
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// 键盘事件的匹配逻辑（与 FFI 外壳分离，便于 catch_unwind 包裹与后续测试）。
fn handle_key_event(msg: usize, vk: u32) {
    let is_keyup = msg == WM_KEYUP as usize || msg == WM_SYSKEYUP as usize;
    if is_keyup {
        if let Ok(mut filter) = REPEAT_FILTER.lock() {
            filter.on_keyup(vk);
        }
        return;
    }
    if msg != WM_KEYDOWN as usize && msg != WM_SYSKEYDOWN as usize {
        return;
    }
    // 自动重复的 WM_KEYDOWN 直接忽略（按住热键不会连续触发动作）
    let fresh = REPEAT_FILTER
        .lock()
        .map(|mut f| f.on_keydown(vk))
        .unwrap_or(false);
    if !fresh {
        return;
    }
    let mods = Modifiers {
        ctrl: key_down(0xA2) || key_down(0xA3),
        shift: key_down(0xA0) || key_down(0xA1),
        alt: key_down(0xA4) || key_down(0xA5),
        win: key_down(0x5B) || key_down(0x5C),
    };
    let key_name = vk_to_name(vk);

    // 匹配顺序（与原行为兼容）：
    //   1. 带修饰键的组合（如 ctrl+shift+f7）—— 配置里可以整键改绑；
    //   2. 无修饰键时的单键（原有槽位 / 监听开关热键），行为与以前完全一致；
    //   3. 未配置修饰键组合时，按住 ctrl/alt 的按键一律忽略（避免干扰游戏操作）。
    let Some(shared) = HOTKEY_STATE.lock().ok().and_then(|s| s.as_ref().cloned()) else {
        return;
    };
    let Ok(map) = shared.map.lock() else {
        return;
    };
    let action = if mods.any() {
        map.get(&combo_name(mods, &key_name)).copied()
    } else if !mods.ctrl && !mods.alt {
        map.get(&key_name).copied()
    } else {
        None
    };
    if let Some(a) = action {
        let _ = shared.tx.send(a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vk_names_digits_letters_and_fkeys() {
        assert_eq!(vk_to_name(0x30), "0");
        assert_eq!(vk_to_name(0x39), "9");
        assert_eq!(vk_to_name(0x41), "a");
        assert_eq!(vk_to_name(0x5A), "z");
        assert_eq!(vk_to_name(0x70), "f1");
        assert_eq!(vk_to_name(0x7B), "f12");
        assert_eq!(vk_to_name(0x7C), "f13");
        assert_eq!(vk_to_name(0x87), "f24");
        assert_eq!(vk_to_name(0x20), "space");
        assert_eq!(vk_to_name(0x1B), "esc");
        assert_eq!(vk_to_name(0x60), "numpad0");
        assert_eq!(vk_to_name(0x25), "left");
        assert_eq!(vk_to_name(0x26), "up");
        assert_eq!(vk_to_name(0x27), "right");
        assert_eq!(vk_to_name(0x28), "down");
        assert_eq!(vk_to_name(0x2D), "insert");
        assert_eq!(vk_to_name(0x2E), "delete");
        assert_eq!(vk_to_name(0x21), "pageup");
        assert_eq!(vk_to_name(0x22), "pagedown");
    }

    #[test]
    fn vk_names_punctuation_base_symbols() {
        assert_eq!(vk_to_name(0xBC), ",");
        assert_eq!(vk_to_name(0xBE), ".");
        assert_eq!(vk_to_name(0xBF), "/");
        assert_eq!(vk_to_name(0xBA), ";");
        assert_eq!(vk_to_name(0xBB), "=");
        assert_eq!(vk_to_name(0xBD), "-");
        assert_eq!(vk_to_name(0xC0), "`");
        assert_eq!(vk_to_name(0xDB), "[");
        assert_eq!(vk_to_name(0xDC), "\\");
        assert_eq!(vk_to_name(0xDD), "]");
        assert_eq!(vk_to_name(0xDE), "'");
        assert_eq!(vk_to_name(0x6F), "/");
    }

    #[test]
    fn repeat_filter_blocks_held_keys_only() {
        let mut filter = KeyRepeatFilter::default();
        // 首次按下放行
        assert!(filter.on_keydown(0x76));
        // 按住时的自动重复被忽略（否则浮窗会疯狂显示/隐藏）
        assert!(!filter.on_keydown(0x76));
        assert!(!filter.on_keydown(0x76));
        // 抬起 → 再次按下重新放行
        filter.on_keyup(0x76);
        assert!(filter.on_keydown(0x76));
        // 不同键互不影响
        assert!(filter.on_keydown(0x41));
        filter.reset();
        assert!(filter.on_keydown(0x76));
    }

    #[test]
    fn normalize_hotkey_supports_modifier_combos() {
        // 规范写法
        assert_eq!(normalize_hotkey("ctrl+shift+f7"), "ctrl+shift+f7");
        // 空格 / 大小写 / 别名
        assert_eq!(normalize_hotkey("Ctrl + Shift + F7"), "ctrl+shift+f7");
        assert_eq!(normalize_hotkey("SHIFT+CTRL+F8"), "ctrl+shift+f8");
        assert_eq!(normalize_hotkey("control+alt+f9"), "ctrl+alt+f9");
        assert_eq!(normalize_hotkey("win+space"), "win+space");
        assert_eq!(normalize_hotkey("Alt + Comma"), "alt+,");
        // 单键行为与旧版一致（原 normalize_key_name 的语义不变）
        assert_eq!(normalize_hotkey("F8"), "f8");
        assert_eq!(normalize_hotkey("comma"), ",");
        assert_eq!(normalize_hotkey(""), "");
        // 只有修饰键没有主键 → 视为未绑定
        assert_eq!(normalize_hotkey("ctrl+shift"), "");
    }

    #[test]
    fn combo_name_orders_modifiers_canonically() {
        let mods = Modifiers {
            ctrl: true,
            shift: true,
            alt: false,
            win: false,
        };
        assert_eq!(combo_name(mods, "f7"), "ctrl+shift+f7");
        assert_eq!(combo_name(Modifiers::default(), "f7"), "f7");
        assert!(mods.any());
        assert!(!Modifiers::default().any());
    }

    #[test]
    fn normalize_key_name_aliases() {
        assert_eq!(normalize_key_name("Comma"), ",");
        assert_eq!(normalize_key_name("Period"), ".");
        assert_eq!(normalize_key_name("slash"), "/");
        assert_eq!(normalize_key_name("semicolon"), ";");
        assert_eq!(normalize_key_name("F8"), "f8");
    }

    #[test]
    fn vk_names_unknown_falls_back() {
        assert_eq!(vk_to_name(0x1234), "vk(4660)");
    }
}
