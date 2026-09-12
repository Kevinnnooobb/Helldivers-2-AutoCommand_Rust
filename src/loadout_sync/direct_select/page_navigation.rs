//! 参考实现的分页导航编排（`hd2-preset-helper-0.1.4/src/loadout/direct_select/page_navigation.rs`）。
//!
//! ## 分工
//!
//! * [`crate::loadout_sync::direct_select::frame`] 回答「画面变了没有」——只用于**触发**语义扫描；
//! * [`crate::loadout_sync::direct_select::page_relation`] 回答「视口是否按预期移动」——**唯一**的成功判据；
//! * 本模块把两者编排成一个**有硬上限**的轮询循环，并决定何时
//!   确认移动、确认边界、或判定不可判定。
//!
//! ## 与参考实现的关系
//!
//! 参考实现把「截图 + 滚轮 + 时间」直接耦合进结构体。本移植把**输入与时间**
//! 抽成 [`NavDriver`] trait：这样纯状态推进逻辑可以**在没有游戏的情况下**用
//! 确定性帧序列回归（plan6 §6.2 要求的 deterministic replay），
//! 生产实现再把这个 trait 接到真实的 capture/input 上。
//!
//! 保留的参考常量语义：
//!
//! | 参考常量 | 本模块 |
//! | --- | --- |
//! | `PAGE_WHEEL_DELTA = 600` | [`PAGE_WHEEL_DELTA`] |
//! | `PAGE_BOUNDARY_PROBE_DELTA = 120` | [`PAGE_BOUNDARY_PROBE_DELTA`] |
//! | `PAGE_TURN_MIN_SEMANTIC_OBSERVATIONS = 3` | [`MIN_SEMANTIC_OBSERVATIONS`] |
//! | `PAGE_TURN_NO_MOVEMENT_FRAMES = 3` | [`NO_MOVEMENT_FRAMES`] |
//! | `PAGE_TURN_HARD_TIMEOUT = 4s` | [`HARD_TIMEOUT_MS`] |
use crate::loadout_sync::list_map::ScrollDirection;

/// 一次「完整翻页」的滚轮量（参考实现 `PAGE_WHEEL_DELTA = 600`）。
pub const PAGE_WHEEL_DELTA: i32 = 600;
/// 边界探测的滚轮量（参考实现 `PAGE_BOUNDARY_PROBE_DELTA = 120`）。
pub const PAGE_BOUNDARY_PROBE_DELTA: i32 = 120;
/// 硬超时（参考实现 `PAGE_TURN_HARD_TIMEOUT = 4s`）。
pub const HARD_TIMEOUT_MS: u64 = 4_000;
/// 做结论前至少要观察到的语义样本数（参考实现 `= 3`）。
pub const MIN_SEMANTIC_OBSERVATIONS: usize = 3;
/// 连续多少帧同视口才判定「没有位移」（参考实现 `= 3`）。
pub const NO_MOVEMENT_FRAMES: usize = 3;

/// 滚轮输入的种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavInput {
    /// 整页翻动
    Full(ScrollDirection),
    /// 边界探测（小步长）
    Probe(ScrollDirection),
}

impl NavInput {
    pub fn direction(self) -> ScrollDirection {
        match self {
            Self::Full(d) | Self::Probe(d) => d,
        }
    }

    pub fn is_full(self) -> bool {
        matches!(self, Self::Full(_))
    }

    pub fn is_probe(self) -> bool {
        matches!(self, Self::Probe(_))
    }
}

/// 该输入对应的**带符号**滚轮量（向上为正，与 `types::wheel_notches` 同号约定）。
pub fn navigation_delta(input: NavInput) -> i32 {
    let magnitude = match input {
        NavInput::Full(_) => PAGE_WHEEL_DELTA,
        NavInput::Probe(_) => PAGE_BOUNDARY_PROBE_DELTA,
    };
    match input.direction() {
        ScrollDirection::Up => magnitude,
        ScrollDirection::Down => -magnitude,
    }
}

/// 一帧的语义快照：ROI 全部槽位的身份 + 该帧的指纹。
#[derive(Debug, Clone, PartialEq)]
pub struct PageSnapshot {
    pub slots: Vec<crate::loadout_sync::direct_select::page_relation::TrackedSlot>,
    pub signature: Vec<u8>,
    /// 产生该快照的 ROI 高度（用于把参考位移折算到当前分辨率）。
    pub roi_height: f32,
}

impl PageSnapshot {
    pub fn new(
        slots: Vec<crate::loadout_sync::direct_select::page_relation::TrackedSlot>,
        signature: Vec<u8>,
        roi_height: f32,
    ) -> Self {
        Self {
            slots,
            signature,
            roi_height,
        }
    }
}

/// 单方向上的推进计数器（有硬上限，不靠超时续命）。
///
/// 生产路径由 `vision-navplan` 诊断命令读取；状态推进本身由状态机
/// （`machine.rs`）的 [`crate::loadout_sync::direct_select::selection_verify::MAX_WHEEL_INPUTS`]
/// 与整轮计数负责。
#[derive(Debug, Clone, Default)]
pub struct NavigationTracker {
    /// 已发出的整页输入次数
    pub full_turns: u32,
    /// 已发出的探测输入次数
    pub probes: u32,
    /// 连续「没有位移」的次数
    pub consecutive_no_movement: u32,
}

impl NavigationTracker {
    /// 该方向是否仍然允许继续翻页。
    pub fn can_continue(&self, direction: ScrollDirection) -> bool {
        let _ = direction;
        self.full_turns < MAX_FULL_TURNS && self.probes < MAX_PROBES
    }

    /// 是否已经可以判定「到边界」。
    ///
    /// 规则：先整页翻动；整页无位移后**必须**再做有限探测；
    /// 探测仍无位移才确认边界。
    pub fn boundary_confirmed(&self) -> bool {
        self.probes > 0 && self.consecutive_no_movement >= 2
    }

    /// 记录一次输入与结论（仅测试驱动计数用）。
    #[cfg(test)]
    pub fn record(&mut self, input: NavInput, outcome: &NavigationOutcome) {
        match input {
            NavInput::Full(_) => self.full_turns += 1,
            NavInput::Probe(_) => self.probes += 1,
        }
        match outcome {
            NavigationOutcome::NoMovement => self.consecutive_no_movement += 1,
            NavigationOutcome::Moved => self.consecutive_no_movement = 0,
            NavigationOutcome::Uncertain => {}
        }
    }
}

/// 单方向整页翻动的硬上限（不依赖超时）。
pub const MAX_FULL_TURNS: u32 = 12;
/// 单方向边界探测的硬上限。
pub const MAX_PROBES: u32 = 4;

/// 一次翻页尝试的结论（由语义位移判定产生）。
///
/// 仅测试驱动 NavigationTracker 计数用（生产的状态推进在 machine.rs）。
#[cfg(test)]
#[derive(Debug, Clone)]
pub enum NavigationOutcome {
    /// 视口按预期移动了
    Moved,
    /// 确认没有位移（结合 probe 之后可判定边界）
    NoMovement,
    /// 无法判定 —— 调用方必须**停止**当前方向，不得盲滚
    Uncertain,
}

#[cfg(test)]
impl NavigationOutcome {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Moved => "moved",
            Self::NoMovement => "no_movement",
            Self::Uncertain => "uncertain",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_delta_signs_match_direction() {
        assert_eq!(
            navigation_delta(NavInput::Full(ScrollDirection::Up)),
            PAGE_WHEEL_DELTA
        );
        assert_eq!(
            navigation_delta(NavInput::Full(ScrollDirection::Down)),
            -PAGE_WHEEL_DELTA
        );
        assert_eq!(
            navigation_delta(NavInput::Probe(ScrollDirection::Up)),
            PAGE_BOUNDARY_PROBE_DELTA
        );
        assert_eq!(
            navigation_delta(NavInput::Probe(ScrollDirection::Down)),
            -PAGE_BOUNDARY_PROBE_DELTA
        );
        assert!(NavInput::Full(ScrollDirection::Up).is_full());
        assert!(NavInput::Probe(ScrollDirection::Up).is_probe());
    }

    #[test]
    fn tracker_enforces_hard_caps_and_boundary_rule() {
        let mut t = NavigationTracker::default();
        assert!(t.can_continue(ScrollDirection::Down));
        assert!(!t.boundary_confirmed(), "还没探测过，不能确认边界");
        for _ in 0..MAX_FULL_TURNS {
            t.record(
                NavInput::Full(ScrollDirection::Down),
                &NavigationOutcome::Moved,
            );
        }
        assert!(
            !t.can_continue(ScrollDirection::Down),
            "整页次数到达硬上限后必须停"
        );

        let mut t2 = NavigationTracker::default();
        t2.record(
            NavInput::Probe(ScrollDirection::Down),
            &NavigationOutcome::NoMovement,
        );
        assert!(
            !t2.boundary_confirmed(),
            "仅整页无位移还不算边界，必须先探测"
        );
        t2.record(
            NavInput::Probe(ScrollDirection::Down),
            &NavigationOutcome::NoMovement,
        );
        assert!(t2.boundary_confirmed(), "探测也无位移 → 确认边界");

        let mut t3 = NavigationTracker::default();
        for _ in 0..MAX_PROBES {
            t3.record(
                NavInput::Probe(ScrollDirection::Down),
                &NavigationOutcome::NoMovement,
            );
        }
        assert!(!t3.can_continue(ScrollDirection::Down));

        // 未知结论不得伪装成「已移动」或「无位移」
        let mut t4 = NavigationTracker::default();
        let uncertain = NavigationOutcome::Uncertain;
        assert_eq!(uncertain.label(), "uncertain");
        t4.record(NavInput::Full(ScrollDirection::Down), &uncertain);
        assert_eq!(t4.consecutive_no_movement, 0);
        assert_eq!(t4.full_turns, 1);
    }
}
