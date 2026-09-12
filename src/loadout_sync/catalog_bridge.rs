//! S1 适配层 —— H2AC 图标键 ↔ 参考目录 `item_id`。
//!
//! ## 为什么需要这一层
//!
//! 参考实现（`hd2-preset-helper-0.1.4`）没有这一层：它的槽位目标**就是**目录
//! `item_id`。而 H2AC 的槽位保存的是自己的图标键（`assets/icons/{key}.png`
//! 的文件名，snake_case），两者必须先桥接，才能把 preset 翻译成状态机目标。
//!
//! ## 桥的三条规则（不猜、不模糊匹配）
//!
//! 1. **规范化**：`item_id.replace('-', "_")`，命中即用；
//! 2. **显式别名表**：仅登记规范化命不中的条目（参考与 H2AC 命名不同）；
//! 3. **命不中就是 `None`**，由调用方按「目录缺失」拒答。
//!
//! 参考 manifest **不含任何任务战备**，所以含任务战备的 preset 在普通战备列表里
//! 永远搜不到 —— 这是**规则 3 必须存在**的原因，不允许退化成"滚动到找到为止"。
//!
//! ## 目录数据来源
//!
//! 一律来自参考 [`crate::assets::IconCatalog`]（`assets/reference/icons` 内嵌资产），
//! 本模块**不带自己的目录、不读磁盘、不做资产校验**：那些是参考 `assets.rs` 的职责。

use crate::assets::{IconCatalog, IconEntry};
use crate::item::ItemKind;

/// 参考 `item_id` → H2AC 图标键的显式别名表。
///
/// 迁移自 `vision/reference_catalog.rs`（原 `REFERENCE_ID_ALIASES`），逐条保留：
/// * 参考项目有、H2AC 键名不同（例如参考 `meltagun`、H2AC `40_k_meltagun`）；
/// * 参考项目没有、H2AC 有的任务战备（`hellbomb` / `solo-silo` / `seaf-artillery`）
///   —— 它们**不在参考目录里**，因此 `resolve_item_id` 对它们仍返回 `None`，
///   保留在此只为让"为什么没命中"可读。
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
];

/// `item_id` → 规范化后的 H2AC 图标键。
///
/// 只是规范化猜测，不保证命中；可靠对应关系由 [`resolve_current_key`] 给出。
pub fn normalized_current_key(item_id: &str) -> String {
    item_id.replace('-', "_")
}

/// 目录中某类型的全部条目，按 `item_id` **升序**。
///
/// 参考 `IconCatalog` 内部是 `HashMap`，迭代顺序每次进程启动都不同；
/// 而调用方（分类器的 `resolved` / `items` 平行数组、打分并列时的下标比较）
/// 依赖顺序稳定，因此这里显式排序，保持与既有 `BTreeMap` 版目录一致的确定性。
pub fn entries_of_kind<'a>(
    catalog: &'a IconCatalog,
    kind: ItemKind,
) -> Vec<(&'a str, &'a IconEntry)> {
    let mut entries: Vec<(&str, &IconEntry)> = catalog
        .iter()
        .filter(|(_, entry)| entry.kind == kind)
        .collect();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    entries
}

/// 参考 `item_id` → H2AC 图标键。
///
/// 两步：先按规范化猜（`-` → `_`），命中即返回；否则查显式别名表。
/// * `current_keys` 通常是 `icons::all_icon_keys()`；
/// * 目录里没有该 `item_id`、或两侧都命不中 → `None`（**不猜**）。
pub fn resolve_current_key<'a>(
    catalog: &IconCatalog,
    item_id: &str,
    current_keys: &'a [&'a str],
) -> Option<&'a str> {
    catalog.get(item_id)?;
    let candidate = normalized_current_key(item_id);
    if let Some(hit) = current_keys.iter().find(|key| **key == candidate) {
        return Some(hit);
    }
    REFERENCE_ID_ALIASES
        .iter()
        .find(|(reference, _)| *reference == item_id)
        .and_then(|(_, current)| current_keys.iter().find(|key| **key == *current).copied())
}

/// H2AC 图标键 → 参考 `item_id`（[`resolve_current_key`] 的逆查）。
///
/// 用于把 preset 里的图标键翻译成状态机使用的目录 ID。
/// 多个 `item_id` 规范化后撞到同一个键时取**字典序最小**者，保证结果稳定。
pub fn resolve_item_id<'a>(catalog: &'a IconCatalog, current_key: &str) -> Option<&'a str> {
    let mut ids: Vec<&str> = catalog.iter().map(|(item_id, _)| item_id).collect();
    ids.sort_unstable();
    if let Some(item_id) = ids
        .into_iter()
        .find(|item_id| normalized_current_key(item_id) == current_key)
    {
        return Some(item_id);
    }
    REFERENCE_ID_ALIASES
        .iter()
        .find(|(_, current)| *current == current_key)
        .and_then(|(reference, _)| catalog.get(reference).map(|_| *reference))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{IconCatalog, default_icon_manifest};

    fn catalog() -> IconCatalog {
        IconCatalog::load(default_icon_manifest()).expect("内嵌参考目录必须可用")
    }

    #[test]
    fn normalization_resolves_matching_names() {
        let catalog = catalog();
        let keys = ["orbital_laser", "eagle_airstrike"];
        assert_eq!(
            resolve_current_key(&catalog, "orbital-laser", &keys),
            Some("orbital_laser")
        );
        assert_eq!(
            resolve_current_key(&catalog, "eagle-airstrike", &keys),
            Some("eagle_airstrike")
        );
    }

    #[test]
    fn aliases_resolve_renamed_entries() {
        let catalog = catalog();
        let keys = ["40_k_meltagun", "guard_dog_k_9"];
        assert_eq!(
            resolve_current_key(&catalog, "meltagun", &keys),
            Some("40_k_meltagun")
        );
        assert_eq!(resolve_current_key(&catalog, "k-9", &keys), Some("guard_dog_k_9"));
    }

    #[test]
    fn mission_stratagems_are_absent_and_never_guessed() {
        let catalog = catalog();
        let keys = ["reinforce", "resupply", "hellbomb"];
        // 参考 manifest 不含任务战备 → 目录里没有、逆查返回 None
        assert_eq!(resolve_current_key(&catalog, "reinforce", &keys), None);
        assert_eq!(resolve_item_id(&catalog, "reinforce"), None);
        assert_eq!(resolve_item_id(&catalog, "hellbomb"), None);
        assert_eq!(resolve_current_key(&catalog, "does-not-exist", &keys), None);
    }

    #[test]
    fn reverse_resolution_is_deterministic_and_covers_both_kinds() {
        let catalog = catalog();
        assert_eq!(
            resolve_item_id(&catalog, "orbital_laser"),
            Some("orbital-laser")
        );
        assert_eq!(resolve_item_id(&catalog, "40_k_meltagun"), Some("meltagun"));
        // 两次调用必须一致（参考目录是 HashMap，未排序时顺序会漂移）
        let first: Vec<&str> = catalog.iter().map(|(id, _)| id).collect();
        let second: Vec<&str> = catalog.iter().map(|(id, _)| id).collect();
        assert_eq!(
            {
                let mut a = first;
                a.sort_unstable();
                a
            },
            {
                let mut b = second;
                b.sort_unstable();
                b
            }
        );
        assert_eq!(entries_of_kind(&catalog, ItemKind::Booster).len(), 18);
        assert_eq!(entries_of_kind(&catalog, ItemKind::Stratagem).len(), 92);
    }
}
