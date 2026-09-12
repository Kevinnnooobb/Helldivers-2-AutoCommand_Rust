// 插件管理器 — 从 plugins/ 目录加载 JSON 插件并合并到运行时
use crate::stratagems::{PluginManifest, PluginStratagem};
use crate::util;
use std::fs;
use std::path::PathBuf;

/// 页面数据自动获取结果的持久化文件名（位于 plugins/ 目录，按插件清单格式保存）
pub const WIKI_PLUGIN_FILE: &str = "_wiki.json";
/// 页面数据自动获取结果插件清单的 id
pub const WIKI_PLUGIN_ID: &str = "_wiki";
/// 战备的 runtime 来源标记：自动获取的页面数据
pub const WIKI_SOURCE: &str = WIKI_PLUGIN_ID;
/// 强化（Booster）自动获取结果的持久化文件名（plugins/ 目录）
pub const BOOSTER_PLUGIN_FILE: &str = "_boosters.json";
/// 强化自动获取结果插件清单的 id
pub const BOOSTER_PLUGIN_ID: &str = "_boosters";
/// 强化的 runtime 来源标记：自动获取的页面数据（helldivers.wiki.gg/wiki/Boosters）
pub const BOOSTER_SOURCE: &str = BOOSTER_PLUGIN_ID;
/// 旧版缓存文件名（迁移时删除；旧格式带 NEW (Wiki) 分类与 (Wiki) 名称后缀）
const LEGACY_WIKI_PLUGIN_FILE: &str = "_wiki_new.json";

pub fn plugins_dir() -> PathBuf {
    util::app_dir().join("plugins")
}

/// Wiki 拉取结果文件路径（战备）
pub fn wiki_plugin_path() -> PathBuf {
    plugins_dir().join(WIKI_PLUGIN_FILE)
}

/// 强化拉取结果文件路径
pub fn booster_plugin_path() -> PathBuf {
    plugins_dir().join(BOOSTER_PLUGIN_FILE)
}

/// 该来源是否属于「在线获取的数据」（战备或强化）：重命名/替换时需要一起处理
pub fn is_wiki_source(source: &str) -> bool {
    source == WIKI_SOURCE || source == BOOSTER_SOURCE
}

/// 扫描 plugins/ 目录，加载所有启用的插件
pub fn load_all() -> Vec<PluginStratagem> {
    let dir = plugins_dir();
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
        return Vec::new();
    }

    let mut stratagems = Vec::new();

    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return stratagems,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }

        // 迁移：删除旧版缓存 _wiki_new.json（旧格式带 NEW (Wiki) 分类与 (Wiki) 后缀）
        let is_wiki = path.file_stem().is_some_and(|stem| stem == WIKI_PLUGIN_ID);
        let is_boosters = path
            .file_stem()
            .is_some_and(|stem| stem == BOOSTER_PLUGIN_ID);
        let is_legacy = path
            .file_name()
            .is_some_and(|name| name == LEGACY_WIKI_PLUGIN_FILE);
        if is_legacy {
            let _ = fs::remove_file(&path);
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

        for mut s in manifest.stratagems {
            if is_wiki {
                s.source = WIKI_SOURCE.to_string();
            } else if is_boosters {
                s.source = BOOSTER_SOURCE.to_string();
            }
            stratagems.push(s);
        }
    }

    stratagems
}

/// 对 plugins/ 目录下所有 JSON 清单执行 transform（修改整个战备列表）；
/// 返回 true 表示该文件被修改，仅变更过的文件会被回写。
pub fn rewrite_stratagems(
    transform: &mut dyn FnMut(&mut Vec<PluginStratagem>) -> bool,
) -> std::io::Result<()> {
    let dir = plugins_dir();
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&dir)?.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let data = match fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let mut manifest: PluginManifest = match serde_json::from_str(&data) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if transform(&mut manifest.stratagems) {
            crate::util::save_json(&path, &manifest)?;
        }
    }
    Ok(())
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
            source: String::new(),
            icon_url: None,
        }],
    };

    if let Ok(json) = serde_json::to_string_pretty(&manifest) {
        let _ = fs::write(&example, json);
    }
}
