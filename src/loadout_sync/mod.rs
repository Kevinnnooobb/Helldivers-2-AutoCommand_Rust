// Loadout Sync —— H2AC 下排槽位（Slot 06~10）→ HELLDIVERS 2 配装界面的视觉自动化。
//
// 边界（硬性约束）：
//   * 不修改游戏快捷键 / 配置文件 / 资源 / 内存 / 网络；
//   * 不注入 DLL、不使用游戏内部 API；
//   * 只使用「截图识别 + 正常鼠标输入」，等价于用户手动操作；
//   * 与 executor.rs（战备指令键盘注入）完全独立，上排 Slot 01~05 不受影响。
//
// 动作权威（plan6 §5.3）：整轮装配只有一条动作路径 —— `direct_select`。
// `controller` 只负责窗口、取消、日志、生命周期；`matcher` / `recognizer` /
// `viewport` / `list_map` / `verifier` / `types` 是它的**底层观察适配**，
// 不允许在状态机之外发送装配输入。
//
// edition 2024：本模块内的 Win32/DXGI 互操作（wgc / window）仍是 2021 时代的写法，
// 其 `unsafe fn` 体内没有逐条 `unsafe` 块。这些文件将在 plan7 Step 9 被参考实现
// 原文取代并整体归档，故此处只做模块级豁免，不为即将删除的代码做逐点改写。
#![allow(unsafe_op_in_unsafe_fn)]
pub mod capture;
pub mod config;
pub mod controller;
/// 参考实现（`hd2-preset-helper-0.1.4`）的确定性装配状态机迁移层。
/// 迁移状态与差距见 `docs/reference-migration.md`。
pub mod direct_select;
pub mod error;
pub mod input;
/// 旧滚动位移适配（生产路径已由 `direct_select::page_relation` 承担；
/// 保留为底层适配契约的回归测试对象）。
pub mod list_map;
/// 图标模板匹配：`direct_select::classify` 的底层观察适配。
pub mod matcher;
/// 逐帧几何与界面识别：`direct_select::real_io` 的底层观察适配。
pub mod recognizer;
/// 参考实现视觉层的类型与常量（`ItemKind` / `SlotKind` / `Classification` /
/// `RoiObservation` / `UiState`）。迁移映射见 `docs/reference-migration.md`。
pub mod reference_vision;
pub mod selection;
pub mod state;
pub mod types;
/// 槽位验证的底层适配契约（保留 `verify_slot_selected`）。
pub mod verifier;
/// Viewport 亮度签名（生产路径已由 `direct_select::page_relation` 承担；
/// 保留为底层适配契约的回归测试对象）。
pub mod viewport;
pub mod wgc;
pub mod window;

#[cfg(test)]
mod tests;
/// 合成帧构造入口 —— 供 test-only 的底层 spike 复用同一份帧构造。
#[cfg(test)]
pub(crate) mod fixture_support {
    pub(crate) use super::tests::make_frame;
}
