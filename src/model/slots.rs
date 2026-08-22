use std::collections::HashMap;
use std::thread;

use crate::config::{save_config, SLOT_COUNT};
use crate::executor;
use crate::H2ACApp;
use crate::LogKind;
use crate::stratagems::{self, command_to_string, dir_to_arrow, PluginStratagem, StratagemRef, STRATAGEMS};

/// 从 slot 的下一位起循环查找下一个空槽位（内置列表为空且非插件槽）
fn next_free_slot(
    slots: &[Option<usize>],
    plugin_slots: &HashMap<usize, PluginStratagem>,
    slot: usize,
) -> Option<usize> {
    (0..SLOT_COUNT)
        .map(|i| (slot + 1 + i) % SLOT_COUNT)
        .find(|&i| slots[i].is_none() && !plugin_slots.contains_key(&i))
}

/// 写入内置战备到槽位：同时清除同槽插件条目（单点保证「一槽一内容」不变量）。
/// 修复：此前用内置战备替换插件/NEW 战备时 plugin_slots 残留，导致槽位继续
/// 显示和执行旧插件战备（所有读取路径均优先查 plugin_slots）。
fn write_base_slot(
    slots: &mut [Option<usize>],
    plugin_slots: &mut HashMap<usize, PluginStratagem>,
    loadout: &mut [Option<usize>],
    slot: usize,
    idx: usize,
) {
    if slot >= SLOT_COUNT { return; }
    slots[slot] = Some(idx);
    plugin_slots.remove(&slot);
    loadout[slot] = Some(idx);
}

impl H2ACApp {
    /// 装填后：武装态推进到下一个空槽（无空槽则以当前槽为详情槽）
    fn advance_armed(&mut self, slot: usize) {
        self.model.armed = next_free_slot(&self.model.slots, &self.model.plugin_slots, slot);
        self.model.detail_slot = Some(self.model.armed.unwrap_or(slot));
    }
    pub fn execute_slot(&mut self, slot: usize) {
        if let Some(p) = self.model.plugin_slots.get(&slot) {
            let name = p.name.clone();
            let log_msg = format!("执行 {} [{}] {}", p.name, p.model, p.command.join(""));
            let cmd = p.command.clone();
            self.log(LogKind::Exec, log_msg);
            self.model.flash.insert(slot, 0.0);
            let cg = self.model.config.clone();
            thread::spawn(move || {
                if let Err(e) = executor::execute_plugin(&cg, &cmd) {
                    crate::util::log_to_file("exec_error.log", &format!("执行 {name} 失败: {e}"));
                }
            });
            return;
        }
        if let Some(Some(idx)) = self.model.slots.get(slot) {
            if let Some(s) = STRATAGEMS.get(*idx) {
                let name = s.name;
                self.log(LogKind::Exec, format!("执行 {} [{}] {}", s.name, s.model, command_to_string(s.command)));
                self.model.flash.insert(slot, 0.0);
                let sc = s.clone();
                let cg = self.model.config.clone();
                thread::spawn(move || {
                    if let Err(e) = executor::execute_stratagem(&sc, &cg) {
                        crate::util::log_to_file("exec_error.log", &format!("执行 {name} 失败: {e}"));
                    }
                });
            }
        }
    }

    pub fn assign_stratagem(&mut self, s: &'static stratagems::Stratagem) {
        let Some(slot) = self.model.armed else {
            self.log(LogKind::Info, format!("先点选一个槽位，再装入 {}", s.name));
            return;
        };
        if let Some(idx) = STRATAGEMS.iter().position(|x| x.name == s.name && x.model == s.model) {
            self.set_slot(slot, idx);
            self.log(LogKind::Info, format!("槽位 {} ← {} [{}]", slot + 1, s.name, s.model));
            self.advance_armed(slot);
        }
    }

    pub fn assign_stratagem_ref(&mut self, s: &StratagemRef) {
        match s {
            StratagemRef::Base(base) => self.assign_stratagem(base),
            StratagemRef::Plugin(p) => {
                let Some(slot) = self.model.armed else {
                    self.log(LogKind::Info, format!("先点选槽位再装入: {}", p.name));
                    return;
                };
                self.model.plugin_slots.insert(slot, (*p).clone());
                self.model.slots[slot] = Some(crate::stratagems::PLUGIN_SLOT_MARK);
                self.log(LogKind::Info, format!("槽位 {} <- {} (插件)", slot + 1, p.name));
                self.advance_armed(slot);
            }
        }
    }

    pub fn set_slot(&mut self, slot: usize, idx: usize) {
        write_base_slot(
            &mut self.model.slots,
            &mut self.model.plugin_slots,
            &mut self.model.config.loadout,
            slot,
            idx,
        );
        save_config(&self.model.config);
    }

    pub fn clear_slot(&mut self, slot: usize) {
        if slot >= SLOT_COUNT { return; }
        self.model.slots[slot] = None;
        self.model.plugin_slots.remove(&slot);
        self.model.config.loadout[slot] = None;
        self.model.config.slot_hotkeys.remove(&slot.to_string());
        // 状态一致性：被清除的槽位不能仍是武装/详情槽位
        if self.model.armed == Some(slot) { self.model.armed = None; }
        if self.model.detail_slot == Some(slot) { self.model.detail_slot = None; }
        save_config(&self.model.config);
        self.sync_hotkey_map();
    }

    /// 清空全部槽位（单次写盘，替代逐槽 clear_slot 的 10 次保存）
    pub fn clear_all_slots(&mut self) {
        for slot in 0..SLOT_COUNT {
            self.model.slots[slot] = None;
            self.model.plugin_slots.remove(&slot);
            self.model.config.loadout[slot] = None;
            self.model.config.slot_hotkeys.remove(&slot.to_string());
        }
        self.model.armed = None;
        self.model.detail_slot = None;
        save_config(&self.model.config);
        self.sync_hotkey_map();
    }

    pub fn slot_filled(&self, idx: usize) -> bool {
        self.model.slots.get(idx).is_some_and(|s| s.is_some())
            || self.model.plugin_slots.contains_key(&idx)
    }

    pub fn slot_name(&self, idx: usize) -> Option<&str> {
        if let Some(p) = self.model.plugin_slots.get(&idx) { return Some(&p.name); }
        self.base_slot(idx).map(|s| s.name)
    }

    pub fn slot_icon(&self, idx: usize) -> Option<&str> {
        if let Some(p) = self.model.plugin_slots.get(&idx) { return Some(&p.icon); }
        self.base_slot(idx).map(|s| s.icon)
    }

    pub fn slot_command(&self, idx: usize) -> Vec<&str> {
        if let Some(p) = self.model.plugin_slots.get(&idx) {
            return p.command.iter().map(|c| dir_to_arrow(c.as_str())).collect();
        }
        self.base_slot(idx).map(|s| s.command.to_vec()).unwrap_or_default()
    }

    pub fn slot_category(&self, idx: usize) -> Option<&str> {
        if let Some(p) = self.model.plugin_slots.get(&idx) { return Some(&p.category); }
        self.base_slot(idx).map(|s| s.category)
    }

    /// 槽位内的内置战备（插件槽位或越界索引返回 None）
    fn base_slot(&self, idx: usize) -> Option<&'static crate::stratagems::Stratagem> {
        let si = self.model.slots.get(idx).copied().flatten()?;
        if si == crate::stratagems::PLUGIN_SLOT_MARK { return None; }
        STRATAGEMS.get(si)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(name: &str) -> PluginStratagem {
        PluginStratagem {
            name: name.into(),
            category: "Test".into(),
            model: String::new(),
            command: Vec::new(),
            description: String::new(),
            icon: String::new(),
        }
    }

    #[test]
    fn next_free_skips_filled_and_plugin_slots() {
        let mut slots = vec![None; SLOT_COUNT];
        slots[0] = Some(1);
        let mut plugin_slots = HashMap::new();
        plugin_slots.insert(2, plugin("p"));
        assert_eq!(next_free_slot(&slots, &plugin_slots, 0), Some(1));
        slots[1] = Some(2);
        assert_eq!(next_free_slot(&slots, &plugin_slots, 0), Some(3));
    }

    #[test]
    fn next_free_wraps_and_none_when_full() {
        let mut slots = vec![None; SLOT_COUNT];
        slots[9] = Some(1);
        assert_eq!(next_free_slot(&slots, &HashMap::new(), 9), Some(0));
        for s in slots.iter_mut() { *s = Some(1); }
        assert_eq!(next_free_slot(&slots, &HashMap::new(), 0), None);
    }

    #[test]
    fn write_base_slot_clears_stale_plugin_entry() {
        // 复现「NEW 战备无法被替换」：插件条目残留的槽位写入内置战备
        let mut slots = vec![None; SLOT_COUNT];
        let mut plugin_slots = HashMap::new();
        let mut loadout = vec![None; SLOT_COUNT];
        slots[3] = Some(crate::stratagems::PLUGIN_SLOT_MARK);
        loadout[3] = Some(crate::stratagems::PLUGIN_SLOT_MARK);
        plugin_slots.insert(3, plugin("stale"));

        write_base_slot(&mut slots, &mut plugin_slots, &mut loadout, 3, 7);

        assert_eq!(slots[3], Some(7));
        assert_eq!(loadout[3], Some(7));
        assert!(!plugin_slots.contains_key(&3), "同槽插件条目必须被清除");
    }

    #[test]
    fn write_base_slot_ignores_out_of_range() {
        let mut slots = vec![None; SLOT_COUNT];
        let mut plugin_slots = HashMap::new();
        let mut loadout = vec![None; SLOT_COUNT];
        write_base_slot(&mut slots, &mut plugin_slots, &mut loadout, SLOT_COUNT, 0);
        write_base_slot(&mut slots, &mut plugin_slots, &mut loadout, 99, 0);
        assert!(slots.iter().all(|s| s.is_none()));
        assert!(loadout.iter().all(|s| s.is_none()));
        assert!(plugin_slots.is_empty());
    }
}
