use crate::config::save_config;
use crate::H2ACApp;

impl H2ACApp {
    /// 战备的有效分类（优先取用户覆盖，否则用默认分类）；返回借用，避免每帧克隆
    pub fn effective_category<'a>(&'a self, name: &str, default_cat: &'a str) -> &'a str {
        self.model
            .config
            .category_overrides
            .get(name)
            .map(String::as_str)
            .unwrap_or(default_cat)
    }

    pub fn set_category_override(&mut self, name: &str, category: &str) {
        self.model
            .config
            .category_overrides
            .insert(name.to_string(), category.to_string());
        save_config(&self.model.config);
        // 运行时同步插件战备分类（持久化由 category_overrides 在 config.json 承担）
        for p in &mut self.plugins.stratagems {
            if p.name == name {
                p.category = category.to_string();
                break;
            }
        }
    }

    pub fn clear_category_override(&mut self, name: &str) {
        self.model.config.category_overrides.remove(name);
        save_config(&self.model.config);
    }
}
