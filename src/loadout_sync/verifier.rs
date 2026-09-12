// 验证层 —— 点击后槽位验证（`verify_slot_selected`）。
//
// plan6 §5.3 之后，整轮装配的动作与验证都在 `direct_select` 里：
//   * 点击前的 hover 判定 → `direct_select::hover`（自适应判据 + 亮度下降）；
//   * 点击后的选中判定 → `direct_select::selection_verify`（共同位移 / 边框亮度下降）；
//   * 最终 Home 判定 → `direct_select` 的 HomeFilled 终态。
//
// 本模块因此只保留一条**底层适配契约**：给定一帧、槽位几何和目标图标，
// 判断该槽位当前显示的确实是目标（供探针与底层回归测试使用）。
use crate::loadout_sync::capture::CapturedFrame;
use crate::loadout_sync::error::LoadoutSyncError;
use crate::loadout_sync::matcher::{CellKind, IconMatcher};
use crate::loadout_sync::selection::{LoadoutItem, TargetSlot};
use crate::loadout_sync::types::GameLoadoutSlots;

/// 点击后：确认对应游戏槽位已经显示目标图标（选中验证的核心条件）。
///
/// `CellKind` 必须与**被验证的界面**一致：这里验证的是 Home 槽位，
/// 所以战备用 `HomeStratagem`、Booster 用 `HomeBooster`。
///
/// 实机实测（`probe_home_verify_cellkind`，真实 Home 帧上的同一槽位）：
/// 用 `ListStratagem` 会比 `HomeStratagem` 低 0.03~0.05，
/// 因为它按列表的 `LIST_GLYPH_SCALE = 0.654` 而不是 Home 的
/// `HOME_GLYPH_SCALE = 0.894` 去缩放资源，两者尺子不同。
#[allow(dead_code)] // 底层适配契约：由探针/回归测试直接调用
pub fn verify_slot_selected(
    matcher: &IconMatcher,
    frame: &CapturedFrame,
    slots: &GameLoadoutSlots,
    target: TargetSlot,
    item: &LoadoutItem,
    threshold: f32,
) -> Result<f32, LoadoutSyncError> {
    let (rect, kind) = match target {
        TargetSlot::Stratagem(i) => (
            slots
                .stratagem(i)
                .ok_or(LoadoutSyncError::SlotNotDetected { slot: i + 1 })?
                .rect,
            CellKind::HomeStratagem,
        ),
        TargetSlot::Booster => (slots.booster.rect, CellKind::HomeBooster),
    };
    let score = matcher
        .score_cell_for(frame, rect, item, kind)
        .unwrap_or(0.0);
    if score < threshold {
        return Err(LoadoutSyncError::SelectionVerificationFailed {
            name: item.name.clone(),
        });
    }
    Ok(score)
}
