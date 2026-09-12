// Loadout Sync 错误类型 —— 每条失败路径都有独立分类，供日志、GUI 提示与测试断言使用。
// 禁止用裸 String 表示所有错误：调用方必须能区分「游戏没开」「不在 Loadout 界面」
// 「识别不到目标」「权限不足」等不同处置方式。
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum LoadoutSyncError {
    /// 本地校验失败：槽位缺失 / 重复 / 非法内容（未进入自动化）
    InvalidSelection { detail: String },
    /// 找不到 HELLDIVERS 2 窗口
    GameNotFound,
    /// 游戏不是前台窗口（不抢焦点，直接失败）
    GameNotForeground,
    /// 窗口存在但不可用（最小化 / 尺寸为 0）
    GameWindowInvalid { detail: String },
    /// 截图失败（GDI BitBlt / GetDIBits）
    CaptureFailed { detail: String },
    /// 当前画面不是 Loadout Home
    LoadoutHomeNotDetected { score: f32 },
    /// Loadout Home 已存在部分战备（第一版拒绝，不自动清空）
    LoadoutNotEmpty { filled: usize },
    /// 识别不到游戏槽位区域
    SlotNotDetected { slot: usize },
    /// 点击槽位后没有出现选择列表
    ListNotDetected { slot: usize },
    /// 列表滚遍也没有找到目标
    TargetNotFound { name: String },
    /// 滚动没有产生预期的 Viewport 位移
    ViewportNavigationFailed { detail: String },
    /// 置信度不足：可能是图标被 Mod 替换 / HDR / 非标准分辨率
    RecognitionUnsupported { name: String, score: f32 },
    /// 悬停验证失败
    HoverVerificationFailed { name: String, score: f32 },
    /// 点击后未确认选中
    SelectionVerificationFailed { name: String },
    /// 最终 Loadout 与 H2AC 配置不一致
    FinalVerificationFailed { detail: String },
    /// 输入注入失败
    InputError { detail: String },
    /// 权限低于游戏进程（UIPI 拦截注入）
    PermissionError,
    /// 阶段超时
    Timeout { stage: &'static str, ms: u64 },
    /// 用户取消
    Cancelled,
    /// 内部状态机不一致（防御性）
    UnexpectedState { detail: String },
}

impl LoadoutSyncError {
    /// 稳定助记符 —— 写进日志，便于用户检索与反馈。
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidSelection { .. } => "InvalidSelection",
            Self::GameNotFound => "GameNotFound",
            Self::GameNotForeground => "GameNotForeground",
            Self::GameWindowInvalid { .. } => "GameWindowInvalid",
            Self::CaptureFailed { .. } => "CaptureFailed",
            Self::LoadoutHomeNotDetected { .. } => "LoadoutHomeNotDetected",
            Self::LoadoutNotEmpty { .. } => "LoadoutNotEmpty",
            Self::SlotNotDetected { .. } => "SlotNotDetected",
            Self::ListNotDetected { .. } => "ListNotDetected",
            Self::TargetNotFound { .. } => "TargetNotFound",
            Self::ViewportNavigationFailed { .. } => "ViewportNavigationFailed",
            Self::RecognitionUnsupported { .. } => "RecognitionUnsupported",
            Self::HoverVerificationFailed { .. } => "HoverVerificationFailed",
            Self::SelectionVerificationFailed { .. } => "SelectionVerificationFailed",
            Self::FinalVerificationFailed { .. } => "FinalVerificationFailed",
            Self::InputError { .. } => "InputError",
            Self::PermissionError => "PermissionError",
            Self::Timeout { .. } => "Timeout",
            Self::Cancelled => "Cancelled",
            Self::UnexpectedState { .. } => "UnexpectedState",
        }
    }

    /// 面向用户的中文说明（GUI/日志直接展示）。
    pub fn message(&self) -> String {
        match self {
            Self::InvalidSelection { detail } => format!("自动装配失败：{detail}"),
            Self::GameNotFound => "自动装配失败：未找到 HELLDIVERS 2 窗口。".into(),
            Self::GameNotForeground => "自动装配失败：HELLDIVERS 2 不是当前前台窗口。".into(),
            Self::GameWindowInvalid { detail } => format!("自动装配失败：游戏窗口不可用（{detail}）。"),
            Self::CaptureFailed { detail } => format!("自动装配失败：游戏画面捕获失败（{detail}）。"),
            Self::LoadoutHomeNotDetected { score } => format!(
                "自动装配失败：未识别到 Loadout 主界面（匹配度 {:.2}）。请停留在配装主界面后重试。",
                score
            ),
            Self::LoadoutNotEmpty { filled } => format!(
                "自动装配失败：游戏当前已有 {filled} 个战备，本版本仅支持四个战备槽位全空时装配（不会自动清空）。"
            ),
            Self::SlotNotDetected { slot } => {
                format!("自动装配失败：识别不到游戏战备槽位 {slot}。")
            }
            Self::ListNotDetected { slot } => {
                format!("自动装配失败：点击槽位 {slot} 后没有出现战备选择列表。")
            }
            Self::TargetNotFound { name } => format!("自动装配失败：列表中没有找到「{name}」。"),
            Self::ViewportNavigationFailed { detail } => {
                format!("自动装配失败：列表滚动未生效（{detail}）。")
            }
            Self::RecognitionUnsupported { name, score } => format!(
                "自动装配失败：无法可靠识别「{name}」（最高匹配度 {score:.2}）。游戏战备图标可能已被替换，视觉匹配无法确认。"
            ),
            Self::HoverVerificationFailed { name, score } => {
                format!("自动装配失败：悬停验证未通过「{name}」（匹配度 {score:.2}）。")
            }
            Self::SelectionVerificationFailed { name } => {
                format!("自动装配失败：点击后未能确认「{name}」被选中。")
            }
            Self::FinalVerificationFailed { detail } => {
                format!("自动装配失败：最终 Loadout 校验未通过（{detail}）。")
            }
            Self::InputError { detail } => format!("自动装配失败：输入注入失败（{detail}）。"),
            Self::PermissionError => "自动装配失败：鼠标/键盘注入被系统拒绝。请以与游戏相同或更高的权限运行 H2AC-RS（右键 → 以管理员身份运行）。".into(),
            Self::Timeout { stage, ms } => {
                format!("自动装配失败：阶段「{stage}」超时（{ms} ms）。")
            }
            Self::Cancelled => "自动装配已取消。".into(),
            Self::UnexpectedState { detail } => format!("自动装配失败：内部状态异常（{detail}）。"),
        }
    }

    /// 可选的补充建议（日志/工具提示）。
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Self::GameNotForeground => {
                Some("点击游戏窗口使其成为前台窗口后再按快捷键（不会自动抢焦点）。")
            }
            Self::PermissionError => Some("若游戏以管理员运行，H2AC-RS 也必须以管理员运行。"),
            Self::LoadoutHomeNotDetected { .. } => {
                Some("请确认游戏处于「配装 / 武装配置」界面且未被其它窗口遮挡。")
            }
            Self::RecognitionUnsupported { .. } => {
                Some("若安装了替换战备图标的 Mod，请先移除后重试。")
            }
            Self::LoadoutNotEmpty { .. } => Some("请先在游戏内手动清空四个战备槽位。"),
            _ => None,
        }
    }

    /// 是否是用户主动取消（不作为错误弹窗）。
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

impl fmt::Display for LoadoutSyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code(), self.message())
    }
}

impl std::error::Error for LoadoutSyncError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_and_unique_per_variant() {
        let samples = [
            LoadoutSyncError::InvalidSelection { detail: "x".into() },
            LoadoutSyncError::GameNotFound,
            LoadoutSyncError::GameNotForeground,
            LoadoutSyncError::LoadoutHomeNotDetected { score: 0.1 },
            LoadoutSyncError::TargetNotFound { name: "n".into() },
            LoadoutSyncError::PermissionError,
            LoadoutSyncError::Timeout {
                stage: "home",
                ms: 1500,
            },
            LoadoutSyncError::Cancelled,
        ];
        let mut seen = std::collections::HashSet::new();
        for e in &samples {
            assert!(seen.insert(e.code()), "duplicate code {}", e.code());
            assert!(!e.message().is_empty());
            assert!(e.to_string().contains(e.code()));
        }
    }

    #[test]
    fn cancelled_is_not_reported_as_error() {
        assert!(LoadoutSyncError::Cancelled.is_cancelled());
        assert!(!LoadoutSyncError::GameNotFound.is_cancelled());
    }
}
