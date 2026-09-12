//! 参考实现的分页关系判定（`hd2-preset-helper-0.1.4/src/loadout/direct_select/page_relation.rs`）。
//!
//! ## 为什么迁移这一段
//!
//! 当前项目的 `list_map::relocalize` 用「行签名」判断滚动位移：它比较两帧的
//! 行级亮度签名，在**内容被替换**或**只有部分行重叠**时容易得到
//! `delta_rows = 0`（实测真实运行里反复出现 `delta_rows=0 matched=5`），
//! 于是控制器只能靠「连续零位移」这种启发式收尾。
//!
//! 参考实现换了一个更可靠的锚：
//!
//! > 只用**两帧都成功识别、且是同一个 item_id、且在同一列**的槽位，
//! > 取它们垂直位移的中位数。
//!
//! 这个锚天然对「分类名不同就不要」和「跨列不算」免疫，
//! 因此不会因为分类标题行、说明面板或动画抖动而被骗。
//! 内容整体被替换（没有任何共同身份）时给出 `DifferentViewport`，
//! 而不是错误地宣布「没动」。
//!
//! 本模块是**纯函数**：不截图、不发送输入、不读时间。时间与输入的编排见
//! `page_navigation`。
use crate::loadout_sync::list_map::ScrollDirection;

/// 允许的几何抖动（参考实现 `PAGE_SHIFT_JITTER_PX = 2.0`）。
pub const PAGE_SHIFT_JITTER_PX: f32 = 2.0;

/// 参考 ROI 高度（参考实现按 `ROI_REFERENCE_H = 832` 折算期望位移）。
pub const ROI_REFERENCE_H: f32 = 832.0;

/// 一次完整翻页在参考 ROI 里大约移动的像素（参考实现 `338.0`）。
pub const PAGE_TURN_SHIFT_REFERENCE_PX: f32 = 338.0;

/// 位移比例低于该值视为「短翻页」（参考实现 `0.80`）。
pub const PAGE_TURN_SHORT_THRESHOLD_RATIO: f32 = 0.80;

/// 一个参与身份比对的槽位。
///
/// 刻意只带「判定所需」的字段：行列、矩形、以及**已分类的 item_id**。
/// 没有分类结果的槽位不参与比对（否则会把未识别当成身份）。
#[derive(Debug, Clone, PartialEq)]
pub struct TrackedSlot {
    pub row: u32,
    pub col: u32,
    pub rect: crate::loadout_sync::types::ImageRect,
    /// 该槽位被分类到的目录 ID（`None` = 未分类，不参与身份比对）。
    pub item_id: Option<String>,
}

impl TrackedSlot {
    pub fn center_f32(&self) -> (f32, f32) {
        (
            self.rect.x as f32 + self.rect.w as f32 / 2.0,
            self.rect.y as f32 + self.rect.h as f32 / 2.0,
        )
    }

    /// 是否可参与身份比对：必须有分类结果。
    pub fn is_identity(&self) -> bool {
        self.item_id.is_some()
    }
}

/// 两帧分页之间的关系。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PageRelation {
    /// 视口没动（位移在抖动范围内）
    SameViewport,
    /// 视口按预期移动了
    Shifted(PageShift),
    /// 没有共同身份 —— 内容整体换了（或换得太多测不出位移）
    DifferentViewport,
    /// 有共同身份但位移方向/幅度无法解释为一次翻页
    Uncertain,
}

impl PageRelation {
    pub fn label(self) -> &'static str {
        match self {
            Self::SameViewport => "same_viewport",
            Self::Shifted(_) => "shifted",
            Self::DifferentViewport => "different_viewport",
            Self::Uncertain => "uncertain",
        }
    }
}

/// 一次位移的量化结果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageShift {
    /// 与滚动方向一致的位移（正式值，已按方向取号）
    pub directed_shift: f32,
    /// 占「一次完整翻页」的比例
    pub shift_ratio: f32,
}

impl PageShift {
    /// 是否是短翻页（位移明显小于一整页）。
    pub fn is_short(&self) -> bool {
        self.shift_ratio < PAGE_TURN_SHORT_THRESHOLD_RATIO
    }
}

/// 判定两帧之间的分页关系。
///
/// `roi_height` 是当前帧 ROI 的高度，用于把 338px 的参考位移折算到当前分辨率。
pub fn compare_page_turn(
    previous: &[TrackedSlot],
    current: &[TrackedSlot],
    direction: ScrollDirection,
    roi_height: f32,
) -> PageRelation {
    // 任一侧没有可用的身份锚 → 无法判定（不是「没动」，也不是「动了」）
    if previous.iter().all(|s| !s.is_identity()) || current.iter().all(|s| !s.is_identity()) {
        return PageRelation::Uncertain;
    }

    let Some(median_dy) = common_identity_vertical_shift(previous, current) else {
        // 两侧都有身份，却找不到任何共同身份 → 内容整体被替换
        return PageRelation::DifferentViewport;
    };

    if median_dy.abs() <= PAGE_SHIFT_JITTER_PX {
        return PageRelation::SameViewport;
    }

    let directed_shift = match direction {
        ScrollDirection::Down => -median_dy,
        ScrollDirection::Up => median_dy,
    };
    if directed_shift <= PAGE_SHIFT_JITTER_PX {
        return PageRelation::Uncertain;
    }

    let expected_full_shift = PAGE_TURN_SHIFT_REFERENCE_PX * roi_height / ROI_REFERENCE_H;
    let shift_ratio = if expected_full_shift > 0.0 {
        directed_shift / expected_full_shift
    } else {
        0.0
    };
    PageRelation::Shifted(PageShift {
        directed_shift,
        shift_ratio,
    })
}

/// 两帧之间「共同身份」的垂直位移中位数。
///
/// 匹配条件（缺一不可）：
///   * 两帧都有分类结果；
///   * `item_id` 相同；
///   * `col` 相同（跨列不算同一个格子）。
///
/// 同一个当前槽位只能被匹配一次（按水平距离取最近者），
/// 避免一个格子被多个旧槽位重复计入。
pub fn common_identity_vertical_shift(
    previous: &[TrackedSlot],
    current: &[TrackedSlot],
) -> Option<f32> {
    let mut used_current = vec![false; current.len()];
    let mut shifts: Vec<f32> = Vec::new();

    for previous_slot in previous.iter().filter(|s| s.is_identity()) {
        let Some(previous_id) = previous_slot.item_id.as_deref() else {
            continue;
        };
        let (previous_x, previous_y) = previous_slot.center_f32();

        let best = current
            .iter()
            .enumerate()
            .filter(|(index, slot)| !used_current[*index] && slot.is_identity())
            .filter_map(|(index, slot)| {
                if slot.item_id.as_deref() != Some(previous_id) || slot.col != previous_slot.col {
                    return None;
                }
                let (current_x, current_y) = slot.center_f32();
                let dx = (current_x - previous_x).abs();
                Some((index, dx, current_y - previous_y))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1));

        if let Some((index, _, dy)) = best {
            used_current[index] = true;
            shifts.push(dy);
        }
    }

    interpolated_median(&mut shifts)
}

/// 中位数（偶数个取中间两个的均值）。
fn interpolated_median(values: &mut [f32]) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f32::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 0 {
        Some((values[middle - 1] + values[middle]) * 0.5)
    } else {
        Some(values[middle])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadout_sync::types::ImageRect;

    fn slot(row: u32, col: u32, y: i32, id: Option<&str>) -> TrackedSlot {
        TrackedSlot {
            row,
            col,
            rect: ImageRect::new(100 + col as i32 * 100, y, 80, 80),
            item_id: id.map(|s| s.to_string()),
        }
    }

    const H: f32 = 832.0;

    #[test]
    fn identical_viewport_is_same() {
        let a = vec![
            slot(0, 0, 100, Some("machine-gun")),
            slot(0, 1, 100, Some("autocannon")),
        ];
        let b = a.clone();
        assert_eq!(
            compare_page_turn(&a, &b, ScrollDirection::Down, H),
            PageRelation::SameViewport
        );
    }

    #[test]
    fn small_jitter_is_still_same() {
        let a = vec![slot(0, 0, 100, Some("machine-gun"))];
        let b = vec![slot(0, 0, 102, Some("machine-gun"))]; // 2px == 抖动上限
        assert_eq!(
            compare_page_turn(&a, &b, ScrollDirection::Down, H),
            PageRelation::SameViewport
        );
    }

    #[test]
    fn expected_direction_shift_is_reported_with_ratio() {
        // Down 滚动 → 内容向上走 → dy 为负
        let a = vec![
            slot(0, 0, 400, Some("machine-gun")),
            slot(1, 0, 500, Some("autocannon")),
            slot(2, 0, 600, Some("stalwart")),
        ];
        let b = vec![
            slot(0, 0, 62, Some("machine-gun")),
            slot(1, 0, 162, Some("autocannon")),
            slot(2, 0, 262, Some("stalwart")),
        ];
        match compare_page_turn(&a, &b, ScrollDirection::Down, H) {
            PageRelation::Shifted(s) => {
                assert!(
                    (s.directed_shift - 338.0).abs() < 1.0,
                    "got {}",
                    s.directed_shift
                );
                assert!((s.shift_ratio - 1.0).abs() < 0.01);
                assert!(!s.is_short());
            }
            other => panic!("期望 Shifted，实际 {other:?}"),
        }
    }

    #[test]
    fn wrong_direction_is_uncertain_not_shifted() {
        // 期望 Down，但内容实际上往下走（dy 为正）
        let a = vec![slot(0, 0, 100, Some("machine-gun"))];
        let b = vec![slot(0, 0, 200, Some("machine-gun"))];
        assert_eq!(
            compare_page_turn(&a, &b, ScrollDirection::Down, H),
            PageRelation::Uncertain
        );
    }

    #[test]
    fn fully_replaced_content_is_different_viewport() {
        let a = vec![
            slot(0, 0, 100, Some("machine-gun")),
            slot(1, 0, 200, Some("autocannon")),
        ];
        let b = vec![
            slot(0, 0, 100, Some("railgun")),
            slot(1, 0, 200, Some("quasar-cannon")),
        ];
        assert_eq!(
            compare_page_turn(&a, &b, ScrollDirection::Down, H),
            PageRelation::DifferentViewport
        );
    }

    #[test]
    fn cross_column_match_is_not_an_identity() {
        // 同一个 item 但换了列 → 不算共同身份（列是格子身份的一部分）
        let a = vec![slot(0, 0, 100, Some("machine-gun"))];
        let b = vec![slot(0, 1, 100, Some("machine-gun"))];
        assert_eq!(
            compare_page_turn(&a, &b, ScrollDirection::Down, H),
            PageRelation::DifferentViewport
        );
    }

    #[test]
    fn unclassified_slots_never_form_identity() {
        let a = vec![slot(0, 0, 100, None)];
        let b = vec![slot(0, 0, 100, None)];
        assert_eq!(
            compare_page_turn(&a, &b, ScrollDirection::Down, H),
            PageRelation::Uncertain
        );
    }

    #[test]
    fn short_page_turn_is_flagged() {
        // 位移 100px / 338px ≈ 0.30 → 短翻页
        let a = vec![slot(0, 0, 300, Some("machine-gun"))];
        let b = vec![slot(0, 0, 200, Some("machine-gun"))];
        match compare_page_turn(&a, &b, ScrollDirection::Down, H) {
            PageRelation::Shifted(s) => {
                assert!(s.is_short(), "ratio={}", s.shift_ratio);
                assert!((s.shift_ratio - 100.0 / 338.0).abs() < 0.01);
            }
            other => panic!("期望 Shifted，实际 {other:?}"),
        }
    }

    #[test]
    fn median_resists_a_single_outlier() {
        let a = vec![
            slot(0, 0, 100, Some("a")),
            slot(1, 0, 200, Some("b")),
            slot(2, 0, 300, Some("c")),
        ];
        // 两个正常位移 -50，一个异常 -2（会被中位数吸收）
        let b = vec![
            slot(0, 0, 50, Some("a")),
            slot(1, 0, 150, Some("b")),
            slot(2, 0, 298, Some("c")),
        ];
        let dy = common_identity_vertical_shift(&a, &b).expect("有共同身份");
        assert!((dy + 50.0).abs() < 0.01, "got {dy}");
    }

    #[test]
    fn each_current_slot_is_used_at_most_once() {
        // 两个旧槽位指向同一个当前槽位（同 id 同列）时，只能计入一次
        let a = vec![slot(0, 0, 100, Some("same")), slot(1, 0, 200, Some("same"))];
        let b = vec![slot(0, 0, 50, Some("same"))];
        let dy = common_identity_vertical_shift(&a, &b).expect("有一个匹配");
        assert!((dy + 50.0).abs() < 0.01, "got {dy}");
    }

    #[test]
    fn interpolated_median_handles_even_and_odd() {
        assert_eq!(interpolated_median(&mut []), None);
        assert_eq!(interpolated_median(&mut [5.0]), Some(5.0));
        assert_eq!(interpolated_median(&mut [1.0, 3.0]), Some(2.0));
        assert_eq!(interpolated_median(&mut [3.0, 1.0, 2.0]), Some(2.0));
        // 偶数个取中间两个均值，保证「取中位」不偏向任一极值
        assert_eq!(
            interpolated_median(&mut [0.0, 10.0, 20.0, 30.0]),
            Some(15.0)
        );
    }
}
