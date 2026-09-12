// 配置与 Profile 管理
use crate::stratagems::PluginStratagem;
use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

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
    /// 全局监听开关快捷键（按键名，例如 "f8" 或 ","），空字符串表示未绑定
    #[serde(default)]
    pub listen_hotkey: String,
    #[serde(default = "empty_loadout")]
    pub loadout: Vec<Option<usize>>,
    #[serde(default = "default_true")]
    pub listening_enabled: bool,
    #[serde(default)]
    pub last_profile: String,
    /// 战备名 → 新分类名（运行时修改分类的持久覆盖）
    #[serde(default)]
    pub category_overrides: HashMap<String, String>,
    /// Auto Loadout 全局快捷键（唯一入口：正常模式 / 紧凑模式 / 浮窗隐藏时都可用）
    #[serde(default = "default_loadout_sync_hotkey")]
    pub loadout_sync_hotkey: String,
    /// Loadout Sync 取消快捷键（全局，独立于紧凑模式；运行中按下即停止自动化）
    #[serde(default = "default_loadout_sync_cancel_hotkey")]
    pub loadout_sync_cancel_hotkey: String,
    /// Loadout Sync 自动化参数（视觉/时序），默认值可用
    #[serde(default)]
    pub loadout_sync: crate::loadout_sync::config::LoadoutSyncConfig,
    /// 紧凑模式（游戏内配装预设 Overlay）：快捷键 / 透明度 / 位置
    #[serde(default)]
    pub compact_mode: crate::compact_mode::config::CompactModeConfig,
}

fn default_key_bindings() -> HashMap<String, String> {
    HashMap::from([
        ("↑".into(), "w".into()),
        ("↓".into(), "s".into()),
        ("←".into(), "a".into()),
        ("→".into(), "d".into()),
    ])
}

/// Loadout Sync 默认快捷键：F7（与默认 slot_hotkeys / listen_hotkey 不冲突；
/// 若用户已占用，启动时会给出冲突提示并要求改绑）
fn default_loadout_sync_hotkey() -> String {
    "f7".into()
}

/// 取消装配：与自动装配、浮窗显示都无关的第三个独立热键
fn default_loadout_sync_cancel_hotkey() -> String {
    "ctrl+shift+f9".into()
}

fn default_stratagem_key() -> String {
    "ctrl".into()
}
fn default_key_delay() -> f64 {
    0.08
}
fn default_pre_delay() -> f64 {
    0.12
}
fn empty_loadout() -> Vec<Option<usize>> {
    vec![None; SLOT_COUNT]
}
fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            key_bindings: default_key_bindings(),
            stratagem_key: default_stratagem_key(),
            key_delay: default_key_delay(),
            pre_delay: default_pre_delay(),
            slot_hotkeys: HashMap::new(),
            listen_hotkey: String::new(),
            loadout: empty_loadout(),
            listening_enabled: true,
            last_profile: String::new(),
            category_overrides: HashMap::new(),
            loadout_sync_hotkey: default_loadout_sync_hotkey(),
            loadout_sync_cancel_hotkey: default_loadout_sync_cancel_hotkey(),
            loadout_sync: crate::loadout_sync::config::LoadoutSyncConfig::default(),
            compact_mode: crate::compact_mode::config::CompactModeConfig::default(),
        }
    }
}

pub fn load_config() -> Config {
    let path = default_config_path();
    if !path.exists() {
        return Config::default();
    }
    let Some(raw) = fs::read_to_string(&path).ok() else {
        return Config::default();
    };
    let Some(mut value) = serde_json::from_str::<serde_json::Value>(&raw).ok() else {
        return Config::default();
    };
    migrate_legacy_hotkeys(&mut value);
    serde_json::from_value::<Config>(value)
        .unwrap_or_default()
        .sanitize()
}

/// v1.2 起自动装配 / 取消快捷键提升为全局 Loadout Sync 配置（不再属于紧凑模式）。
///
/// 旧配置里写在 compact_mode 段下的两个键会被迁移到新字段，随后删除旧键，
/// 保证老用户的自定义快捷键不丢。
pub(crate) fn migrate_legacy_hotkeys(value: &mut serde_json::Value) {
    let Some(root) = value.as_object_mut() else {
        return;
    };
    let legacy_auto = root
        .get_mut("compact_mode")
        .and_then(|cm| cm.as_object_mut())
        .and_then(|cm| cm.remove("auto_loadout_hotkey"))
        .and_then(|v| v.as_str().map(str::to_string));
    let legacy_cancel = root
        .get_mut("compact_mode")
        .and_then(|cm| cm.as_object_mut())
        .and_then(|cm| cm.remove("cancel_hotkey"))
        .and_then(|v| v.as_str().map(str::to_string));

    let is_blank = |root: &serde_json::Map<String, serde_json::Value>, key: &str| {
        root.get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
    };
    if let Some(key) = legacy_auto {
        if !key.trim().is_empty() && is_blank(root, "loadout_sync_hotkey") {
            root.insert("loadout_sync_hotkey".into(), serde_json::Value::String(key));
        }
    }
    if let Some(key) = legacy_cancel {
        if !key.trim().is_empty() && is_blank(root, "loadout_sync_cancel_hotkey") {
            root.insert(
                "loadout_sync_cancel_hotkey".into(),
                serde_json::Value::String(key),
            );
        }
    }
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
    pub(crate) fn sanitize(mut self) -> Self {
        sanitize_loadout(&mut self.loadout);
        self.loadout_sync = self.loadout_sync.clone().sanitize();
        self.compact_mode = self.compact_mode.clone().sanitize();
        self.loadout_sync_hotkey = crate::hotkey::normalize_key_name(&self.loadout_sync_hotkey);
        if self.loadout_sync_hotkey == "vk(0)" {
            self.loadout_sync_hotkey.clear();
        }
        self
    }

    /// 全局快捷键冲突检查：listen / loadout_sync / slot 三者之间不允许重复绑同一个键。
    /// 返回人类可读的冲突描述（空表示无冲突）。
    pub fn hotkey_conflicts(&self) -> Vec<String> {
        let mut seen: HashMap<String, String> = HashMap::new();
        let mut out = Vec::new();
        let mut check = |key: &str, owner: String, out: &mut Vec<String>| {
            let k = crate::hotkey::normalize_hotkey(key);
            if k.is_empty() {
                return;
            }
            match seen.get(&k) {
                Some(prev) => out.push(format!(
                    "快捷键 {} 同时绑定了 {} 与 {}",
                    k.to_uppercase(),
                    prev,
                    owner
                )),
                None => {
                    seen.insert(k, owner);
                }
            }
        };
        check(&self.listen_hotkey, "监听开关".into(), &mut out);
        check(&self.loadout_sync_hotkey, "自动装配".into(), &mut out);
        check(
            &self.loadout_sync_cancel_hotkey,
            "取消装配".into(),
            &mut out,
        );
        check(
            &self.compact_mode.toggle_hotkey,
            "紧凑浮窗".into(),
            &mut out,
        );
        let mut slots: Vec<(String, String)> = self
            .slot_hotkeys
            .iter()
            .filter(|(k, v)| !v.trim().is_empty() && k.parse::<usize>().is_ok())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        slots.sort_by_key(|(k, _)| k.parse::<usize>().unwrap_or(0));
        for (slot, key) in slots {
            let n: usize = slot.parse().unwrap_or(0) + 1;
            check(&key, format!("槽位 {n:02}"), &mut out);
        }
        out
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

pub fn save_profile(
    name: &str,
    loadout: &[Option<usize>],
    hotkeys: &HashMap<String, String>,
    plugin_slots: &HashMap<String, PluginStratagem>,
) {
    let profile = Profile {
        loadout: loadout.to_vec(),
        slot_hotkeys: hotkeys.clone(),
        plugin_slots: plugin_slots
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
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
        assert!(cfg.listen_hotkey.is_empty());
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
