// 插件管理器 — 从 plugins/ 目录加载 JSON 插件并合并到运行时
use crate::stratagems::{PluginManifest, PluginStratagem, PluginTheme};
use std::fs;
use std::path::PathBuf;

pub fn plugins_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("plugins")
}

/// 扫描 plugins/ 目录，加载所有启用的插件
pub fn load_all() -> (Vec<PluginStratagem>, Vec<PluginTheme>) {
    let dir = plugins_dir();
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
        return (Vec::new(), Vec::new());
    }

    let mut stratagems = Vec::new();
    let mut themes = Vec::new();

    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return (stratagems, themes),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map_or(true, |e| e != "json") {
            continue;
        }

        let data = match fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let manifest: PluginManifest = match serde_json::from_str(&data) {
            Ok(m) => m,
            Err(_) => continue,
        };

        if !manifest.enabled {
            continue;
        }

        for s in manifest.stratagems {
            stratagems.push(s);
        }
        for t in manifest.themes {
            themes.push(t);
        }
    }

    (stratagems, themes)
}

/// 创建示例插件 JSON 存到 plugins/example.json（供 UI 创建器首次使用时参考）
pub fn create_example_plugin() {
    let dir = plugins_dir();
    let _ = fs::create_dir_all(&dir);
    let example = dir.join("_example.json");
    if example.exists() {
        return;
    }

    let manifest = PluginManifest {
        id: "example".into(),
        name: "示例插件".into(),
        enabled: false,
        stratagems: vec![PluginStratagem {
            name: "示例战备".into(),
            category: "任务战备".into(),
            model: "EXAMPLE".into(),
            command: vec!["up".into(), "down".into(), "left".into(), "right".into()],
            description: "这是一个通过插件加载的示例战备".into(),
            icon: "reinforce".into(),
        }],
        themes: vec![PluginTheme {
            name: "示例主题".into(),
            background_color: "#0a0f0a".into(),
            border_color: "#3ddc84".into(),
            accent_color: "#66ffaa".into(),
        }],
    };

    if let Ok(json) = serde_json::to_string_pretty(&manifest) {
        let _ = fs::write(&example, json);
    }
}
