// 紧凑模式（游戏内配装预设 Overlay）
//
// 职责划分：
//   * config —— 快捷键 / 透明度 / 位置等可持久化配置；
//   * preset —— 预设草稿模型与 Overlay 生命周期（纯逻辑，可单测）；
//   * UI（src/ui/preset_overlay.rs）与窗口行为（src/overlay_win.rs）单独成文件。
//
// 与 Loadout Sync 的关系：紧凑模式只负责「编辑预设 + 触发自动装配」，
// 视觉识别 / 鼠标注入仍然全部在 loadout_sync 工作线程里完成。
pub mod config;
pub mod preset;

pub use preset::CompactPresetState;
