//! 点击后的状态变化验证
//! （`hd2-preset-helper-0.1.4/src/loadout/direct_select.rs` 的
//! `observe_post_click_state` / `track_post_click_slot` / `wait_for_terminal_home`）。
//!
//! ## 为什么这是本迁移最关键的一段
//!
//! 当前项目的 `SelectionVerificationFailed` 来自
//! `verifier::verify_slot_selected`：点击后**回到 Home 再完整识别一次图标**，
//! 要求分数 ≥ `RECOGNITION_THRESHOLD`。这个判据很脆：
//! 一旦图标在 Home 上的渲染与模板有偏差、或识别帧还没稳定，就会失败，
//! 而且失败后只能靠重试 —— 重试的判据同样是那个脆弱的识别。
//!
//! 参考实现不用「重新识别图标」，而是用**状态变化**：
//!
//! 1. **优先**：共同 identity 检测到视口垂直位移
//!    （选择成功会让列表滚动/重排）；
//! 2. 否则：跟踪原目标槽位（按列 + 预测 y），看它的边框亮度是否**明显下降**
//!    （游戏撤掉悬停高亮）；
//! 3. 两者都没有 → 记录 `Unchanged`，由调用方按有限次数重试，最后安全失败。
//!
//! 判据 2 就是参考实现的「亮度下降」：复用
//! [`crate::loadout_sync::direct_select::hover::HoverSample::is_dimmer_than`]，
//! 它用「绝对下限与相对比例的较大者」自适应，不需要重新分类图标。
//!
//! 本模块是纯逻辑：截图由调用方注入。
use crate::loadout_sync::direct_select::click_plan::DirectClickTarget;
use crate::loadout_sync::direct_select::hover::{border_center_line_score, HoverSample};
use crate::loadout_sync::direct_select::page_navigation::PageSnapshot;
use crate::loadout_sync::direct_select::page_relation::common_identity_vertical_shift;
use crate::loadout_sync::reference_vision::ItemKind;
use crate::loadout_sync::types::ImageRect;
use image::RgbaImage;

/// 点击后的确认超时（参考实现 `POST_CLICK_CONFIRM_TIMEOUT = 400ms`）。
pub const POST_CLICK_CONFIRM_TIMEOUT_MS: u64 = 400;
/// 下结论前最少的观察帧数（参考实现 `POST_CLICK_MIN_OBSERVATIONS = 2`）。
pub const POST_CLICK_MIN_OBSERVATIONS: u32 = 2;
/// 目标槽位的行吸附容差（参考实现 `POST_CLICK_ROW_SNAP_TOLERANCE = 4.0`）。
pub const POST_CLICK_ROW_SNAP_TOLERANCE: f32 = 4.0;
/// 判定「位置变了」的像素容差（参考实现 `TARGET_POSITION_TOLERANCE = 2`）。
pub const TARGET_POSITION_TOLERANCE: i32 = 2;
/// 最终目标的 Home 回读超时（参考实现 `TERMINAL_SETTLE_TIMEOUT = 600ms`）。
pub const TERMINAL_SETTLE_TIMEOUT_MS: u64 = 600;
/// 单目标的最大点击次数（参考实现 `MAX_TARGET_CLICK_ATTEMPTS = 3`）。
pub const MAX_TARGET_CLICK_ATTEMPTS: usize = 3;
/// 悬停前的最大重定位次数（参考实现 `MAX_HOVER_RELOCATIONS = 4`）。
pub const MAX_HOVER_RELOCATIONS: usize = 4;
/// 单方向的最大滚轮输入次数（参考实现 `MAX_WHEEL_INPUTS = 20`）。
pub const MAX_WHEEL_INPUTS: u32 = 20;

/// 点击后的观察结论。
#[derive(Debug, Clone, PartialEq)]
pub enum PostClickObservation {
    /// 已选中。`moved` 为真表示靠**视口位移**确认，为假表示靠**亮度下降**确认。
    Selected { moved: bool },
    /// 没有观察到任何变化 —— 调用方按有限次数重试，超过上限则安全失败。
    Unchanged { after_score: f32 },
    /// 无法评估（几何/截图不可用）—— 调用方必须安全失败，不得当成成功。
    NotEvaluable { detail: String },
}

impl PostClickObservation {
    /// 仅测试与日志使用。
    #[cfg(test)]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Selected { moved: true } => "selected_by_viewport_shift",
            Self::Selected { moved: false } => "selected_by_brightness_drop",
            Self::Unchanged { .. } => "unchanged",
            Self::NotEvaluable { .. } => "not_evaluable",
        }
    }

    /// 只有 `Selected` 允许推进到下一个目标。
    /// 仅测试与诊断使用。
    #[cfg(test)]
    pub fn is_selected(&self) -> bool {
        matches!(self, Self::Selected { .. })
    }
}

/// 一次点击后观察的输入。
pub struct PostClickStep<'a> {
    pub image: &'a RgbaImage,
    pub page: &'a PageSnapshot,
    /// 点击前的悬停样本（亮度下降的基准）
    pub before: HoverSample,
    /// 点击前的页面槽位（用于共同 identity 位移判定）
    pub before_slots: &'a [crate::loadout_sync::direct_select::page_relation::TrackedSlot],
    pub elapsed_ms: u64,
    pub observation: u32,
}

/// 单步评估点击后状态。
///
/// 判据顺序与参考实现一致：**先看视口位移，再看亮度下降**。
/// 视口位移更可靠（它是整个列表重排的证据），亮度下降是单槽位的局部证据。
pub fn observe_post_click(
    step: &PostClickStep<'_>,
    clicked: &DirectClickTarget,
    kind: ItemKind,
) -> PostClickObservation {
    // 判据 1：共同 identity 的垂直位移
    let shift = common_identity_vertical_shift(step.before_slots, &step.page.slots);
    let moved = shift.is_some_and(|s| s.abs() > TARGET_POSITION_TOLERANCE as f32);
    if moved {
        return PostClickObservation::Selected { moved: true };
    }

    // 判据 2：跟踪原槽位，看边框亮度是否明显下降
    let Some(slot) =
        track_post_click_slot(step.page, kind, clicked, shift.unwrap_or(0.0), step.image)
    else {
        // 追踪不到：给足观察窗口后仍未追上 → 不可评估
        if step.elapsed_ms >= POST_CLICK_CONFIRM_TIMEOUT_MS
            && step.observation >= POST_CLICK_MIN_OBSERVATIONS
        {
            return PostClickObservation::NotEvaluable {
                detail: format!("点击后无法追踪目标 {} 的槽位", clicked.item_id),
            };
        }
        return PostClickObservation::Unchanged {
            after_score: f32::NAN,
        };
    };

    let Some(after_score) = border_center_line_score(step.image, slot) else {
        return PostClickObservation::NotEvaluable {
            detail: "目标槽位越界，无法采样边框".to_string(),
        };
    };
    let after = HoverSample {
        target_score: after_score,
    };
    if after.is_dimmer_than(step.before) {
        return PostClickObservation::Selected { moved: false };
    }

    if step.elapsed_ms >= POST_CLICK_CONFIRM_TIMEOUT_MS
        && step.observation >= POST_CLICK_MIN_OBSERVATIONS
    {
        return PostClickObservation::Unchanged {
            after_score: after.target_score(),
        };
    }
    PostClickObservation::Unchanged {
        after_score: after.target_score(),
    }
}

/// 追踪点击后的目标槽位。
///
/// 优先用**分类身份**直接找；分类可能因为选中态而暂时失败，
/// 此时退化为「同列 + 预测 y」的几何追踪（参考实现口径）。
pub fn track_post_click_slot(
    page: &PageSnapshot,
    kind: ItemKind,
    clicked: &DirectClickTarget,
    vertical_shift: f32,
    image: &RgbaImage,
) -> Option<ImageRect> {
    // 1) 分类身份仍在 → 直接用
    if let Some(slot) = page.slots.iter().find(|s| {
        s.item_id.as_deref() == Some(clicked.item_id.as_str()) && s.col == clicked.slot.col
    }) {
        let _ = kind;
        return Some(slot.rect);
    }

    // 2) 几何追踪：同列 + 预测 y 最近
    let predicted_y =
        clicked.slot.rect.y as f32 + clicked.slot.rect.h as f32 * 0.5 + vertical_shift;
    let candidate = page
        .slots
        .iter()
        .filter(|s| s.col == clicked.slot.col)
        .map(|s| {
            let center_y = s.rect.y as f32 + s.rect.h as f32 * 0.5;
            (s.rect, (center_y - predicted_y).abs())
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    // 容差内才认；超出说明这一行已经不在视口里
    if candidate.1 > POST_CLICK_ROW_SNAP_TOLERANCE {
        return None;
    }
    // 槽位必须仍在图内，否则采样会失败
    let r = candidate.0;
    if r.x < 0
        || r.y < 0
        || r.right() >= image.width() as i32
        || r.bottom() >= image.height() as i32
    {
        return None;
    }
    Some(r)
}

/// 最终目标的 Home 回读结论。
#[derive(Debug, Clone, PartialEq)]
pub enum TerminalHomeOutcome {
    /// Home 已出现且该类型列表已关闭
    Settled,
    /// 超时仍未回到 Home
    Timeout { elapsed_ms: u64 },
    /// Home 出现了但目标仍在列表上（可能点错了）
    StillInList,
}

impl TerminalHomeOutcome {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Settled => "settled",
            Self::Timeout { .. } => "timeout",
            Self::StillInList => "still_in_list",
        }
    }

    pub fn is_settled(&self) -> bool {
        matches!(self, Self::Settled)
    }
}

/// 最终目标点击后，等待 Home 稳定。
///
/// **必须**接入（plan6 §4 P4.3）：不允许「点击发出就算成功」。
///
/// `probe` 每次返回 `(是否已稳定在 Home, 当前毫秒)`；时间由调用方注入，
/// 这样这个等待循环可以在没有游戏、没有真实时钟的情况下被回归测试。
/// 返回 `None` 表示**无法观察**（截图/识别失败）—— 那绝不会被当成成功。
pub fn wait_for_terminal_home<F>(mut probe: F, timeout_ms: u64) -> TerminalHomeOutcome
where
    F: FnMut() -> Option<(bool, u64)>,
{
    let mut last_elapsed = 0u64;
    loop {
        match probe() {
            Some((true, _)) => return TerminalHomeOutcome::Settled,
            Some((false, now)) => {
                // 已知「Home 未就绪」：超时后区分「仍在列表」与一般超时
                last_elapsed = now;
                if now >= timeout_ms {
                    return TerminalHomeOutcome::StillInList;
                }
            }
            None => {
                if last_elapsed >= timeout_ms {
                    return TerminalHomeOutcome::Timeout {
                        elapsed_ms: last_elapsed,
                    };
                }
            }
        }
        if last_elapsed >= timeout_ms {
            return TerminalHomeOutcome::Timeout {
                elapsed_ms: last_elapsed,
            };
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadout_sync::direct_select::click_plan::SlotRef;
    use crate::loadout_sync::direct_select::page_navigation::PageSnapshot;
    use crate::loadout_sync::direct_select::page_relation::TrackedSlot;
    use crate::loadout_sync::reference_vision::Classification;
    use image::Rgba;

    fn target(col: u32, x: i32, y: i32) -> DirectClickTarget {
        DirectClickTarget {
            item_id: "machine-gun".to_string(),
            match_score: 0.9,
            match_margin: 0.2,
            gate_quality: 1.0,
            slot: SlotRef {
                row: 0,
                col,
                rect: ImageRect::new(x, y, 80, 80),
                selectable: true,
                occupied: true,
                classification: Some(Classification::new("machine-gun", 0.9, 0.2, 1.0)),
            },
        }
    }

    fn tracked(row: u32, col: u32, y: i32, id: &str) -> TrackedSlot {
        TrackedSlot {
            row,
            col,
            rect: ImageRect::new(col as i32 * 90, y, 80, 80),
            item_id: Some(id.to_string()),
        }
    }

    fn snap(slots: Vec<TrackedSlot>) -> PageSnapshot {
        PageSnapshot::new(slots, vec![1, 2, 3], 832.0)
    }

    /// 造一张图，在给定槽位画指定亮度的边框。
    fn img_with(slots: &[ImageRect], brightness: &[u8]) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(1024, 1024, Rgba([10, 10, 10, 255]));
        for (slot, v) in slots.iter().zip(brightness) {
            let (w, h) = (img.width() as i32, img.height() as i32);
            let mut put = |x: i32, y: i32| {
                if x >= 0 && y >= 0 && x < w && y < h {
                    img.put_pixel(x as u32, y as u32, Rgba([*v, *v, *v, 255]));
                }
            };
            for x in slot.x..=slot.right() {
                put(x, slot.y);
                put(x, slot.bottom());
            }
            for y in slot.y..=slot.bottom() {
                put(slot.x, y);
                put(slot.right(), y);
            }
        }
        img
    }

    #[test]
    fn viewport_shift_confirms_selection_first() {
        let before_slots = vec![
            tracked(0, 0, 400, "machine-gun"),
            tracked(1, 0, 500, "autocannon"),
        ];
        // 点击后内容上移 338（共同 identity）
        let page = snap(vec![
            tracked(0, 0, 62, "machine-gun"),
            tracked(1, 0, 162, "autocannon"),
        ]);
        let img = img_with(&[ImageRect::new(0, 400, 80, 80)], &[200]);
        let step = PostClickStep {
            image: &img,
            page: &page,
            before: HoverSample {
                target_score: 200.0,
            },
            before_slots: &before_slots,
            elapsed_ms: 20,
            observation: 1,
        };
        let out = observe_post_click(&step, &target(0, 0, 400), ItemKind::Stratagem);
        assert_eq!(out.label(), "selected_by_viewport_shift");
        assert!(out.is_selected());
    }

    #[test]
    fn brightness_drop_confirms_selection_without_reclassifying() {
        let before_slots = vec![tracked(0, 0, 400, "machine-gun")];
        // 视口没动，但目标槽位边框从 200 降到 150（降 50 > max(20, 14)）
        let page = snap(vec![tracked(0, 0, 400, "machine-gun")]);
        let img = img_with(&[ImageRect::new(0, 400, 80, 80)], &[150]);
        let step = PostClickStep {
            image: &img,
            page: &page,
            before: HoverSample {
                target_score: 200.0,
            },
            before_slots: &before_slots,
            elapsed_ms: 20,
            observation: 1,
        };
        let out = observe_post_click(&step, &target(0, 0, 400), ItemKind::Stratagem);
        assert_eq!(out.label(), "selected_by_brightness_drop");
    }

    #[test]
    fn unchanged_is_reported_before_the_timeout() {
        let before_slots = vec![tracked(0, 0, 400, "machine-gun")];
        // 视口没动、亮度没降、且还没到超时 → Unchanged（不是 Selected、也不是 NotEvaluable）
        let page = snap(vec![tracked(0, 0, 400, "machine-gun")]);
        let img = img_with(&[ImageRect::new(0, 400, 80, 80)], &[200]);
        let step = PostClickStep {
            image: &img,
            page: &page,
            before: HoverSample {
                target_score: 200.0,
            },
            before_slots: &before_slots,
            elapsed_ms: 10,
            observation: 1,
        };
        let out = observe_post_click(&step, &target(0, 0, 400), ItemKind::Stratagem);
        assert_eq!(out.label(), "unchanged");
        assert!(!out.is_selected(), "未观察到变化绝不能算成功");
    }

    #[test]
    fn untrackable_target_becomes_not_evaluable_after_timeout() {
        let before_slots = vec![tracked(0, 0, 400, "machine-gun")];
        // 目标行完全不在新页面的可吸附范围内、也没有共同 identity →
        // 几何追踪也追不到 → 必须 NotEvaluable，绝不当成成功
        let page = snap(vec![tracked(0, 0, 700, "railgun")]);
        let img = img_with(&[ImageRect::new(0, 400, 80, 80)], &[200]);
        let step = PostClickStep {
            image: &img,
            page: &page,
            before: HoverSample {
                target_score: 200.0,
            },
            before_slots: &before_slots,
            elapsed_ms: POST_CLICK_CONFIRM_TIMEOUT_MS,
            observation: POST_CLICK_MIN_OBSERVATIONS,
        };
        let out = observe_post_click(&step, &target(0, 0, 400), ItemKind::Stratagem);
        assert_eq!(out.label(), "not_evaluable");
        assert!(!out.is_selected());
    }

    #[test]
    fn track_post_click_slot_prefers_identity_then_geometry() {
        let clicked = target(0, 0, 100);
        let img = img_with(&[ImageRect::new(0, 100, 80, 80)], &[200]);

        // 身份命中
        let p1 = snap(vec![tracked(0, 0, 120, "machine-gun")]);
        assert_eq!(
            track_post_click_slot(&p1, ItemKind::Stratagem, &clicked, 0.0, &img),
            Some(ImageRect::new(0, 120, 80, 80))
        );

        // 身份丢失 → 几何追踪（同列 + 预测 y 最近）
        let p2 = snap(vec![tracked(0, 0, 100, "unknown-a")]);
        assert_eq!(
            track_post_click_slot(&p2, ItemKind::Stratagem, &clicked, 0.0, &img),
            Some(ImageRect::new(0, 100, 80, 80))
        );

        // 同列但没有足够近的行 → None（不硬套到别的行上）
        let p3 = snap(vec![tracked(0, 0, 300, "unknown-b")]);
        assert_eq!(
            track_post_click_slot(&p3, ItemKind::Stratagem, &clicked, 0.0, &img),
            None
        );

        // 不同列不算
        let p4 = snap(vec![tracked(0, 2, 100, "unknown-c")]);
        assert_eq!(
            track_post_click_slot(&p4, ItemKind::Stratagem, &clicked, 0.0, &img),
            None
        );
    }

    #[test]
    fn click_attempts_are_bounded_by_a_hard_cap() {
        // 常量本身就是硬上限；这里锁住它们，防止以后被悄悄放宽
        assert_eq!(MAX_TARGET_CLICK_ATTEMPTS, 3);
        assert_eq!(MAX_HOVER_RELOCATIONS, 4);
        assert_eq!(MAX_WHEEL_INPUTS, 20);
        assert!(POST_CLICK_MIN_OBSERVATIONS >= 2);
    }

    #[test]
    fn terminal_home_outcome_labels_are_stable() {
        assert_eq!(TerminalHomeOutcome::Settled.label(), "settled");
        assert!(TerminalHomeOutcome::Settled.is_settled());
        assert!(!TerminalHomeOutcome::StillInList.is_settled());
        assert!(!TerminalHomeOutcome::Timeout { elapsed_ms: 1 }.is_settled());
        assert_eq!(
            PostClickObservation::Selected { moved: true }.label(),
            "selected_by_viewport_shift"
        );
        assert_eq!(
            PostClickObservation::Selected { moved: false }.label(),
            "selected_by_brightness_drop"
        );
    }
}
