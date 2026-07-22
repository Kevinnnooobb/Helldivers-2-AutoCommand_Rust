use crate::config::save_config;
use crate::H2ACApp;
use crate::plugin;

impl H2ACApp {
    pub fn effective_category(&self, name: &str, default_cat: &str) -> String {
        self.model.config.category_overrides.get(name)
            .cloned()
            .unwrap_or_else(|| default_cat.to_string())
    }

    pub fn set_category_override(&mut self, name: &str, category: &str) {
        self.model.config.category_overrides.insert(name.to_string(), category.to_string());
        save_config(&self.model.config);
        self.sync_plugin_category(name, category);
    }

    pub fn clear_category_override(&mut self, name: &str) {
        self.model.config.category_overrides.remove(name);
        save_config(&self.model.config);
    }

    fn sync_plugin_category(&mut self, name: &str, new_cat: &str) {
        for p in &mut self.plugins.stratagems {
            if p.name == name || p.name == format!("{name} (Wiki)") {
                p.category = new_cat.to_string();
                let dir = plugin::plugins_dir();
                let _ = std::fs::create_dir_all(&dir);
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().map_or(true, |e| e != "json") { continue; }
                        if let Ok(data) = std::fs::read_to_string(&path) {
                            if data.contains(&format!("\"name\": \"{}\"", p.name)) {
                                if let Ok(mut m) = serde_json::from_str::<crate::stratagems::PluginManifest>(&data) {
                                    for s in &mut m.stratagems {
                                        if s.name == p.name { s.category = new_cat.to_string(); }
                                    }
                                }
                            }
                        }
                    }
                }
                break;
            }
        }
    }
}
