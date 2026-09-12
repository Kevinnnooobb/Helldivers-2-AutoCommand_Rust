// 视觉识别系统 —— Helldivers 2 战备图标识别（需求 §二 / §38）。
//
// 设计原则（需求 §二）：
//   A. 几何负责回答「在哪里」           → calibration.rs / grid.rs / crop.rs / roi.rs
//   B. 图像分割负责回答「图标是什么区域」 → segment.rs / components.rs / normalize.rs
//   C. 模板 / 特征负责回答「像哪个已知图标」 → template.rs / id.rs
//   D. 神经网络只解决传统方法解决不了的情况（本阶段不引入）
//   E. OCR 不参与战备图标识别；F. YOLO 不作为第一方案
//   G. 不允许用模型复杂度掩盖几何定位问题
//
// 与 `loadout_sync` 的关系：
//   * 复用其捕获帧、标定模型、图标资源、以及逐帧实测几何识别器；
//   * 本模块**只读**，不做任何输入模拟（不按键、不点击、不修改 loadout），
//     上层通过 `recognize::LoadoutRecognition` 取结果（需求 §33）。
pub mod adapter;
pub mod calibration;
pub mod cli;
pub mod components;
pub mod config;
pub mod crop;
pub mod dataset;
pub mod debug;
pub mod error;
pub mod geometry;
pub mod grid;
pub mod id;
pub mod normalize;
#[cfg(test)]
mod real_fixture_tests;
pub mod recognize;
/// 参考实现（`hd2-preset-helper-0.1.4`）图标目录的迁移层。
pub mod reference_catalog;
pub mod roi;
pub mod segment;
pub mod template;

// 公开面刻意保持最小：不在这里做全量 re-export。
//
// 视觉模块的使用方一律通过 `vision::<module>::Type` 精确引用。
// 全量 re-export 会让每个内部类型都变成「公开但从未使用」的 dead API 警告，
// 而用 `#[allow(dead_code)]` 掩盖是被明令禁止的。
