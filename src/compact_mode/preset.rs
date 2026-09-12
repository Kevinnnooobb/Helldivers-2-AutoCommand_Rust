// 紧凑模式预设模型 —— Overlay 中的 5 个槽位草稿 + 生命周期状态
//
// 关键约束（对应实施要求 §五 / §七 / §十）：
//   * 不新建第二套 Loadout 数据系统：槽位 6~9 = 游戏 Stratagem 1~4，槽位 10 = Booster，
//     草稿内容由现有 Slot06~10 / Profile 生成，选中项复用现有战备库与图标；
//   * 编辑只改内存草稿，不写磁盘；只有「自动装配成功」之后才写回 Slot06~10 与当前 Profile；
//   * 「显示浮窗」与「自动装配」是两个独立动作，互不推断。
use std::collections::HashMap;

use crate::loadout_sync::error::LoadoutSyncError;
use crate::loadout_sync::selection::{LoadoutItem, LoadoutSyncSelection};
use crate::stratagems::{PluginStratagem, PLUGIN_SLOT_MARK};

/// 预设槽位数：4 个 Stratagem + 1 个 Booster
pub const PRESET_SLOTS: usize = 5;
/// Booster 在预设中的下标
pub const BOOSTER_INDEX: usize = 4;
/// 预设第一格对应的 H2AC 槽位下标（Slot06）
pub const FIRST_H2AC_SLOT: usize = 5;

/// 该 H2AC 槽位是否属于「自动装配」范围（Slot06~10）。
///
/// 浮窗同时显示上排 TASK（01~05）与下排 LOADOUT（06~10），但只有下排参与自动装配：
/// 上排沿用既有战斗中呼叫语义，编辑后立即写盘；下排是预设草稿，装配成功后才写回。
pub fn is_loadout_slot(slot: usize) -> bool {
    (FIRST_H2AC_SLOT..FIRST_H2AC_SLOT + PRESET_SLOTS).contains(&slot)
}

/// H2AC 槽位 → 预设草稿下标（0..4 = S1..S4 + Booster）；非 LOADOUT 槽位返回 None。
pub fn draft_index_of(slot: usize) -> Option<usize> {
    is_loadout_slot(slot).then(|| slot - FIRST_H2AC_SLOT)
}

/// 写回槽位所需的附加数据：内置战备记索引，插件/强化存整条数据
#[derive(Debug, Clone, PartialEq)]
pub enum PresetPayload {
    Base(usize),
    Plugin(Box<PluginStratagem>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PresetEntry {
    pub item: LoadoutItem,
    pub payload: PresetPayload,
}

impl PresetEntry {
    pub fn from_base(index: usize) -> Option<Self> {
        LoadoutItem::base(index).map(|item| Self {
            item,
            payload: PresetPayload::Base(index),
        })
    }

    pub fn from_plugin(p: &PluginStratagem) -> Self {
        Self {
            item: LoadoutItem::plugin(p),
            payload: PresetPayload::Plugin(Box::new(p.clone())),
        }
    }

    pub fn name(&self) -> &str {
        &self.item.name
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PresetDraft {
    pub entries: [Option<PresetEntry>; PRESET_SLOTS],
}

impl PresetDraft {
    /// 由 H2AC Slot06~10 生成（复用现有槽位数据；越界/非法索引视为空）。
    pub fn from_slots(
        slots: &[Option<usize>],
        plugin_slots: &HashMap<usize, PluginStratagem>,
    ) -> Self {
        let mut draft = Self::default();
        for (offset, entry) in draft.entries.iter_mut().enumerate() {
            let index = FIRST_H2AC_SLOT + offset;
            let Some(raw) = slots.get(index).copied().flatten() else {
                continue;
            };
            if raw == PLUGIN_SLOT_MARK {
                if let Some(p) = plugin_slots.get(&index) {
                    *entry = Some(PresetEntry::from_plugin(p));
                }
            } else {
                *entry = PresetEntry::from_base(raw);
            }
        }
        draft
    }

    /// 转换为 Loadout Sync 的选择模型（严格按顺序，禁止左移 / 补空 / 猜测）。
    pub fn to_selection(&self) -> LoadoutSyncSelection {
        let mut selection = LoadoutSyncSelection::default();
        for i in 0..4 {
            selection.stratagems[i] = self.entries[i].as_ref().map(|e| e.item.clone());
        }
        selection.booster = self.entries[BOOSTER_INDEX].as_ref().map(|e| e.item.clone());
        selection
    }

    /// 本地校验：4 个 Stratagem 必须齐全且不重复；Booster 可空。
    pub fn validate(&self) -> Result<(), LoadoutSyncError> {
        self.to_selection().validate()
    }

    pub fn stratagem_count(&self) -> usize {
        self.entries[..4].iter().filter(|e| e.is_some()).count()
    }

    pub fn set(&mut self, index: usize, entry: Option<PresetEntry>) {
        if let Some(slot) = self.entries.get_mut(index) {
            *slot = entry;
        }
    }

    pub fn name(&self, index: usize) -> Option<&str> {
        self.entries
            .get(index)
            .and_then(|e| e.as_ref())
            .map(|e| e.name())
    }

    /// 状态栏摘要，例如 "4/4 + Booster"。
    pub fn summary(&self) -> String {
        let strat = self.stratagem_count();
        if self.entries[BOOSTER_INDEX].is_some() {
            format!("{strat}/4 + Booster")
        } else {
            format!("{strat}/4")
        }
    }

    /// 成功后写回 H2AC 槽位（这是唯一的持久化入口，由调用方在验证成功后触发）。
    pub fn apply_to_slots(
        &self,
        slots: &mut [Option<usize>],
        plugin_slots: &mut HashMap<usize, PluginStratagem>,
    ) {
        for (offset, entry) in self.entries.iter().enumerate() {
            let index = FIRST_H2AC_SLOT + offset;
            if index >= slots.len() {
                continue;
            }
            match entry {
                Some(e) => match &e.payload {
                    PresetPayload::Base(i) => {
                        slots[index] = Some(*i);
                        plugin_slots.remove(&index);
                    }
                    PresetPayload::Plugin(p) => {
                        slots[index] = Some(PLUGIN_SLOT_MARK);
                        plugin_slots.insert(index, (**p).clone());
                    }
                },
                None => {
                    slots[index] = None;
                    plugin_slots.remove(&index);
                }
            }
        }
    }
}

// ─── Overlay 生命周期 ───
//
// 关键设计（对应本次修订 §3 / §5 / §6 / §7 / §14）：
//   * 浮窗的「可见性」与「自动装配状态」是两个正交维度；
//   * 只有 Compact Toggle 热键能改变可见性；自动装配只做「临时隐藏 + 结束后恢复」，
//     并且恢复依据是自动化开始前记录的 were_visible，而不是自动化阶段本身；
//   * 运行期间允许再次打开浮窗查看进度（只读：禁止编辑）。

/// 自动装配是否需要「临时隐藏浮窗」。
///
/// 判据是「浮窗此刻是否真的显示在屏幕上」＝ 当前处于浮窗视图 **且** 可见性为可见。
/// 正常模式（主界面）下返回 false：自动装配绝不触碰窗口（不切视图、不改透明度），
/// 这正是「正常模式与紧凑模式互不干扰」的落点。
pub fn needs_temporary_hide(view_is_compact: bool, phase: OverlayPhase) -> bool {
    view_is_compact && phase.allows_overlay()
}

/// 浮窗可见性 / 编辑状态（**只由浮窗热键与恢复逻辑驱动**）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayPhase {
    /// 浮窗不可见
    Hidden,
    /// 浮窗可见
    Visible,
    /// 浮窗可见且正在编辑某个槽位
    Editing,
}

impl OverlayPhase {
    pub fn name(self) -> &'static str {
        match self {
            Self::Hidden => "Hidden",
            Self::Visible => "Visible",
            Self::Editing => "Editing",
        }
    }

    pub fn allows_overlay(self) -> bool {
        matches!(self, Self::Visible | Self::Editing)
    }
}

/// 自动装配状态（与浮窗可见性正交；浮窗隐藏时依然继续推进）
#[derive(Debug, Clone, PartialEq, Default)]
pub enum AutomationPhase {
    #[default]
    Idle,
    Running,
    Succeeded,
    Failed(String),
    Cancelled,
}

impl AutomationPhase {
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }
}

/// 兼容旧命名的结果类型（浮窗状态栏与日志使用）
#[derive(Debug, Clone, PartialEq, Default)]
pub enum OverlayOutcome {
    #[default]
    None,
    Success,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PresetEditorState {
    /// 正在编辑的 H2AC 槽位下标（0..9；0~4 = 上排 TASK，5~9 = 下排 LOADOUT）
    pub slot: usize,
    pub search: String,
    /// 分类过滤；None = 全部
    pub category: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompactPresetState {
    phase: OverlayPhase,
    /// 自动装配状态（Running 时浮窗只读）
    pub automation: AutomationPhase,
    /// 自动化开始前浮窗是否可见 —— 结束后据此恢复（None = 当前没有进行中的恢复待办）
    were_visible: Option<bool>,
    pub draft: PresetDraft,
    pub editor: Option<PresetEditorState>,
    pub outcome: OverlayOutcome,
    /// 状态栏文本（错误原因 / 结果）
    pub status: String,
    /// 自动装配成功后的快照：仅用于断言与日志
    pub last_requested: Option<PresetDraft>,
    /// 阶段轨迹（调试与测试断言用）
    pub history: Vec<OverlayPhase>,
}

impl Default for CompactPresetState {
    fn default() -> Self {
        Self {
            phase: OverlayPhase::Hidden,
            automation: AutomationPhase::Idle,
            were_visible: None,
            draft: PresetDraft::default(),
            editor: None,
            outcome: OverlayOutcome::None,
            status: String::new(),
            last_requested: None,
            history: vec![OverlayPhase::Hidden],
        }
    }
}

impl CompactPresetState {
    pub fn phase(&self) -> OverlayPhase {
        self.phase
    }

    /// 浮窗是否应当占据屏幕（唯一判据是可见性维度，不看自动化状态）
    pub fn overlay_visible(&self) -> bool {
        self.phase.allows_overlay()
    }

    /// 自动化进行中禁止编辑（但允许查看）
    pub fn editing_allowed(&self) -> bool {
        !self.automation.is_running()
    }

    fn goto(&mut self, phase: OverlayPhase) {
        self.phase = phase;
        self.history.push(phase);
    }

    /// 用当前 Slot06~10 刷新草稿（显示浮窗时调用；不覆盖正在编辑的内容）
    pub fn reseed(&mut self, draft: PresetDraft) {
        self.draft = draft;
        self.editor = None;
    }

    /// 浮窗热键：只切换可见性，任何自动化状态下都可用（§7 / §14）。
    pub fn toggle(&mut self) -> bool {
        if self.overlay_visible() {
            self.hide();
            false
        } else {
            self.show();
            true
        }
    }

    /// 显示浮窗（运行中允许显示，仅只读）。
    ///
    /// 这是**用户主动改变可见性**的入口：一旦用户自己动过可见性，
    /// 自动化结束时就不再替他做决定（清掉待恢复的快照）。
    pub fn show(&mut self) {
        self.editor = None;
        self.were_visible = None;
        if matches!(self.automation, AutomationPhase::Idle) {
            // 只有在没有自动化在跑时才清掉上次结果
            self.outcome = OverlayOutcome::None;
            self.status.clear();
        }
        self.goto(OverlayPhase::Visible);
    }

    /// 隐藏浮窗（用户主动隐藏；自动化临时隐藏走 `begin_automation_hide`）。
    pub fn hide(&mut self) {
        self.editor = None;
        self.were_visible = None;
        self.goto(OverlayPhase::Hidden);
    }

    /// 自动化开始：记录「开始前是否可见」并临时隐藏浮窗（§5 / §6）。
    pub fn begin_automation_hide(&mut self) {
        if self.were_visible.is_none() {
            self.were_visible = Some(self.overlay_visible());
        }
        self.editor = None;
        self.goto(OverlayPhase::Hidden);
    }

    /// 登记一次自动化请求：冻结快照 → 临时隐藏 → **立刻**标记运行中。
    ///
    /// 必须在启动工作线程之前调用：线程可能在第一次 poll 之前就已经结束
    /// （例如前台校验只要几毫秒就失败），先登记才能保证 poll 一定消费到终态，
    /// 否则浮窗会永远停留在「临时隐藏」，成功结果也不会写盘。
    pub fn begin_automation(&mut self) -> PresetDraft {
        let snapshot = self.snapshot_for_automation();
        self.begin_automation_hide();
        self.mark_running();
        snapshot
    }

    /// 自动化结束：按开始前的状态恢复浮窗可见性（成功 / 失败 / 取消 / 异常都会走到这里）。
    ///
    /// 若用户在自动化期间自己改过可见性（快照已被 `show` / `hide` 清空），
    /// 则保持用户当前的选择不动 —— 恢复永远是「快照驱动」，绝不覆盖用户操作。
    pub fn restore_after_automation(&mut self) {
        match self.were_visible.take() {
            Some(true) => self.goto(OverlayPhase::Visible),
            Some(false) => self.goto(OverlayPhase::Hidden),
            None => {}
        }
    }

    pub fn open_editor(&mut self, slot: usize, default_category: Option<String>) -> bool {
        if slot >= crate::config::SLOT_COUNT || !self.editing_allowed() {
            return false;
        }
        self.editor = Some(PresetEditorState {
            slot,
            search: String::new(),
            category: default_category,
        });
        if self.phase == OverlayPhase::Visible {
            self.goto(OverlayPhase::Editing);
        }
        true
    }

    /// 关闭选择器并交出编辑目标：由调用方决定写「槽位」还是写「预设草稿」。
    pub fn take_editor(&mut self) -> Option<PresetEditorState> {
        let editor = self.editor.take();
        if editor.is_some() && self.phase == OverlayPhase::Editing {
            self.goto(OverlayPhase::Visible);
        }
        editor
    }

    /// 选择器取消：保持原配置不变。
    pub fn cancel_editor(&mut self) {
        self.editor = None;
        if self.phase == OverlayPhase::Editing {
            self.goto(OverlayPhase::Visible);
        }
    }

    /// 记录本次装配要写入的快照（不改变浮窗可见性）。
    pub fn snapshot_for_automation(&mut self) -> PresetDraft {
        self.last_requested = Some(self.draft.clone());
        self.draft.clone()
    }

    /// 工作线程已启动。
    pub fn mark_running(&mut self) {
        self.automation = AutomationPhase::Running;
    }

    /// 自动化结束（成功 / 失败 / 取消）：写状态 + 按快照恢复浮窗，然后清掉恢复待办。
    pub fn finish(&mut self, outcome: OverlayOutcome, status: impl Into<String>) {
        self.automation = match &outcome {
            OverlayOutcome::Success => AutomationPhase::Succeeded,
            OverlayOutcome::Failed(message) => AutomationPhase::Failed(message.clone()),
            OverlayOutcome::Cancelled => AutomationPhase::Cancelled,
            OverlayOutcome::None => AutomationPhase::Idle,
        };
        self.outcome = outcome;
        self.status = status.into();
        self.restore_after_automation();
    }

    /// 状态栏文本（供 Overlay 底部显示）。
    pub fn status_line(&self) -> String {
        if self.automation.is_running() {
            return "AUTO LOADOUT · Running…".into();
        }
        if !self.status.is_empty() {
            return self.status.clone();
        }
        match self.outcome {
            OverlayOutcome::Success => "AUTO LOADOUT · Completed".into(),
            OverlayOutcome::Failed(_) => "AUTO LOADOUT · Failed".into(),
            OverlayOutcome::Cancelled => "AUTO LOADOUT · Cancelled".into(),
            OverlayOutcome::None => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stratagems::STRATAGEMS;

    fn base_entry(name: &str) -> PresetEntry {
        let idx = STRATAGEMS
            .iter()
            .position(|s| s.name == name)
            .unwrap_or_else(|| panic!("缺少内置战备 {name}"));
        PresetEntry::from_base(idx).expect("内置战备")
    }

    fn full_draft() -> PresetDraft {
        let mut draft = PresetDraft::default();
        for (i, name) in ["反器材步枪", "飞鹰空袭", "轨道炮攻击", "类星体加农炮"]
            .iter()
            .enumerate()
        {
            draft.set(i, Some(base_entry(name)));
        }
        draft
    }

    #[test]
    fn draft_maps_slot06_to_10_without_shifting() {
        let mut slots = vec![None; crate::config::SLOT_COUNT];
        slots[FIRST_H2AC_SLOT] = Some(0);
        slots[FIRST_H2AC_SLOT + 2] = Some(2);
        let draft = PresetDraft::from_slots(&slots, &HashMap::new());
        assert!(draft.entries[0].is_some());
        assert!(draft.entries[1].is_none(), "不允许左移填充");
        assert!(draft.entries[2].is_some());
        assert!(draft.entries[3].is_none());
        assert!(draft.entries[BOOSTER_INDEX].is_none());
        assert!(draft.validate().is_err(), "缺少 Stratagem 必须校验失败");
    }

    #[test]
    fn draft_roundtrip_through_slots() {
        let draft = full_draft();
        let mut slots = vec![None; crate::config::SLOT_COUNT];
        let mut plugins = HashMap::new();
        draft.apply_to_slots(&mut slots, &mut plugins);
        let back = PresetDraft::from_slots(&slots, &plugins);
        assert_eq!(back, draft);
    }

    #[test]
    fn booster_only_draft_is_invalid() {
        let mut draft = PresetDraft::default();
        draft.set(BOOSTER_INDEX, Some(base_entry("增援")));
        assert_eq!(draft.stratagem_count(), 0);
        assert!(draft.validate().is_err());
    }

    #[test]
    fn toggle_only_changes_visibility_and_works_while_running() {
        let mut state = CompactPresetState::default();
        assert_eq!(state.phase(), OverlayPhase::Hidden);

        // Hidden → Visible
        assert!(state.toggle());
        assert_eq!(state.phase(), OverlayPhase::Visible);
        // Visible → Hidden
        assert!(!state.toggle());
        assert_eq!(state.phase(), OverlayPhase::Hidden);

        // 运行中依然可以显示/隐藏（只读）：浮窗热键必须始终可用
        state.mark_running();
        assert!(state.toggle());
        assert_eq!(state.phase(), OverlayPhase::Visible);
        assert!(!state.editing_allowed(), "运行中禁止编辑");
        assert!(!state.toggle());
        assert_eq!(state.phase(), OverlayPhase::Hidden);
    }

    #[test]
    fn auto_loadout_temporarily_hides_and_then_restores_visible_overlay() {
        // 场景 B/E：浮窗可见 → 自动装配 → 临时隐藏 → 结束后恢复可见
        let mut state = CompactPresetState::default();
        state.show();
        state.snapshot_for_automation();
        state.begin_automation_hide();
        assert_eq!(state.phase(), OverlayPhase::Hidden, "自动化开始即临时隐藏");

        state.mark_running();
        assert_eq!(state.automation, AutomationPhase::Running);
        // 运行中用户又打开浮窗看进度
        state.show();
        assert_eq!(state.phase(), OverlayPhase::Visible);

        state.finish(OverlayOutcome::Success, "ok");
        assert_eq!(
            state.phase(),
            OverlayPhase::Visible,
            "结束后按「开始前可见」恢复显示"
        );
        assert_eq!(state.automation, AutomationPhase::Succeeded);
    }

    #[test]
    fn auto_loadout_from_hidden_overlay_stays_hidden() {
        // 场景 C：浮窗本来就是隐藏的 → 装配期间与结束后都必须保持隐藏
        let mut state = CompactPresetState::default();
        assert_eq!(state.phase(), OverlayPhase::Hidden);
        state.snapshot_for_automation();
        state.begin_automation_hide();
        state.mark_running();
        state.finish(OverlayOutcome::Success, "ok");
        assert_eq!(state.phase(), OverlayPhase::Hidden);
    }

    #[test]
    fn temporary_hide_only_applies_to_the_displayed_overlay() {
        // 只有「浮窗视图 + 可见」才需要临时隐藏；正常模式下自动装配不碰窗口
        assert!(needs_temporary_hide(true, OverlayPhase::Visible));
        assert!(needs_temporary_hide(true, OverlayPhase::Editing));
        assert!(!needs_temporary_hide(true, OverlayPhase::Hidden));
        assert!(!needs_temporary_hide(false, OverlayPhase::Visible));
        assert!(!needs_temporary_hide(false, OverlayPhase::Editing));
        assert!(!needs_temporary_hide(false, OverlayPhase::Hidden));
    }

    #[test]
    fn begin_automation_registers_running_before_the_worker_starts() {
        // 回归：任务「秒失败」时（前台校验几毫秒即失败），
        // 若运行中标记要等 poll 看到 Running 才设置，终态就会被漏掉，
        // 浮窗将永远停留在临时隐藏。begin_automation 必须先登记。
        let mut state = CompactPresetState::default();
        state.show();
        state.draft = full_draft();
        let snapshot = state.begin_automation();
        assert_eq!(snapshot, full_draft(), "快照在被隐藏之前冻结");
        assert_eq!(state.phase(), OverlayPhase::Hidden, "临时隐藏");
        assert!(
            state.automation.is_running(),
            "启动工作线程之前就必须标记为运行中"
        );

        // 线程在第一次 poll 之前就失败：poll 仍能消费终态 → 恢复可见
        state.finish(OverlayOutcome::Failed("GameNotForeground".into()), "x");
        assert_eq!(state.phase(), OverlayPhase::Visible, "秒失败也必须恢复浮窗");
        assert!(!state.automation.is_running());
    }

    #[test]
    fn user_visibility_change_during_automation_wins_over_snapshot_restore() {
        // 开始前隐藏 → 运行中用户自己打开查看 → 结束后保持用户打开的状态
        let mut state = CompactPresetState::default();
        state.begin_automation_hide();
        state.mark_running();
        assert!(state.toggle(), "运行中允许打开浮窗（只读）");
        state.finish(OverlayOutcome::Success, "ok");
        assert_eq!(
            state.phase(),
            OverlayPhase::Visible,
            "恢复不得反手关掉用户运行中手动打开的浮窗"
        );

        // 反向：开始前可见 → 运行中用户自己关掉 → 结束后保持隐藏
        let mut state = CompactPresetState::default();
        state.show();
        state.begin_automation_hide();
        state.mark_running();
        state.show();
        state.hide();
        state.finish(OverlayOutcome::Failed("X".into()), "x");
        assert_eq!(state.phase(), OverlayPhase::Hidden);
    }

    #[test]
    fn failure_and_cancel_also_restore_visibility() {
        for outcome in [
            OverlayOutcome::Failed("GameNotForeground".into()),
            OverlayOutcome::Cancelled,
        ] {
            let mut state = CompactPresetState::default();
            state.show();
            state.snapshot_for_automation();
            state.begin_automation_hide();
            state.mark_running();
            state.finish(outcome.clone(), "done");
            assert_eq!(
                state.phase(),
                OverlayPhase::Visible,
                "失败/取消之后同样必须恢复浮窗"
            );
            // 恢复之后浮窗热键仍然正常
            assert!(!state.toggle());
            assert_eq!(state.phase(), OverlayPhase::Hidden);
        }
    }

    #[test]
    fn repeated_automation_requests_keep_the_original_snapshot() {
        // 已经在跑时又收到一次请求：快照不能被覆盖成「隐藏」
        let mut state = CompactPresetState::default();
        state.show();
        state.begin_automation_hide();
        assert_eq!(state.phase(), OverlayPhase::Hidden);
        state.begin_automation_hide();
        state.finish(OverlayOutcome::Success, "ok");
        assert_eq!(state.phase(), OverlayPhase::Visible);
    }

    #[test]
    fn status_line_reports_running_and_results() {
        let mut state = CompactPresetState::default();
        state.mark_running();
        assert!(state.status_line().contains("Running"));
        state.finish(
            OverlayOutcome::Failed("StratagemNotFound".into()),
            "AUTO LOADOUT · Failed: StratagemNotFound",
        );
        let line = state.status_line();
        assert!(
            line.contains("Failed") && line.contains("StratagemNotFound"),
            "{line}"
        );
    }

    #[test]
    fn lifecycle_follows_spec_chain() {
        let mut state = CompactPresetState::default();
        assert_eq!(state.phase(), OverlayPhase::Hidden);
        assert!(!state.overlay_visible());

        // 浮窗热键 → Visible
        assert!(state.toggle());
        assert_eq!(state.phase(), OverlayPhase::Visible);
        assert!(state.overlay_visible());

        // 右键编辑 → Editing
        assert!(state.open_editor(5, None));
        assert_eq!(state.phase(), OverlayPhase::Editing);
        assert_eq!(state.editor.as_ref().map(|e| e.slot), Some(5));

        // 交出编辑目标后回到 Visible（写盘 / 写草稿由调用方决定）
        let editor = state.take_editor().expect("编辑器状态");
        assert_eq!(editor.slot, 5);
        assert_eq!(state.phase(), OverlayPhase::Visible);

        // 取消选择：保持原配置不变
        state.draft.set(0, Some(base_entry("飞鹰空袭")));
        state.open_editor(5, None);
        let before = state.draft.clone();
        state.cancel_editor();
        assert_eq!(state.draft, before);

        // 自动装配：临时隐藏 → 结束后按快照恢复可见
        state.snapshot_for_automation();
        state.begin_automation_hide();
        assert_eq!(state.phase(), OverlayPhase::Hidden);
        assert!(!state.overlay_visible());
        state.mark_running();
        assert_eq!(state.automation, AutomationPhase::Running);
        state.finish(OverlayOutcome::Success, "装配完成");
        assert_eq!(state.phase(), OverlayPhase::Visible, "结束后恢复原状态");

        // 失败后仍可再次唤出浮窗（生命周期不被破坏）
        assert!(!state.toggle());
        assert!(state.toggle());
        assert_eq!(state.phase(), OverlayPhase::Visible);
    }

    #[test]
    fn loadout_slots_are_06_to_10_only() {
        // 浮窗显示 10 个槽位，但只有下排 5 个参与自动装配
        assert!(!is_loadout_slot(0));
        assert!(!is_loadout_slot(4));
        for slot in FIRST_H2AC_SLOT..FIRST_H2AC_SLOT + PRESET_SLOTS {
            assert!(is_loadout_slot(slot), "槽位 {slot} 应属于 LOADOUT");
            assert_eq!(draft_index_of(slot), Some(slot - FIRST_H2AC_SLOT));
        }
        assert!(!is_loadout_slot(FIRST_H2AC_SLOT + PRESET_SLOTS));
        assert_eq!(draft_index_of(0), None);
        assert_eq!(FIRST_H2AC_SLOT + BOOSTER_INDEX, 9);
    }

    #[test]
    fn running_overlay_is_view_only() {
        // §14：运行中允许显示查看，但禁止编辑
        let mut state = CompactPresetState::default();
        state.mark_running();
        state.show();
        assert_eq!(state.phase(), OverlayPhase::Visible);
        assert!(!state.editing_allowed());
        assert!(!state.open_editor(5, None), "运行中不得打开选择器");
        assert!(state.editor.is_none());
    }

    #[test]
    fn hide_does_not_touch_the_draft() {
        // 隐藏浮窗只是 UI 动作：草稿与结果都不受影响
        let mut state = CompactPresetState::default();
        state.show();
        state.draft = full_draft();
        let draft = state.draft.clone();
        state.hide();
        assert_eq!(state.draft, draft);
        assert_eq!(state.phase(), OverlayPhase::Hidden);
    }

    #[test]
    fn snapshot_is_frozen_for_persistence() {
        let mut state = CompactPresetState::default();
        state.show();
        state.draft = full_draft();
        let snapshot = state.snapshot_for_automation();
        assert_eq!(snapshot, full_draft());
        assert_eq!(state.last_requested.as_ref(), Some(&full_draft()));
        // 请求之后再改草稿也不影响已提交的快照（失败不覆盖原预设）
        state.draft.set(0, None);
        assert_eq!(state.last_requested.as_ref().unwrap(), &full_draft());
    }
}
