// Loadout Sync —— H2AC 下排槽位（Slot 06~10）→ HELLDIVERS 2 配装界面的视觉自动化。
//
// 边界（硬性约束）：
//   * 不修改游戏快捷键 / 配置文件 / 资源 / 内存 / 网络；
//   * 不注入 DLL、不使用游戏内部 API；
//   * 只使用「截图识别 + 正常鼠标输入」，等价于用户手动操作；
//   * 与 executor.rs（战备指令键盘注入）完全独立，上排 Slot 01~05 不受影响。
//
// 动作权威（plan7）：整轮装配**完全由参考实现原文承担** ——
//   `loadout::apply_empty_loadout_preset` / `loadout::apply_booster_from_home`
//   （`src/loadout/direct_select.rs`），
//   捕获与输入来自参考 `src/capture/*` + `src/input/windows.rs`，
//   识别来自参考 `src/vision/*`，标定来自内嵌 `data/calibration.json`。
//
// 本模块只保留 H2AC 侧的三样东西，不含任何识别/导航/点击判定：
//   * `catalog_bridge` —— S1：H2AC 图标键 ↔ 参考目录 `item_id`（解析不出即整轮拒答）；
//   * `controller`     —— S3：任务生命周期、日志上报、取消检查、调试帧；
//   * `selection` / `state` / `error` / `config` —— H2AC 的输入模型与共享状态。
//
// edition 2024：本模块不再包含 Win32/DXGI 互操作（已由参考 capture/input 承担）。
pub mod catalog_bridge;
pub mod config;
pub mod controller;
pub mod error;
/// plan7 Step 6 的真实帧对照探针（仅测试构建，非生产路径）。
#[cfg(test)]
mod fixture_compare;
pub mod selection;
pub mod state;
