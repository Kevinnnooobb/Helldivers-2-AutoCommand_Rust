// Loadout Sync 选择模型 —— H2AC 下排槽位（Slot 06~10）到游戏 Loadout 的映射。
//
// 业务规则（唯一权威定义）：
//   Slot 06 (index 5) → 游戏 Stratagem Slot 1
//   Slot 07 (index 6) → 游戏 Stratagem Slot 2
//   Slot 08 (index 7) → 游戏 Stratagem Slot 3
//   Slot 09 (index 8) → 游戏 Stratagem Slot 4
//   Slot 10 (index 9) → 游戏 Booster Slot
//
// 上排 Slot 01~05 完全不参与本功能。不做自动左移、不做排序、不做「猜测用户意图」。
use std::collections::HashMap;

use crate::loadout_sync::error::LoadoutSyncError;
use crate::stratagems::{PluginStratagem, PLUGIN_SLOT_MARK, STRATAGEMS};

/// 游戏侧战备槽位数（固定 4）。
pub const GAME_STRATAGEM_SLOTS: usize = 4;
/// H2AC 下排起点：Slot 06 的 index。
pub const LOADOUT_FIRST_SLOT: usize = 5;
/// H2AC Booster 槽：Slot 10 的 index。
pub const BOOSTER_SLOT: usize = 9;

/// 一个 Loadout 目标项 —— 复用 H2AC 现有战备身份（内置索引或插件名），
/// 不新建第二套战备数据库。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadoutItem {
    pub name: String,
    /// 图标资源键（对应 assets/icons/{icon}.png），视觉匹配的模板来源。
    pub icon: String,
    /// 内置战备在 STRATAGEMS 中的索引；插件战备为 None。
    pub base_index: Option<usize>,
}

impl LoadoutItem {
    pub fn base(index: usize) -> Option<Self> {
        STRATAGEMS.get(index).map(|s| Self {
            name: s.name.to_string(),
            icon: s.icon.to_string(),
            base_index: Some(index),
        })
    }

    pub fn plugin(p: &PluginStratagem) -> Self {
        Self {
            name: p.name.clone(),
            icon: p.icon.clone(),
            base_index: None,
        }
    }

    /// 身份键：用于重复检测（同名插件与内置战备视为不同项）。
    pub fn key(&self) -> String {
        match self.base_index {
            Some(i) => format!("base:{i}"),
            None => format!("plugin:{}", self.name),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoadoutSyncSelection {
    pub stratagems: [Option<LoadoutItem>; GAME_STRATAGEM_SLOTS],
    /// Booster 可选：为空时跳过 Booster 阶段。
    pub booster: Option<LoadoutItem>,
}

impl LoadoutSyncSelection {
    /// 唯一职责：读取 H2AC 下排 Slot 06~10（index 5..=9），转换成 S1/S2/S3/S4/Booster。
    ///
    /// 越界索引、非法插件哨兵一律按「未配置」处理（不 panic、不猜测）。
    pub fn from_slots(
        slots: &[Option<usize>],
        plugin_slots: &HashMap<usize, PluginStratagem>,
    ) -> Self {
        let mut selection = Self::default();
        for i in 0..GAME_STRATAGEM_SLOTS {
            selection.stratagems[i] = Self::read_slot(slots, plugin_slots, LOADOUT_FIRST_SLOT + i);
        }
        selection.booster = Self::read_slot(slots, plugin_slots, BOOSTER_SLOT);
        selection
    }

    fn read_slot(
        slots: &[Option<usize>],
        plugin_slots: &HashMap<usize, PluginStratagem>,
        index: usize,
    ) -> Option<LoadoutItem> {
        let raw = slots.get(index).copied().flatten()?;
        if raw == PLUGIN_SLOT_MARK {
            return plugin_slots.get(&index).map(LoadoutItem::plugin);
        }
        LoadoutItem::base(raw)
    }

    /// 本地校验：4 个 Stratagem 必须全部配置，Booster 可空，禁止重复。
    pub fn validate(&self) -> Result<(), LoadoutSyncError> {
        for (i, item) in self.stratagems.iter().enumerate() {
            if item.is_none() {
                return Err(LoadoutSyncError::InvalidSelection {
                    detail: format!("Loadout Stratagem Slot {} 未配置。", i + 1),
                });
            }
        }
        for i in 0..GAME_STRATAGEM_SLOTS {
            for j in (i + 1)..GAME_STRATAGEM_SLOTS {
                let (a, b) = (
                    self.stratagems[i].as_ref().expect("已校验非空"),
                    self.stratagems[j].as_ref().expect("已校验非空"),
                );
                if a.key() == b.key() {
                    return Err(LoadoutSyncError::InvalidSelection {
                        detail: format!(
                            "存在重复 Stratagem（{} 与 {}），无法创建有效 Loadout。",
                            a.name, b.name
                        ),
                    });
                }
            }
        }
        if let Some(b) = &self.booster {
            if b.icon.trim().is_empty() {
                return Err(LoadoutSyncError::InvalidSelection {
                    detail: "Booster 缺少图标资源，无法进行视觉匹配。".into(),
                });
            }
        }
        Ok(())
    }

    pub fn stratagem(&self, index: usize) -> Option<&LoadoutItem> {
        self.stratagems.get(index).and_then(|s| s.as_ref())
    }

    pub fn stratagems_iter(&self) -> impl Iterator<Item = (usize, &LoadoutItem)> {
        self.stratagems
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|s| (i, s)))
    }

    pub fn filled_stratagem_count(&self) -> usize {
        self.stratagems.iter().filter(|s| s.is_some()).count()
    }

    /// 全部目标（4 战备 + 可选 Booster），按 GUI 顺序。
    pub fn targets(&self) -> Vec<(TargetSlot, &LoadoutItem)> {
        let mut v: Vec<(TargetSlot, &LoadoutItem)> = self
            .stratagems_iter()
            .map(|(i, item)| (TargetSlot::Stratagem(i), item))
            .collect();
        if let Some(b) = &self.booster {
            v.push((TargetSlot::Booster, b));
        }
        v
    }
}

/// 目标在游戏侧的位置语义（不是屏幕坐标）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetSlot {
    /// 游戏 Stratagem Slot 1~4（0-based）
    Stratagem(usize),
    Booster,
}

impl TargetSlot {
    /// GUI 进度显示用的序号（1~5）。
    pub fn ordinal(self) -> usize {
        match self {
            Self::Stratagem(i) => i + 1,
            Self::Booster => GAME_STRATAGEM_SLOTS + 1,
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Stratagem(i) => format!("SLOT {}/5", i + 1),
            Self::Booster => "BOOSTER".to_string(),
        }
    }
}
