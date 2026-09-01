// 应用状态 — 按关注点分组，替代 H2ACApp God Object
use std::collections::HashMap;
use std::sync::mpsc;

use crate::config::Config;
use crate::icons::IconStore;
use crate::stratagems::PluginStratagem;
use crate::theme::UiMetrics;
use crate::wiki_fetcher;

/// 控制槽位网格、待命、执行的业务模型
pub struct AppModel {
    pub slots: Vec<Option<usize>>,
    /// 插件/Wiki 战备装入槽位（key=slot index）
    pub plugin_slots: std::collections::HashMap<usize, crate::stratagems::PluginStratagem>,
    pub armed: Option<usize>,
    pub detail_slot: Option<usize>,
    pub listening: bool,
    pub config: Config,
    pub compact: bool,
    pub flash: HashMap<usize, f64>,
    pub icons: IconStore,
    /// 调试日志开关
    pub debug_mode: bool,
    pub profile_names: Vec<String>,
    pub current_profile: String,
    pub save_profile_name: String,
    /// 缩放系数 = min(window_w / DESIGN_W, window_h / DESIGN_H)
    pub scale: f32,
    /// 缩放后的 UI 尺寸缓存
    pub metrics: UiMetrics,
}

/// 战备库面板状态
pub struct LibraryState {
    pub lib_category: String,
    pub lib_search: String,
    pub categories: Vec<String>,
}

/// 热键/按键捕获状态
#[derive(Default)]
pub struct CaptureState {
    pub capturing: Option<usize>,
    pub capturing_listen: bool,
    pub captured: String,
    pub settings_capture: Option<String>,
}

/// 插件加载的数据
pub struct PluginData {
    pub stratagems: Vec<PluginStratagem>,
}

/// Wiki 拉取进度
pub struct WikiState {
    pub fetch_rx: Option<mpsc::Receiver<wiki_fetcher::FetchProgress>>,
    pub fetch_status: String,
    pub cache_exists: bool,
}

/// 插件创建器模态窗状态
pub struct CreatorState {
    pub open: bool,
    pub tab: CreatorTab,
    pub plugin_name: String,
    pub department: String,
    pub stratagem_name: String,
    pub stratagem_sequence: Vec<String>,
    pub sequence_recording: bool,
    pub icon_key: String,
    pub saved_entries: Vec<(String, Vec<String>, String)>,
    pub status: String,
}

#[derive(Clone, Copy, PartialEq)]
pub enum CreatorTab {
    Fetch,
    Create,
}

impl Default for CreatorState {
    fn default() -> Self {
        Self {
            open: false,
            tab: CreatorTab::Fetch,
            plugin_name: String::new(),
            department: "自定义战备".into(),
            stratagem_name: String::new(),
            stratagem_sequence: Vec::new(),
            sequence_recording: false,
            icon_key: "reinforce".into(),
            saved_entries: Vec::new(),
            status: String::new(),
        }
    }
}
