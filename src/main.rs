// 绝地潜兵2 自动呼叫战备 — egui 沉浸式 HUD 界面
// Helldivers 2 Auto Stratagem Caller — Rust + egui
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod compact_view;
mod config;
mod executor;
mod hotkey;
mod icons;
mod main_view;
mod model;
mod plugin;
mod state;
mod stratagems;
mod theme;
mod ui;
mod widgets;
mod wiki_fetcher;

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use config::{list_profiles, load_config, save_config, SLOT_COUNT};
use eframe::egui::{self, Context, Pos2};
use icons::IconStore;
use state::{
    AppModel, CaptureState, CreatorState, LibraryState, PluginData, WikiState,
};
use stratagems::{get_categories, STRATAGEMS};

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

/// 库行右键菜单状态
#[derive(Clone)]
pub struct LibraryContext {
    pub name: String,
    pub category: String,
    pub icon_key: String,
    pub command: Vec<String>,
    pub description: String,
    pub is_plugin: bool,
    pub pos: Pos2,
}

/// 战备设置弹窗状态
pub struct StratagemSettings {
    pub visible: bool,
    pub name: String,
    pub icon_key: String,
    pub command_text: String,
    pub description: String,
    pub category: String,
    pub is_plugin: bool,
    pub original_name: String,
}

pub struct H2ACApp {
    pub model: AppModel,
    pub library: LibraryState,
    pub capture: CaptureState,
    pub plugins: PluginData,
    pub wiki: WikiState,
    pub creator: CreatorState,
    pub logs: VecDeque<LogEntry>,
    pub show_settings: bool,
    pub settings_bindings: HashMap<String, String>,
    pub settings_key: String,
    pub settings_delay: f64,
    pub settings_pre_delay: f64,
    pub context: Option<ContextState>,
    pub library_context: Option<LibraryContext>,
    pub stratagem_settings: StratagemSettings,
    pub hotkey_rx: Option<mpsc::Receiver<usize>>,
    pub hotkey_run: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl H2ACApp {
    fn new(ctx: &Context) -> Self {
        theme::install_fonts(ctx);
        theme::apply_style(ctx);

        let config = load_config();
        let slots = config.loadout.clone();
        let listening = config.listening_enabled;
        let categories: Vec<String> = get_categories().iter().map(|s| s.to_string()).collect();
        let profile_names = list_profiles();
        let icons = IconStore::load(ctx);

        plugin::create_example_plugin();
        let (plugin_stratagems, plugin_themes) = plugin::load_all();

        let mut logs = VecDeque::new();
        logs.push_back(LogEntry {
            time: now_hms(),
            text: "终端就绪 — 点选槽位待命，从战备库装入".into(),
            kind: LogKind::Info,
        });

        let model = AppModel {
            slots,
            plugin_slots: HashMap::new(),
            armed: None,
            detail_slot: None,
            listening,
            config,
            compact: false,
            flash: HashMap::new(),
            icons,
            debug_mode: false,
            profile_names,
            current_profile: String::new(),
            save_profile_name: String::new(),
            scale: 1.0,
            metrics: theme::UiMetrics::new(1.0),
        };

        let library = LibraryState {
            lib_category: categories.first().cloned().unwrap_or_default(),
            lib_search: String::new(),
            categories,
        };

        let mut app = Self {
            model,
            library,
            capture: CaptureState::default(),
            plugins: PluginData { stratagems: plugin_stratagems, themes: plugin_themes },
            wiki: WikiState { fetch_rx: None, fetch_status: String::new(), cache_exists: false },
            creator: CreatorState::default(),
            logs,
            show_settings: false,
            settings_bindings: HashMap::new(),
            settings_key: String::new(),
            settings_delay: 0.05,
            settings_pre_delay: 0.12,
            context: None,
            library_context: None,
            stratagem_settings: StratagemSettings { visible: false, name: String::new(), icon_key: String::new(), command_text: String::new(), description: String::new(), category: String::new(), is_plugin: false, original_name: String::new() },
            hotkey_rx: None,
            hotkey_run: None,
        };

        if app.model.listening {
            app.start_hotkeys();
        }
        app
    }

    pub fn log(&mut self, kind: LogKind, text: impl Into<String>) {
        self.logs.push_back(LogEntry { time: now_hms(), text: text.into(), kind });
        while self.logs.len() > 32 {
            self.logs.pop_front();
        }
    }

    /// 调试日志（写入 panic.log 同目录的 debug.log，同时入应用日志）
    pub fn debug(&mut self, text: impl Into<String>) {
        let msg = text.into();
        let line = format!("{} DBG {}", now_hms(), msg);
        eprintln!("{line}");
        let dir = std::env::current_exe()
            .ok().and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_default();
        let _ = std::fs::write(dir.join("debug.log"), format!("{line}\n"));
        // append to existing log
        let path = dir.join("debug.log");
        if path.exists() {
            let _ = std::fs::OpenOptions::new().append(true).open(&path)
                .map(|mut f| std::io::Write::write_all(&mut f, format!("{line}\n").as_bytes()));
        }
        self.log(LogKind::Info, format!("[DBG] {msg}"));
    }

    pub fn open_settings(&mut self) {
        self.settings_bindings = self.model.config.key_bindings.clone();
        self.settings_key = self.model.config.stratagem_key.clone();
        self.settings_delay = self.model.config.key_delay;
        self.settings_pre_delay = self.model.config.pre_delay;
        self.show_settings = true;
    }

    pub fn set_compact(&mut self, ctx: &Context, compact: bool) {
        self.model.compact = compact;
        self.context = None;
        if compact {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(
                theme::COMPACT_DESIGN_W, theme::COMPACT_DESIGN_H,
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(theme::DESIGN_W, theme::DESIGN_H)));
        }
    }

    fn start_hotkeys(&mut self) {
        let map: HashMap<String, usize> = self.model.config.slot_hotkeys.iter()
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
        if let Some(ref r) = self.hotkey_run { r.store(false, std::sync::atomic::Ordering::Relaxed); }
        self.hotkey_rx = None;
        self.hotkey_run = None;
    }

    pub fn toggle_listening(&mut self) {
        self.model.listening = !self.model.listening;
        if self.model.listening {
            self.start_hotkeys();
            self.log(LogKind::Info, "热键监听已开启");
        } else {
            self.stop_hotkeys();
            self.log(LogKind::Warn, "热键监听已关闭");
        }
        self.model.config.listening_enabled = self.model.listening;
        save_config(&self.model.config);
    }

    pub fn refresh_profiles(&mut self) {
        self.model.profile_names = list_profiles();
    }

}

impl eframe::App for H2ACApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // 计算当前窗口对应的缩放比例
        let sr = ctx.screen_rect();
        let (dw, dh) = if self.model.compact {
            (theme::COMPACT_DESIGN_W, theme::COMPACT_DESIGN_H)
        } else {
            (theme::DESIGN_W, theme::DESIGN_H)
        };
        let scale = (sr.width() / dw).min(sr.height() / dh).max(0.25);

        if (scale - self.model.scale).abs() > 0.001 {
            self.model.scale = scale;
            self.model.metrics = theme::UiMetrics::new(scale);
            ctx.set_style(theme::apply_scaled(&self.model.metrics));
        }

        if let Some(ref rx) = self.hotkey_rx {
            if let Ok(s) = rx.try_recv() { self.execute_slot(s); }
        }
        let now = ctx.input(|i| i.time);
        for v in self.model.flash.values_mut() {
            if *v == 0.0 { *v = now; }
        }
        if let Some(rx) = self.wiki.fetch_rx.take() {
            let mut still_active = true;
            while let Ok(progress) = rx.try_recv() {
                self.wiki.fetch_status = progress.stage.clone();
                if progress.done {
                    still_active = false;
                    if let Some(Ok(new_items)) = progress.result {
                        // 差集比对：只保留指令序列不在内置数据库中出现的
                        let mut truly_new: Vec<crate::stratagems::PluginStratagem> = Vec::new();
                        for item in &new_items {
                            let cmd_str = item.command.join(",");
                            let exists = STRATAGEMS.iter().any(|bs| {
                                let bs_cmd: String = bs.command.iter().map(|d| {
                                    match *d { "↑" => "up", "↓" => "down", "←" => "left", "→" => "right", _ => *d }
                                }).collect::<Vec<_>>().join(",");
                                bs_cmd == cmd_str
                            });
                            if !exists { truly_new.push(item.clone()); }
                        }
                        let new_count = truly_new.len();
                        // 统一分类为 "NEW (Wiki)"
                        for item in &mut truly_new { item.category = "NEW (Wiki)".into(); }
                        // 注入运行时（此时分类已统一）
                        self.plugins.stratagems.retain(|p| !p.name.ends_with("(Wiki)"));
                        self.plugins.stratagems.extend(truly_new.clone());
                        // 写入 _wiki_new.json
                        if new_count > 0 {
                            let manifest = crate::stratagems::PluginManifest {
                                id: "_wiki_new".into(),
                                name: "Wiki 新增战备".into(),
                                enabled: true,
                                stratagems: truly_new,
                                themes: Vec::new(),
                            };
                            if let Ok(json) = serde_json::to_string_pretty(&manifest) {
                                let dir = plugin::plugins_dir();
                                let _ = std::fs::create_dir_all(&dir);
                                let _ = std::fs::write(dir.join("_wiki_new.json"), json);
                            }
                        }
                        self.wiki.cache_exists = true;
                        self.log(LogKind::Info, format!("Wiki 拉取完成，新增 {} 条 → plugins/_wiki_new.json", new_count));
                    } else {
                        self.log(LogKind::Warn, "Wiki 数据拉取失败，请检查网络");
                    }
                }
            }
            if still_active { self.wiki.fetch_rx = Some(rx); }
        }

        if self.model.compact {
            self.show_compact(ctx);
        } else {
            main_view::show_main(self, ctx);
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let msg = format!("PANIC: {info}\n{bt}\n");
        let dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let _ = std::fs::write(dir.join("panic.log"), msg);
    }));

    let is_admin = unsafe { windows::Win32::UI::Shell::IsUserAnAdmin().as_bool() };
    if !is_admin {
        let dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let _ = std::fs::write(dir.join("admin_warning.txt"),
            "未以管理员身份运行。如果游戏内按键无反应，请右键 h2ac-rs.exe → 以管理员身份运行。");
    }

    // 加载应用图标
    let icon = {
        let icon_bytes = include_bytes!("../assets/icon-removebg.png");
        let img = image::load_from_memory(icon_bytes)
            .ok()
            .map(|i| i.to_rgba8());
        img.map(|rgba| {
            let (w, h) = (rgba.width() as u32, rgba.height() as u32);
            egui::IconData {
                rgba: rgba.into_raw(),
                width: w,
                height: h,
            }
        })
    };

    eframe::run_native(
        "H2AC-RS",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([theme::DESIGN_W, theme::DESIGN_H])
                .with_resizable(true)
                .with_decorations(false)
                .with_title("H2AC-RS 绝地潜兵2 战备终端")
                .with_icon(icon.map(std::sync::Arc::new).unwrap_or_default()),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(H2ACApp::new(&cc.egui_ctx)))),
    )
}
