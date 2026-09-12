// 紧凑模式（游戏内配装预设 Overlay）配置
//
// 设计目标（对应实施要求 §二 / §十四）：
//   * 「显示 Overlay」与「执行自动装配」是两个互相独立的快捷键，绝不根据游戏状态
//     自动判断 Save / Apply；
//   * 快捷键可在配置里改，代码里不硬编码；
//   * 位置与透明度可调，写入 config.json 的 compact_mode 段；
//   * 旧版 config.json（没有该段）必须能正常加载。
use serde::{Deserialize, Serialize};

/// Overlay 设计尺寸（逻辑像素，随窗口缩放系数一起缩放）
pub const OVERLAY_DESIGN_W: f32 = 384.0;
/// 10 行（上排 TASK 5 + 下排 LOADOUT 5）+ 两个分组标题 + 标题栏 + 状态栏
pub const OVERLAY_DESIGN_H: f32 = 536.0;

fn default_true() -> bool {
    true
}

fn default_toggle_hotkey() -> String {
    "ctrl+shift+f7".into()
}

fn default_opacity() -> f32 {
    0.92
}

/// 未设置位置（首次显示时自动贴到游戏窗口右上角）
fn default_position() -> i32 {
    -1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompactModeConfig {
    /// 是否启用紧凑模式 Overlay（关闭后快捷键不再唤出浮窗）
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 显示 / 隐藏 Overlay
    #[serde(default = "default_toggle_hotkey")]
    pub toggle_hotkey: String,
    /// Overlay 不透明度（0.35 ~ 1.0）
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// Overlay 屏幕位置（物理像素）；-1 表示未设置
    #[serde(default = "default_position")]
    pub position_x: i32,
    #[serde(default = "default_position")]
    pub position_y: i32,
}

impl Default for CompactModeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            toggle_hotkey: default_toggle_hotkey(),
            opacity: default_opacity(),
            position_x: default_position(),
            position_y: default_position(),
        }
    }
}

impl CompactModeConfig {
    /// 非法值自愈：快捷键归一化、透明度越界、位置离谱
    pub fn sanitize(mut self) -> Self {
        self.toggle_hotkey = crate::hotkey::normalize_hotkey(&self.toggle_hotkey);
        if !self.opacity.is_finite() {
            self.opacity = default_opacity();
        }
        self.opacity = self.opacity.clamp(0.35, 1.0);
        if self.position_x.abs() > 32_000 {
            self.position_x = default_position();
        }
        if self.position_y.abs() > 32_000 {
            self.position_y = default_position();
        }
        self
    }

    pub fn has_position(&self) -> bool {
        self.position_x != default_position() || self.position_y != default_position()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_mode_defaults_are_modifier_combos() {
        let cfg: CompactModeConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.enabled);
        // 自动装配 / 取消已提升为全局 Loadout Sync 配置，不再属于紧凑模式
        assert_eq!(cfg.toggle_hotkey, "ctrl+shift+f7");
        assert!(!cfg.has_position());
        assert!(cfg.opacity > 0.5 && cfg.opacity <= 1.0);
    }

    #[test]
    fn compact_mode_sanitize_clamps_opacity_and_normalizes_hotkeys() {
        let cfg: CompactModeConfig = serde_json::from_str::<CompactModeConfig>(
            r#"{"opacity":5.0,"toggle_hotkey":"Ctrl + Shift + F7","position_x":999999}"#,
        )
        .unwrap()
        .sanitize();
        assert_eq!(cfg.opacity, 1.0);
        assert_eq!(cfg.toggle_hotkey, "ctrl+shift+f7");
        assert_eq!(cfg.position_x, -1);
    }
}
