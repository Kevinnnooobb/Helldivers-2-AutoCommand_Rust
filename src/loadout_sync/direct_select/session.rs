//! 端到端装配会话：把「Home 激活」和「目录驱动选择」串成一次完整装配。
//!
//! ## 为什么需要这一层
//!
//! [`crate::loadout_sync::direct_select::machine::DirectSelectMachine`] 只负责
//! **列表已打开之后**的目标选择；[`home_activation`] 只负责**打开一个列表**。
//! 参考实现里这两件事由同一个 `AutomationSession` 循环驱动：
//!
//! ```text
//! 观察 Home → 找空槽 → 点击打开列表 → 在列表里选目标 → 等回到 Home → 下一个目标
//! ```
//!
//! 本模块就是这个循环，且**不复制**任何判定逻辑：找空槽用 `home_activation`，
//! 选目标用 `machine`，它自己只做「有界循环 + 证据汇总」。
//!
//! ## 硬上限（plan6 §2）
//!
//! * 每个目标只打开一次列表、只跑一次状态机；状态机内部的滚动/点击/重试上限
//!   全部沿用（见 `machine.rs`）；
//! * 会话自身的循环次数上限 = 目标数（每个目标一次），**不额外重试**；
//! * 任一目标失败 → 立即整轮安全停止，绝不「跳过失败目标继续装」。
//!
//! ## 动作权威
//!
//! 由 controller 直接调用，是当前唯一的装配动作路径。
use std::fmt::Write as _;

use crate::loadout_sync::direct_select::click_plan::SlotRef;
use crate::loadout_sync::direct_select::home_activation::{
    find_home_slot_kinded, open_slot_list, ActivationOutcome, HomeDriver, HomeOpenTarget,
};
use crate::loadout_sync::direct_select::machine::{
    DirectSelectIo, DirectSelectMachine, DirectSelectTrace,
};
use crate::loadout_sync::reference_vision::{
    ItemKind, RoiObservation, SlotKind, UiState as RefUiState,
};

/// 会话需要的 IO：状态机的观察/动作 + Home 激活的两个动作。
///
/// 单独定义而不是直接 `DirectSelectIo + HomeDriver`，是为了避开两个 trait
/// 同名方法（`now_ms` / `cancelled`）在同一次调用里的歧义。
pub trait SessionIo: DirectSelectIo {
    /// 在**帧坐标** `point` 处点击一次。
    ///
    /// 按住时长由输入层决定（当前实现固定 20ms，见 `LoadoutInputController`），
    /// 因此这里不接受时长参数。
    fn click_at(&mut self, point: (i32, i32)) -> Result<(), String>;

    /// 当前是否已显示指定类型的列表。
    fn list_is_open(&mut self, kind: ItemKind) -> Result<bool, String>;

    /// 轮询间隔（真实环境用 `POLL_MS`；确定性回放返回 0，不做真实等待）。
    fn poll_ms(&mut self) -> u64 {
        crate::loadout_sync::direct_select::home_activation::POLL_MS
    }

    /// 最近一次观察到的参考层界面状态（拒答原因 / 日志用）。
    fn ui_state(&mut self) -> Option<RefUiState> {
        None
    }

    /// 当前界面上的 Home 槽位（`(SlotRef, SlotKind)`）。
    ///
    /// 默认实现从一次普通观察里推导；回放可以覆盖它以便脚本化多状态界面。
    fn home_slots(&mut self) -> Result<Vec<(SlotRef, SlotKind)>, String> {
        let obs = self.observe()?;
        Ok(slots_of(&obs))
    }
}

/// 从一次观察里取出 `(SlotRef, SlotKind)`。
pub fn slots_of(obs: &RoiObservation) -> Vec<(SlotRef, SlotKind)> {
    obs.slots
        .iter()
        .map(|s| (s.as_slot_ref(), s.kind))
        .collect()
}

/// 把 `SessionIo` 包装成 [`HomeDriver`]（`open_slot_list` 需要它）。
struct DriverProxy<'a> {
    io: &'a mut dyn SessionIo,
}

impl HomeDriver for DriverProxy<'_> {
    fn click(&mut self, point: (i32, i32), _hold_ms: u64) -> Result<(), String> {
        self.io.click_at(point)
    }

    fn list_is_open(&mut self, kind: ItemKind) -> Result<bool, String> {
        self.io.list_is_open(kind)
    }

    fn now_ms(&mut self) -> u64 {
        DirectSelectIo::now_ms(self.io)
    }

    fn poll_ms(&mut self) -> u64 {
        SessionIo::poll_ms(self.io)
    }

    fn cancelled(&mut self) -> bool {
        DirectSelectIo::cancelled(self.io)
    }
}

/// 一次会话要装配什么（都是**目录 ID**，不是显示名）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPlan {
    /// 四个战备（按 preset 顺序，先装第一个）
    pub stratagems: Vec<String>,
    /// Booster（可空）
    pub booster: Option<String>,
}

impl SessionPlan {
    pub fn is_empty(&self) -> bool {
        self.stratagems.is_empty() && self.booster.is_none()
    }
}

/// 一次会话的完整证据（plan6 §11 要求的「期望/识别/确认/阶段/重试/弃权原因」）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionReport {
    /// 已确认装上的目录 ID（按确认顺序）
    pub selected: Vec<String>,
    /// 成功打开的列表次数
    pub opened_lists: u32,
    /// 每个目标跑出的状态机轨迹汇总
    pub wheel_inputs: u32,
    pub clicks: u32,
    pub hovers_confirmed: u32,
    /// 阶段迁移序列（跨目标拼接）
    pub phases: Vec<String>,
    /// 失败原因（空 = 全部成功）
    pub failures: Vec<String>,
    /// 计划里期望装上的条目总数
    pub expected: usize,
}

#[derive(Debug)]
pub struct SessionError {
    pub detail: String,
    pub report: SessionReport,
}

impl SessionError {
    /// 失败原因里是否包含某个线索（诊断/断言便利方法，供调用方与测试使用）。
    #[allow(dead_code)]
    pub fn contains(&self, needle: &str) -> bool {
        self.detail.contains(needle)
    }
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail)
    }
}

impl std::error::Error for SessionError {}

impl SessionReport {
    pub fn is_success(&self) -> bool {
        self.failures.is_empty() && self.selected.len() == self.expected
    }

    fn absorb(&mut self, trace: &DirectSelectTrace) {
        self.wheel_inputs += trace.wheel_inputs;
        self.clicks += trace.clicks;
        self.hovers_confirmed += trace.hovers_confirmed;
        for phase in &trace.phases {
            self.phases.push(phase.label().to_string());
        }
    }

    /// 一行可读摘要（写入同步日志）。
    pub fn summary(&self) -> String {
        let mut s = String::new();
        let _ = write!(
            s,
            "direct_select: 已确认 {} 项 [{}]，打开列表 {} 次，滚轮 {} 次，点击 {} 次，hover 确认 {} 次",
            self.selected.len(),
            self.selected.join(", "),
            self.opened_lists,
            self.wheel_inputs,
            self.clicks,
            self.hovers_confirmed
        );
        if !self.failures.is_empty() {
            let _ = write!(s, "；失败: {}", self.failures.join(" / "));
        }
        s
    }
}

/// 跑一次完整装配会话。
///
/// `log` 用于逐条上报（controller 会转成同步日志）；失败返回 `Err(原因)`，
/// 并且**已经装上的部分不会回滚**（游戏里无法安全回滚，只能报告）。
pub fn run(
    io: &mut dyn SessionIo,
    plan: &SessionPlan,
    log: &mut dyn FnMut(&str),
) -> Result<SessionReport, SessionError> {
    let mut report = SessionReport::default();
    if plan.is_empty() {
        log("direct_select: 没有需要装配的目标，跳过");
        return Ok(report);
    }
    report.expected = plan.stratagems.len() + usize::from(plan.booster.is_some());

    // 战备：逐个「打开一个空槽 → 选中」。
    for item_id in &plan.stratagems {
        log(&format!("direct_select: 目标 {item_id}"));
        if let Err(detail) = open_next_slot(io, ItemKind::Stratagem, log, &mut report) {
            report.failures.push(detail.clone());
            return Err(SessionError { detail, report });
        }
        if let Err(detail) = select_one(io, item_id, ItemKind::Stratagem, log, &mut report) {
            report.failures.push(detail.clone());
            return Err(SessionError { detail, report });
        }
    }

    // Booster：单槽、单目标。
    if let Some(booster) = &plan.booster {
        log(&format!("direct_select: Booster {booster}"));
        if let Err(detail) = open_next_slot(io, ItemKind::Booster, log, &mut report) {
            report.failures.push(detail.clone());
            return Err(SessionError { detail, report });
        }
        if let Err(detail) = select_one(io, booster, ItemKind::Booster, log, &mut report) {
            report.failures.push(detail.clone());
            return Err(SessionError { detail, report });
        }
    }

    log(&report.summary());
    Ok(report)
}

/// 找「该类型第一个空 Home 槽」并点击打开列表。
fn open_next_slot(
    io: &mut dyn SessionIo,
    kind: ItemKind,
    log: &mut dyn FnMut(&str),
    report: &mut SessionReport,
) -> Result<(), String> {
    let slots = io.home_slots()?;
    // 只认 Home 槽位：列表槽位不该出现在这里，出现了就是识别出了问题
    let home_only: Vec<(SlotRef, SlotKind)> = slots
        .iter()
        .filter(|(_, k)| !k.is_list())
        .cloned()
        .collect();
    let found = find_home_slot_kinded(&home_only, kind);
    let Some(slot) = found else {
        // 拒答原因必须可诊断：把「界面状态 + 看到了哪些槽位」原样带出去
        let ui = io
            .ui_state()
            .map(|s| s.label().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let seen: Vec<String> = slots
            .iter()
            .map(|(s, k)| format!("{}({},{} occupied={})", k.label(), s.row, s.col, s.occupied))
            .collect();
        return Err(format!(
            "Home 上没有可用的空槽（{:?}）；界面状态={ui}，槽位: [{}]",
            kind,
            seen.join(", ")
        ));
    };
    let target = HomeOpenTarget {
        item_kind: kind,
        // `SlotRef.rect` 是帧坐标；换算成屏幕坐标由 IO 负责
        point: slot.center(),
    };
    log(&format!(
        "direct_select: 点击空槽 {:?} ({}, {})",
        kind, target.point.0, target.point.1
    ));
    let outcome = open_slot_list(&mut DriverProxy { io }, target)?;
    match outcome {
        ActivationOutcome::ListOpened => {
            report.opened_lists += 1;
            Ok(())
        }
        other => {
            let reason = format!("打开列表失败: {}", other.label());
            Err(reason)
        }
    }
}

/// 在已打开的列表里选择一个目标（跑一次状态机，等回到 Home）。
fn select_one(
    io: &mut dyn SessionIo,
    item_id: &str,
    kind: ItemKind,
    log: &mut dyn FnMut(&str),
    report: &mut SessionReport,
) -> Result<(), String> {
    let mut machine = DirectSelectMachine::new(vec![item_id.to_string()], kind);
    let outcome = machine.run(io);
    report.absorb(machine.trace());
    // 证据：最终阶段 / 剩余目标 / 已确认集合（plan6 §11 要求可核对）
    log(&format!(
        "direct_select: 阶段={} 剩余={:?} 已确认={:?}",
        machine.phase().label(),
        machine.remaining(),
        machine.selected()
    ));
    if outcome.is_success() {
        for id in outcome.selected() {
            log(&format!("direct_select: 已确认 {id}"));
            report.selected.push(id.clone());
        }
        if !outcome.selected().iter().any(|id| id == item_id) {
            let reason = format!("状态机报告成功但没有确认 {item_id}");
            report.failures.push(reason.clone());
            return Err(reason);
        }
        Ok(())
    } else {
        let reason = match outcome.failure() {
            Some(f) => format!("{item_id}: {} — {}", f.label(), f.message()),
            None => format!("{item_id}: 未确认成功"),
        };
        log(&format!("direct_select: 安全停止（{reason}）"));
        Err(reason)
    }
}
