use std::thread;

use crate::config::{save_config, SLOT_COUNT};
use crate::executor;
use crate::H2ACApp;
use crate::LogKind;
use crate::stratagems::{self, command_to_string, dir_to_arrow, StratagemRef, STRATAGEMS};

impl H2ACApp {
    pub fn execute_slot(&mut self, slot: usize) {
        if let Some(p) = self.model.plugin_slots.get(&slot) {
            let log_msg = format!("执行 {} [{}] {}", p.name, p.model, p.command.join(""));
            let cmd = p.command.clone();
            self.log(LogKind::Exec, log_msg);
            self.model.flash.insert(slot, 0.0);
            let cg = self.model.config.clone();
            thread::spawn(move || executor::execute_plugin(&cg, &cmd));
            return;
        }
        if let Some(idx) = self.model.slots[slot] {
            if idx != usize::MAX {
                if let Some(s) = STRATAGEMS.get(idx) {
                    self.log(LogKind::Exec, format!("执行 {} [{}] {}", s.name, s.model, command_to_string(&s.command)));
                    self.model.flash.insert(slot, 0.0);
                    let sc = s.clone();
                    let cg = self.model.config.clone();
                    thread::spawn(move || executor::execute_stratagem(&sc, &cg));
                }
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
            self.model.armed = (0..SLOT_COUNT)
                .map(|i| (slot + 1 + i) % SLOT_COUNT)
                .find(|&i| self.model.slots[i].is_none());
            if let Some(a) = self.model.armed {
                self.model.detail_slot = Some(a);
            } else {
                self.model.detail_slot = Some(slot);
            }
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
                self.model.slots[slot] = Some(usize::MAX);
                self.log(LogKind::Info, format!("槽位 {} <- {} (插件)", slot + 1, p.name));
                self.model.armed = (0..SLOT_COUNT)
                    .map(|i| (slot + 1 + i) % SLOT_COUNT)
                    .find(|&i| self.model.slots[i].is_none() && !self.model.plugin_slots.contains_key(&i));
                if let Some(a) = self.model.armed { self.model.detail_slot = Some(a); }
                else { self.model.detail_slot = Some(slot); }
            }
        }
    }

    pub fn set_slot(&mut self, slot: usize, idx: usize) {
        self.model.slots[slot] = Some(idx);
        self.model.config.loadout[slot] = Some(idx);
        save_config(&self.model.config);
    }

    pub fn clear_slot(&mut self, slot: usize) {
        self.model.slots[slot] = None;
        self.model.plugin_slots.remove(&slot);
        self.model.config.loadout[slot] = None;
        self.model.config.slot_hotkeys.remove(&slot.to_string());
        save_config(&self.model.config);
    }

    pub fn slot_filled(&self, idx: usize) -> bool {
        self.model.slots[idx].is_some() || self.model.plugin_slots.contains_key(&idx)
    }

    pub fn slot_name(&self, idx: usize) -> Option<String> {
        if let Some(p) = self.model.plugin_slots.get(&idx) { Some(p.name.clone()) }
        else { self.model.slots[idx].and_then(|si| if si == usize::MAX { None } else { STRATAGEMS.get(si).map(|s| s.name.to_string()) }) }
    }

    pub fn slot_icon(&self, idx: usize) -> Option<&str> {
        if let Some(p) = self.model.plugin_slots.get(&idx) { Some(&p.icon) }
        else { self.model.slots[idx].and_then(|si| if si == usize::MAX { None } else { STRATAGEMS.get(si).map(|s| s.icon) }) }
    }

    pub fn slot_command(&self, idx: usize) -> Vec<&str> {
        if let Some(p) = self.model.plugin_slots.get(&idx) { p.command.iter().map(|c| dir_to_arrow(c.as_str())).collect() }
        else { self.model.slots[idx].and_then(|si| if si == usize::MAX { None } else { STRATAGEMS.get(si).map(|s| s.command.to_vec()) }).unwrap_or_default() }
    }

    pub fn slot_category(&self, idx: usize) -> Option<String> {
        if let Some(p) = self.model.plugin_slots.get(&idx) { Some(p.category.clone()) }
        else { self.model.slots[idx].and_then(|si| if si == usize::MAX { None } else { STRATAGEMS.get(si).map(|s| s.category.to_string()) }) }
    }

}
