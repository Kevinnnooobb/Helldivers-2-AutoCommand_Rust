// 稳定内部 ID（需求 §34）。
//
// 中文名 / 英文名 / 型号 / 别名 全部映射到同一个内部 ID；内部 ID 本身是
// `assets/icons/{key}.png` 的键（英文 snake_case），与 `stratagems::STRATAGEMS.icon` 一致。
// 中文名只是 localization / display name，绝不作为核心 ID。
use crate::stratagems::STRATAGEMS;

/// 英文显示名 → 内部图标键。
///
/// 游戏内显示名常带型号前缀，而资源键不带（"CQC-9 Defoliation Tool" → `defoliation_tool`），
/// 因此不能只靠 `normalize_key` 猜。这里是一份**集中、可测试**的别名表：
/// 左边是 `normalize_key(英文显示名)`，右边是 `assets/icons/{key}.png` 的键。
pub const ENGLISH_ALIASES: &[(&str, &str)] = &[
    // 型号前缀与资源键不一致的战备（其余战备的规范化英文名就等于资源键）
    ("cqc_9_defoliation_tool", "defoliation_tool"),
    ("b_100_portable_hellbomb", "hellbomb_portable"),
    ("b_flam_80_cremator", "cremator"),
    ("cqc_20_breaching_hammer", "cqc_20"),
    ("a_arc_3_tesla_tower", "tesla_tower"),
    ("exo_51_lumberer_exosuit", "lumberer_exosuit"),
    ("exo_55_breakthrough_exosuit", "breakthrough_exosuit"),
    ("exo_45_patriot_exosuit", "patriot_exosuit"),
    ("exo_49_emancipator_exosuit", "emancipator_exosuit"),
];

/// 仅由外部数据源（wiki 强化页 / 插件）提供的 ID。
///
/// 它们不在内置战备表里，因此 `category()` 返回 `None`；但资源键是稳定的
/// 内部 ID，`is_booster()` 必须能正确识别，否则 Booster 槽位会去比较战备模板
/// （反过来战备槽位也会把 Booster 当候选）。这里显式登记，不猜。
pub const EXTERNAL_BOOSTER_IDS: &[&str] = &[
    "experimental_infusion",
    "hellpod_space_optimization",
    "motivational_shocks",
    "muscle_enhancement",
    "stamina_enhancement",
    "vitality_enhancement",
    "increased_reinforcement_budget",
    "flexible_reinforcement_budget",
    "localized_confusion",
    "radar_enhancement",
    "super_stimulants",
    "expert_extraction_pilot",
    "firebomb_hellpods",
    "armed_supply_pods",
    "sample_extraction",
    "democracy_space_station",
    "dead_sprint",
    "faster_extraction",
];

/// 外部强化（Booster）的 `(英文显示名, 内部 ID)`。
///
/// 内置战备表不含强化，这里登记英文显示名，让 `from_display_name` /
/// `display_name` 对强化也和战备一样可用（集中、可测试，不做模糊猜测）。
pub const EXTERNAL_BOOSTER_NAMES: &[(&str, &str)] = &[
    ("Experimental Infusion", "experimental_infusion"),
    ("Hellpod Space Optimization", "hellpod_space_optimization"),
    ("Motivational Shocks", "motivational_shocks"),
    ("Muscle Enhancement", "muscle_enhancement"),
    ("Stamina Enhancement", "stamina_enhancement"),
    ("Vitality Enhancement", "vitality_enhancement"),
    (
        "Increased Reinforcement Budget",
        "increased_reinforcement_budget",
    ),
    (
        "Flexible Reinforcement Budget",
        "flexible_reinforcement_budget",
    ),
    ("Localized Confusion", "localized_confusion"),
    ("Radar Enhancement", "radar_enhancement"),
    ("Super Stimulants", "super_stimulants"),
    ("Expert Extraction Pilot", "expert_extraction_pilot"),
    ("Firebomb Hellpods", "firebomb_hellpods"),
    ("Armed Supply Pods", "armed_supply_pods"),
    ("Sample Extractor", "sample_extraction"),
    ("Dead Sprint", "dead_sprint"),
    ("Faster Extraction", "faster_extraction"),
];

/// 稳定的战备内部 ID。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StratagemId(String);

impl StratagemId {
    /// 从图标键构造（做规范化：小写、空格/连字符 → 下划线）。
    pub fn new(key: &str) -> Self {
        Self(normalize_key(key))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 由显示名（英文名 / 中文名 / 型号 / 图标键）反查内部 ID。
    ///
    /// 查找顺序：图标键 → 英文别名表 → 中文名 → 型号（大小写不敏感，去空白）。
    pub fn from_display_name(name: &str) -> Option<Self> {
        let needle = name.trim();
        if needle.is_empty() {
            return None;
        }
        let key = normalize_key(needle);
        if let Some(found) = STRATAGEMS.iter().find(|s| s.icon == key) {
            return Some(Self(found.icon.to_string()));
        }
        // 英文显示名与图标键经常不同（游戏内名字带型号前缀，资源键不带），
        // 例如 "CQC-9 Defoliation Tool" → `defoliation_tool`。
        // 用集中、可测试的别名表，不做模糊字符串猜测。
        if let Some(icon) = ENGLISH_ALIASES
            .iter()
            .find(|(alias, _)| *alias == key)
            .map(|(_, icon)| *icon)
        {
            return Some(Self(icon.to_string()));
        }
        // 强化（Booster）只有外部数据源提供，按英文显示名映射。
        if let Some(id) = EXTERNAL_BOOSTER_NAMES
            .iter()
            .find(|(name, _)| normalize_key(name) == key)
            .map(|(_, id)| *id)
        {
            return Some(Self(id.to_string()));
        }
        STRATAGEMS
            .iter()
            .find(|s| s.name == needle || s.model.eq_ignore_ascii_case(needle))
            .map(|s| Self(s.icon.to_string()))
    }

    /// 显示名（中文名，来自内置库；外部强化返回其英文名）。
    pub fn display_name(&self) -> Option<&'static str> {
        STRATAGEMS
            .iter()
            .find(|s| s.icon == self.0)
            .map(|s| s.name)
            .or_else(|| {
                EXTERNAL_BOOSTER_NAMES
                    .iter()
                    .find(|(_, id)| *id == self.0)
                    .map(|(name, _)| *name)
            })
    }

    pub fn category(&self) -> Option<&'static str> {
        STRATAGEMS
            .iter()
            .find(|s| s.icon == self.0)
            .map(|s| s.category)
            .or_else(|| {
                // 外部数据源的 Booster 没有内置分类，但它们确实属于 Boosters。
                EXTERNAL_BOOSTER_IDS
                    .contains(&self.0.as_str())
                    .then_some(crate::stratagems::CAT_BOOSTERS)
            })
    }

    /// 是否是 Booster 分类（决定参与比较的模板集合）。
    pub fn is_booster(&self) -> bool {
        if self
            .category()
            .map(|c| c.eq_ignore_ascii_case(crate::stratagems::CAT_BOOSTERS))
            .unwrap_or(false)
        {
            return true;
        }
        // 外部数据源（wiki 强化页 / 插件）提供的 Booster 不在内置表里，
        // 必须靠显式登记识别，否则 Booster 槽位会去比战备模板。
        EXTERNAL_BOOSTER_IDS.contains(&self.0.as_str())
    }
}

impl std::fmt::Display for StratagemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 图标键规范化：`Orbital Precision Strike` → `orbital_precision_strike`。
pub fn normalize_key(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_underscore = false;
    for ch in raw.trim().chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            last_underscore = false;
            Some(ch.to_ascii_lowercase())
        } else if ch == '_' || ch == '-' || ch == ' ' || ch == '.' || ch == '/' {
            if last_underscore {
                None
            } else {
                last_underscore = true;
                Some('_')
            }
        } else {
            None
        };
        if let Some(c) = mapped {
            out.push(c);
        }
    }
    out.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_normalization_is_stable() {
        assert_eq!(
            normalize_key("Orbital Precision Strike"),
            "orbital_precision_strike"
        );
        assert_eq!(normalize_key("MG-43  Machine  Gun"), "mg_43_machine_gun");
        assert_eq!(normalize_key("__weird__name__"), "weird_name");
        assert_eq!(StratagemId::new(" EAT-17 ").as_str(), "eat_17");
    }

    #[test]
    fn display_names_map_to_the_same_internal_id() {
        let by_display = StratagemId::from_display_name("轨道炮攻击").expect("中文名");
        assert_eq!(by_display.as_str(), "orbital_railcannon_strike");
        let by_key = StratagemId::from_display_name("orbital_railcannon_strike").expect("图标键");
        assert_eq!(by_key, by_display);
        assert_eq!(by_key.display_name(), Some("轨道炮攻击"));
        assert_eq!(by_key.category(), Some(crate::stratagems::CAT_ORBITAL));
        // 型号（大小写不敏感）也应映射到同一 ID
        let by_model = StratagemId::from_display_name("gl-21").expect("型号");
        assert_eq!(by_model.as_str(), "grenade_launcher");
    }

    #[test]
    fn unknown_name_has_no_id() {
        assert!(StratagemId::from_display_name("这个战备不存在").is_none());
        assert!(StratagemId::from_display_name("   ").is_none());
    }

    #[test]
    fn booster_flag_follows_category() {
        let booster = StratagemId::from_display_name("补给背包").expect("补给背包");
        assert!(!booster.is_booster());
        // Booster 分类由插件/在线库提供；内置库若含 Boosters 分类则必须被识别
        let any_booster = STRATAGEMS.iter().find(|s| {
            s.category
                .eq_ignore_ascii_case(crate::stratagems::CAT_BOOSTERS)
        });
        if let Some(s) = any_booster {
            assert!(StratagemId::new(s.icon).is_booster());
        }
    }
}
