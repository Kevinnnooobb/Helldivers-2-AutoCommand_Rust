// ListMap —— 通过共同 landmark 判断 Viewport 的位移（滚动是否真的生效）。
//
// plan6 §5.3 之后，整轮装配的滚动确认由 `direct_select::page_relation` /
// `direct_select::list_map` 承担，本模块在生产路径上已无调用者，仅作为
// 底层适配契约的回归测试对象保留（`tests.rs` 的真实截图位移断言）。
#![allow(dead_code)]
use crate::loadout_sync::viewport::ViewportState;

/// 判定「确实移动了」所需的最低置信度。
pub const MIN_RELOCALIZE_CONFIDENCE: f32 = 0.5;
/// 判定移动所需的最少共同 landmark 数（单行重合不足以证明整体位移）。
pub const MIN_MATCHED_ROWS: usize = 2;

/// 目标列表相对当前视口的期望移动方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    /// 查看更靠后的项目（列表向下滚动）
    Down,
    /// 查看更靠前的项目（列表向上滚动）
    Up,
}

impl ScrollDirection {
    pub fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
        }
    }

    /// Windows 滚轮增量符号：正 = 向上滚（内容下移）。
    pub fn wheel_sign(self) -> i32 {
        match self {
            Self::Up => 1,
            Self::Down => -1,
        }
    }
}

/// delta_rows > 0 表示内容下移（用户向上滚），< 0 表示内容上移（用户向下滚）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Relocalization {
    pub delta_rows: i32,
    pub matched: usize,
    pub confidence: f32,
    /// 画面内容确定整体换掉了，但共同 landmark 太少，测不出位移
    /// （典型场景：一次滚动的幅度 ≥ 可见行数，例如整页翻页）。
    pub dislocated: bool,
}

impl Relocalization {
    pub fn no_movement() -> Self {
        Self {
            delta_rows: 0,
            matched: 0,
            confidence: 0.0,
            dislocated: false,
        }
    }

    /// 内容移动方向（有足够证据时）。
    pub fn direction(&self) -> Option<ScrollDirection> {
        if self.matched < MIN_MATCHED_ROWS || self.confidence < MIN_RELOCALIZE_CONFIDENCE {
            return None;
        }
        if self.delta_rows > 0 {
            Some(ScrollDirection::Up)
        } else if self.delta_rows < 0 {
            Some(ScrollDirection::Down)
        } else {
            None
        }
    }

    /// 是否按期望方向移动了（滚动验证的核心断言）。
    pub fn moved_as_expected(&self, expected: ScrollDirection) -> bool {
        self.direction() == Some(expected)
    }

    pub fn moved_at_all(&self) -> bool {
        self.direction().is_some() || self.dislocated
    }
}

/// 用共同 landmark 估计 old → new 的行位移。
///
/// * 对每个候选位移 shift，统计 old.row[i] 与 new.row[i+shift] 判定为同一 landmark 的数量；
/// * 取匹配数最多的 shift（并列时取位移较小者，避免把整体平移误判为大幅滚动）；
/// * 完全没有共同项目时返回 confidence = 0（调用方必须降低滚轮幅度重试，而不是假设成功）。
pub fn relocalize(old: &ViewportState, new: &ViewportState) -> Relocalization {
    if old.rows.is_empty() || new.rows.is_empty() {
        return Relocalization::no_movement();
    }
    let max_shift = (old.rows.len() + new.rows.len()) as i32;
    let mut best = Relocalization::no_movement();
    for shift in -max_shift..=max_shift {
        let mut matched = 0usize;
        for i in 0..old.rows.len() {
            let j = i as i32 + shift;
            if j < 0 || j as usize >= new.rows.len() {
                continue;
            }
            let a = &old.rows[i].signature;
            let b = &new.rows[j as usize].signature;
            if a.is_same_landmark(b) {
                matched += 1;
            }
        }
        if matched == 0 {
            continue;
        }
        // 置信度按「重叠行数」而不是「整屏行数」计算：
        // 大幅滚动（例如一次滚 5 行）只会留下少量共同 landmark，
        // 用整屏行数做分母会把真实位移误判为「没动」（实测 confidence 0.40 < 0.5）。
        let overlap = (old.rows.len().min(new.rows.len()) as i32 - shift.abs()).max(1);
        let confidence = (matched as f32 / overlap as f32).clamp(0.0, 1.0);
        let better = matched > best.matched
            || (matched == best.matched && best.matched > 0 && shift.abs() < best.delta_rows.abs());
        if better {
            best = Relocalization {
                delta_rows: shift,
                matched,
                confidence,
                dislocated: false,
            };
        }
    }
    if best.matched < MIN_MATCHED_ROWS {
        // 一行都对不上：内容被整体换掉了（例如整页滚动），此时「没位移」是错误结论。
        // 只有在两帧各自都识别到足够多的行时才能这么判定，否则宁可保守地说「没动」。
        let enough_rows = old.rows.len() >= MIN_MATCHED_ROWS && new.rows.len() >= MIN_MATCHED_ROWS;
        if enough_rows {
            best.dislocated = true;
        }
    }
    best
}

/// 期望方向 + 幅度 → 滚轮格数（含方向符号）。
pub fn wheel_notches(direction: ScrollDirection, magnitude: i32) -> i32 {
    direction.wheel_sign() * magnitude.abs()
}
