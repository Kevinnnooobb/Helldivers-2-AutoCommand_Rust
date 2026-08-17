// 配置与 Profile 管理
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use crate::stratagems::PluginStratagem;
use crate::util;

pub const SLOT_COUNT: usize = 10;

fn app_dir() -> PathBuf {
    util::app_dir()
}

fn default_config_path() -> PathBuf {
    app_dir().join("config.json")
}

fn profiles_dir() -> PathBuf {
    app_dir().join("profiles")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_key_bindings")]
    pub key_bindings: HashMap<String, String>,
    #[serde(default = "default_stratagem_key")]
    pub stratagem_key: String,
    #[serde(default = "default_key_delay")]
    pub key_delay: f64,
    /// 激活键按下后等待指令面板弹出的延迟（秒）
    #[serde(default = "default_pre_delay")]
    pub pre_delay: f64,
    #[serde(default)]
    pub slot_hotkeys: HashMap<String, String>,
    #[serde(default = "empty_loadout")]
    pub loadout: Vec<Option<usize>>,
    #[serde(default = "default_true")]
    pub listening_enabled: bool,
    #[serde(default)]
    pub last_profile: String,
    /// 战备名 → 新分类名（运行时修改分类的持久覆盖）
    #[serde(default)]
    pub category_overrides: HashMap<String, String>,
}

fn default_key_bindings() -> HashMap<String, String> {
    HashMap::from([
        ("↑".into(), "w".into()),
        ("↓".into(), "s".into()),
        ("←".into(), "a".into()),
        ("→".into(), "d".into()),
    ])
}

fn default_stratagem_key() -> String { "ctrl".into() }
fn default_key_delay() -> f64 { 0.08 }
fn default_pre_delay() -> f64 { 0.12 }
fn empty_loadout() -> Vec<Option<usize>> { vec![None; SLOT_COUNT] }
fn default_true() -> bool { true }

impl Default for Config {
    fn default() -> Self {
        Self {
            key_bindings: default_key_bindings(),
            stratagem_key: default_stratagem_key(),
            key_delay: default_key_delay(),
            pre_delay: default_pre_delay(),
            slot_hotkeys: HashMap::new(),
            loadout: empty_loadout(),
            listening_enabled: true,
            last_profile: String::new(),
            category_overrides: HashMap::new(),
        }
    }
}

pub fn load_config() -> Config {
    let path = default_config_path();
    if !path.exists() {
        return Config::default();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Config>(&s).ok())
        .unwrap_or_default()
        .sanitize()
}

/// 槽位列表归一化：补足/截断到 SLOT_COUNT，并把越界索引（非插件哨兵）置为空槽
fn sanitize_loadout(loadout: &mut Vec<Option<usize>>) {
    loadout.resize(SLOT_COUNT, None);
    let max_idx = crate::stratagems::STRATAGEMS.len();
    for slot in loadout.iter_mut() {
        if let Some(v) = slot {
            if *v >= max_idx && *v != crate::stratagems::PLUGIN_SLOT_MARK {
                *slot = None;
            }
        }
    }
}

impl Config {
    fn sanitize(mut self) -> Self {
        sanitize_loadout(&mut self.loadout);
        self
    }
}

pub fn save_config(config: &Config) {
    // 序列化失败时不写文件，避免损坏现有配置
    let _ = util::save_json(&default_config_path(), config);
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Profile {
    pub loadout: Vec<Option<usize>>,
    pub slot_hotkeys: HashMap<String, String>,
    /// 插件战备槽位映射（key=slot index, value=战备序列化数据）
    #[serde(default)]
    pub plugin_slots: HashMap<String, PluginStratagem>,
}

pub fn list_profiles() -> Vec<String> {
    let dir = profiles_dir();
    let _ = fs::create_dir_all(&dir);
    let mut names: Vec<String> = fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
                .filter_map(|e| {
                    e.path()
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

pub fn save_profile(name: &str, loadout: &[Option<usize>], hotkeys: &HashMap<String, String>, plugin_slots: &HashMap<String, PluginStratagem>) {
    let profile = Profile {
        loadout: loadout.to_vec(),
        slot_hotkeys: hotkeys.clone(),
        plugin_slots: plugin_slots.iter().map(|(k,v)| (k.clone(), v.clone())).collect(),
    };
    let path = profiles_dir().join(format!("{name}.json"));
    let _ = util::save_json(&path, &profile);
}

pub fn load_profile(name: &str) -> Option<Profile> {
    let path = profiles_dir().join(format!("{name}.json"));
    if !path.exists() {
        return None;
    }
    let data = fs::read_to_string(&path).ok()?;
    let mut p: Profile = serde_json::from_str(&data).ok()?;
    sanitize_loadout(&mut p.loadout);
    Some(p)
}

pub fn delete_profile(name: &str) {
    let path = profiles_dir().join(format!("{name}.json"));
    let _ = fs::remove_file(&path);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_from_partial_json() {
        let cfg: Config = serde_json::from_str(r#"{ "loadout": [1] }"#).unwrap();
        assert_eq!(cfg.key_bindings.get("↑").map(String::as_str), Some("w"));
        assert_eq!(cfg.stratagem_key, "ctrl");
        assert_eq!(cfg.key_delay, 0.08);
        assert_eq!(cfg.pre_delay, 0.12);
        assert!(cfg.listening_enabled);
        assert!(cfg.slot_hotkeys.is_empty());
    }

    #[test]
    fn sanitize_resizes_and_clears_out_of_range() {
        let mark = crate::stratagems::PLUGIN_SLOT_MARK;
        let cfg = Config {
            loadout: vec![Some(0), Some(1), Some(999_999), Some(mark)],
            ..Config::default()
        }
        .sanitize();
        assert_eq!(cfg.loadout.len(), SLOT_COUNT);
        assert_eq!(cfg.loadout[0], Some(0));
        assert_eq!(cfg.loadout[1], Some(1));
        // 越界索引 → 空槽
        assert_eq!(cfg.loadout[2], None);
        // 插件哨兵必须保留（Profile 用它表达插件槽位）
        assert_eq!(cfg.loadout[3], Some(mark));
    }

    #[test]
    fn profile_roundtrip_json() {
        let p = Profile {
            loadout: vec![Some(2), None],
            slot_hotkeys: HashMap::from([("0".into(), "f1".into())]),
            plugin_slots: HashMap::new(),
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.loadout, p.loadout);
        assert_eq!(back.slot_hotkeys, p.slot_hotkeys);
    }
}
