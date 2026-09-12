//! 参考实现的 Hover 验证（`hd2-preset-helper-0.1.4/src/loadout/direct_select/hover.rs`）。
//!
//! ## 为什么迁移这一段
//!
//! 当前项目点击前的 hover 验证依赖 `controller::poll_hover` 里的高亮增量启发式；
//! 参考实现用的是一套**统计判据**：
//!
//! 1. 目标槽位的「边框中线白色响应」得分；
//! 2. 其余槽位同样打分，取**中位数**作为 baseline；
//! 3. 用 **MAD（中位绝对偏差 × 1.4826）** 估算离散度；
//! 4. 要求 `target - baseline >= max(24, 3×dispersion)`（**自适应**，不是固定阈值）；
//! 5. 连续稳定（得分变化 ≤ 3、持续 ≥ 15ms）才确认。
//!
//! 这套判据的好处是**对场景亮度自适应**：不同地图/光照下 baseline 会变，
//! 但「目标比其它格子亮多少」是稳定的。固定阈值做不到这一点。
//!
//! 同一模块还提供**点击后**的判据 `HoverSample::is_dimmer_than`：
//! 选中后游戏会去掉悬停高亮，因此同一个槽位的边框得分会**下降**；
//! 参考实现据此确认「点击生效」，而不需要重新完整识别图标 ——
//! 这正是当前 `SelectionVerificationFailed` 卡住的地方。
//!
//! 本模块是**纯函数 + 无 I/O 的等待循环**：截图与时间由调用方注入。
use image::RgbaImage;

use crate::loadout_sync::direct_select::frame::luma601_u8;
use crate::loadout_sync::types::ImageRect;

/// Hover 总超时（参考实现 `HOVER_TIMEOUT = 700ms`）。
pub const HOVER_TIMEOUT_MS: u64 = 700;
/// 确认所需的稳定持续时间（参考实现 `HOVER_STABLE_DURATION = 15ms`）。
pub const HOVER_STABLE_DURATION_MS: u64 = 15;
/// 判定「得分稳定」的允许波动（参考实现 `HOVER_STABLE_SCORE_DELTA = 3.0`）。
pub const HOVER_STABLE_SCORE_DELTA: f32 = 3.0;
/// 白色响应里的色度惩罚系数（参考实现 `CHROMA_PENALTY = 0.75`）。
pub const CHROMA_PENALTY: f32 = 0.75;
/// 目标相对 baseline 的最小间隔（参考实现 `HOVER_MIN_SCORE_GAP = 24.0`）。
pub const HOVER_MIN_SCORE_GAP: f32 = 24.0;
/// MAD 乘数（参考实现 `HOVER_MAD_MULTIPLIER = 3.0`）。
pub const HOVER_MAD_MULTIPLIER: f32 = 3.0;
/// 正态分布下 MAD → σ 的换算（参考实现 `MAD_NORMAL_SCALE = 1.4826`）。
pub const MAD_NORMAL_SCALE: f32 = 1.4826;
/// 点击后判定「已选中」所需的最小得分下降（参考实现 `SELECTED_MIN_SCORE_DROP = 14.0`）。
pub const SELECTED_MIN_SCORE_DROP: f32 = 14.0;
/// 点击后判定「已选中」所需的相对下降比例（参考实现 `SELECTED_SCORE_DROP_RATIO = 0.10`）。
pub const SELECTED_SCORE_DROP_RATIO: f32 = 0.10;

/// 一次边框得分采样。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoverSample {
    pub target_score: f32,
}

impl HoverSample {
    pub fn target_score(self) -> f32 {
        self.target_score
    }

    /// 判定「已选中」所需的下降量。
    ///
    /// 取「绝对下限」与「相对比例」的较大者：背景亮时靠相对比例，
    /// 背景暗时靠绝对下限，避免暗场景里 10% 只有 1~2 分而误判。
    pub fn required_score_drop(self) -> f32 {
        (self.target_score * SELECTED_SCORE_DROP_RATIO).max(SELECTED_MIN_SCORE_DROP)
    }

    /// 当前样本（已选中）是否比未选中样本**明显更暗**。
    ///
    /// 这是**点击后**的确认判据：游戏去掉 hover 高亮 → 边框变暗。
    pub fn is_dimmer_than(self, unselected: HoverSample) -> bool {
        let drop = unselected.target_score - self.target_score;
        drop >= unselected.required_score_drop()
    }
}

/// Hover 的三项证据（诊断用，失败时能说清差在哪）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoverEvidence {
    pub target_score: f32,
    pub baseline: f32,
    pub required_gap: f32,
}

impl HoverEvidence {
    /// 目标是否明显亮于 baseline。
    pub fn confirmed(self) -> bool {
        self.target_score >= self.baseline + self.required_gap
    }

    /// 目标高出 baseline 的幅度（仅测试断言用）。
    #[cfg(test)]
    pub fn gap(self) -> f32 {
        self.target_score - self.baseline
    }

    pub fn sample(self) -> HoverSample {
        HoverSample {
            target_score: self.target_score,
        }
    }
}

/// 槽位边框的「白色响应」得分（参考实现 `border_center_line_score`）。
///
/// 采样该槽位四边（上下边各取 `slot.w` 个点，左右边各取 `slot.h` 个点），
/// 对每个点算 `luma - 0.75×chroma`，取**上三分位**。
///
/// 用上三分位而不是均值：边框是亮线，均值会被格子内部的暗像素稀释；
/// 上三分位取「边框那一撮亮像素」的典型值，对局部噪声也稳健。
pub fn border_center_line_score(image: &RgbaImage, slot: ImageRect) -> Option<f32> {
    if slot.w <= 0 || slot.h <= 0 {
        return None;
    }
    let (x0, y0) = (slot.x, slot.y);
    let right = slot.right();
    let bottom = slot.bottom();
    if x0 < 0 || y0 < 0 || right >= image.width() as i32 || bottom >= image.height() as i32 {
        return None;
    }

    let mut values = Vec::with_capacity((2 * (slot.w + slot.h)) as usize);
    for x in x0..right {
        values.push(white_response(image.get_pixel(x as u32, y0 as u32).0));
        values.push(white_response(image.get_pixel(x as u32, bottom as u32).0));
    }
    for y in y0..bottom {
        values.push(white_response(image.get_pixel(x0 as u32, y as u32).0));
        values.push(white_response(image.get_pixel(right as u32, y as u32).0));
    }
    if values.is_empty() {
        return None;
    }
    Some(upper_tertile(&values))
}

/// 单像素白色响应：`luma - 0.75 × chroma`，负值截到 0。
///
/// 减色度是为了压掉「彩色但不是白色边框」的像素（例如图标本身的色块）。
pub fn white_response([r, g, b, _]: [u8; 4]) -> f32 {
    let luma = luma601_u8(r, g, b) as f32;
    let max_channel = r.max(g).max(b) as f32;
    let min_channel = r.min(g).min(b) as f32;
    (luma - CHROMA_PENALTY * (max_channel - min_channel)).max(0.0)
}

/// 对目标槽位评估 hover 证据：目标得分 vs 其余槽位的自适应 baseline。
///
/// `slots` 是当前页**全部**槽位（用于建立 baseline）；`target` 必须在其中
/// （按 `row`/`col` 匹配）。样本不足时返回 `None` —— 调用方必须拒绝确认，
/// 不得退回固定阈值。
pub fn evaluate_hover(
    image: &RgbaImage,
    slots: &[ImageRect],
    target_index: usize,
) -> Option<HoverEvidence> {
    if target_index >= slots.len() {
        return None;
    }
    let mut scored: Vec<f32> = Vec::with_capacity(slots.len());
    for slot in slots {
        scored.push(border_center_line_score(image, *slot)?);
    }

    let mut baseline_samples: Vec<f32> = scored
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != target_index)
        .map(|(_, score)| *score)
        .collect();
    // 去掉最高的一个：如果光标意外停在别的格子上，它的高亮会污染 baseline
    if baseline_samples.len() > 1 {
        if let Some((index, _)) = baseline_samples
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
        {
            baseline_samples.swap_remove(index);
        }
    }
    if baseline_samples.is_empty() {
        return None;
    }

    let baseline = median(&baseline_samples);
    let deviations: Vec<f32> = baseline_samples
        .iter()
        .map(|score| (score - baseline).abs())
        .collect();
    let dispersion = MAD_NORMAL_SCALE * median(&deviations);
    let required_gap = HOVER_MIN_SCORE_GAP.max(HOVER_MAD_MULTIPLIER * dispersion);

    Some(HoverEvidence {
        target_score: scored[target_index],
        baseline,
        required_gap,
    })
}

/// 排序后取中位数（偶数个取中间两个均值）。
pub fn median(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        0.5 * (sorted[mid - 1] + sorted[mid])
    } else {
        sorted[mid]
    }
}

/// 排序后取上三分位（参考实现口径：`sorted[(len-1)*2/3]`）。
pub fn upper_tertile(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    sorted[(sorted.len() - 1) * 2 / 3]
}

/// 带图像采样的 hover 确认（状态机使用的版本）。
///
/// `observe` 每次返回 `(图像, 全部槽位矩形, 当前毫秒)`。
pub fn wait_for_hover_frames<'a, F>(target_index: usize, mut observe: F) -> HoverOutcome
where
    F: FnMut() -> Option<(image::RgbaImage, Vec<ImageRect>, u64)>,
{
    let mut stable_since: Option<u64> = None;
    let mut previous_score: Option<f32> = None;
    let mut start: Option<u64> = None;
    let mut last_evidence: Option<HoverEvidence>;

    loop {
        let Some((image, slots, now)) = observe() else {
            return HoverOutcome::NotEvaluable {
                detail: "观察失败（截图或几何不可用）".to_string(),
            };
        };
        let start = *start.get_or_insert(now);
        let Some(evidence) = evaluate_hover(&image, &slots, target_index) else {
            return HoverOutcome::NotEvaluable {
                detail: "槽位越界或非目标样本不足，无法建立 hover baseline".to_string(),
            };
        };
        last_evidence = Some(evidence);

        if evidence.confirmed() {
            let stable = previous_score
                .map(|prev| (prev - evidence.target_score).abs() <= HOVER_STABLE_SCORE_DELTA)
                .unwrap_or(false);
            if stable {
                if let Some(since) = stable_since {
                    if now.saturating_sub(since) >= HOVER_STABLE_DURATION_MS {
                        return HoverOutcome::Confirmed(evidence.sample());
                    }
                }
            } else {
                stable_since = Some(now);
            }
            previous_score = Some(evidence.target_score);
        } else {
            stable_since = None;
            previous_score = None;
        }

        if now.saturating_sub(start) >= HOVER_TIMEOUT_MS {
            return HoverOutcome::Timeout(last_evidence.unwrap_or(HoverEvidence {
                target_score: 0.0,
                baseline: 0.0,
                required_gap: HOVER_MIN_SCORE_GAP,
            }));
        }
    }
}

/// Hover 等待的结论。
#[derive(Debug, Clone, PartialEq)]
pub enum HoverOutcome {
    /// 确认悬停成功
    Confirmed(HoverSample),
    /// 超时未确认（携带最后一次证据，便于解释差在哪）
    Timeout(HoverEvidence),
    /// 无法评估（槽位越界 / 样本不足）—— 调用方必须拒绝点击
    NotEvaluable { detail: String },
}

impl HoverOutcome {
    /// 仅测试与日志使用。
    #[cfg(test)]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Confirmed(_) => "confirmed",
            Self::Timeout(_) => "timeout",
            Self::NotEvaluable { .. } => "not_evaluable",
        }
    }

    /// 只有 `Confirmed` 才允许点击。
    /// 仅测试与诊断使用。
    #[cfg(test)]
    pub fn is_confirmed(&self) -> bool {
        matches!(self, Self::Confirmed(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// 造一张图：指定槽位画亮边框，其余画暗边框。
    fn image_with_bright(slots: &[ImageRect], bright: Option<usize>) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(400, 400, Rgba([10, 10, 10, 255]));
        for (i, slot) in slots.iter().enumerate() {
            let v = if Some(i) == bright { 230u8 } else { 120u8 };
            for x in slot.x..slot.right() {
                img.put_pixel(x as u32, slot.y as u32, Rgba([v, v, v, 255]));
                img.put_pixel(x as u32, slot.bottom() as u32, Rgba([v, v, v, 255]));
            }
            for y in slot.y..slot.bottom() {
                img.put_pixel(slot.x as u32, y as u32, Rgba([v, v, v, 255]));
                img.put_pixel(slot.right() as u32, y as u32, Rgba([v, v, v, 255]));
            }
        }
        img
    }

    fn slots() -> Vec<ImageRect> {
        (0..5)
            .map(|i| ImageRect::new(20, 20 + i * 60, 50, 50))
            .collect()
    }

    #[test]
    fn white_response_penalises_chroma() {
        // 纯白：chroma 0，响应 = luma
        assert!((white_response([255, 255, 255, 255]) - 255.0).abs() < 1.0);
        // 纯灰：同样 luma，chroma 0
        assert!(white_response([120, 120, 120, 255]) > 100.0);
        // 高饱和彩色：chroma 很高 → 被压下去
        let colored = white_response([255, 0, 0, 255]);
        let white = white_response([255, 255, 255, 255]);
        assert!(colored < white, "彩色响应 {colored} 应低于白色 {white}");
        // 全黑 → 0
        assert_eq!(white_response([0, 0, 0, 255]), 0.0);
    }

    #[test]
    fn median_and_tertile_follow_reference_definitions() {
        assert_eq!(median(&[1.0, 3.0]), 2.0);
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[]), 0.0);
        // (len-1)*2/3：len=4 → index 2（第三大）
        assert_eq!(upper_tertile(&[10.0, 20.0, 30.0, 40.0]), 30.0);
        // len=3 → index 1
        assert_eq!(upper_tertile(&[10.0, 20.0, 30.0]), 20.0);
        assert_eq!(upper_tertile(&[]), 0.0);
    }

    #[test]
    fn border_score_requires_geometry_inside_image() {
        let img = RgbaImage::from_pixel(50, 50, Rgba([10, 10, 10, 255]));
        // 完全在内
        assert!(border_center_line_score(&img, ImageRect::new(2, 2, 10, 10)).is_some());
        // 越界（右边贴到 50）
        assert!(border_center_line_score(&img, ImageRect::new(45, 45, 10, 10)).is_none());
        // 零尺寸
        assert!(border_center_line_score(&img, ImageRect::new(5, 5, 0, 5)).is_none());
    }

    #[test]
    fn hover_confirms_only_when_target_is_clearly_brighter() {
        let s = slots();
        // 目标（index 2）亮，其余暗
        let bright = image_with_bright(&s, Some(2));
        let ev = evaluate_hover(&bright, &s, 2).expect("可评估");
        assert!(ev.confirmed(), "目标应被确认：gap={}", ev.gap());
        assert!(ev.gap() > HOVER_MIN_SCORE_GAP);

        // 没有格子亮 → 目标不比 baseline 亮，必须不确认
        let none_bright = image_with_bright(&s, None);
        let ev2 = evaluate_hover(&none_bright, &s, 2).expect("可评估");
        assert!(!ev2.confirmed(), "无高亮时不得确认（gap={}）", ev2.gap());
    }

    #[test]
    fn hover_is_not_evaluable_instead_of_falling_back() {
        let s = slots();
        let img = image_with_bright(&s, Some(0));
        // 目标索引越界 → 必须 NotEvaluable，不得退回固定阈值
        assert!(evaluate_hover(&img, &s, 99).is_none());
        // 只有 1 个槽位 → 无法建立 baseline
        let one = vec![s[0]];
        assert!(evaluate_hover(&img, &one, 0).is_none());
    }

    #[test]
    fn selected_drop_uses_max_of_absolute_and_ratio() {
        // 高亮很强 → 相对比例主导
        let bright = HoverSample {
            target_score: 200.0,
        };
        assert!((bright.required_score_drop() - 20.0).abs() < 1e-3);
        // 高亮很弱 → 绝对下限主导
        let dim = HoverSample { target_score: 50.0 };
        assert!((dim.required_score_drop() - SELECTED_MIN_SCORE_DROP).abs() < 1e-3);

        // 点击后边框变暗到 180（降 20）→ 对 200 而言刚好达标
        let after = HoverSample {
            target_score: 180.0,
        };
        assert!(after.is_dimmer_than(bright));
        // 只降 5 → 不达标
        let barely = HoverSample {
            target_score: 195.0,
        };
        assert!(!barely.is_dimmer_than(bright));
    }

    #[test]
    fn wait_for_hover_confirms_after_stable_frames() {
        let s = slots();
        let bright = image_with_bright(&s, Some(2));
        let mut tick = 0u64;
        let out = wait_for_hover_frames(2, || {
            tick += 20;
            Some((bright.clone(), s.clone(), tick))
        });
        assert!(out.is_confirmed(), "稳定亮帧应确认，实际 {out:?}");
        assert_eq!(out.label(), "confirmed");
    }

    #[test]
    fn wait_for_hover_times_out_without_fabricating_success() {
        let s = slots();
        let none = image_with_bright(&s, None);
        let mut tick = 0u64;
        let out = wait_for_hover_frames(2, || {
            tick += 50;
            Some((none.clone(), s.clone(), tick))
        });
        match out {
            HoverOutcome::Timeout(ev) => {
                assert!(!ev.confirmed(), "超时的证据不应是 confirmed");
            }
            other => panic!("期望 Timeout，实际 {other:?}"),
        }
    }

    #[test]
    fn wait_for_hover_reports_not_evaluable_when_observation_fails() {
        let out = wait_for_hover_frames(0, || None);
        assert_eq!(out.label(), "not_evaluable");
        assert!(!out.is_confirmed(), "不可评估绝不能被当成确认");
    }
}
