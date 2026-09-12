//! 参考实现的临时列表映射
//! （`hd2-preset-helper-0.1.4/src/loadout/direct_select/list_map.rs`）。
//!
//! ## 它解决什么问题
//!
//! 列表是可滚动的虚拟列表：屏幕上只有一部分行可见，
//! 而每一行**没有**稳定的全局行号。要「跳到一个当前不可见的目标」，
//! 必须先知道它大概在可见窗口的上方还是下方。
//!
//! 参考实现的做法是维护一张**临时**映射：
//!
//! * `row_base` = 当前可见窗口第一行对应的全局行号（初始 0）；
//! * `items: item_id → (global_row, col)`，随着每次扫描不断补充；
//! * 每次翻页后先尝试 `relocalize`：用「同一个 item_id 且同列」的槽位
//!   反推 `row_base`（取**众数**，比中位数更抗离群）；
//! * 没有任何共同 identity 时，退化为「按前后两帧的局部行范围拼接近似」。
//!
//! 关键点：**不按固定行号映射分类**。分类身份完全来自每帧实测的
//! `classification.item_id`；行号只是临时坐标。
//!
//! 本模块是纯函数：不截图、不发送输入。
use std::collections::HashMap;

use crate::loadout_sync::list_map::ScrollDirection;
use crate::loadout_sync::reference_vision::{ItemKind, RoiObservation};

/// 一个物品在临时映射里的位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridPosition {
    /// 全局行号（相对于首次建立映射时的可见窗口）
    pub row: i32,
    pub col: u32,
}

/// 导航建议。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationHint {
    /// 目标在可见窗口之外，按该方向翻页
    Scroll(ScrollDirection),
    /// 目标在窗口边缘，稍微回一点避免它刚翻出视野
    Recenter(ScrollDirection),
    /// 目标应该已经可见
    ExpectedVisible,
}

impl NavigationHint {
    /// 仅测试与诊断使用。
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn direction(self) -> Option<ScrollDirection> {
        match self {
            Self::Scroll(d) | Self::Recenter(d) => Some(d),
            Self::ExpectedVisible => None,
        }
    }
}

/// 临时列表映射。
#[derive(Debug, Clone, Default)]
pub struct ListMap {
    row_base: i32,
    items: HashMap<String, GridPosition>,
}

impl ListMap {
    /// 用第一帧建立映射。
    pub fn new(page: &RoiObservation, item_kind: ItemKind) -> Self {
        let mut map = Self {
            row_base: 0,
            items: HashMap::new(),
        };
        map.record_current(page, item_kind);
        map
    }

    /// 仅测试与诊断使用。
    #[cfg(test)]
    pub fn row_base(&self) -> i32 {
        self.row_base
    }

    /// 仅测试与诊断使用。
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 该映射是否为空（一帧都没有可用分类）。
    /// 仅测试与诊断使用。
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 仅测试与诊断使用。
    #[cfg(test)]
    pub fn get(&self, item_id: &str) -> Option<GridPosition> {
        self.items.get(item_id).copied()
    }

    /// 用共同 identity 重新对齐 `row_base`。
    ///
    /// 匹配条件：同一个 `item_id`、同一 `col`；候选 `row_base` 取**众数**。
    /// 找不到任何共同 identity 时返回 `false`（**不修改**自身状态）——
    /// 调用方必须退化到 [`Self::advance`] 的拼接路径，而不是猜。
    pub fn relocalize(&mut self, page: &RoiObservation, item_kind: ItemKind) -> bool {
        let candidates: Vec<i32> = page
            .slots
            .iter()
            .filter(|slot| slot.kind.is_selectable_item_for(item_kind))
            .filter_map(|slot| {
                let item_id = &slot.classification.as_ref()?.item_id;
                let mapped = self.items.get(item_id)?;
                (mapped.col == slot.col).then_some(mapped.row - slot.row as i32)
            })
            .collect();
        let Some(row_base) = dominant_value(&candidates) else {
            return false;
        };
        self.row_base = row_base;
        self.record_current(page, item_kind);
        true
    }

    /// 翻页后推进映射。
    ///
    /// 优先 `relocalize`；失败时按「前后两帧的局部行范围」拼接：
    /// 向下翻时新窗口的第一行接在旧窗口最后一行之后；向上翻反之。
    /// 这是**近似**，但优于「猜 0」或「放弃导航」。
    pub fn advance(
        &mut self,
        previous: &RoiObservation,
        current: &RoiObservation,
        item_kind: ItemKind,
        direction: ScrollDirection,
    ) {
        if self.relocalize(current, item_kind) {
            return;
        }
        let Some((previous_min, previous_max)) = local_row_range(previous, item_kind) else {
            return;
        };
        let Some((current_min, current_max)) = local_row_range(current, item_kind) else {
            return;
        };
        self.row_base = match direction {
            ScrollDirection::Down => self.row_base + previous_max + 1 - current_min,
            ScrollDirection::Up => self.row_base + previous_min - 1 - current_max,
        };
        self.record_current(current, item_kind);
    }

    /// 记录当前帧所有已分类槽位。
    pub fn record_current(&mut self, page: &RoiObservation, item_kind: ItemKind) {
        for slot in page
            .slots
            .iter()
            .filter(|slot| slot.kind.is_selectable_item_for(item_kind))
        {
            let Some(classification) = &slot.classification else {
                continue;
            };
            self.items.insert(
                classification.item_id.clone(),
                GridPosition {
                    row: self.row_base + slot.row as i32,
                    col: slot.col,
                },
            );
        }
    }

    /// 目标不在映射里时的默认方向。
    ///
    /// 参考实现选择向下列表（战备列表按分类顺序排列，未见过的大概率在下方）。
    /// 这里保留同样的默认值，但调用方应当把它当作**未知**处理，
    /// 并结合翻页次数上限决定何时放弃。
    pub const UNKNOWN_TARGET_HINT: NavigationHint = NavigationHint::Scroll(ScrollDirection::Down);

    /// 给出「目标在哪个方向」的建议。
    pub fn navigation_hint(
        &self,
        item_id: &str,
        page: &RoiObservation,
        item_kind: ItemKind,
    ) -> NavigationHint {
        let Some(target) = self.items.get(item_id) else {
            return Self::UNKNOWN_TARGET_HINT;
        };
        let Some((local_min, local_max)) = local_row_range(page, item_kind) else {
            return NavigationHint::ExpectedVisible;
        };
        let visible_min = self.row_base + local_min;
        let visible_max = self.row_base + local_max;

        if target.row < visible_min {
            NavigationHint::Scroll(ScrollDirection::Up)
        } else if target.row > visible_max {
            NavigationHint::Scroll(ScrollDirection::Down)
        } else if local_min != local_max && target.row == visible_min {
            // 在窗口最上一行：再往上翻一点，避免它在滚动中消失
            NavigationHint::Recenter(ScrollDirection::Up)
        } else if local_min != local_max && target.row == visible_max {
            NavigationHint::Recenter(ScrollDirection::Down)
        } else {
            NavigationHint::ExpectedVisible
        }
    }
}

/// 当前页里该类型槽位的局部行范围。
fn local_row_range(page: &RoiObservation, item_kind: ItemKind) -> Option<(i32, i32)> {
    let mut rows = page
        .slots
        .iter()
        .filter(|slot| slot.kind.is_selectable_item_for(item_kind))
        .map(|slot| slot.row as i32);
    let first = rows.next()?;
    Some(rows.fold((first, first), |(min, max), row| {
        (min.min(row), max.max(row))
    }))
}

/// 取众数（并列时取较大值，保证确定性）。
fn dominant_value(values: &[i32]) -> Option<i32> {
    values.iter().copied().max_by_key(|candidate| {
        (
            values.iter().filter(|value| **value == *candidate).count(),
            *candidate,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadout_sync::reference_vision::{Classification, ObservedSlot, SlotKind};
    use crate::loadout_sync::types::ImageRect;

    fn page(entries: &[(u32, u32, Option<&str>)]) -> RoiObservation {
        let slots = entries
            .iter()
            .map(|(row, col, id)| ObservedSlot {
                row: *row,
                col: *col,
                rect: ImageRect::new(*col as i32 * 90, *row as i32 * 90, 80, 80),
                kind: SlotKind::ListStratagem,
                occupied: true,
                classification: id.map(|i| Classification::new(i, 0.8, 0.1, 1.0)),
            })
            .collect();
        RoiObservation::new(slots, 832.0, vec![])
    }

    #[test]
    fn new_records_the_first_window_at_row_base_zero() {
        let p = page(&[(0, 0, Some("a")), (1, 0, Some("b"))]);
        let m = ListMap::new(&p, ItemKind::Stratagem);
        assert_eq!(m.row_base(), 0);
        assert_eq!(m.get("a"), Some(GridPosition { row: 0, col: 0 }));
        assert_eq!(m.get("b"), Some(GridPosition { row: 1, col: 0 }));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn relocalize_recovers_row_base_from_shared_identity() {
        let first = page(&[(0, 0, Some("a")), (1, 0, Some("b"))]);
        let mut m = ListMap::new(&first, ItemKind::Stratagem);
        // 向下滚 3 行：a,b 现在在局部 row 3,4
        let second = page(&[(3, 0, Some("a")), (4, 0, Some("b")), (5, 0, Some("c"))]);
        assert!(m.relocalize(&second, ItemKind::Stratagem), "有共同身份");
        assert_eq!(m.row_base(), -3);
        // a 的全局行号仍是 0
        assert_eq!(m.get("a").map(|g| g.row), Some(0));
        // c 是新增的，全局行号 = -3 + 5 = 2
        assert_eq!(m.get("c").map(|g| g.row), Some(2));
    }

    #[test]
    fn relocalize_uses_dominant_value_and_ignores_outliers() {
        let first = page(&[
            (0, 0, Some("a")),
            (1, 0, Some("b")),
            (2, 0, Some("c")),
            (3, 0, Some("d")),
        ]);
        let mut m = ListMap::new(&first, ItemKind::Stratagem);
        // 三个一致（row_base -5）+ 一个离群（会给出 -99）
        let second = page(&[
            (5, 0, Some("a")),
            (6, 0, Some("b")),
            (7, 0, Some("c")),
            (102, 0, Some("d")),
        ]);
        assert!(m.relocalize(&second, ItemKind::Stratagem));
        assert_eq!(m.row_base(), -5, "众数应压掉离群候选");
    }

    #[test]
    fn cross_column_match_is_rejected() {
        let first = page(&[(0, 0, Some("a"))]);
        let mut m = ListMap::new(&first, ItemKind::Stratagem);
        // 同一个 id 但列变了 → 不算 landmark
        let second = page(&[(0, 2, Some("a"))]);
        assert!(!m.relocalize(&second, ItemKind::Stratagem));
        assert_eq!(m.row_base(), 0, "relocalize 失败不得修改状态");
    }

    #[test]
    fn advance_falls_back_to_range_splicing_without_landmarks() {
        let first = page(&[(0, 0, Some("a")), (1, 0, Some("b"))]);
        let mut m = ListMap::new(&first, ItemKind::Stratagem);
        // 全新内容（无共同身份），局部行 0..2
        let second = page(&[(0, 0, Some("x")), (1, 0, Some("y")), (2, 0, Some("z"))]);
        m.advance(&first, &second, ItemKind::Stratagem, ScrollDirection::Down);
        // row_base = 0 + previous_max(1) + 1 - current_min(0) = 2
        assert_eq!(m.row_base(), 2);
        assert_eq!(m.get("x").map(|g| g.row), Some(2));

        // 向上翻回：局部行 0..2，应接在旧窗口之前
        let third = page(&[(0, 0, Some("p")), (1, 0, Some("q"))]);
        m.advance(&second, &third, ItemKind::Stratagem, ScrollDirection::Up);
        // row_base = row_base(2) + current_min(0) - 1 - current_max(1) = 0
        assert_eq!(m.row_base(), 0);
        assert_eq!(m.get("p").map(|g| g.row), Some(0));
    }

    #[test]
    fn navigation_hint_points_to_the_target_direction() {
        let p = page(&[(0, 0, Some("a")), (1, 0, Some("b"))]);
        let mut m = ListMap::new(&p, ItemKind::Stratagem);
        // 登记一个远在下方的目标（直接写入内部映射，与生产同一条路径）
        m.items
            .insert("far-below".to_string(), GridPosition { row: 50, col: 0 });
        assert_eq!(
            m.navigation_hint("far-below", &p, ItemKind::Stratagem),
            NavigationHint::Scroll(ScrollDirection::Down)
        );
        // 远在上方
        m.items
            .insert("far-above".to_string(), GridPosition { row: -50, col: 0 });
        assert_eq!(
            m.navigation_hint("far-above", &p, ItemKind::Stratagem),
            NavigationHint::Scroll(ScrollDirection::Up)
        );
        // 未知目标 → 默认向下（调用方须按未知处理）
        assert_eq!(
            m.navigation_hint("never-seen", &p, ItemKind::Stratagem),
            ListMap::UNKNOWN_TARGET_HINT
        );
    }

    #[test]
    fn navigation_hint_recenters_at_window_edges() {
        let p = page(&[
            (0, 0, Some("top")),
            (1, 0, Some("mid")),
            (2, 0, Some("bot")),
        ]);
        let m = ListMap::new(&p, ItemKind::Stratagem);
        // 目标在最上一行 → 建议向上回一点（避免滚动中丢失）
        assert_eq!(
            m.navigation_hint("top", &p, ItemKind::Stratagem),
            NavigationHint::Recenter(ScrollDirection::Up)
        );
        // 目标在最下一行 → 向下回一点
        assert_eq!(
            m.navigation_hint("bot", &p, ItemKind::Stratagem),
            NavigationHint::Recenter(ScrollDirection::Down)
        );
        // 中间行 → 可见
        assert_eq!(
            m.navigation_hint("mid", &p, ItemKind::Stratagem),
            NavigationHint::ExpectedVisible
        );
    }

    #[test]
    fn single_visible_row_never_recenters() {
        // 只有一行可见时，窗口边缘 == 窗口中心，不应该建议 recenter
        let p = page(&[(0, 0, Some("only"))]);
        let m = ListMap::new(&p, ItemKind::Stratagem);
        assert_eq!(
            m.navigation_hint("only", &p, ItemKind::Stratagem),
            NavigationHint::ExpectedVisible
        );
    }

    #[test]
    fn booster_map_is_separate_from_stratagem_map() {
        let strat = page(&[(0, 0, Some("machine-gun"))]);
        let mut booster = RoiObservation::new(
            vec![ObservedSlot {
                row: 0,
                col: 0,
                rect: ImageRect::new(0, 0, 80, 80),
                kind: SlotKind::ListBooster,
                occupied: true,
                classification: Some(Classification::new("experimental-infusion", 0.9, 0.2, 1.0)),
            }],
            832.0,
            vec![],
        );
        booster.fingerprint = vec![1];
        // 战备映射不应收录 Booster 槽
        let m = ListMap::new(&strat, ItemKind::Stratagem);
        assert!(m.get("experimental-infusion").is_none());
        // Booster 映射应收录 Booster
        let b = ListMap::new(&booster, ItemKind::Booster);
        assert!(b.get("experimental-infusion").is_some());
        // 战备映射对 Booster 页面为空
        let empty = ListMap::new(&booster, ItemKind::Stratagem);
        assert!(empty.is_empty());
    }

    #[test]
    fn dominant_value_picks_mode_with_deterministic_tie_break() {
        assert_eq!(dominant_value(&[]), None);
        assert_eq!(dominant_value(&[7]), Some(7));
        assert_eq!(dominant_value(&[1, 2, 2, 3, 3, 3]), Some(3));
        // 并列时取较大值（确定性）
        assert_eq!(dominant_value(&[1, 1, 2, 2]), Some(2));
    }
}
