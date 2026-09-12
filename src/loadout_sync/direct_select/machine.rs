//! 参考实现的目标选择状态机
//! （`hd2-preset-helper-0.1.4/src/loadout/direct_select.rs`）。
//!
//! ## 与参考实现的关系
//!
//! 参考实现把「截图 / 滚轮 / 鼠标 / 时钟」直接耦合进 `AutomationSession`。
//! 本移植把它们抽成 [`DirectSelectIo`] trait，于是**同一份状态推进逻辑**
//! 既能跑在真实游戏上，也能跑在确定性帧序列上（plan6 §6.2 的 replay）。
//!
//! 保留的参考结构：
//!
//! ```text
//! while remaining 非空:
//!     在当前页找「最靠上的可见目标」
//!     ├─ 找到 → relocate_and_wait_hover → click → observe_post_click_state
//!     │          ├─ Selected            → 移除该目标，继续
//!     │          └─ Unchanged/NotEval   → 有限重试，超限则安全失败
//!     └─ 未找到 → navigation_hint → 有界翻页（full / probe）
//! ```
//!
//! ## 硬上限（全部来自参考实现，不允许放宽）
//!
//! | 上限 | 值 | 常量 |
//! | --- | ---: | --- |
//! | 单方向滚轮输入 | 20 | [`MAX_WHEEL_INPUTS`] |
//! | 单目标点击尝试 | 3 | [`MAX_TARGET_CLICK_ATTEMPTS`] |
//! | 悬停前重定位 | 4 | [`MAX_HOVER_RELOCATIONS`] |
//!
//! 任何一条到达上限都会**安全失败**，而不是继续发输入。
use crate::loadout_sync::direct_select::click_plan::{
    find_visible_target, next_visible_target, DirectClickTarget,
};
use crate::loadout_sync::direct_select::hover::{wait_for_hover_frames, HoverOutcome, HoverSample};
use crate::loadout_sync::direct_select::list_map::{ListMap, NavigationHint};
use crate::loadout_sync::direct_select::page_navigation::PageSnapshot;
use crate::loadout_sync::direct_select::page_relation::{compare_page_turn, PageRelation};
use crate::loadout_sync::direct_select::selection_verify::{
    observe_post_click, PostClickObservation, PostClickStep, MAX_HOVER_RELOCATIONS,
    MAX_TARGET_CLICK_ATTEMPTS, MAX_WHEEL_INPUTS,
};
use crate::loadout_sync::reference_vision::{ItemKind, RoiObservation};

/// 状态机的阶段（诊断与 replay 断言用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectSelectPhase {
    /// 还没开始
    Idle,
    /// 在当前页规划目标（找可见目标 / 决定翻页）
    PlanningTarget,
    /// 移动光标并等待 hover 确认
    Hovering,
    /// 点击
    Clicking,
    /// 点击后验证状态变化
    VerifyingSelection,
    /// 全部目标完成，等待 Home 稳定
    WaitingTerminalHome,
    /// 成功结束
    Done,
    /// 安全失败
    Failed,
}

impl DirectSelectPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::PlanningTarget => "planning_target",
            Self::Hovering => "hovering",
            Self::Clicking => "clicking",
            Self::VerifyingSelection => "verifying_selection",
            Self::WaitingTerminalHome => "waiting_terminal_home",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
}

/// 失败原因（全部可解释；绝不用裸字符串）。
#[derive(Debug, Clone, PartialEq)]
pub enum DirectSelectFailure {
    /// 目标在有限滚轮次数内没找到
    TargetNotFound { item_id: String, wheel_inputs: u32 },
    /// 悬停始终未确认
    HoverNotConfirmed { item_id: String },
    /// 悬停前目标离开了可见视口
    TargetLostWhileRelocating { item_id: String },
    /// 点击后未观察到状态变化（已用尽重试）
    SelectionUnchanged { item_id: String, attempts: usize },
    /// 点击后无法验证（几何/截图不可用）
    SelectionNotEvaluable { item_id: String, detail: String },
    /// 最终目标未回到 Home
    TerminalHomeNotReached { detail: String },
    /// IO 层失败
    Io { detail: String },
}

impl DirectSelectFailure {
    pub fn label(&self) -> &'static str {
        match self {
            Self::TargetNotFound { .. } => "target_not_found",
            Self::HoverNotConfirmed { .. } => "hover_not_confirmed",
            Self::TargetLostWhileRelocating { .. } => "target_lost_while_relocating",
            Self::SelectionUnchanged { .. } => "selection_unchanged",
            Self::SelectionNotEvaluable { .. } => "selection_not_evaluable",
            Self::TerminalHomeNotReached { .. } => "terminal_home_not_reached",
            Self::Io { .. } => "io_error",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::TargetNotFound {
                item_id,
                wheel_inputs,
            } => {
                format!("目标「{item_id}」在 {wheel_inputs} 次滚轮输入内未找到（列表边界）")
            }
            Self::HoverNotConfirmed { item_id } => format!("目标「{item_id}」悬停始终未确认"),
            Self::TargetLostWhileRelocating { item_id } => {
                format!("目标「{item_id}」在悬停重定位期间离开可见视口")
            }
            Self::SelectionUnchanged { item_id, attempts } => {
                format!("目标「{item_id}」在 {attempts} 次点击后仍未观察到选中")
            }
            Self::SelectionNotEvaluable { item_id, detail } => {
                format!("目标「{item_id}」点击后无法验证: {detail}")
            }
            Self::TerminalHomeNotReached { detail } => format!("最终未确认回到 Home: {detail}"),
            Self::Io { detail } => format!("IO 失败: {detail}"),
        }
    }
}

/// 一次选择的结论。
#[derive(Debug, Clone, PartialEq)]
pub enum DirectSelectOutcome {
    /// 全部目标已装配并确认
    Succeeded { selected: Vec<String> },
    /// 安全失败（已停止发输入）
    Failed {
        failure: DirectSelectFailure,
        selected: Vec<String>,
    },
}

impl DirectSelectOutcome {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Succeeded { .. })
    }

    pub fn failure(&self) -> Option<&DirectSelectFailure> {
        match self {
            Self::Succeeded { .. } => None,
            Self::Failed { failure, .. } => Some(failure),
        }
    }

    pub fn selected(&self) -> &[String] {
        match self {
            Self::Succeeded { selected } => selected,
            Self::Failed { selected, .. } => selected,
        }
    }
}

/// IO 边界：状态机所需的一切外部交互。
///
/// 生产实现接到真实 capture / 鼠标 / 时钟；
/// replay 实现从一个脚本化的帧序列取值。
pub trait DirectSelectIo {
    /// 抓一帧并分类成 ROI 观察。
    fn observe(&mut self) -> Result<RoiObservation, String>;
    /// 把光标移到帧内坐标（已由调用方换算到屏幕坐标）。
    fn move_cursor(&mut self, point: (i32, i32)) -> Result<(), String>;
    /// 点击当前光标位置。
    fn click(&mut self) -> Result<(), String>;
    /// 发送滚轮（正值向上）。
    fn scroll(&mut self, delta: i32) -> Result<(), String>;
    /// 抓一帧的原始 RGBA（hover / 点击后边框亮度判定需要像素）。
    ///
    /// 与 `observe()` 分开是因为分类与像素采样开销不同：
    /// 只做几何导航时不必付像素分类的代价。
    fn capture_rgba(&mut self) -> Result<image::RgbaImage, String>;
    /// 当前时间（毫秒）。
    fn now_ms(&mut self) -> u64;
    /// 当前是否已回到 Home（列表已关闭）。
    fn is_home_settled(&mut self) -> Result<bool, String>;
    /// 是否被取消。
    fn cancelled(&mut self) -> bool {
        false
    }
}

/// 由预设构建一次 `direct_select` 运行的计划预览（诊断用，**不发送任何输入**）。
///
/// 这让「打开开关后打算装哪 4 个 + 哪个 Booster」在真正跑之前就能被核对，
/// 也让迁移期的类型契约有一个明确的生产调用点。
pub fn plan_preview(
    stratagems: &[Option<String>],
    booster: Option<&String>,
) -> (Vec<String>, Vec<String>) {
    let mut stratagem_targets: Vec<String> = Vec::new();
    for item in stratagems.iter().flatten() {
        if !stratagem_targets.contains(item) {
            stratagem_targets.push(item.clone());
        }
    }
    let mut booster_targets: Vec<String> = Vec::new();
    if let Some(b) = booster {
        if !booster_targets.contains(b) {
            booster_targets.push(b.clone());
        }
    }
    (stratagem_targets, booster_targets)
}

/// 状态机一次运行的完整轨迹（replay 断言用）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DirectSelectTrace {
    pub phases: Vec<DirectSelectPhase>,
    pub wheel_inputs: u32,
    pub clicks: u32,
    pub hovers_confirmed: u32,
    pub fallbacks: u32,
}

/// 有界的直接选择状态机。
pub struct DirectSelectMachine {
    /// 剩余待装配的目标（目录 ID，按预设顺序）
    remaining: Vec<String>,
    item_kind: ItemKind,
    selected: Vec<String>,
    phase: DirectSelectPhase,
    trace: DirectSelectTrace,
    wheel_inputs: u32,
    map: Option<ListMap>,
    current: Option<PageSnapshot>,
}

impl DirectSelectMachine {
    pub fn new(targets: Vec<String>, item_kind: ItemKind) -> Self {
        Self {
            remaining: targets,
            item_kind,
            selected: Vec::new(),
            phase: DirectSelectPhase::Idle,
            trace: DirectSelectTrace::default(),
            wheel_inputs: 0,
            map: None,
            current: None,
        }
    }

    pub fn phase(&self) -> DirectSelectPhase {
        self.phase
    }

    pub fn trace(&self) -> &DirectSelectTrace {
        &self.trace
    }

    pub fn remaining(&self) -> &[String] {
        &self.remaining
    }

    pub fn selected(&self) -> &[String] {
        &self.selected
    }

    fn enter(&mut self, phase: DirectSelectPhase) {
        if self.trace.phases.last() != Some(&phase) {
            self.trace.phases.push(phase);
        }
        self.phase = phase;
    }

    /// 运行到结束。
    ///
    /// 每个目标：规划 → 悬停 → 点击 → 验证；翻页、点击、重定位都有硬上限。
    pub fn run(&mut self, io: &mut dyn DirectSelectIo) -> DirectSelectOutcome {
        // 先抓一帧：即使目标列表为空，也要有一次观察才能判定 Home 状态。
        // （不做这一步的话「空预设 + 已在 Home」会被误判成超时失败。）
        if self.current.is_none() {
            match io.observe() {
                Ok(o) => {
                    if self.map.is_none() {
                        self.map = Some(ListMap::new(&o, self.item_kind));
                    }
                    self.current = Some(snapshot_from(&o));
                }
                Err(detail) => {
                    return self.fail(DirectSelectFailure::Io { detail });
                }
            }
        }
        loop {
            if io.cancelled() {
                return self.fail(DirectSelectFailure::Io {
                    detail: "已取消".to_string(),
                });
            }
            if self.remaining.is_empty() {
                // 全部选完：必须确认回到 Home（不允许「点击发出就算成功」）
                self.enter(DirectSelectPhase::WaitingTerminalHome);
                let timeout = crate::loadout_sync::direct_select::selection_verify::TERMINAL_SETTLE_TIMEOUT_MS;
                let start = io.now_ms();
                let outcome =
                    crate::loadout_sync::direct_select::selection_verify::wait_for_terminal_home(
                        || match io.is_home_settled() {
                            Ok(settled) => Some((settled, io.now_ms().saturating_sub(start))),
                            // 观察不到（截图/识别失败）绝不当成成功
                            Err(_) => None,
                        },
                        timeout,
                    );
                if outcome.is_settled() {
                    self.enter(DirectSelectPhase::Done);
                    return DirectSelectOutcome::Succeeded {
                        selected: self.selected.clone(),
                    };
                }
                return self.fail(DirectSelectFailure::TerminalHomeNotReached {
                    detail: format!("{}（{}）", outcome.label(), timeout),
                });
            }

            // ── 取当前页 ──
            let observation = match io.observe() {
                Ok(o) => o,
                Err(detail) => return self.fail(DirectSelectFailure::Io { detail }),
            };
            let page = snapshot_from(&observation);
            if self.map.is_none() {
                self.map = Some(ListMap::new(&observation, self.item_kind));
            }

            // ── 规划：优先装屏幕上已经看到的 ──
            self.enter(DirectSelectPhase::PlanningTarget);
            let slots = observation.slot_refs();
            let Some(target) = next_visible_target(&slots, &self.remaining, self.item_kind) else {
                // 没有可见目标 → 按导航建议翻页。
                // 先把 map 取出（避免同时借 self 的其它字段），算完再放回。
                let mut map = self.map.take().expect("刚初始化");
                let result = self.turn_page(io, &mut map, &observation, &page);
                self.map = Some(map);
                match result {
                    Ok(Some(new_page)) => {
                        self.current = Some(new_page);
                        continue;
                    }
                    Ok(None) => continue,
                    Err(failure) => return self.fail(failure),
                }
            };

            // ── 悬停 → 点击 → 验证 ──
            match self.select_one(io, &target, &page) {
                Ok(()) => {
                    self.selected.push(target.item_id.clone());
                    self.remaining.retain(|t| t != &target.item_id);
                    self.current = None;
                }
                Err(failure) => return self.fail(failure),
            }
        }
    }

    /// 有界翻页。返回 `Ok(Some(page))` 表示视口已移动。
    fn turn_page(
        &mut self,
        io: &mut dyn DirectSelectIo,
        map: &mut ListMap,
        observation: &RoiObservation,
        current: &PageSnapshot,
    ) -> Result<Option<PageSnapshot>, DirectSelectFailure> {
        let item_id = self.remaining[0].clone();
        if self.wheel_inputs >= MAX_WHEEL_INPUTS {
            return Err(DirectSelectFailure::TargetNotFound {
                item_id,
                wheel_inputs: self.wheel_inputs,
            });
        }
        let hint = map.navigation_hint(&item_id, observation, self.item_kind);
        let direction = match hint {
            NavigationHint::Scroll(d) | NavigationHint::Recenter(d) => d,
            NavigationHint::ExpectedVisible => {
                // 映射说它应该在可见行里，但上一轮没识别到 →
                // 不得盲滚；按「找不到」安全失败
                return Err(DirectSelectFailure::TargetNotFound {
                    item_id,
                    wheel_inputs: self.wheel_inputs,
                });
            }
        };

        let delta = crate::loadout_sync::direct_select::page_navigation::navigation_delta(
            crate::loadout_sync::direct_select::page_navigation::NavInput::Full(direction),
        );
        if let Err(detail) = io.scroll(delta) {
            return Err(DirectSelectFailure::Io { detail });
        }
        self.wheel_inputs += 1;
        self.trace.wheel_inputs += 1;

        // A single unchanged frame can be an animation/timing artifact. Keep
        // observing for a bounded window before deciding that the turn failed.
        let mut previous = observation.clone();
        for _ in 0..crate::loadout_sync::direct_select::page_navigation::NO_MOVEMENT_FRAMES {
            let after = match io.observe() {
                Ok(o) => o,
                Err(detail) => return Err(DirectSelectFailure::Io { detail }),
            };
            let relation = compare_page_turn(
                &previous_page_slots(&previous),
                &previous_page_slots(&after),
                direction,
                after.roi_height,
            );
            let after_page = snapshot_from(&after);
            if !matches!(
                relation,
                PageRelation::SameViewport | PageRelation::Uncertain
            ) || after_page.signature != current.signature
            {
                map.advance(observation, &after, self.item_kind, direction);
                return Ok(Some(after_page));
            }
            previous = after;
        }

        // Confirm a boundary with a smaller probe instead of treating the
        // first unchanged full-page sample as definitive.
        if self.wheel_inputs >= MAX_WHEEL_INPUTS {
            return Err(DirectSelectFailure::TargetNotFound {
                item_id,
                wheel_inputs: self.wheel_inputs,
            });
        }
        let probe = crate::loadout_sync::direct_select::page_navigation::navigation_delta(
            crate::loadout_sync::direct_select::page_navigation::NavInput::Probe(direction),
        );
        io.scroll(probe)
            .map_err(|detail| DirectSelectFailure::Io { detail })?;
        self.wheel_inputs += 1;
        self.trace.wheel_inputs += 1;
        for _ in 0..crate::loadout_sync::direct_select::page_navigation::NO_MOVEMENT_FRAMES {
            let after = io
                .observe()
                .map_err(|detail| DirectSelectFailure::Io { detail })?;
            let after_page = snapshot_from(&after);
            if after_page.signature != current.signature
                || !matches!(
                    compare_page_turn(
                        &previous_page_slots(observation),
                        &previous_page_slots(&after),
                        direction,
                        after.roi_height,
                    ),
                    PageRelation::SameViewport | PageRelation::Uncertain
                )
            {
                map.advance(observation, &after, self.item_kind, direction);
                return Ok(Some(after_page));
            }
        }
        Err(DirectSelectFailure::TargetNotFound {
            item_id,
            wheel_inputs: self.wheel_inputs,
        })
    }

    /// 单目标：重定位 + 悬停 + 点击 + 验证。
    fn select_one(
        &mut self,
        io: &mut dyn DirectSelectIo,
        initial: &DirectClickTarget,
        page: &PageSnapshot,
    ) -> Result<(), DirectSelectFailure> {
        let item_id = initial.item_id.clone();
        let mut target = initial.clone();
        let mut click_before_slots = page.slots.clone();

        // ── 悬停前重定位（有界）──
        self.enter(DirectSelectPhase::Hovering);
        let mut before_sample = HoverSample { target_score: 0.0 };
        let mut hovered = false;
        for _ in 0..MAX_HOVER_RELOCATIONS {
            let center = target.slot.center();
            if let Err(detail) = io.move_cursor(center) {
                return Err(DirectSelectFailure::Io { detail });
            }
            let obs = match io.observe() {
                Ok(o) => o,
                Err(detail) => return Err(DirectSelectFailure::Io { detail }),
            };
            let slots = obs.slot_refs();
            let Some(relocated) = find_visible_target(&slots, &item_id, self.item_kind) else {
                return Err(DirectSelectFailure::TargetLostWhileRelocating { item_id });
            };
            let moved = (relocated.slot.rect.x - target.slot.rect.x).abs() > 2
                || (relocated.slot.rect.y - target.slot.rect.y).abs() > 2;
            target = relocated;
            if moved {
                continue;
            }
            // 几何稳定 → 等 hover 确认
            let rects: Vec<_> = obs.slots.iter().map(|s| s.rect).collect();
            let index = obs
                .slots
                .iter()
                .position(|s| s.row == target.slot.row && s.col == target.slot.col);
            let Some(index) = index else {
                return Err(DirectSelectFailure::TargetLostWhileRelocating { item_id });
            };
            let _ = &rects;
            match wait_for_hover_frames(index, || {
                let image = io.capture_rgba().ok()?;
                let o = io.observe().ok()?;
                let rects: Vec<_> = o.slots.iter().map(|s| s.rect).collect();
                Some((image, rects, io.now_ms()))
            }) {
                HoverOutcome::Confirmed(sample) => {
                    before_sample = sample;
                    // The page used for the click baseline must be the last
                    // stable page observed during relocation, not the page
                    // from before relocation started.
                    if let Ok(stable) = io.observe() {
                        click_before_slots = snapshot_from(&stable).slots;
                    }
                    hovered = true;
                    break;
                }
                HoverOutcome::Timeout(_) | HoverOutcome::NotEvaluable { .. } => {}
            }
        }
        if !hovered {
            return Err(DirectSelectFailure::HoverNotConfirmed { item_id });
        }
        self.trace.hovers_confirmed += 1;

        // ── 点击 + 有界验证 ──
        for attempt in 1..=MAX_TARGET_CLICK_ATTEMPTS {
            self.enter(DirectSelectPhase::Clicking);
            if let Err(detail) = io.click() {
                return Err(DirectSelectFailure::Io { detail });
            }
            self.trace.clicks += 1;

            self.enter(DirectSelectPhase::VerifyingSelection);
            let start = io.now_ms();
            let mut observation_index = 0u32;
            loop {
                observation_index += 1;
                let obs = match io.observe() {
                    Ok(o) => o,
                    Err(detail) => return Err(DirectSelectFailure::Io { detail }),
                };
                let page_after = snapshot_from(&obs);
                let img = match io.capture_rgba() {
                    Ok(i) => i,
                    Err(detail) => return Err(DirectSelectFailure::Io { detail }),
                };
                let step = PostClickStep {
                    image: &img,
                    page: &page_after,
                    before: before_sample,
                    before_slots: &click_before_slots,
                    elapsed_ms: io.now_ms().saturating_sub(start),
                    observation: observation_index,
                };
                match observe_post_click(&step, &target, self.item_kind) {
                    PostClickObservation::Selected { .. } => return Ok(()),
                    PostClickObservation::Unchanged { .. } => {
                        if io.now_ms().saturating_sub(start)
                            >= crate::loadout_sync::direct_select::selection_verify::POST_CLICK_CONFIRM_TIMEOUT_MS
                            && observation_index
                                >= crate::loadout_sync::direct_select::selection_verify::POST_CLICK_MIN_OBSERVATIONS
                        {
                            break;
                        }
                    }
                    PostClickObservation::NotEvaluable { detail } => {
                        return Err(DirectSelectFailure::SelectionNotEvaluable { item_id, detail });
                    }
                }
            }
            if attempt == MAX_TARGET_CLICK_ATTEMPTS {
                return Err(DirectSelectFailure::SelectionUnchanged {
                    item_id,
                    attempts: attempt,
                });
            }
        }
        Err(DirectSelectFailure::SelectionUnchanged {
            item_id,
            attempts: MAX_TARGET_CLICK_ATTEMPTS,
        })
    }

    fn fail(&mut self, failure: DirectSelectFailure) -> DirectSelectOutcome {
        self.enter(DirectSelectPhase::Failed);
        DirectSelectOutcome::Failed {
            failure,
            selected: self.selected.clone(),
        }
    }
}

/// 由 ROI 观察构造页面快照。
fn snapshot_from(observation: &RoiObservation) -> PageSnapshot {
    PageSnapshot::new(
        observation.tracked(),
        observation.fingerprint.clone(),
        observation.roi_height,
    )
}

fn previous_page_slots(
    observation: &RoiObservation,
) -> Vec<crate::loadout_sync::direct_select::page_relation::TrackedSlot> {
    snapshot_from(observation).slots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_and_failures_have_stable_labels() {
        assert_eq!(DirectSelectPhase::Hovering.label(), "hovering");
        assert_eq!(DirectSelectPhase::Done.label(), "done");
        let f = DirectSelectFailure::TargetNotFound {
            item_id: "x".into(),
            wheel_inputs: 20,
        };
        assert_eq!(f.label(), "target_not_found");
        assert!(f.message().contains("x"));
        let o = DirectSelectOutcome::Succeeded { selected: vec![] };
        assert!(o.is_success());
        assert!(o.failure().is_none());
        let o2 = DirectSelectOutcome::Failed {
            failure: DirectSelectFailure::HoverNotConfirmed {
                item_id: "y".into(),
            },
            selected: vec!["a".into()],
        };
        assert!(!o2.is_success());
        assert_eq!(o2.selected(), &["a".to_string()]);
    }
}
