// 绝地潜兵2 自动呼叫战备 — egui 沉浸式 HUD 界面
// Helldivers 2 Auto Stratagem Caller — Rust + egui
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod compact_view;
mod config;
mod executor;
mod hotkey;
mod icons;
mod main_view;
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
use std::thread;

use config::{list_profiles, load_config, save_config, Config, SLOT_COUNT};
use eframe::egui::{self, Context, Pos2};
use executor::execute_stratagem;
use icons::IconStore;
use state::{
    AppModel, CaptureState, CreatorState, LibraryState, PluginData, WikiState,
};
use stratagems::{command_to_string, dir_to_arrow, get_categories, OwnedStratagem, Stratagem, StratagemRef, STRATAGEMS};
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
            profile_names,
            current_profile: String::new(),
            save_profile_name: String::new(),
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

    pub fn execute_slot(&mut self, slot: usize) {
        if let Some(p) = self.model.plugin_slots.get(&slot) {
            let log_msg = format!("执行 {} [{}] {}", p.name, p.model, p.command.join(""));
            let cmd = p.command.clone();
            self.log(LogKind::Exec, log_msg);
            self.model.flash.insert(slot, 0.0);
            let cg = self.model.config.clone();
            thread::spawn(move || executor::execute_plugin(&cg, &cmd));
            return;
        }
        if let Some(idx) = self.model.slots[slot] {
            if idx != usize::MAX {
                if let Some(s) = STRATAGEMS.get(idx) {
                    self.log(LogKind::Exec, format!("执行 {} [{}] {}", s.name, s.model, command_to_string(&s.command)));
                    self.model.flash.insert(slot, 0.0);
                    let sc = s.clone();
                    let cg = self.model.config.clone();
                    thread::spawn(move || execute_stratagem(&sc, &cg));
                }
            }
        }
    }

    pub fn assign_stratagem(&mut self, s: &'static Stratagem) {
        let Some(slot) = self.model.armed else {
            self.log(LogKind::Info, format!("先点选一个槽位，再装入 {}", s.name));
            return;
        };
        if let Some(idx) = STRATAGEMS.iter().position(|x| x.name == s.name && x.model == s.model) {
            self.set_slot(slot, idx);
            self.log(LogKind::Info, format!("槽位 {} ← {} [{}]", slot + 1, s.name, s.model));
            self.model.armed = (0..SLOT_COUNT)
                .map(|i| (slot + 1 + i) % SLOT_COUNT)
                .find(|&i| self.model.slots[i].is_none());
            if let Some(a) = self.model.armed {
                self.model.detail_slot = Some(a);
            } else {
                self.model.detail_slot = Some(slot);
            }
        }
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
                compact_view::compact_width(), theme::COMPACT_H,
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(egui::WindowLevel::Normal));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(MAIN_W, MAIN_H)));
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

    pub fn set_slot(&mut self, slot: usize, idx: usize) {
        self.model.slots[slot] = Some(idx);
        self.model.config.loadout[slot] = Some(idx);
        save_config(&self.model.config);
    }

    pub fn clear_slot(&mut self, slot: usize) {
        self.model.slots[slot] = None;
        self.model.plugin_slots.remove(&slot);
        self.model.config.loadout[slot] = None;
        self.model.config.slot_hotkeys.remove(&slot.to_string());
        save_config(&self.model.config);
    }

    /// 统合内置 + 插件战备的分类列表
    pub fn lib_categories(&self) -> Vec<String> {
        let mut cats: Vec<String> = self.library.categories.clone();
        for p in &self.plugins.stratagems {
            if !cats.contains(&p.category) { cats.push(p.category.clone()); }
        }
        // 覆盖目标分类也加入列表
        for cat in self.model.config.category_overrides.values() {
            if !cats.contains(cat) { cats.push(cat.clone()); }
        }
        cats
    }

    /// 查询战备的有效分类（覆盖优先）
    pub fn effective_category(&self, name: &str, default_cat: &str) -> String {
        self.model.config.category_overrides.get(name)
            .cloned()
            .unwrap_or_else(|| default_cat.to_string())
    }

    /// 设置战备分类覆盖并持久化
    pub fn set_category_override(&mut self, name: &str, category: &str) {
        self.model.config.category_overrides.insert(name.to_string(), category.to_string());
        save_config(&self.model.config);
        // 同步到插件 JSON 文件（如果是插件战备）
        self.sync_plugin_category(name, category);
    }

    /// 清除覆盖
    pub fn clear_category_override(&mut self, name: &str) {
        self.model.config.category_overrides.remove(name);
        save_config(&self.model.config);
    }

    /// 同步插件 JSON 文件中该战备的分类
    fn sync_plugin_category(&mut self, name: &str, new_cat: &str) {
        for p in &mut self.plugins.stratagems {
            if p.name == name || p.name == format!("{name} (Wiki)") {
                p.category = new_cat.to_string();
                // 尝试写回源 JSON 文件
                let dir = plugin::plugins_dir();
                let _ = std::fs::create_dir_all(&dir);
                // 搜索匹配的 JSON 文件
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().map_or(true, |e| e != "json") { continue; }
                        if let Ok(data) = std::fs::read_to_string(&path) {
                            if data.contains(&format!("\"name\": \"{}\"", p.name)) {
                                if let Ok(mut m) = serde_json::from_str::<crate::stratagems::PluginManifest>(&data) {
                                    for s in &mut m.stratagems {
                                        if s.name == p.name { s.category = new_cat.to_string(); }
                                    }
                                }
                            }
                        }
                    }
                }
                break;
            }
        }
    }

    pub fn lib_by_category(&self, cat: &str) -> Vec<StratagemRef> {
        let mut out: Vec<StratagemRef> = stratagems::get_by_category(cat)
            .into_iter().map(StratagemRef::Base).collect();
        for p in &self.plugins.stratagems {
            if p.category == cat { out.push(StratagemRef::Plugin(p)); }
        }
        out
    }

    pub fn lib_by_category_owned(&self, cat: &str) -> Vec<OwnedStratagem> {
        let mut out: Vec<OwnedStratagem> = stratagems::get_by_category(cat)
            .into_iter().map(OwnedStratagem::Base).collect();
        for p in &self.plugins.stratagems {
            if p.category == cat { out.push(OwnedStratagem::Plugin(p.clone())); }
        }
        out
    }

    pub fn lib_search(&self, query: &str) -> Vec<StratagemRef> {
        let q = query.trim();
        if q.is_empty() { return Vec::new(); }
        let mut out: Vec<StratagemRef> = stratagems::search(q)
            .into_iter().map(StratagemRef::Base).collect();
        for p in &self.plugins.stratagems {
            if p.name.contains(q) || p.model.contains(q) { out.push(StratagemRef::Plugin(p)); }
        }
        out
    }

    pub fn lib_search_owned(&self, query: &str) -> Vec<OwnedStratagem> {
        let q = query.trim();
        if q.is_empty() { return Vec::new(); }
        let mut out: Vec<OwnedStratagem> = stratagems::search(q)
            .into_iter().map(OwnedStratagem::Base).collect();
        for p in &self.plugins.stratagems {
            if p.name.contains(q) || p.model.contains(q) { out.push(OwnedStratagem::Plugin(p.clone())); }
        }
        out
    }

    pub fn assign_stratagem_ref(&mut self, s: &StratagemRef) {
        match s {
            StratagemRef::Base(base) => self.assign_stratagem(base),
            StratagemRef::Plugin(p) => {
                let Some(slot) = self.model.armed else {
                    self.log(LogKind::Info, format!("先点选槽位再装入: {}", p.name));
                    return;
                };
                self.model.plugin_slots.insert(slot, (*p).clone());
                self.model.slots[slot] = Some(usize::MAX);
                self.log(LogKind::Info, format!("槽位 {} <- {} (插件)", slot + 1, p.name));
                self.model.armed = (0..SLOT_COUNT)
                    .map(|i| (slot + 1 + i) % SLOT_COUNT)
                    .find(|&i| self.model.slots[i].is_none() && !self.model.plugin_slots.contains_key(&i));
                if let Some(a) = self.model.armed { self.model.detail_slot = Some(a); }
                else { self.model.detail_slot = Some(slot); }
            }
        }
    }

    /// 删除插件战备并同步 JSON
    pub fn delete_plugin_stratagem(&mut self, name: &str) {
        self.plugins.stratagems.retain(|p| p.name != name);
        let dir = plugin::plugins_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(true, |e| e != "json") { continue; }
                if let Ok(data) = std::fs::read_to_string(&path) {
                    if data.contains(&format!("\"name\": \"{}\"", name)) {
                        if let Ok(mut m) = serde_json::from_str::<crate::stratagems::PluginManifest>(&data) {
                            let before = m.stratagems.len();
                            m.stratagems.retain(|s| s.name != name);
                            if m.stratagems.len() < before {
                                let _ = std::fs::write(&path, serde_json::to_string_pretty(&m).unwrap_or_default());
                            }
                        }
                    }
                }
            }
        }
        self.log(LogKind::Warn, format!("已删除: {name}"));
    }

    /// 槽位是否已装填（内置或插件）
    pub fn slot_filled(&self, idx: usize) -> bool {
        self.model.slots[idx].is_some() || self.model.plugin_slots.contains_key(&idx)
    }

    /// 获取槽位战备名（插件优先）
    pub fn slot_name(&self, idx: usize) -> Option<String> {
        if let Some(p) = self.model.plugin_slots.get(&idx) { Some(p.name.clone()) }
        else { self.model.slots[idx].and_then(|si| if si == usize::MAX { None } else { STRATAGEMS.get(si).map(|s| s.name.to_string()) }) }
    }

    pub fn slot_icon(&self, idx: usize) -> Option<&str> {
        if let Some(p) = self.model.plugin_slots.get(&idx) { Some(&p.icon) }
        else { self.model.slots[idx].and_then(|si| if si == usize::MAX { None } else { STRATAGEMS.get(si).map(|s| s.icon) }) }
    }

    pub fn slot_command(&self, idx: usize) -> Vec<&str> {
        if let Some(p) = self.model.plugin_slots.get(&idx) { p.command.iter().map(|c| dir_to_arrow(c.as_str())).collect() }
        else { self.model.slots[idx].and_then(|si| if si == usize::MAX { None } else { STRATAGEMS.get(si).map(|s| s.command.to_vec()) }).unwrap_or_default() }
    }

    pub fn slot_category(&self, idx: usize) -> Option<String> {
        if let Some(p) = self.model.plugin_slots.get(&idx) { Some(p.category.clone()) }
        else { self.model.slots[idx].and_then(|si| if si == usize::MAX { None } else { STRATAGEMS.get(si).map(|s| s.category.to_string()) }) }
    }

    pub fn start_wiki_fetch(&mut self) {
        let (rx, has_cache) = wiki_fetcher::start_fetch();
        self.wiki.fetch_rx = Some(rx);
        self.wiki.cache_exists = has_cache;
        self.wiki.fetch_status = "连接中…".into();
    }

    pub fn load_profile_data(&mut self, name: &str) {
        if let Some(pr) = config::load_profile(name) {
            self.model.slots = pr.loadout;
            self.model.plugin_slots = pr.plugin_slots.iter().map(|(k,v)| (k.parse().unwrap_or(0), v.clone())).collect();
            self.model.config.slot_hotkeys = pr.slot_hotkeys;
            self.model.config.last_profile = name.to_string();
            config::save_config(&self.model.config);
            self.model.current_profile = name.to_string();
            self.model.armed = None;
            self.model.detail_slot = None;
            self.log(LogKind::Info, format!("已加载 Profile: {name}"));
        }
    }
}

impl eframe::App for H2ACApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
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
