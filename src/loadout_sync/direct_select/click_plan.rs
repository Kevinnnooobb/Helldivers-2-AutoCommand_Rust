//! 参考实现的目标规划与点击决策
//! （`hd2-preset-helper-0.1.4/src/loadout/direct_select/click_plan.rs`）。
//!
//! ## 职责
//!
//! 从**当前帧已识别出的槽位**里，找出「下一个应该点击的目标」。
//!
//! 与当前项目旧路径的关键区别：
//!
//! * 旧路径用「目标是否可见」的布尔判断 + 固定顺序遍历；
//! * 参考实现把它做成一次**排序**：同一目标若在多个槽位命中
//!   （重复资源 / 分类抖动），按 `gate_quality → match_margin → match_score`
//!   取最优；多个目标同时可见时，按「屏幕从上到下、再从左到右」取第一个。
//!
//! 这样做的意义是**为屏幕上可见的目标优先装配**，而不是死守预设顺序去滚屏找
//! 一个当前不可见的目标 —— 后者正是真实运行超时的行为模式。
//!
//! 本模块是纯函数：不截图、不发送输入。
use crate::loadout_sync::reference_vision::{Classification, ItemKind};
use crate::loadout_sync::types::ImageRect;

/// 一个可点击目标：既包含**目录身份**，也包含**当前帧的实际位置**。
///
/// 两者必须分开：目录提供「是什么」，当前帧提供「点哪里」。
/// 任何用固定屏幕坐标表替代当前帧几何的做法都违反 plan6 §6.2。
#[derive(Debug, Clone, PartialEq)]
pub struct DirectClickTarget {
    pub item_id: String,
    pub match_score: f32,
    pub match_margin: f32,
    pub gate_quality: f32,
    pub slot: SlotRef,
}

/// 当前帧里的一个槽位（几何 + 可选分类）。
#[derive(Debug, Clone, PartialEq)]
pub struct SlotRef {
    pub row: u32,
    pub col: u32,
    pub rect: ImageRect,
    /// 该槽位是否**可选中**该类型的物品。
    ///
    /// Stratagem 与 Booster 是两个不同的列表，且 Home 与 List 的可选性不同，
    /// 因此这个判断必须由调用方按 `ItemKind` + 界面给出，不能在这里猜。
    pub selectable: bool,
    /// 槽位上是否有内容（与「认得出是什么」无关）。
    ///
    /// Home 空槽必须靠这个字段判断：`classification == None` 只说明
    /// **认不出**，并不说明槽位是空的（Mod 替换图标 / 分割失败都会是 `None`）。
    /// 用 `classification` 判空会导致「点开已填槽位并覆盖」这类误动作。
    pub occupied: bool,
    pub classification: Option<Classification>,
}

impl SlotRef {
    pub fn center(&self) -> (i32, i32) {
        self.rect.center()
    }

    /// 该槽位是否可参与本类型的目标规划。
    pub fn is_selectable_item_for(&self, kind: ItemKind) -> bool {
        let _ = kind;
        self.selectable
    }
}

/// 在**一个**目标 ID 上找最优槽位。
///
/// 多个槽位命中同一 ID 时按 `gate_quality → match_margin → match_score` 取最优。
/// 全部并列时才依赖几何（此处不加几何偏置，交由调用方决定）。
pub fn find_visible_target(
    slots: &[SlotRef],
    item_id: &str,
    kind: ItemKind,
) -> Option<DirectClickTarget> {
    slots
        .iter()
        .filter_map(|slot| {
            if !slot.is_selectable_item_for(kind) {
                return None;
            }
            let classification = slot.classification.as_ref()?;
            if classification.item_id != item_id {
                return None;
            }
            Some(DirectClickTarget {
                item_id: classification.item_id.clone(),
                match_score: classification.match_score,
                match_margin: classification.match_margin,
                gate_quality: classification.gate_quality,
                slot: slot.clone(),
            })
        })
        .max_by(|left, right| {
            left.gate_quality
                .total_cmp(&right.gate_quality)
                .then_with(|| left.match_margin.total_cmp(&right.match_margin))
                .then_with(|| left.match_score.total_cmp(&right.match_score))
        })
}

/// 在若干剩余目标里，选**当前位置最靠上（再靠左）**的一个。
///
/// 这就是「可见优先」策略：先装屏幕上已经看到的，避免为不可见目标反复滚屏。
pub fn next_visible_target(
    slots: &[SlotRef],
    remaining: &[String],
    kind: ItemKind,
) -> Option<DirectClickTarget> {
    remaining
        .iter()
        .filter_map(|item_id| find_visible_target(slots, item_id, kind))
        .min_by(compare_center_then_x)
}

/// 从上到下、再从左右比较（参考实现口径）。
pub fn compare_center_then_x(
    left: &DirectClickTarget,
    right: &DirectClickTarget,
) -> std::cmp::Ordering {
    let left_center_y = left.slot.rect.y as f32 + left.slot.rect.h as f32 * 0.5;
    let right_center_y = right.slot.rect.y as f32 + right.slot.rect.h as f32 * 0.5;
    left_center_y
        .total_cmp(&right_center_y)
        .then_with(|| left.slot.rect.x.cmp(&right.slot.rect.x))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(row: u32, col: u32, x: i32, y: i32, id: Option<&str>) -> SlotRef {
        SlotRef {
            row,
            col,
            rect: ImageRect::new(x, y, 80, 80),
            selectable: true,
            occupied: true,
            classification: id.map(|i| Classification {
                item_id: i.to_string(),
                match_score: 0.80,
                match_margin: 0.10,
                gate_quality: 1.0,
            }),
        }
    }

    fn with_scores(
        row: u32,
        x: i32,
        y: i32,
        id: &str,
        score: f32,
        margin: f32,
        gate: f32,
    ) -> SlotRef {
        SlotRef {
            row,
            col: 0,
            rect: ImageRect::new(x, y, 80, 80),
            selectable: true,
            occupied: true,
            classification: Some(Classification {
                item_id: id.to_string(),
                match_score: score,
                match_margin: margin,
                gate_quality: gate,
            }),
        }
    }

    #[test]
    fn finds_target_by_catalog_id() {
        let slots = vec![
            slot(0, 0, 0, 0, Some("machine-gun")),
            slot(0, 1, 100, 0, Some("autocannon")),
        ];
        let t = find_visible_target(&slots, "autocannon", ItemKind::Stratagem).expect("应命中");
        assert_eq!(t.item_id, "autocannon");
        assert_eq!(t.slot.center(), (140, 40));
    }

    #[test]
    fn missing_or_unclassified_target_is_none() {
        let slots = vec![
            slot(0, 0, 0, 0, Some("machine-gun")),
            slot(0, 1, 100, 0, None),
        ];
        assert!(find_visible_target(&slots, "railgun", ItemKind::Stratagem).is_none());
        // 未分类的槽位不能被当成身份
        assert!(find_visible_target(&slots, "autocannon", ItemKind::Stratagem).is_none());
    }

    #[test]
    fn non_selectable_slots_are_ignored() {
        let mut s = slot(0, 0, 0, 0, Some("machine-gun"));
        s.selectable = false;
        assert!(find_visible_target(&[s], "machine-gun", ItemKind::Stratagem).is_none());
    }

    #[test]
    fn duplicate_hits_prefer_gate_then_margin_then_score() {
        let slots = vec![
            // gate 低但 score 高
            with_scores(0, 0, 0, "machine-gun", 0.99, 0.50, 0.60),
            // gate 高 → 应该赢
            with_scores(1, 100, 0, "machine-gun", 0.70, 0.10, 0.95),
        ];
        let t = find_visible_target(&slots, "machine-gun", ItemKind::Stratagem).expect("命中");
        assert!((t.gate_quality - 0.95).abs() < 1e-6, "应优先 gate_quality");

        // gate 相同 → 比 margin
        let slots2 = vec![
            with_scores(0, 0, 0, "x", 0.99, 0.05, 0.90),
            with_scores(1, 100, 0, "x", 0.70, 0.30, 0.90),
        ];
        let t2 = find_visible_target(&slots2, "x", ItemKind::Stratagem).expect("命中");
        assert!(
            (t2.match_margin - 0.30).abs() < 1e-6,
            "gate 相同时应优先 margin"
        );

        // gate + margin 相同 → 比 score
        let slots3 = vec![
            with_scores(0, 0, 0, "y", 0.71, 0.20, 0.90),
            with_scores(1, 100, 0, "y", 0.88, 0.20, 0.90),
        ];
        let t3 = find_visible_target(&slots3, "y", ItemKind::Stratagem).expect("命中");
        assert!((t3.match_score - 0.88).abs() < 1e-6, "应优先 score");
    }

    #[test]
    fn next_visible_target_picks_topmost_then_leftmost() {
        let slots = vec![
            slot(0, 0, 0, 300, Some("c")),
            slot(0, 1, 100, 100, Some("b")),
            slot(0, 2, 50, 100, Some("a")), // 与 b 同 y，x 更小 → 赢
            slot(1, 0, 0, 500, Some("d")),
        ];
        let remaining = vec![
            "c".to_string(),
            "b".to_string(),
            "a".to_string(),
            "d".to_string(),
        ];
        let t = next_visible_target(&slots, &remaining, ItemKind::Stratagem).expect("命中");
        assert_eq!(t.item_id, "a", "应选最靠上再靠左的可见目标");
    }

    #[test]
    fn next_visible_target_is_none_when_nothing_visible() {
        let slots = vec![slot(0, 0, 0, 0, Some("machine-gun"))];
        let remaining = vec!["railgun".to_string()];
        assert!(next_visible_target(&slots, &remaining, ItemKind::Stratagem).is_none());
        // 空 remaining
        assert!(next_visible_target(&slots, &[], ItemKind::Stratagem).is_none());
    }

    #[test]
    fn visible_priority_avoids_scrolling_for_an_invisible_target() {
        // 预设顺序是 [滚屏目标, 很远的可见目标]；可见优先应选后者的位置
        let slots = vec![
            slot(0, 0, 0, 10, Some("visible-far")),
            slot(1, 0, 0, 200, Some("visible-near")),
        ];
        let remaining = vec!["offscreen".to_string(), "visible-far".to_string()];
        let t = next_visible_target(&slots, &remaining, ItemKind::Stratagem).expect("命中");
        assert_eq!(t.item_id, "visible-far");
    }
}
