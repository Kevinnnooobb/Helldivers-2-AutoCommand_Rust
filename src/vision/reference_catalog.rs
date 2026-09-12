//! 参考实现（`hd2-preset-helper-0.1.4`）图标目录的迁移层。
//!
//! 迁移来源与边界见 `docs/reference-migration.md`。这里只做两件事：
//!   1. 解析参考项目的 `manifest.json`（**唯一资源索引**，不做手工重命名）；
//!   2. 校验每条目目的 `item_id` / `kind` / `path`，缺失资源**明确报错**。
//!
//! 刻意不做的：
//!   * 不静默跳过缺失资源（plan6 §2 约束：不允许缺失资源静默生成空模板）；
//!   * 不把 `item_id`（kebab-case）与当前项目的图标键（snake_case）混为一谈 ——
//!     两者的对应关系由 [`IconCatalog::resolve_current_key`] 显式表达。
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// 参考目录里的物品类型。
///
/// 这是 plan6 §3.2 要求的 `ItemKind`：Stratagem 与 Booster **必须明确分离**，
/// 因为游戏里它们属于两个不同的列表，混用会导致在错误的列表里搜索。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ItemKind {
    Stratagem,
    Booster,
}

impl ItemKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stratagem => "stratagem",
            Self::Booster => "booster",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "stratagem" => Some(Self::Stratagem),
            "booster" => Some(Self::Booster),
            _ => None,
        }
    }
}

/// 目录中的一条图标条目。
#[derive(Debug, Clone, PartialEq)]
pub struct IconEntry {
    /// 参考项目的稳定 ID（kebab-case），与 manifest 一致。
    pub item_id: String,
    pub display_name: String,
    pub kind: ItemKind,
    /// 相对 `manifest.json` 所在的目录。
    pub path: String,
}

impl IconEntry {
    /// `item_id` 转成当前项目使用的 snake_case 图标键。
    ///
    /// 这只是**规范化猜测**，不保证命中；真正可靠的对应关系由
    /// [`IconCatalog::resolve_current_key`] 的别名表给出。
    pub fn normalized_current_key(&self) -> String {
        self.item_id.replace('-', "_")
    }
}

/// 加载或校验目录时的错误。全部显式，不用 `Option` 吞掉。
#[derive(Debug, Clone, PartialEq)]
pub enum CatalogError {
    /// manifest 无法读取
    Io { path: String, detail: String },
    /// manifest 不是合法 JSON / 结构不符
    Parse { detail: String },
    /// `kind` 不是 `stratagem` / `booster`
    UnknownKind { item_id: String, raw: String },
    /// `item_id` 重复
    DuplicateId { item_id: String },
    /// 条目字段为空
    EmptyField {
        item_id: String,
        field: &'static str,
    },
    /// 磁盘上找不到该资源
    MissingAsset { item_id: String, path: String },
    /// 资源路径不是 catalog 根目录下的相对路径
    InvalidAssetPath { item_id: String, path: String },
}

impl CatalogError {
    pub fn message(&self) -> String {
        match self {
            Self::Io { path, detail } => format!("读取 manifest 失败 {path}: {detail}"),
            Self::Parse { detail } => format!("manifest 解析失败: {detail}"),
            Self::UnknownKind { item_id, raw } => {
                format!("条目 {item_id} 的 kind「{raw}」未知（应为 stratagem / booster）")
            }
            Self::DuplicateId { item_id } => format!("item_id 重复: {item_id}"),
            Self::EmptyField { item_id, field } => format!("条目 {item_id} 的 {field} 为空"),
            Self::MissingAsset { item_id, path } => {
                format!("条目 {item_id} 的资源不存在: {path}")
            }
            Self::InvalidAssetPath { item_id, path } => {
                format!("条目 {item_id} 的资源路径非法: {path}")
            }
        }
    }
}

/// 参考图标目录（内存中按 `item_id` 索引）。
#[derive(Debug, Clone, Default)]
pub struct IconCatalog {
    entries: BTreeMap<String, IconEntry>,
    /// manifest 所在目录，用于解析相对 `path`。
    root: PathBuf,
}

impl IconCatalog {
    /// 解析 manifest 文本并校验结构（**不读磁盘上的图片**）。
    pub fn from_manifest_str(text: &str, root: &Path) -> Result<Self, CatalogError> {
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| CatalogError::Parse {
                detail: e.to_string(),
            })?;
        let items = value
            .get("items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| CatalogError::Parse {
                detail: "缺少 items 数组".to_string(),
            })?;
        let mut entries = BTreeMap::new();
        for item in items {
            let get_str = |key: &str| -> Option<String> {
                item.get(key)
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
            };
            let item_id = get_str("item_id").unwrap_or_default();
            if item_id.is_empty() {
                return Err(CatalogError::EmptyField {
                    item_id: "<无 id>".to_string(),
                    field: "item_id",
                });
            }
            let display_name = get_str("display_name").unwrap_or_default();
            if display_name.is_empty() {
                return Err(CatalogError::EmptyField {
                    item_id: item_id.clone(),
                    field: "display_name",
                });
            }
            let raw_kind = get_str("kind").unwrap_or_default();
            let kind = ItemKind::parse(&raw_kind).ok_or_else(|| CatalogError::UnknownKind {
                item_id: item_id.clone(),
                raw: raw_kind.clone(),
            })?;
            let path = get_str("path").unwrap_or_default();
            if path.is_empty() {
                return Err(CatalogError::EmptyField {
                    item_id: item_id.clone(),
                    field: "path",
                });
            }
            let path_obj = Path::new(&path);
            if path_obj.is_absolute()
                || path_obj.components().any(|component| {
                    matches!(
                        component,
                        Component::ParentDir | Component::RootDir | Component::Prefix(_)
                    )
                })
            {
                return Err(CatalogError::InvalidAssetPath {
                    item_id: item_id.clone(),
                    path,
                });
            }
            if entries.contains_key(&item_id) {
                return Err(CatalogError::DuplicateId { item_id });
            }
            entries.insert(
                item_id.clone(),
                IconEntry {
                    item_id,
                    display_name,
                    kind,
                    path,
                },
            );
        }
        Ok(Self {
            entries,
            root: root.to_path_buf(),
        })
    }

    /// 从磁盘加载并**校验每个资源确实存在**。
    pub fn load(root: &Path) -> Result<Self, CatalogError> {
        let manifest = root.join("manifest.json");
        let text = std::fs::read_to_string(&manifest).map_err(|e| CatalogError::Io {
            path: manifest.display().to_string(),
            detail: e.to_string(),
        })?;
        let catalog = Self::from_manifest_str(&text, root)?;
        catalog.validate_assets()?;
        Ok(catalog)
    }

    /// 逐条确认 `path` 指向的文件存在。缺失即报错，绝不静默跳过。
    pub fn validate_assets(&self) -> Result<(), CatalogError> {
        for entry in self.entries.values() {
            let full = self.root.join(&entry.path);
            if !full.is_file() {
                return Err(CatalogError::MissingAsset {
                    item_id: entry.item_id.clone(),
                    path: full.display().to_string(),
                });
            }
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn get(&self, item_id: &str) -> Option<&IconEntry> {
        self.entries.get(item_id)
    }

    /// 某类型的全部条目（`item_id` 升序）。
    pub fn by_kind(&self, kind: ItemKind) -> impl Iterator<Item = &IconEntry> {
        self.entries.values().filter(move |e| e.kind == kind)
    }

    pub fn count_of(&self, kind: ItemKind) -> usize {
        self.by_kind(kind).count()
    }

    /// 该条目资源的绝对路径。
    pub fn asset_path(&self, item_id: &str) -> Option<PathBuf> {
        self.entries.get(item_id).map(|e| self.root.join(&e.path))
    }

    /// 当前项目的图标键 → 参考 `item_id`（[`Self::resolve_current_key`] 的逆查）。
    ///
    /// 用于把用户 preset 里的图标键翻译成状态机使用的目录 ID。
    /// 多个 `item_id` 可能解析到同一个图标键时取**字典序最小**的那个，
    /// 保证结果稳定（目录按 `BTreeMap` 有序）。
    pub fn resolve_item_id(&self, current_key: &str) -> Option<&str> {
        self.entries
            .values()
            .find(|e| e.normalized_current_key() == current_key)
            .map(|e| e.item_id.as_str())
            .or_else(|| {
                REFERENCE_ID_ALIASES
                    .iter()
                    .find(|(_, current)| *current == current_key)
                    .and_then(|(reference, _)| {
                        self.entries.get(*reference).map(|e| e.item_id.as_str())
                    })
            })
    }

    /// 参考 `item_id` → 当前项目的图标键。
    ///
    /// 两步：先按规范化猜（`-` → `_`），命中即返回；否则查显式别名表。
    /// * `current_keys` 通常是 `icons::all_icon_keys()`；
    /// * 找不到时返回 `None`，**不猜**。
    pub fn resolve_current_key<'a>(
        &self,
        item_id: &str,
        current_keys: &'a [&'a str],
    ) -> Option<&'a str> {
        let entry = self.entries.get(item_id)?;
        let candidate = entry.normalized_current_key();
        if let Some(hit) = current_keys.iter().find(|k| **k == candidate) {
            return Some(hit);
        }
        REFERENCE_ID_ALIASES
            .iter()
            .find(|(reference, _)| *reference == item_id)
            .and_then(|(_, current)| current_keys.iter().find(|k| **k == *current).copied())
    }
}

/// 参考图标目录的默认根目录。
///
/// 与 `icons.rs` 的磁盘回退同一策略：先看 exe 旁的 `assets/reference/icons`
/// （发布布局），再看工作目录（开发/仓库布局）。两个都不存在时返回开发布局，
/// 让调用方的错误信息带上一个可读路径，而不是静默使用空目录。
pub fn default_root() -> PathBuf {
    let candidates = [
        crate::util::app_dir().join("assets/reference/icons"),
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("assets/reference/icons"),
    ];
    candidates
        .iter()
        .find(|p| p.join("manifest.json").is_file())
        .cloned()
        .unwrap_or_else(|| candidates[1].clone())
}

/// 参考 `item_id` → 当前项目图标键的**显式别名表**。///
/// 只登记规范化无法命中的条目（参考项目用了不同的命名，例如
/// 参考叫 `meltagun`、当前项目叫 `40_k_meltagun`）。
/// 不做模糊匹配；未登记的条目由调用方按「缺失」处理。
pub const REFERENCE_ID_ALIASES: &[(&str, &str)] = &[
    // ── 参考项目有、当前项目键名不同 ──
    ("meltagun", "40_k_meltagun"),
    ("breaching-hammer", "cqc_20"),
    ("portable-hellbomb", "hellbomb_portable"),
    ("de-escalator", "gl_52_de_escalator"),
    ("belt-fed-grenade-launcher", "gl_28"),
    ("wasp-launcher", "sta_x3_w_a_s_p_launcher"),
    ("one-true-flag", "one_true_flag"),
    ("k-9", "guard_dog_k_9"),
    ("dog-breath", "guard_dog_breath"),
    ("hot-dog", "guard_dog_hot_dog"),
    ("rover", "guard_dog_rover"),
    ("guard-dog", "guard_dog"),
    ("leveller", "eat_411"),
    ("gas-mines", "gas_mine"),
    ("hellbomb", "hellbomb"),
    ("solo-silo", "solo_silo"),
    ("seaf-artillery", "seaf_artillery"),
    // ── 参考项目没有、当前项目有的任务战备 ──
    //
    // 参考 manifest **不含**任何任务战备（增援 / 补给 / 地狱炸弹 / SSSD 交付 /
    // 超级地球旗帜 / 上传数据 / 地震探测器 / 暗流体容器 / 蜂巢破碎钻机 /
    // SEAF 火炮 / 呼叫超级驱逐舰 / 飞鹰重新装填）。
    // 这意味着：**预设里若含任务战备，在普通战备列表里永远找不到**，
    // 必须由调用方按「目录缺失」拒答，而不是无限滚动搜索。
];

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{
      "items": [
        {"item_id":"orbital-laser","display_name":"Orbital Laser","kind":"stratagem","path":"stratagem/offensive/orbital-laser.png"},
        {"item_id":"experimental-infusion","display_name":"Experimental Infusion","kind":"booster","path":"booster/experimental-infusion.png"}
      ]
    }"#;

    #[test]
    fn parses_and_splits_item_kinds() {
        let c = IconCatalog::from_manifest_str(MANIFEST, Path::new(".")).expect("解析");
        assert_eq!(c.len(), 2);
        assert_eq!(c.count_of(ItemKind::Stratagem), 1);
        assert_eq!(c.count_of(ItemKind::Booster), 1);
        assert_eq!(
            c.get("orbital-laser").map(|e| e.kind),
            Some(ItemKind::Stratagem)
        );
        assert_eq!(
            c.by_kind(ItemKind::Booster)
                .next()
                .map(|e| e.display_name.as_str()),
            Some("Experimental Infusion")
        );
    }

    #[test]
    fn rejects_bad_manifests_instead_of_silently_skipping() {
        // 未知 kind
        let bad_kind =
            r#"{"items":[{"item_id":"x","display_name":"X","kind":"weapon","path":"x.png"}]}"#;
        assert!(matches!(
            IconCatalog::from_manifest_str(bad_kind, Path::new(".")),
            Err(CatalogError::UnknownKind { .. })
        ));
        // 重复 ID
        let dup = r#"{"items":[
            {"item_id":"x","display_name":"X","kind":"stratagem","path":"x.png"},
            {"item_id":"x","display_name":"Y","kind":"stratagem","path":"y.png"}]}"#;
        assert!(matches!(
            IconCatalog::from_manifest_str(dup, Path::new(".")),
            Err(CatalogError::DuplicateId { .. })
        ));
        // 空字段
        let empty =
            r#"{"items":[{"item_id":"x","display_name":"","kind":"stratagem","path":"x.png"}]}"#;
        assert!(matches!(
            IconCatalog::from_manifest_str(empty, Path::new(".")),
            Err(CatalogError::EmptyField { .. })
        ));
        // 缺少 items
        assert!(matches!(
            IconCatalog::from_manifest_str("{}", Path::new(".")),
            Err(CatalogError::Parse { .. })
        ));
        let escaping_path = r#"{"items":[{"item_id":"x","display_name":"X","kind":"stratagem","path":"../x.png"}]}"#;
        assert!(matches!(
            IconCatalog::from_manifest_str(escaping_path, Path::new(".")),
            Err(CatalogError::InvalidAssetPath { .. })
        ));
    }

    #[test]
    fn missing_asset_is_an_explicit_error() {
        let c = IconCatalog::from_manifest_str(MANIFEST, Path::new("definitely/not/here"))
            .expect("解析");
        let err = c.validate_assets().expect_err("缺资源必须报错");
        assert!(matches!(err, CatalogError::MissingAsset { .. }));
        assert!(err.message().contains("不存在"));
    }

    #[test]
    fn resolves_current_keys_via_normalization_then_alias() {
        let c = IconCatalog::from_manifest_str(
            r#"{"items":[
              {"item_id":"orbital-laser","display_name":"Orbital Laser","kind":"stratagem","path":"a.png"},
              {"item_id":"meltagun","display_name":"Meltagun","kind":"stratagem","path":"b.png"}
            ]}"#,
            Path::new("."),
        )
        .expect("解析");
        let keys = ["orbital_laser", "40_k_meltagun"];
        // 规范化即可命中
        assert_eq!(
            c.resolve_current_key("orbital-laser", &keys),
            Some("orbital_laser")
        );
        // 需要别名表
        assert_eq!(
            c.resolve_current_key("meltagun", &keys),
            Some("40_k_meltagun")
        );
        // 两侧都没有 → None（不猜）
        assert_eq!(c.resolve_current_key("does-not-exist", &keys), None);
        assert_eq!(c.resolve_current_key("orbital-laser", &[]), None);
    }
}
