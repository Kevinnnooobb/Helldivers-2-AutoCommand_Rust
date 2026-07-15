// 配置与 Profile 管理
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub const SLOT_COUNT: usize = 10;

fn app_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
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
    #[serde(default)]
    pub slot_hotkeys: HashMap<String, String>,
    #[serde(default = "empty_loadout")]
    pub loadout: Vec<Option<usize>>,
    #[serde(default = "default_true")]
    pub listening_enabled: bool,
    #[serde(default)]
    pub last_profile: String,
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
fn default_key_delay() -> f64 { 0.05 }
fn empty_loadout() -> Vec<Option<usize>> { vec![None; SLOT_COUNT] }
fn default_true() -> bool { true }

impl Default for Config {
    fn default() -> Self {
        Self {
            key_bindings: default_key_bindings(),
            stratagem_key: default_stratagem_key(),
            key_delay: default_key_delay(),
            slot_hotkeys: HashMap::new(),
            loadout: empty_loadout(),
            listening_enabled: true,
            last_profile: String::new(),
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

impl Config {
    fn sanitize(mut self) -> Self {
        self.loadout.resize(SLOT_COUNT, None);
        self
    }
}

pub fn save_config(config: &Config) {
    let path = default_config_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, serde_json::to_string_pretty(config).unwrap_or_default());
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Profile {
    pub loadout: Vec<Option<usize>>,
    pub slot_hotkeys: HashMap<String, String>,
}

pub fn list_profiles() -> Vec<String> {
    let dir = profiles_dir();
    let _ = fs::create_dir_all(&dir);
    let mut names: Vec<String> = fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
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

pub fn save_profile(name: &str, loadout: &[Option<usize>], hotkeys: &HashMap<String, String>) {
    let dir = profiles_dir();
    let _ = fs::create_dir_all(&dir);
    let profile = Profile {
        loadout: loadout.to_vec(),
        slot_hotkeys: hotkeys.clone(),
    };
    let path = dir.join(format!("{name}.json"));
    let _ = fs::write(&path, serde_json::to_string_pretty(&profile).unwrap_or_default());
}

pub fn load_profile(name: &str) -> Option<Profile> {
    let path = profiles_dir().join(format!("{name}.json"));
    if !path.exists() {
        return None;
    }
    let data = fs::read_to_string(&path).ok()?;
    let mut p: Profile = serde_json::from_str(&data).ok()?;
    p.loadout.resize(SLOT_COUNT, None);
    Some(p)
}

pub fn delete_profile(name: &str) {
    let path = profiles_dir().join(format!("{name}.json"));
    let _ = fs::remove_file(&path);
}
