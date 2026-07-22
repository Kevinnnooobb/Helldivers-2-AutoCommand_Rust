// 绝地潜兵2 自动呼叫战备 — egui 沉浸式 HUD 界面
// Helldivers 2 Auto Stratagem Caller — Rust + egui
// 布局：无边框终端 — 顶栏 → 槽位网格+详情条 / 常显战备库 → 日志底栏；紧凑迷你条模式
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod compact_view;
mod config;
mod executor;
mod hotkey;
mod icons;
mod main_view;
mod stratagems;
mod theme;
mod widgets;

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use config::{list_profiles, load_config, save_config, Config, SLOT_COUNT};
use eframe::egui::{self, Context, Pos2};
use executor::execute_stratagem;
use icons::IconStore;
use stratagems::{command_to_string, get_categories, Stratagem, STRATAGEMS};
use theme::{MAIN_H, MAIN_W};

// ─── 日志 ───

#[derive(Clone, Copy, PartialEq)]
pub enum LogKind {
    Info,
    Exec,
    Warn,
}

pub struct LogEntry {
    pub time: String,
    pub text: String,
    pub kind: LogKind,
}

fn now_hms() -> String {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let st = unsafe { GetLocalTime() };
    format!("{:02}:{:02}:{:02}", st.wHour, st.wMinute, st.wSecond)
}

// ─── 应用状态 ───

#[derive(Clone)]
pub struct ContextState {
    pub slot: usize,
    pub pos: Pos2,
}

pub struct H2ACApp {
    pub config: Config,
    pub categories: Vec<&'static str>,
    pub slots: Vec<Option<usize>>,
    pub listening: bool,
    pub profile_names: Vec<String>,
    pub current_profile: String,
    pub save_profile_name: String,
    pub logs: VecDeque<LogEntry>,
    pub show_settings: bool,
    pub settings_bindings: HashMap<String, String>,
    pub settings_key: String,
    pub settings_delay: f64,
    /// 待命槽位（等待从战备库装入）
    pub armed: Option<usize>,
    /// 详情条展示的槽位
    pub detail_slot: Option<usize>,
    pub lib_category: String,
    pub lib_search: String,
    pub context: Option<ContextState>,
    pub capturing: Option<usize>,
    pub captured: String,
    pub hotkey_rx: Option<mpsc::Receiver<usize>>,
    pub hotkey_run: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub icons: IconStore,
    pub compact: bool,
    /// 执行闪光：slot → 触发时刻（egui time）
    pub flash: HashMap<usize, f64>,
}

impl H2ACApp {
    fn new(ctx: &Context) -> Self {
        theme::install_fonts(ctx);
        theme::apply_style(ctx);

        let config = load_config();
        let slots = config.loadout.clone();
        let listening = config.listening_enabled;
        let categories = get_categories();
        let profile_names = list_profiles();
        let icons = IconStore::load(ctx);

        let mut logs = VecDeque::new();
        logs.push_back(LogEntry {
            time: now_hms(),
            text: "终端就绪 — 点选槽位待命，从战备库装入".into(),
            kind: LogKind::Info,
        });

        let mut app = Self {
            lib_category: categories.first().unwrap_or(&"").to_string(),
            config,
            categories,
            slots,
            listening,
            profile_names,
            current_profile: String::new(),
            save_profile_name: String::new(),
            logs,
            show_settings: false,
            settings_bindings: HashMap::new(),
            settings_key: String::new(),
            settings_delay: 0.05,
            armed: None,
            detail_slot: None,
            lib_search: String::new(),
            context: None,
            capturing: None,
            captured: String::new(),
            hotkey_rx: None,
            hotkey_run: None,
            icons,
            compact: false,
            flash: HashMap::new(),
        };

        if app.listening {
            app.start_hotkeys();
        }
        app
    }

    pub fn log(&mut self, kind: LogKind, text: impl Into<String>) {
        self.logs.push_back(LogEntry {
            time: now_hms(),
            text: text.into(),
            kind,
        });
        while self.logs.len() > 32 {
            self.logs.pop_front();
        }
    }

    pub fn execute_slot(&mut self, slot: usize) {
        if let Some(idx) = self.slots[slot] {
            if let Some(s) = STRATAGEMS.get(idx) {
                self.log(
                    LogKind::Exec,
                    format!("执行 {} [{}] {}", s.name, s.model, command_to_string(&s.command)),
                );
                self.flash.insert(slot, 0.0); // 占位，update 中写入当前时刻
                let sc = s.clone();
                let cg = self.config.clone();
                thread::spawn(move || execute_stratagem(&sc, &cg));
            }
        }
    }

    /// 战备库点击装入：装入待命槽位并自动推进
    pub fn assign_stratagem(&mut self, s: &'static Stratagem) {
        let Some(slot) = self.armed else {
            self.log(LogKind::Info, format!("先点选一个槽位，再装入 {}", s.name));
            return;
        };
        if let Some(idx) = STRATAGEMS.iter().position(|x| x.name == s.name && x.model == s.model) {
            self.set_slot(slot, idx);
            self.log(
                LogKind::Info,
                format!("槽位 {} ← {} [{}]", slot + 1, s.name, s.model),
            );
            // 自动推进到下一个空槽位
            self.armed = (0..SLOT_COUNT)
                .map(|i| (slot + 1 + i) % SLOT_COUNT)
                .find(|&i| self.slots[i].is_none());
            if let Some(a) = self.armed {
                self.detail_slot = Some(a);
            } else {
                self.detail_slot = Some(slot);
            }
        }
    }

    pub fn open_settings(&mut self) {
        self.settings_bindings = self.config.key_bindings.clone();
        self.settings_key = self.config.stratagem_key.clone();
        self.settings_delay = self.config.key_delay;
        self.show_settings = true;
    }

    pub fn set_compact(&mut self, ctx: &Context, compact: bool) {
        self.compact = compact;
        self.context = None;
        if compact {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(
                compact_view::compact_width(),
                theme::COMPACT_H,
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::AlwaysOnTop,
            ));
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(
                MAIN_W, MAIN_H,
            )));
        }
    }

    fn start_hotkeys(&mut self) {
        let map: HashMap<String, usize> = self
            .config
            .slot_hotkeys
            .iter()
            .filter_map(|(k, v)| Some((v.clone(), k.parse::<usize>().ok()?)))
            .filter(|(_, s)| *s < SLOT_COUNT)
            .collect();
        let map = Arc::new(Mutex::new(map));
        let (tx, rx) = mpsc::channel();
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        hotkey::start(map, tx, running.clone());
        self.hotkey_rx = Some(rx);
        self.hotkey_run = Some(running);
    }

    fn stop_hotkeys(&mut self) {
        if let Some(ref r) = self.hotkey_run {
            r.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        self.hotkey_rx = None;
        self.hotkey_run = None;
    }

    pub fn toggle_listening(&mut self) {
        self.listening = !self.listening;
        if self.listening {
            self.start_hotkeys();
            self.log(LogKind::Info, "热键监听已开启");
        } else {
            self.stop_hotkeys();
            self.log(LogKind::Warn, "热键监听已关闭");
        }
        self.config.listening_enabled = self.listening;
        save_config(&self.config);
    }

    pub fn refresh_profiles(&mut self) {
        self.profile_names = list_profiles();
    }

    pub fn set_slot(&mut self, slot: usize, idx: usize) {
        self.slots[slot] = Some(idx);
        self.config.loadout[slot] = Some(idx);
        save_config(&self.config);
    }

    pub fn clear_slot(&mut self, slot: usize) {
        self.slots[slot] = None;
        self.config.loadout[slot] = None;
        self.config.slot_hotkeys.remove(&slot.to_string());
        save_config(&self.config);
    }
}

impl eframe::App for H2ACApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // 全局热键事件 → 执行 + 闪光（记录真实触发时刻）
        if let Some(ref rx) = self.hotkey_rx {
            if let Ok(s) = rx.try_recv() {
                self.execute_slot(s);
            }
        }
        // 把 execute_slot 占位的闪光时间戳更新为当前时刻
        let now = ctx.input(|i| i.time);
        for v in self.flash.values_mut() {
            if *v == 0.0 {
                *v = now;
            }
        }

        if self.compact {
            self.show_compact(ctx);
        } else {
            self.show_main(ctx);
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    // 调试期：panic 写入文件（无控制台也能捕获）
    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let msg = format!("PANIC: {info}\n{bt}\n");
        let dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let _ = std::fs::write(dir.join("panic.log"), msg);
    }));

    eframe::run_native(
        "H2AC-RS",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([MAIN_W, MAIN_H])
                .with_resizable(false)
                .with_decorations(false)
                .with_title("H2AC-RS 绝地潜兵2 战备终端"),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(H2ACApp::new(&cc.egui_ctx)))),
    )
}
