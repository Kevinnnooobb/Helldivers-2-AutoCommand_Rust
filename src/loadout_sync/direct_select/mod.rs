//! `direct_select` —— 参考实现 `hd2-preset-helper-0.1.4` 的确定性装配状态机迁移。
//!
//! ## 迁移定位（plan6 §5）
//!
//! 本模块只负责：**识别当前 UI、规划目标、发送有限输入、验证状态变化**，
//! 并把「成功 / 安全失败」交回 `loadout_sync` 的 controller。
//! 它**不**包含模板评分细节，也**不**直接触碰 preset；底层 recognizer
//! 只提供观察数据，不拥有独立的装配动作路径。
//!
//! ## 迁移状态
//!
//! | 参考实现 | 本模块 | 状态 |
//! | --- | --- | --- |
//! | `loadout/frame.rs` | [`frame`] | ✅ 32×32 luma 指纹 + 距离 |
//! | `direct_select/page_relation.rs` | [`page_relation`] | ✅ 共同身份垂直位移 |
//! | `direct_select/page_navigation.rs` | [`page_navigation`] | ✅ 有界翻页编排 |
//! | `direct_select/hover.rs` | [`hover`] | ✅ 自适应 hover 判据 + 亮度下降 |
//! | `direct_select/click_plan.rs` | [`click_plan`] | ✅ 可见优先的目标规划 |
//! | `direct_select/list_map.rs` | [`list_map`] | ✅ 临时列表映射 + 导航建议 |
//! | `direct_select/home_activation.rs` | [`home_activation`] | ✅ Home 槽位激活 |
//! | `direct_select.rs`（`observe_post_click_state`） | [`selection_verify`] | ✅ 点击后状态变化验证 |
//! | `direct_select.rs`（状态机） | [`machine`] | ✅ 有界状态机 |
//! | `vision/classifier.rs` | — | ⬜ 未迁移（见 `docs/reference-migration.md`） |
//! | `vision/geometry.rs` | — | ⬜ 未迁移（由既有 `vision::geometry` 承担） |
//!
//! ## 动作权威
//!
//! 本模块是当前唯一的装配动作路径。底层截图、几何识别和图标匹配只
//! 提供观察数据，不得在本状态机之外发送装配输入。
pub mod classify;
pub mod click_plan;
pub mod frame;
pub mod home_activation;
pub mod hover;
pub mod list_map;
pub mod machine;
pub mod page_navigation;
pub mod page_relation;
pub mod real_io;
#[cfg(test)]
pub mod replay;
pub mod selection_verify;
pub mod session;
#[cfg(test)]
pub mod zncc_spike;
