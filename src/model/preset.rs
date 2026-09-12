// 紧凑模式 Overlay 的 GUI 侧逻辑（AppModel 只持有状态；窗口与自动化在别处）
//
// 关键行为（对应实施要求 §二 / §五 / §六 / §七 / §九 / §十）：
//   * 浮窗热键只切换显示，绝不触发装配；
//   * 自动装配热键先做本地校验（预设完整性 / 是否已有任务），失败时把错误显示在浮窗上且
//     不隐藏浮窗、不产生任何鼠标动作；
//   * 校验通过 → 记录快照 → 「立即」隐藏浮窗（alpha=0 + 点击穿透）→ 工作线程执行；
//   * 只有最终验证成功，才把快照写回 Slot06~10 + config + 当前 Profile；
//   * 失败/取消不写盘，原预设保持有效。
use eframe::egui::Context;

use crate::compact_mode::config::OVERLAY_DESIGN_W;
use crate::compact_mode::preset::{
    OverlayOutcome, PresetDraft, PresetEntry, BOOSTER_INDEX, FIRST_H2AC_SLOT,
};
use crate::loadout_sync::state::SyncStatus;
use crate::state::ViewMode;
use crate::H2ACApp;
use crate::LogKind;

/// 透明度档位（标题栏 ◐ 按钮循环切换）
pub const OPACITY_STEPS: [f32; 4] = [1.0, 0.92, 0.8, 0.65];

impl H2ACApp {
    // ─── 显示 / 隐藏（只有浮窗热键能改变可见性） ───

    /// 浮窗热键：显示 ↔ 隐藏。任何自动化状态下都可用（运行中打开为只读）。
    pub fn toggle_compact_overlay(&mut self, ctx: &Context) {
        if !self.model.config.compact_mode.enabled {
            self.log(LogKind::Info, "[Compact] 紧凑模式已在配置中关闭");
            return;
        }
        if self.model.compact_preset.toggle() {
            self.show_compact_overlay(ctx);
        } else {
            self.save_overlay_position();
            self.hide_compact_overlay(ctx);
            let hotkey = self.model.config.compact_mode.toggle_hotkey.to_uppercase();
            self.log(
                LogKind::Info,
                format!("[Compact] 浮窗已隐藏（{hotkey} 再次唤出）"),
            );
        }
    }

    /// 显示浮窗：切换到紧凑视图、按当前 Slot06~10 重新播种草稿、贴到游戏窗口附近、置顶。
    pub fn show_compact_overlay(&mut self, ctx: &Context) {
        let draft = PresetDraft::from_slots(&self.model.slots, &self.model.plugin_slots);
        self.model.compact_preset.reseed(draft);
        self.model.view_mode = ViewMode::Compact;
        self.apply_view_mode(ctx);
        self.place_overlay_window(ctx);
        self.set_overlay_window_hidden(false);
        if self.model.loadout_sync.is_running() {
            self.log(
                LogKind::Info,
                "[Compact] 浮窗已显示（自动装配进行中，只读）",
            );
        } else {
            let hotkey = self.model.config.loadout_sync_hotkey.to_uppercase();
            self.log(
                LogKind::Info,
                format!("[Compact] 浮窗已显示（自动装配 {hotkey}）"),
            );
        }
    }

    /// 隐藏浮窗：alpha=0 + 点击穿透（游戏画面无遮挡，进程与后台任务继续运行）。
    pub fn hide_compact_overlay(&mut self, ctx: &Context) {
        let _ = ctx;
        self.model.view_mode = ViewMode::Compact;
        self.set_overlay_window_hidden(true);
    }

    /// 窗口层隐藏开关：只改自己窗口的不透明度与点击穿透，不最小化、不抢焦点、不销毁窗口。
    pub fn set_overlay_window_hidden(&mut self, hidden: bool) {
        let Some(hwnd) = crate::overlay_win::own_window() else {
            self.overlay_window_ready = false;
            return;
        };
        let opacity = self.model.config.compact_mode.opacity;
        let target_alpha = if hidden { 0.0 } else { opacity };
        let ok = crate::overlay_win::apply_alpha(hwnd, target_alpha)
            && crate::overlay_win::set_click_through(hwnd, hidden);
        if !hidden {
            // 真正意义上的 Windows Topmost（HWND_TOPMOST），不受激活状态影响
            let top_ok = crate::overlay_win::set_topmost(hwnd, true);
            if !top_ok {
                self.log(
                    LogKind::Warn,
                    "[Compact] 无法把浮窗设为置顶（可能被独占全屏游戏覆盖）",
                );
            }
        }
        if !ok && self.overlay_window_ready {
            // 分层窗口不可用时退化为真正的窗口隐藏（仍不最小化、不退出）
            self.log(
                LogKind::Warn,
                "[Compact] 分层窗口不可用，改用窗口隐藏（热键可能需点击任务栏恢复）",
            );
        }
        self.overlay_window_ready = ok;
    }

    /// 每帧校准 Overlay 窗口状态（透明度 / 点击穿透 / 置顶）。
    ///
    /// egui 的窗口指令（尺寸、层级）是延迟生效的；这里做「读回 → 不一致才写」的廉价校准，
    /// 顺带保证自动化结束后浮窗重新显示时仍然是 Topmost。
    pub fn ensure_overlay_window_state(&mut self) {
        if !self.model.view_mode.is_compact() {
            return;
        }
        let Some(hwnd) = crate::overlay_win::own_window() else {
            return;
        };
        let visible = self.model.compact_preset.overlay_visible();
        let target = if visible {
            (self.model.config.compact_mode.opacity * 255.0).round() as u8
        } else {
            0
        };
        let alpha_ok = crate::overlay_win::current_alpha(hwnd)
            .map(|current| (current as i32 - target as i32).abs() <= 1)
            .unwrap_or(false);
        let click_ok = crate::overlay_win::is_click_through(hwnd) != visible;
        let top_ok = !visible || crate::overlay_win::is_topmost(hwnd);
        if alpha_ok && click_ok && top_ok {
            return;
        }
        let ok = crate::overlay_win::apply_alpha(hwnd, target as f32 / 255.0)
            && crate::overlay_win::set_click_through(hwnd, !visible);
        if visible {
            crate::overlay_win::set_topmost(hwnd, true);
        }
        self.overlay_window_ready = ok;
    }

    /// 首次显示时贴到游戏窗口右上角；配置里指定过位置则用配置值。
    pub fn place_overlay_window(&mut self, ctx: &Context) {
        let cfg = self.model.config.compact_mode.clone();
        if cfg.has_position() {
            if let Some(hwnd) = crate::overlay_win::own_window() {
                crate::overlay_win::move_to(hwnd, cfg.position_x, cfg.position_y);
            }
            return;
        }
        let Ok(game) = crate::loadout_sync::window::find_game_window() else {
            return;
        };
        let size = ctx.screen_rect().size();
        let margin = 24.0;
        let w = size.x.max(OVERLAY_DESIGN_W);
        let x = (game.client.x as f32 + game.client.w as f32 - w - margin).max(0.0);
        let y = (game.client.y as f32 + margin).max(0.0);
        let dpi_scale = if game.dpi == 0 {
            1.0
        } else {
            game.dpi as f32 / 96.0
        };
        if let Some(hwnd) = crate::overlay_win::own_window() {
            crate::overlay_win::move_to(hwnd, x as i32, (y * dpi_scale) as i32);
        }
    }

    /// 记录浮窗当前位置（拖动结束 / 隐藏时调用），下次唤出保持同一位置。
    pub fn save_overlay_position(&mut self) {
        let Some(hwnd) = crate::overlay_win::own_window() else {
            return;
        };
        let Some((x, y, _, _)) = crate::overlay_win::window_rect(hwnd) else {
            return;
        };
        if self.model.config.compact_mode.position_x == x
            && self.model.config.compact_mode.position_y == y
        {
            return;
        }
        self.model.config.compact_mode.position_x = x;
        self.model.config.compact_mode.position_y = y;
        crate::config::save_config(&self.model.config);
    }

    /// 透明度档位切换（写入 config，立即生效）。
    pub fn cycle_overlay_opacity(&mut self, ctx: &Context) {
        let _ = ctx;
        let current = self.model.config.compact_mode.opacity;
        let next = OPACITY_STEPS
            .iter()
            .copied()
            .find(|v| *v < current - 0.01)
            .unwrap_or(OPACITY_STEPS[0]);
        self.model.config.compact_mode.opacity = next;
        crate::config::save_config(&self.model.config);
        if self.model.compact_preset.overlay_visible() {
            self.set_overlay_window_hidden(false);
        }
        self.log(
            LogKind::Info,
            format!("[Compact] 浮窗不透明度 {:.0}%", next * 100.0),
        );
    }

    // ─── 槽位编辑 ───
    //
    // 上排 Slot01~05（TASK）与下排 Slot06~10（LOADOUT）在浮窗里都能右键编辑，但写盘时机不同：
    //   * 上排 = 普通槽位，沿用既有行为，确认后立即写 config.json；
    //   * 下排 = 预设草稿，只在自动装配成功后才写回（失败不覆盖原预设）。

    /// 当前正在编辑的 H2AC 槽位下标
    pub fn compact_editor_slot(&self) -> Option<usize> {
        self.model.compact_preset.editor.as_ref().map(|e| e.slot)
    }

    /// 右键槽位 → 打开选择器；Booster 槽（Slot10）默认只列 Booster 分类。
    /// 自动装配进行中浮窗为只读（可以查看进度，但不能改预设）。
    pub fn open_compact_selector(&mut self, slot: usize) {
        if !self.model.compact_preset.overlay_visible() {
            return;
        }
        if !self.model.compact_preset.editing_allowed() {
            self.log(
                LogKind::Info,
                "[Compact] 自动装配进行中：浮窗只读，结束后再编辑",
            );
            return;
        }
        let default_category = if slot == FIRST_H2AC_SLOT + BOOSTER_INDEX {
            Some(crate::stratagems::CAT_BOOSTERS.to_string())
        } else {
            None
        };
        self.model
            .compact_preset
            .open_editor(slot, default_category);
    }

    /// 选择器确认：上排槽位立即写盘，下排写入草稿（内存）。
    pub fn apply_compact_choice(&mut self, entry: Option<PresetEntry>) {
        let name = entry
            .as_ref()
            .map(|e| e.name().to_string())
            .unwrap_or_else(|| "（清空）".to_string());
        let Some(editor) = self.model.compact_preset.take_editor() else {
            return;
        };
        if let Some(index) = crate::compact_mode::preset::draft_index_of(editor.slot) {
            self.model.compact_preset.draft.set(index, entry);
            self.log(
                LogKind::Info,
                format!(
                    "[Compact] 预设槽 {} ← {}（装配成功后保存）",
                    editor.slot + 1,
                    name
                ),
            );
        } else {
            self.apply_slot_entry(editor.slot, entry);
            self.log(
                LogKind::Info,
                format!("[Compact] 槽位 {} ← {}", editor.slot + 1, name),
            );
        }
    }

    /// 把选择结果写入某个 H2AC 槽位（上排 TASK 槽用；沿用既有槽位写入语义并立即持久化）。
    pub fn apply_slot_entry(&mut self, slot: usize, entry: Option<PresetEntry>) {
        if slot >= crate::config::SLOT_COUNT {
            return;
        }
        match entry {
            Some(entry) => match &entry.payload {
                crate::compact_mode::preset::PresetPayload::Base(idx) => {
                    self.set_slot(slot, *idx);
                }
                crate::compact_mode::preset::PresetPayload::Plugin(p) => {
                    self.model.plugin_slots.insert(slot, (**p).clone());
                    self.model.slots[slot] = Some(crate::stratagems::PLUGIN_SLOT_MARK);
                    self.model.config.loadout[slot] = Some(crate::stratagems::PLUGIN_SLOT_MARK);
                    crate::config::save_config(&self.model.config);
                }
            },
            // 清除：与主界面「清除」一致（同时解除该槽位热键）
            None => self.clear_slot(slot),
        }
        self.sync_hotkey_map();
    }

    pub fn cancel_compact_choice(&mut self) {
        self.model.compact_preset.cancel_editor();
    }

    // ─── 自动装配（唯一入口：热键 / 主界面按钮 / 浮窗按钮都走这里） ───

    /// Auto Loadout：**与模式、与浮窗可见性完全无关**（§1.B / §9 / §15）。
    ///
    /// 行为：校验当前预设 → 若浮窗正在显示则临时隐藏（记录开始前可见性）→ 启动 Loadout Sync 工作线程。
    /// 结束后（成功 / 失败 / 取消 / 异常）由 poll_compact_preset 按快照恢复浮窗。
    /// 正常模式下不触碰窗口（只有「浮窗正在屏幕上显示」时才做临时隐藏）。
    pub fn auto_loadout(&mut self, ctx: &Context) {
        let _ = ctx;
        if self.model.loadout_sync.is_running() {
            self.log(
                LogKind::Info,
                "[AutoLoadout] 已有自动装配在执行（按取消热键可停止）",
            );
            return;
        }

        // 当前预设 = 紧凑浮窗中的草稿（未编辑时即 Slot06~10 的内容）
        let draft = self.current_preset_draft();
        if let Err(e) = draft.validate() {
            let message = e.message();
            self.model
                .compact_preset
                .finish(OverlayOutcome::Failed(message.clone()), message.clone());
            // 只有浮窗本来就可见时才顺手把错误显示出来；绝不为了报错而改变可见性
            if self.model.compact_preset.overlay_visible() {
                self.model.compact_preset.show();
                self.model.compact_preset.status = message.clone();
            }
            self.log(LogKind::Warn, format!("[Compact] {message}"));
            return;
        }

        // 记录快照 + 登记运行中；只有当浮窗此刻真的显示在屏幕上时才临时隐藏它。
        // 正常模式下自动装配**完全不碰窗口**（不切视图、不改透明度）。
        let overlay_displayed = crate::compact_mode::preset::needs_temporary_hide(
            self.model.view_mode.is_compact(),
            self.model.compact_preset.phase(),
        );
        let snapshot = self.model.compact_preset.begin_automation();
        if overlay_displayed {
            self.set_overlay_window_hidden(true);
            self.log(
                LogKind::Info,
                format!(
                    "[AutoLoadout] 自动装配开始（{}）— 浮窗临时隐藏，结束后恢复",
                    snapshot.summary()
                ),
            );
        } else {
            self.log(
                LogKind::Info,
                format!("[AutoLoadout] 自动装配开始（{}）", snapshot.summary()),
            );
        }
        let selection = snapshot.to_selection();
        if !self.start_loadout_sync_with(selection) {
            // 未能启动（例如已有任务在运行 / 参数校验失败）：立即按快照恢复浮窗，
            // 不让它停留在「临时隐藏」。
            self.model
                .compact_preset
                .finish(OverlayOutcome::None, String::new());
            self.ensure_overlay_window_state();
        }
    }

    /// 当前预设草稿：浮窗编辑过就用草稿，否则从 Slot06~10 现取（保证正常模式亦可用）。
    pub fn current_preset_draft(&mut self) -> PresetDraft {
        if self.model.compact_preset.draft.stratagem_count() > 0
            || self.model.compact_preset.draft.entries[BOOSTER_INDEX].is_some()
        {
            return self.model.compact_preset.draft.clone();
        }
        let draft = PresetDraft::from_slots(&self.model.slots, &self.model.plugin_slots);
        self.model.compact_preset.reseed(draft.clone());
        draft
    }

    /// 取消热键 / 按钮：停止后续自动化并释放输入；浮窗可见性的恢复统一在 poll 里按快照执行。
    pub fn cancel_auto_loadout(&mut self) {
        if self.model.loadout_sync.is_running() {
            self.model.loadout_sync.request_cancel();
            self.log(LogKind::Warn, "[AutoLoadout] 已请求取消自动装配…");
        } else {
            self.log(LogKind::Info, "[AutoLoadout] 当前没有正在执行的自动装配");
        }
    }

    /// 每帧同步 Loadout Sync 状态到浮窗（并按要求恢复可见性 / 成功时持久化预设）。
    pub fn poll_compact_preset(&mut self) {
        let snapshot = self.model.loadout_sync.snapshot();
        if snapshot.is_running() {
            if !self.model.compact_preset.automation.is_running() {
                self.model.compact_preset.mark_running();
            }
            return;
        }
        if !self.model.compact_preset.automation.is_running() {
            return;
        }
        let draft = self
            .model
            .compact_preset
            .last_requested
            .clone()
            .unwrap_or_else(|| self.model.compact_preset.draft.clone());
        // 先判定「本次结果是否应该写盘」（下面的 match 会把 message 移出去）
        let persist = Self::should_persist_preset(&snapshot.status);
        match snapshot.status {
            SyncStatus::Succeeded => {
                self.model
                    .compact_preset
                    .finish(OverlayOutcome::Success, "AUTO LOADOUT · Completed");
            }
            SyncStatus::Failed { message, .. } => {
                self.model.compact_preset.finish(
                    OverlayOutcome::Failed(message.clone()),
                    format!("AUTO LOADOUT · Failed: {message}"),
                );
                self.log(
                    LogKind::Warn,
                    "[Compact] 自动装配失败，原预设保持不变（未写盘）",
                );
            }
            SyncStatus::Cancelled => {
                self.model.compact_preset.finish(
                    OverlayOutcome::Cancelled,
                    "AUTO LOADOUT · Cancelled（预设未保存）",
                );
            }
            // 上面已经 return 过 Running 分支；这里只是让匹配保持穷尽
            SyncStatus::Running => return,
            SyncStatus::Idle => {
                // 防御：已经标记为运行中，但工作线程却回到了 Idle（内部异常）。
                // 按失败处理并恢复浮窗，绝不把浮窗留在临时隐藏状态。
                self.model.compact_preset.finish(
                    OverlayOutcome::Failed("LoadoutSyncNotStarted".into()),
                    "AUTO LOADOUT · 未启动（内部状态异常）",
                );
                self.log(
                    LogKind::Warn,
                    "[Compact] 自动装配未启动：工作线程未进入 Running（内部状态异常）",
                );
            }
        }
        // 只有最终校验成功才写盘：失败 / 取消一律不覆盖原预设
        if persist {
            self.persist_compact_preset(&draft);
        }
        // 恢复可见性后同步窗口状态（隐藏 → 可见 时需要重新置顶）。
        // 只在浮窗视图里动窗口：正常模式的主窗口不受自动装配影响。
        if self.model.view_mode.is_compact() {
            if self.model.compact_preset.overlay_visible() {
                self.ensure_overlay_window_state();
            } else {
                self.set_overlay_window_hidden(true);
            }
        }
    }
    /// 是否应该把预设写盘：只有「来自 Overlay 草稿」且「最终验证成功」才写。
    /// （失败 / 取消一律不写，避免覆盖原来有效的预设 —— §七 / §八）
    /// 是否应该把预设写盘：**只有最终校验成功才写**
    /// （失败 / 取消一律不写，避免覆盖原来有效的预设 —— §七 / §八）。
    pub fn should_persist_preset(status: &SyncStatus) -> bool {
        matches!(status, SyncStatus::Succeeded)
    }

    /// 成功后才调用：把草稿写回 Slot06~10 + config + 当前 Profile（§七）。
    pub fn persist_compact_preset(&mut self, draft: &PresetDraft) {
        draft.apply_to_slots(&mut self.model.slots, &mut self.model.plugin_slots);
        for offset in 0..crate::compact_mode::preset::PRESET_SLOTS {
            let index = FIRST_H2AC_SLOT + offset;
            if index < self.model.config.loadout.len() {
                self.model.config.loadout[index] = self.model.slots[index];
            }
        }
        crate::config::save_config(&self.model.config);

        if !self.model.current_profile.is_empty() {
            let name = self.model.current_profile.clone();
            let ps: std::collections::HashMap<String, _> = self
                .model
                .plugin_slots
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect();
            crate::config::save_profile(
                &name,
                &self.model.slots,
                &self.model.config.slot_hotkeys,
                &ps,
            );
            self.log(
                LogKind::Info,
                format!("[Compact] 预设已保存到 Profile: {name}"),
            );
        } else {
            self.log(
                LogKind::Info,
                "[Compact] 预设已写入 Slot06~10（当前未选择 Profile）",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_is_persisted_only_after_a_successful_run() {
        // 成功 → 写盘
        assert!(H2ACApp::should_persist_preset(&SyncStatus::Succeeded));
        // 失败 / 取消 / 未运行 → 绝不写盘（原预设保持有效）
        assert!(!H2ACApp::should_persist_preset(&SyncStatus::Failed {
            code: "LoadoutHomeNotDetected".into(),
            message: "未识别到 Loadout 主界面".into(),
        }));
        assert!(!H2ACApp::should_persist_preset(&SyncStatus::Cancelled));
        assert!(!H2ACApp::should_persist_preset(&SyncStatus::Idle));
        assert!(!H2ACApp::should_persist_preset(&SyncStatus::Running));
    }

    #[test]
    fn opacity_steps_cycle_from_opaque_to_translucent() {
        // 连续点「透明」必须能回到 100%，不会卡在某个档位
        let mut current = OPACITY_STEPS[0];
        let mut seen = vec![current];
        for _ in 0..OPACITY_STEPS.len() {
            current = OPACITY_STEPS
                .iter()
                .copied()
                .find(|v| *v < current - 0.01)
                .unwrap_or(OPACITY_STEPS[0]);
            seen.push(current);
        }
        assert_eq!(current, OPACITY_STEPS[0]);
        assert!(seen.iter().any(|v| *v < 0.7), "必须存在更透明的档位");
    }
}
