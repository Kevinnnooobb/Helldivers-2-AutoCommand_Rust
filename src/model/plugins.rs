use crate::config;
use crate::H2ACApp;
use crate::LogKind;
use crate::plugin;

impl H2ACApp {
    pub fn delete_plugin_stratagem(&mut self, name: &str) {
        self.plugins.stratagems.retain(|p| p.name != name);
        let dir = plugin::plugins_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(true, |e| e != "json") { continue; }
                if let Ok(data) = std::fs::read_to_string(&path) {
                    if data.contains(&format!("\"name\": \"{}\"", name)) {
                        if let Ok(mut m) = serde_json::from_str::<crate::stratagems::PluginManifest>(&data) {
                            let before = m.stratagems.len();
                            m.stratagems.retain(|s| s.name != name);
                            if m.stratagems.len() < before {
                                let _ = std::fs::write(&path, serde_json::to_string_pretty(&m).unwrap_or_default());
                            }
                        }
                    }
                }
            }
        }
        self.log(LogKind::Warn, format!("已删除: {name}"));
    }

    pub fn load_profile_data(&mut self, name: &str) {
        if let Some(pr) = config::load_profile(name) {
            self.model.slots = pr.loadout.clone();
            // 同步持久化 loadout，避免「加载后编辑单个槽位 → 其余槽位回退为旧值」的混合状态
            self.model.config.loadout = pr.loadout;
            // 非法槽位键直接跳过（不再塌缩到槽位 0 造成数据错位）
            self.model.plugin_slots = pr.plugin_slots.iter()
                .filter_map(|(k, v)| Some((k.parse::<usize>().ok()?, v.clone())))
                .collect();
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
