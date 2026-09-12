// Loadout Sync 自动化参数 —— 写入主 Config（config.json）的 loadout_sync 段。
//
// 设计原则：普通用户只需要一个快捷键；其余是视觉/时序调参，默认值可用，
// 不主动暴露到 GUI（仅在需要时手改 config.json）。
use serde::{Deserialize, Serialize};

/// 大滚动增量（鼠标滚轮一格 = 120）。
pub const WHEEL_UNIT: i32 = 120;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadoutSyncConfig {
    /// 正常滚动量（WHEEL_DELTA 单位）
    #[serde(default = "default_scroll_delta")]
    pub scroll_delta: i32,
    /// 边界探测滚动量（找不到目标且已到列表边界时改用小步长）
    #[serde(default = "default_scroll_probe_delta")]
    pub scroll_probe_delta: i32,
    /// 允许自动点击的最低匹配度
    #[serde(default = "default_recognition_threshold")]
    pub recognition_threshold: f32,
    /// Hover 生效等待上限（毫秒）
    #[serde(default = "default_hover_timeout_ms")]
    pub hover_timeout_ms: u64,
    /// 点击后等待选中确认的上限（毫秒）
    #[serde(default = "default_selection_timeout_ms")]
    pub selection_timeout_ms: u64,
    /// 点击槽位后等待列表出现的上限（毫秒）
    #[serde(default = "default_list_timeout_ms")]
    pub list_timeout_ms: u64,
    /// 等待 Loadout Home 稳定的上限（毫秒）
    #[serde(default = "default_home_timeout_ms")]
    pub home_timeout_ms: u64,
    /// 单次目标搜索的最大滚动次数
    #[serde(default = "default_max_scroll_attempts")]
    pub max_scroll_attempts: u32,
    /// 单步操作（点击/悬停/打开列表）的最大重试次数
    #[serde(default = "default_max_retry_attempts")]
    pub max_retry_attempts: u32,
    /// 整个流程的硬超时（毫秒）
    #[serde(default = "default_total_timeout_ms")]
    pub total_timeout_ms: u64,
    /// 允许在游戏已有战备时覆盖（默认关闭：第一版要求四个战备槽全空，
    /// 且绝不自动清空已有 Loadout）
    #[serde(default)]
    pub allow_overwrite_filled: bool,
    /// 失败时保存调试图（screenshots/loadout_sync/）：默认关闭
    #[serde(default)]
    pub debug_screenshots: bool,
    /// 识别调试叠加（在调试图上绘制 ROI / 匹配框 / 置信度）
    #[serde(default)]
    pub debug_overlay: bool,
}

fn default_scroll_delta() -> i32 {
    600
}
fn default_scroll_probe_delta() -> i32 {
    WHEEL_UNIT
}
/// 匹配度阈值：本实现的 score 是「形状一致度」（Jaccard 与含背景 phi 系数的加权）。
/// 合成 fixture 实测：正确目标 0.61~0.82（含识别矩形抖动），最相似的非目标图标最高 0.72。
/// 因此阈值取 0.60，并且必须配合「目标必须是该格最像的候选 + 与次优候选分差达标」
/// （见 types::MIN_MATCH_MARGIN）一起判定，二者缺一不可。
fn default_recognition_threshold() -> f32 {
    0.60
}
fn default_hover_timeout_ms() -> u64 {
    800
}
fn default_selection_timeout_ms() -> u64 {
    1500
}
fn default_list_timeout_ms() -> u64 {
    1500
}
fn default_home_timeout_ms() -> u64 {
    1500
}
fn default_max_scroll_attempts() -> u32 {
    24
}
fn default_max_retry_attempts() -> u32 {
    3
}
fn default_total_timeout_ms() -> u64 {
    20_000
}

impl Default for LoadoutSyncConfig {
    fn default() -> Self {
        Self {
            scroll_delta: default_scroll_delta(),
            scroll_probe_delta: default_scroll_probe_delta(),
            recognition_threshold: default_recognition_threshold(),
            hover_timeout_ms: default_hover_timeout_ms(),
            selection_timeout_ms: default_selection_timeout_ms(),
            list_timeout_ms: default_list_timeout_ms(),
            home_timeout_ms: default_home_timeout_ms(),
            max_scroll_attempts: default_max_scroll_attempts(),
            max_retry_attempts: default_max_retry_attempts(),
            total_timeout_ms: default_total_timeout_ms(),
            allow_overwrite_filled: false,
            debug_screenshots: false,
            debug_overlay: false,
        }
    }
}

impl LoadoutSyncConfig {
    /// 归一化非法值（手工编辑 config.json 后的自愈）：任何一项异常都不允许
    /// 让自动化「无限重试」或「零阈值点击」。
    pub fn sanitize(mut self) -> Self {
        self.scroll_delta = self.scroll_delta.clamp(WHEEL_UNIT, 10 * WHEEL_UNIT);
        self.scroll_probe_delta = self.scroll_probe_delta.clamp(1, self.scroll_delta);
        if !self.recognition_threshold.is_finite() {
            self.recognition_threshold = default_recognition_threshold();
        }
        // 阈值下限保护：宁可拒绝点击，也不允许误选战备
        self.recognition_threshold = self.recognition_threshold.clamp(0.45, 0.95);
        self.hover_timeout_ms = self.hover_timeout_ms.clamp(100, 5_000);
        self.selection_timeout_ms = self.selection_timeout_ms.clamp(200, 10_000);
        self.list_timeout_ms = self.list_timeout_ms.clamp(200, 10_000);
        self.home_timeout_ms = self.home_timeout_ms.clamp(200, 10_000);
        self.max_scroll_attempts = self.max_scroll_attempts.clamp(1, 60);
        self.max_retry_attempts = self.max_retry_attempts.clamp(1, 8);
        self.total_timeout_ms = self.total_timeout_ms.clamp(2_000, 120_000);
        self
    }
}
