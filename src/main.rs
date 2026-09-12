// 绝地潜兵2 自动呼叫战备 — egui 沉浸式 HUD 界面
// Helldivers 2 Auto Stratagem Caller — Rust + egui
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_events;
mod assets;
mod compact_mode;
mod config;
mod executor;
mod hotkey;
mod icon_fetch;
mod icons;
mod image_rect;
mod item;
mod loadout_sync;
mod main_view;
mod model;
mod overlay_win;
mod plugin;
mod preset;
mod state;
mod stratagems;
mod theme;
mod ui;
mod util;
mod vision;
mod widgets;
mod wiki_fetcher;

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc;
use std::sync::{Arc, LazyLock, Mutex};

use config::{list_profiles, load_config, save_config, SLOT_COUNT};
use eframe::egui::{self, Context, Pos2};
use icons::IconStore;
use state::{AppModel, CaptureState, CreatorState, LibraryState, PluginData, WikiState};
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

/// 内置战备指令签名（英文方向 join），Wiki 差集比对用 O(1) 成员判定
static BUILTIN_SIGNATURES: LazyLock<std::collections::HashSet<String>> = LazyLock::new(|| {
    STRATAGEMS
        .iter()
        .map(|bs| {
            bs.command
                .iter()
                .map(|d| match *d {
                    "↑" => "up",
                    "↓" => "down",
                    "←" => "left",
                    "→" => "right",
                    _ => *d,
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect()
});

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
    pub settings_listen_hotkey: String,
    /// 设置面板中的 Loadout Sync 配置镜像（保存时写回 config）
    pub settings_loadout_sync_hotkey: String,
    pub settings_compact_toggle: String,
    /// 全局取消装配快捷键（属于 Loadout Sync，不属于紧凑模式）
    pub settings_cancel_hotkey: String,
    pub settings_compact_opacity: f32,
    pub settings_allow_overwrite: bool,
    pub settings_debug_shots: bool,
    pub context: Option<ContextState>,
    pub library_context: Option<LibraryContext>,
    pub stratagem_settings: StratagemSettings,
    pub hotkey_rx: Option<mpsc::Receiver<hotkey::HotkeyAction>>,
    pub hotkey: Option<hotkey::HotkeyListener>,
    /// 图标在线补齐任务：worker 线程逐条回传结果，主线程注册纹理 + 落盘
    pub icon_rx: Option<mpsc::Receiver<IconJobResult>>,
    pub icon_jobs_total: usize,
    pub icon_jobs_done: usize,
    pub icon_jobs_ok: usize,
    /// 分层窗口（透明度）是否可用
    pub overlay_window_ready: bool,
    /// 热键钩子启动时刻（用于延迟判定「注册失败」）
    pub hotkey_started_at: Option<std::time::Instant>,
    /// 是否已就注册失败写过日志（只写一次）
    pub hotkey_failure_logged: bool,
}

/// 图标补齐单条任务结果（后台线程 → 主线程）
pub struct IconJobResult {
    pub key: String,
    pub png: Option<Vec<u8>>,
    pub error: Option<String>,
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
        let plugin_stratagems = plugin::load_all();

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
            view_mode: state::ViewMode::Main,
            compact_preset: crate::compact_mode::CompactPresetState::default(),
            flash: HashMap::new(),
            icons,
            debug_mode: false,
            profile_names,
            current_profile: String::new(),
            save_profile_name: String::new(),
            scale: 1.0,
            metrics: theme::UiMetrics::new(1.0),
            loadout_sync: crate::loadout_sync::state::LoadoutSyncHandle::new(),
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
            plugins: PluginData {
                stratagems: plugin_stratagems,
            },
            wiki: WikiState {
                fetch_rx: None,
                fetch_status: String::new(),
                cache_exists: plugin::wiki_plugin_path().exists(),
            },
            creator: CreatorState::default(),
            logs,
            show_settings: false,
            settings_bindings: HashMap::new(),
            settings_key: String::new(),
            settings_delay: 0.05,
            settings_pre_delay: 0.12,
            settings_listen_hotkey: String::new(),
            settings_loadout_sync_hotkey: String::new(),
            settings_compact_toggle: crate::compact_mode::config::CompactModeConfig::default()
                .toggle_hotkey,
            settings_cancel_hotkey: config::Config::default().loadout_sync_cancel_hotkey,
            settings_compact_opacity: crate::compact_mode::config::CompactModeConfig::default()
                .opacity,
            settings_allow_overwrite: false,
            settings_debug_shots: false,
            context: None,
            library_context: None,
            stratagem_settings: StratagemSettings {
                visible: false,
                name: String::new(),
                icon_key: String::new(),
                command_text: String::new(),
                description: String::new(),
                category: String::new(),
                is_plugin: false,
                original_name: String::new(),
            },
            hotkey_rx: None,
            hotkey: None,
            icon_rx: None,
            icon_jobs_total: 0,
            icon_jobs_done: 0,
            icon_jobs_ok: 0,
            overlay_window_ready: true,
            hotkey_started_at: None,
            hotkey_failure_logged: false,
        };

        if app.model.listening || !app.model.config.listen_hotkey.trim().is_empty() {
            app.start_hotkeys();
        }

        // 启动时检查全局快捷键冲突（listen / 自动装配 / 槽位），冲突必须提示用户
        for conflict in app.model.config.hotkey_conflicts() {
            app.log(
                LogKind::Warn,
                format!("[LoadoutSync] 快捷键冲突：{conflict}"),
            );
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

    /// 调试日志（追加写入 exe 同目录 debug.log，同时入应用日志）
    pub fn debug(&mut self, text: impl Into<String>) {
        let msg = text.into();
        let line = format!("{} DBG {}", now_hms(), msg);
        eprintln!("{line}");
        util::log_to_file("debug.log", &line);
        self.log(LogKind::Info, format!("[DBG] {msg}"));
    }

    pub fn open_settings(&mut self) {
        self.settings_bindings = self.model.config.key_bindings.clone();
        self.settings_key = self.model.config.stratagem_key.clone();
        self.settings_delay = self.model.config.key_delay;
        self.settings_pre_delay = self.model.config.pre_delay;
        self.settings_listen_hotkey = self.model.config.listen_hotkey.clone();
        self.settings_loadout_sync_hotkey = self.model.config.loadout_sync_hotkey.clone();
        self.settings_compact_toggle = self.model.config.compact_mode.toggle_hotkey.clone();
        self.settings_cancel_hotkey = self.model.config.loadout_sync_cancel_hotkey.clone();
        self.settings_compact_opacity = self.model.config.compact_mode.opacity;
        self.settings_allow_overwrite = self.model.config.loadout_sync.allow_overwrite_filled;
        self.settings_debug_shots = self.model.config.loadout_sync.debug_screenshots;
        self.show_settings = true;
    }

    /// 按当前视图模式应用窗口尺寸 / 层级 / Overlay 透明度
    pub fn apply_view_mode(&mut self, ctx: &Context) {
        self.context = None;
        match self.model.view_mode {
            state::ViewMode::Main => {
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    egui::WindowLevel::Normal,
                ));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(
                    theme::DESIGN_W,
                    theme::DESIGN_H,
                )));
                self.set_overlay_window_hidden(false);
            }
            state::ViewMode::Compact => {
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    egui::WindowLevel::AlwaysOnTop,
                ));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(
                    crate::compact_mode::config::OVERLAY_DESIGN_W,
                    crate::compact_mode::config::OVERLAY_DESIGN_H,
                )));
                // 隐藏状态下不要把窗口重新点亮
                let visible = self.model.compact_preset.overlay_visible();
                self.set_overlay_window_hidden(!visible);
            }
        }
    }

    /// 由 config 构建 键名→动作 映射（非法槽位/键名被过滤）。
    /// 监听开关热键始终生效；已静音时槽位热键不会进入映射。
    fn hotkey_action_map(
        config: &config::Config,
        listening: bool,
    ) -> HashMap<String, hotkey::HotkeyAction> {
        use hotkey::HotkeyAction;
        let mut map: HashMap<String, hotkey::HotkeyAction> = HashMap::new();

        if listening {
            for (slot_key, key_raw) in &config.slot_hotkeys {
                if let Ok(slot) = slot_key.parse::<usize>() {
                    if slot < SLOT_COUNT && !key_raw.trim().is_empty() {
                        map.insert(hotkey::normalize_hotkey(key_raw), HotkeyAction::Slot(slot));
                    }
                }
            }
        }

        // 插入顺序即优先级（后插入覆盖同键）：
        //   槽位 < 自动装配 < 取消 < 显示浮窗 < 监听开关
        // 这些热键与监听开关一样，无论是否静音都生效（它们不注入战斗键盘序列）。
        let bind =
            |map: &mut HashMap<String, hotkey::HotkeyAction>, raw: &str, action: HotkeyAction| {
                let key = hotkey::normalize_hotkey(raw);
                if !key.is_empty() {
                    map.insert(key, action);
                }
            };
        // ── 三个互相正交的全局动作（§1 / §17）──
        //   AutoLoadout          ：任何模式下都可用；只管启动 Loadout Sync
        //   CancelLoadoutSync    ：任何模式下都可用；只管停止 Loadout Sync
        //   ToggleCompactOverlay ：唯一能改变浮窗可见性的动作（关闭紧凑模式时才不注册）
        // 注意：取消与自动装配都不依赖 compact_mode.enabled，也不依赖浮窗当前状态。
        bind(
            &mut map,
            &config.loadout_sync_hotkey,
            HotkeyAction::AutoLoadout,
        );
        bind(
            &mut map,
            &config.loadout_sync_cancel_hotkey,
            HotkeyAction::CancelLoadoutSync,
        );
        if config.compact_mode.enabled {
            bind(
                &mut map,
                &config.compact_mode.toggle_hotkey,
                HotkeyAction::ToggleCompactOverlay,
            );
        }
        bind(
            &mut map,
            &config.listen_hotkey,
            HotkeyAction::ToggleListening,
        );
        map
    }

    /// 监听运行中热键配置变化后调用，使运行中的钩子立即使用新映射
    pub fn sync_hotkey_map(&self) {
        if self.hotkey.is_some() {
            hotkey::update_map(&Self::hotkey_action_map(
                &self.model.config,
                self.model.listening,
            ));
        }
    }

    /// 启动全局热键钩子（应用生命周期级别：与 Loadout Sync 状态、浮窗可见性都无关）。
    fn start_hotkeys(&mut self) {
        let map = Arc::new(Mutex::new(Self::hotkey_action_map(
            &self.model.config,
            self.model.listening,
        )));
        let (tx, rx) = mpsc::channel();
        self.hotkey = Some(hotkey::HotkeyListener::start(map, tx));
        self.hotkey_rx = Some(rx);
        self.hotkey_started_at = Some(std::time::Instant::now());
        self.hotkey_failure_logged = false;
    }

    /// 热键注册结果检查：失败时给出具体错误，但**绝不退出应用**（§16）。
    fn check_hotkey_registration(&mut self) {
        if self.hotkey_failure_logged {
            return;
        }
        let Some(started) = self.hotkey_started_at else {
            return;
        };
        if started.elapsed() < std::time::Duration::from_millis(1500) {
            return;
        }
        let installed = self.hotkey.as_ref().map(|h| h.installed()).unwrap_or(true);
        if installed {
            self.hotkey_failure_logged = true;
            return;
        }
        self.hotkey_failure_logged = true;
        let bindings = Self::hotkey_action_map(&self.model.config, self.model.listening);
        let mut keys: Vec<String> = bindings.keys().cloned().collect();
        keys.sort();
        self.log(
            LogKind::Warn,
            format!(
                "全局热键注册失败（SetWindowsHookEx 未就绪）：已配置 {:?}；应用继续运行，可改绑后重试",
                keys
            ),
        );
        for conflict in self.model.config.hotkey_conflicts() {
            self.log(LogKind::Warn, format!("快捷键冲突：{conflict}"));
        }
    }

    fn stop_hotkeys(&mut self) {
        if let Some(mut listener) = self.hotkey.take() {
            listener.stop();
        }
        self.hotkey_rx = None;
    }

    pub fn toggle_listening(&mut self) {
        self.model.listening = !self.model.listening;
        if self.model.listening {
            if self.hotkey.is_none() {
                self.start_hotkeys();
            } else {
                self.sync_hotkey_map();
            }
            self.log(LogKind::Info, "热键监听已开启");
        } else {
            // 绑定了监听开关热键时必须保留钩子，否则无法再通过热键重新开启
            if self.model.config.listen_hotkey.trim().is_empty() {
                self.stop_hotkeys();
            } else {
                self.sync_hotkey_map();
            }
            self.log(LogKind::Warn, "热键监听已关闭");
        }
        self.model.config.listening_enabled = self.model.listening;
        save_config(&self.model.config);
    }

    pub fn refresh_profiles(&mut self) {
        self.model.profile_names = list_profiles();
    }

    /// 是否有进行中的网络任务（拉取战备数据 / 补齐图标）
    pub fn network_busy(&self) -> bool {
        self.wiki.fetch_rx.is_some() || self.icon_rx.is_some()
    }

    /// Wiki 数据合并完成后调用：为本地没有图标的自动获取战备发起后台下载。
    /// 每个 job = (图标 key, 源图地址)；线程下载 SVG → 栅格化 PNG 后经 channel 回传，
    /// 由 update() 在主线程注册进 IconStore 并落盘 assets/icons/{key}.png。
    pub fn start_icon_backfill(&mut self) {
        if self.icon_rx.is_some() {
            return;
        }
        // 在线获取的战备与强化都带 icon_url；本地缺图标就补齐
        let jobs: Vec<(String, String)> = self
            .plugins
            .stratagems
            .iter()
            .filter(|p| plugin::is_wiki_source(&p.source))
            .filter(|p| p.icon_url.is_some())
            .filter(|p| !self.model.icons.has(&p.icon))
            .map(|p| (p.icon.clone(), p.icon_url.clone().unwrap_or_default()))
            .filter(|(_, url)| !url.is_empty())
            .collect();
        if jobs.is_empty() {
            return;
        }
        // 同 key 去重
        let mut seen = std::collections::HashSet::new();
        let mut uniq: Vec<(String, String)> = Vec::new();
        for (k, u) in jobs {
            if seen.insert(k.clone()) {
                uniq.push((k, u));
            }
        }
        let total = uniq.len();
        let (tx, rx) = mpsc::channel();
        self.icon_jobs_total = total;
        self.icon_jobs_done = 0;
        self.icon_jobs_ok = 0;
        self.icon_rx = Some(rx);
        self.wiki.fetch_status = format!("发现 {total} 个缺失图标，正在在线补齐…");
        self.log(LogKind::Info, format!("在线补齐 {total} 个缺失图标…"));
        std::thread::spawn(move || {
            for (key, url) in uniq {
                let result = crate::icon_fetch::fetch_icon_png(&url);
                let _ = tx.send(match result {
                    Ok(png) => IconJobResult {
                        key,
                        png: Some(png),
                        error: None,
                    },
                    Err(e) => IconJobResult {
                        key,
                        png: None,
                        error: Some(e),
                    },
                });
            }
        });
    }
}

impl eframe::App for H2ACApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // 计算当前窗口对应的缩放比例
        let sr = ctx.screen_rect();
        let (dw, dh) = if self.model.view_mode.is_compact() {
            (
                crate::compact_mode::config::OVERLAY_DESIGN_W,
                crate::compact_mode::config::OVERLAY_DESIGN_H,
            )
        } else {
            (theme::DESIGN_W, theme::DESIGN_H)
        };
        let scale = (sr.width() / dw).min(sr.height() / dh).max(0.25);

        if (scale - self.model.scale).abs() > 0.001 {
            self.model.scale = scale;
            self.model.metrics = theme::UiMetrics::new(scale);
            ctx.set_style(theme::apply_scaled(&self.model.metrics));
        }

        let mut hotkey_actions: Vec<hotkey::HotkeyAction> = Vec::new();
        if let Some(rx) = &self.hotkey_rx {
            while let Ok(action) = rx.try_recv() {
                hotkey_actions.push(action);
            }
        }
        for action in hotkey_actions {
            match action {
                hotkey::HotkeyAction::Slot(slot) => self.execute_slot(slot),
                hotkey::HotkeyAction::ToggleListening => self.toggle_listening(),
                // 三个动作各自独立：没有任何一个会去调用另一个的切换逻辑
                hotkey::HotkeyAction::AutoLoadout => self.auto_loadout(ctx),
                hotkey::HotkeyAction::ToggleCompactOverlay => self.toggle_compact_overlay(ctx),
                hotkey::HotkeyAction::CancelLoadoutSync => self.cancel_auto_loadout(),
            }
        }
        // Loadout Sync 日志/状态：每帧消费工作线程回传的事件（GUI 主线程不阻塞）
        self.poll_loadout_sync();
        // 紧凑模式：把装配结果同步到 Overlay 状态栏，并在成功后持久化预设
        self.poll_compact_preset();
        if self.model.loadout_sync.is_running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(listener) = &mut self.hotkey {
            listener.poll();
        }
        // 热键注册失败只记录、不退出；也不在任何装配状态下注销热键
        self.check_hotkey_registration();
        // 钩子存在时保持低频重绘，保证窗口未聚焦时也能及时消费全局热键消息
        if self.hotkey.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(66));
        }
        let now = ctx.input(|i| i.time);
        for v in self.model.flash.values_mut() {
            if *v == 0.0 {
                *v = now;
            }
        }
        // 清理已结束的闪光动画条目
        self.model
            .flash
            .retain(|_, &mut v| ((now - v) as f32) < theme::FLASH_DURATION);
        if let Some(rx) = self.wiki.fetch_rx.take() {
            let mut still_active = true;
            while let Ok(progress) = rx.try_recv() {
                self.wiki.fetch_status = progress.stage.clone();
                if progress.done {
                    still_active = false;
                    if let Some(Ok(new_items)) = progress.result {
                        // 战备与强化来自两个页面（Stratagems / Boosters），分开入库：
                        //   战备 → plugins/_wiki.json（source = wiki）
                        //   强化 → plugins/_boosters.json（source = wiki_boosters，category = Boosters）
                        // 差集比对只针对战备：强化没有方向指令，本来就不在内置数据库里。
                        let truly_new: Vec<crate::stratagems::PluginStratagem> = new_items
                            .iter()
                            .filter(|item| item.source != plugin::BOOSTER_SOURCE)
                            .filter(|item| !BUILTIN_SIGNATURES.contains(&item.command.join(",")))
                            .cloned()
                            .collect();
                        let boosters: Vec<crate::stratagems::PluginStratagem> = new_items
                            .iter()
                            .filter(|item| item.source == plugin::BOOSTER_SOURCE)
                            .cloned()
                            .collect();
                        let new_count = truly_new.len();
                        let booster_count = boosters.len();
                        // 分类与位置以网页为准：直接保留页面分类，不再附加任何后缀。
                        // 替换上一次自动获取的数据（含旧版 "(Wiki)" 后缀残留），保留用户插件。
                        self.plugins.stratagems.retain(|p| {
                            !plugin::is_wiki_source(&p.source) && !p.name.ends_with("(Wiki)")
                        });
                        // 战备：全部命中内置时删除陈旧文件，避免重启后加载过期数据
                        if new_count > 0 {
                            let manifest = crate::stratagems::PluginManifest {
                                id: plugin::WIKI_PLUGIN_ID.into(),
                                name: "页面自动获取的战备数据".into(),
                                enabled: true,
                                stratagems: truly_new,
                            };
                            let _ = util::save_json(&plugin::wiki_plugin_path(), &manifest);
                            self.plugins.stratagems.extend(manifest.stratagems);
                        } else {
                            let _ = std::fs::remove_file(plugin::wiki_plugin_path());
                        }
                        // 强化：写入 _boosters.json（有数据才写，无数据不动旧文件以免误删）
                        if booster_count > 0 {
                            let manifest = crate::stratagems::PluginManifest {
                                id: plugin::BOOSTER_PLUGIN_ID.into(),
                                name: "页面自动获取的强化数据".into(),
                                enabled: true,
                                stratagems: boosters,
                            };
                            let _ = util::save_json(&plugin::booster_plugin_path(), &manifest);
                            self.plugins.stratagems.extend(manifest.stratagems);
                        }
                        self.wiki.cache_exists = new_count > 0 || booster_count > 0;
                        self.log(
                            LogKind::Info,
                            format!(
                                "战备数据获取完成，新增 {} 条 → plugins/{}",
                                new_count,
                                plugin::WIKI_PLUGIN_FILE
                            ),
                        );
                        if booster_count > 0 {
                            self.log(
                                LogKind::Info,
                                format!(
                                    "强化数据获取完成，共 {} 条 → plugins/{}",
                                    booster_count,
                                    plugin::BOOSTER_PLUGIN_FILE
                                ),
                            );
                        }
                        // 本地无图标的战备 → 从页面图标源在线补齐
                        self.start_icon_backfill();
                    } else if let Some(Err(e)) = progress.result {
                        self.log(LogKind::Warn, format!("战备数据获取失败: {e}"));
                    } else {
                        self.log(LogKind::Warn, "战备数据获取失败，请检查网络");
                    }
                }
            }
            if still_active {
                self.wiki.fetch_rx = Some(rx);
                // 拉取进行中：定时驱动重绘以刷新进度显示
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            } else {
                // 拉取完成/失败：立即重绘呈现最终状态
                ctx.request_repaint();
            }
        }

        // ─── 图标在线补齐：消费后台结果（注册纹理 + 落盘 assets/icons/{key}.png）───
        if self.icon_rx.is_some() {
            let mut results: Vec<IconJobResult> = Vec::new();
            if let Some(rx) = &self.icon_rx {
                while let Ok(r) = rx.try_recv() {
                    results.push(r);
                }
            }
            for r in results {
                self.icon_jobs_done += 1;
                let key = r.key.clone();
                match r.png {
                    Some(png) => {
                        // 写入 exe 旁 assets/icons/（IconStore 启动时自动发现，重启免重下）
                        let dir = util::app_dir().join("assets/icons");
                        let _ = std::fs::create_dir_all(&dir);
                        let wrote = std::fs::write(dir.join(format!("{key}.png")), &png).is_ok();
                        let tex_ok = self.model.icons.insert_png(ctx, key.clone(), &png);
                        if wrote && tex_ok {
                            self.icon_jobs_ok += 1;
                            self.log(LogKind::Info, format!("图标已就位: {key}"));
                        } else {
                            self.log(LogKind::Warn, format!("图标写入失败: {key}"));
                        }
                    }
                    None => {
                        self.log(
                            LogKind::Warn,
                            format!("图标补齐失败 [{}]: {}", key, r.error.unwrap_or_default()),
                        );
                    }
                }
                self.wiki.fetch_status = format!(
                    "正在补齐图标 {}/{}…",
                    self.icon_jobs_done, self.icon_jobs_total
                );
            }

            if self.icon_jobs_done >= self.icon_jobs_total {
                let total = self.icon_jobs_total;
                let ok = self.icon_jobs_ok;
                self.icon_rx = None;
                if ok > 0 {
                    self.log(LogKind::Info, format!("图标补齐完成：成功 {ok}/{total}"));
                } else {
                    self.log(LogKind::Warn, format!("图标补齐失败：成功 0/{total}"));
                }
                ctx.request_repaint();
            } else {
                // 仍在进行：驱动进度显示
                ctx.request_repaint_after(std::time::Duration::from_millis(150));
            }
        }

        match self.model.view_mode {
            state::ViewMode::Compact => {
                // Overlay 与主窗口共用同一个 eframe 窗口：隐藏期间只是 alpha=0，
                // 因此这里必须继续渲染（不渲染会让事件循环空转、热键动作排队）。
                crate::ui::preset_overlay::show_preset_overlay(self, ctx);
            }
            state::ViewMode::Main => main_view::show_main(self, ctx),
        }
    }
}

fn main() -> Result<(), eframe::Error> {
    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::force_capture();
        let msg = format!("PANIC: {info}\n{bt}\n");
        let _ = std::fs::write(util::app_dir().join("panic.log"), msg);
    }));

    let is_admin = unsafe { windows::Win32::UI::Shell::IsUserAnAdmin().as_bool() };
    if !is_admin {
        let _ = std::fs::write(
            util::app_dir().join("admin_warning.txt"),
            "未以管理员身份运行。如果游戏内按键无反应，请右键 h2ac-rs.exe → 以管理员身份运行。",
        );
    }

    // 加载应用图标
    let icon = {
        let icon_bytes = include_bytes!("../assets/icon-removebg.png");
        let img = image::load_from_memory(icon_bytes)
            .ok()
            .map(|i| i.to_rgba8());
        img.map(|rgba| {
            let (w, h) = (rgba.width(), rgba.height());
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

#[cfg(test)]
mod tests {
    use super::*;
    use hotkey::HotkeyAction;

    #[test]
    fn listen_hotkey_wins_over_slot_hotkey() {
        let mut cfg = config::Config {
            listen_hotkey: "f8".into(),
            ..Default::default()
        };
        cfg.slot_hotkeys.insert("0".into(), "f8".into());
        cfg.slot_hotkeys.insert("1".into(), ",".into());

        let map = H2ACApp::hotkey_action_map(&cfg, true);
        assert_eq!(map.get("f8"), Some(&HotkeyAction::ToggleListening));
        assert_eq!(map.get(","), Some(&HotkeyAction::Slot(1)));
    }

    #[test]
    fn three_global_actions_are_bound_independently() {
        // §1 / §17：三个动作互相正交，且都不依赖浮窗可见性
        let mut cfg = config::Config::default();
        cfg.slot_hotkeys.insert("5".into(), "u".into());
        let map = H2ACApp::hotkey_action_map(&cfg, true);
        assert_eq!(map.get("f7"), Some(&HotkeyAction::AutoLoadout));
        assert_eq!(
            map.get("ctrl+shift+f9"),
            Some(&HotkeyAction::CancelLoadoutSync)
        );
        assert_eq!(
            map.get("ctrl+shift+f7"),
            Some(&HotkeyAction::ToggleCompactOverlay)
        );
        assert_eq!(map.get("u"), Some(&HotkeyAction::Slot(5)));
        // 三个动作必须是三个不同的键
        let mut keys: Vec<&String> = map
            .iter()
            .filter(|(_, a)| {
                matches!(
                    a,
                    HotkeyAction::AutoLoadout
                        | HotkeyAction::CancelLoadoutSync
                        | HotkeyAction::ToggleCompactOverlay
                )
            })
            .map(|(k, _)| k)
            .collect();
        keys.sort();
        assert_eq!(keys.len(), 3, "{keys:?}");
    }

    #[test]
    fn auto_and_cancel_hotkeys_survive_compact_mode_being_disabled() {
        // 取消 / 自动装配属于 Loadout Sync，不因紧凑模式关闭而消失（§1.B / §1.C）
        let mut cfg = config::Config::default();
        cfg.compact_mode.enabled = false;
        let map = H2ACApp::hotkey_action_map(&cfg, true);
        assert_eq!(map.get("f7"), Some(&HotkeyAction::AutoLoadout));
        assert_eq!(
            map.get("ctrl+shift+f9"),
            Some(&HotkeyAction::CancelLoadoutSync)
        );
        assert_eq!(
            map.get("ctrl+shift+f7"),
            None,
            "紧凑模式关闭则不注册浮窗热键"
        );
    }

    #[test]
    fn auto_loadout_hotkey_is_the_only_binding_for_auto_loadout() {
        // 只允许一个 Auto Loadout 热键：旧配置里的紧凑专用热键必须迁移过来
        let cfg: config::Config = serde_json::from_str(
            // 旧版配置里存在紧凑专用自动装配热键字段：解析时应被忽略，不得产生第二个绑定
            r#"{"loadout_sync_hotkey":"f7","compact_mode":{"toggle_hotkey":"ctrl+shift+f7","auto_loadout_hotkey":"ctrl+shift+f8","cancel_hotkey":"ctrl+shift+f10"}}"#,
        )
        .unwrap();
        let map = H2ACApp::hotkey_action_map(&cfg, true);
        let auto_count = map
            .values()
            .filter(|a| **a == HotkeyAction::AutoLoadout)
            .count();
        assert_eq!(auto_count, 1, "自动装配只能有一个热键绑定：{map:?}");
    }

    #[test]
    fn hotkeys_can_be_rebound_and_disabled() {
        let mut cfg = config::Config {
            loadout_sync_hotkey: "Ctrl + Alt + F10".into(),
            loadout_sync_cancel_hotkey: "ctrl+alt+f11".into(),
            ..Default::default()
        };
        cfg.compact_mode.toggle_hotkey = "ctrl+alt+f12".into();
        let map = H2ACApp::hotkey_action_map(&cfg, true);
        assert_eq!(map.get("ctrl+alt+f10"), Some(&HotkeyAction::AutoLoadout));
        assert_eq!(
            map.get("ctrl+alt+f11"),
            Some(&HotkeyAction::CancelLoadoutSync)
        );
        assert_eq!(
            map.get("ctrl+alt+f12"),
            Some(&HotkeyAction::ToggleCompactOverlay)
        );
        assert_eq!(map.get("f7"), None);
    }

    #[test]
    fn legacy_compact_hotkeys_are_migrated_to_global_config() {
        // 旧配置：自动装配/取消写在 compact_mode 段下 → 迁移到全局字段，且旧键被清掉
        let legacy = r#"{"loadout":[1],"compact_mode":{"toggle_hotkey":"ctrl+shift+f7","auto_loadout_hotkey":"ctrl+shift+f8","cancel_hotkey":"alt+f9","opacity":0.8}}"#;
        let mut value: serde_json::Value = serde_json::from_str(legacy).unwrap();
        config::migrate_legacy_hotkeys(&mut value);
        let cfg = serde_json::from_value::<config::Config>(value)
            .unwrap()
            .sanitize();
        assert_eq!(cfg.loadout_sync_hotkey, "ctrl+shift+f8");
        assert_eq!(cfg.loadout_sync_cancel_hotkey, "alt+f9");
        assert_eq!(cfg.compact_mode.toggle_hotkey, "ctrl+shift+f7");
        // 新配置优先，不被旧键覆盖
        let modern = r#"{"loadout_sync_hotkey":"f6","loadout_sync_cancel_hotkey":"f5","compact_mode":{"auto_loadout_hotkey":"ctrl+shift+f8","cancel_hotkey":"alt+f9"}}"#;
        let mut value: serde_json::Value = serde_json::from_str(modern).unwrap();
        config::migrate_legacy_hotkeys(&mut value);
        let cfg = serde_json::from_value::<config::Config>(value)
            .unwrap()
            .sanitize();
        assert_eq!(cfg.loadout_sync_hotkey, "f6");
        assert_eq!(cfg.loadout_sync_cancel_hotkey, "f5");
    }

    #[test]
    fn compact_hotkeys_do_not_shadow_existing_bindings_by_accident() {
        let cfg = config::Config::default();
        assert!(
            cfg.hotkey_conflicts().is_empty(),
            "{:?}",
            cfg.hotkey_conflicts()
        );
        // 故意制造冲突：浮窗热键与槽位热键相同
        let mut clash = config::Config::default();
        clash
            .slot_hotkeys
            .insert("3".into(), "ctrl+shift+f7".into());
        let conflicts = clash.hotkey_conflicts();
        assert_eq!(conflicts.len(), 1, "{conflicts:?}");
        assert!(
            conflicts[0].to_uppercase().contains("CTRL+SHIFT+F7"),
            "冲突提示必须指出具体按键: {}",
            conflicts[0]
        );
    }

    #[test]
    fn legacy_config_without_compact_section_still_loads() {
        let cfg: config::Config = serde_json::from_str(r#"{"loadout":[1],"listen_hotkey":"f8"}"#)
            .expect("旧配置必须能加载");
        assert_eq!(cfg.compact_mode.toggle_hotkey, "ctrl+shift+f7");
        assert_eq!(cfg.loadout_sync_hotkey, "f7");
        assert_eq!(cfg.loadout_sync_cancel_hotkey, "ctrl+shift+f9");
        assert!(cfg.compact_mode.enabled);
    }
}
