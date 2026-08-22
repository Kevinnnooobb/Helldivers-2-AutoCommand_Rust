use std::collections::HashMap;

use crate::config;
use crate::H2ACApp;
use crate::LogKind;
use crate::plugin;
use crate::stratagems::{PluginStratagem, PLUGIN_SLOT_MARK};

/// 只保留与 loadout 哨兵一致的插件槽位条目：
/// 键解析失败、越界、以及「loadout 已不是插件哨兵但 plugin_slots 仍有残留」的
/// 旧 bug 损坏数据一律丢弃（自愈）。
fn consistent_plugin_slots(
    loadout: &[Option<usize>],
    plugin_slots: &HashMap<String, PluginStratagem>,
) -> HashMap<usize, PluginStratagem> {
    plugin_slots
        .iter()
        .filter_map(|(k, v)| {
            let slot = k.parse::<usize>().ok()?;
            let is_plugin_slot = loadout.get(slot).copied().flatten() == Some(PLUGIN_SLOT_MARK);
            is_plugin_slot.then(|| (slot, v.clone()))
        })
        .collect()
}

impl H2ACApp {
    pub fn delete_plugin_stratagem(&mut self, name: &str) {
        self.plugins.stratagems.retain(|p| p.name != name);
        let name = name.to_string();
        let _ = plugin::rewrite_stratagems(&mut |strats| {
            let before = strats.len();
            strats.retain(|s| s.name != name);
            strats.len() != before
        });
        self.log(LogKind::Warn, format!("已删除: {name}"));
    }

    pub fn load_profile_data(&mut self, name: &str) {
        if let Some(pr) = config::load_profile(name) {
            self.model.slots = pr.loadout.clone();
            // 同步持久化 loadout，避免「加载后编辑单个槽位 → 其余槽位回退为旧值」的混合状态
            self.model.config.loadout = pr.loadout;
            // 插件条目必须与 loadout 哨兵一致（非法键/越界/旧 bug 残留一律丢弃）
            self.model.plugin_slots = consistent_plugin_slots(&self.model.slots, &pr.plugin_slots);
            self.model.config.slot_hotkeys = pr.slot_hotkeys;
            self.model.config.last_profile = name.to_string();
            config::save_config(&self.model.config);
            self.sync_hotkey_map();
            self.model.current_profile = name.to_string();
            self.model.armed = None;
            self.model.detail_slot = None;
            self.log(LogKind::Info, format!("已加载 Profile: {name}"));
        }
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
    fn consistent_slots_keep_valid_and_drop_stale() {
        let mut loadout = vec![None; crate::config::SLOT_COUNT];
        loadout[1] = Some(PLUGIN_SLOT_MARK); // 合法插件槽
        loadout[2] = Some(5);                // 内置战备索引（旧 bug：plugin_slots 残留）
        let mut raw = HashMap::new();
        raw.insert("1".to_string(), plugin("valid"));
        raw.insert("2".to_string(), plugin("stale-over-base"));
        raw.insert("3".to_string(), plugin("stale-over-empty"));
        raw.insert("bad".to_string(), plugin("bad-key"));
        raw.insert("99".to_string(), plugin("out-of-range"));

        let kept = consistent_plugin_slots(&loadout, &raw);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[&1].name, "valid");
    }
}
