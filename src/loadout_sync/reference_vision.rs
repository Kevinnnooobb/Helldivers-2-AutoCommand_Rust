//! 参考实现的视觉层类型与常量
//! （`hd2-preset-helper-0.1.4/src/vision/*`、`src/item.rs`）。
//!
//! ## 定位
//!
//! 这是 plan6 §3.1 里 `ReferenceVision` 的**类型层**：`ItemKind`、`SlotKind`、
//! `Classification`、`RoiObservation`。它把参考实现的语义固化成当前项目的类型，
//! 让 `direct_select` 的各个模块有一致的输入输出契约。
//!
//! ## 与当前项目既有 Vision 的关系
//!
//! **不替换** `src/vision/`（那是旁路观察路径，且已通过 115 个测试）。
//! 本模块只是为新状态机提供类型；分类器实现见
//! [`crate::vision::reference_catalog`]（目录）与后续的 classifier 迁移。
//!
//! ## 刻意保留的差异
//!
//! 参考的 `TEMPLATE_BACKGROUND = 30.0`、`LIST_ICON_SCALE = 68/104`、
//! `HOME_STRATAGEM_SCALE = 93/104` 属于**参考分类器的内部口径**，
//! 与当前项目经真实截图验证过的 `WHITE_LUMA_MIN = 120` 不是同一把尺子。
//! 按 plan6 §5 要求，本模块**不引入**这些常量作为可用的分类参数；
//! 替换必须先在真实帧 fixture 上逐槽对照。
use crate::loadout_sync::types::ImageRect;

/// 物品类型：Stratagem 与 Booster **必须分离**。
///
/// 游戏里它们是两个不同的列表；混用会导致在错误的列表里搜索目标。
/// 直接复用参考实现的定义（`crate::item`），避免两套 `ItemKind` 漂移。
pub use crate::item::ItemKind;

/// 槽位类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    /// 战备列表里的槽位
    ListStratagem,
    /// Home 的战备槽
    HomeStratagem,
    /// Home 的 Booster 六边形槽
    HomeBooster,
    /// Booster 列表里的槽位
    ListBooster,
}

impl SlotKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ListStratagem => "list_stratagem",
            Self::HomeStratagem => "home_stratagem",
            Self::HomeBooster => "home_booster",
            Self::ListBooster => "list_booster",
        }
    }

    /// 该槽位是否承载 Booster。
    /// 仅测试与诊断使用。
    #[cfg(test)]
    pub fn is_booster(self) -> bool {
        matches!(self, Self::HomeBooster | Self::ListBooster)
    }

    /// 该槽位是否在列表中（可滚动视口）。
    pub fn is_list(self) -> bool {
        matches!(self, Self::ListStratagem | Self::ListBooster)
    }

    /// 该槽位是否是「可选中该类型物品」的槽位。
    ///
    /// Booster 只能落在 Booster 槽位；战备只能落在战备槽位。
    /// 这条约束是当前项目旧路径里靠隐式约定维持的，这里显式化。
    pub fn is_selectable_item_for(self, kind: ItemKind) -> bool {
        match kind {
            ItemKind::Stratagem => matches!(self, Self::ListStratagem | Self::HomeStratagem),
            ItemKind::Booster => matches!(self, Self::ListBooster | Self::HomeBooster),
        }
    }

    /// 该槽位对应的列表类型。
    /// 仅测试与诊断使用。
    #[cfg(test)]
    pub fn list_kind_for(kind: ItemKind) -> Self {
        match kind {
            ItemKind::Stratagem => Self::ListStratagem,
            ItemKind::Booster => Self::ListBooster,
        }
    }
}

/// 一次分类的结果（参考实现 `Classification`）。
#[derive(Debug, Clone, PartialEq)]
pub struct Classification {
    /// 目录 ID（来自 [`crate::vision::reference_catalog`]）
    pub item_id: String,
    /// 最佳候选的匹配分
    pub match_score: f32,
    /// 最佳与次优的间隔
    pub match_margin: f32,
    /// 几何/门的质量分（0~1）：分割与几何是否可信。
    /// **不是**类别证据，只影响可信度。
    pub gate_quality: f32,
}

impl Classification {
    /// 便利构造（测试与迁移期装配用）。
    #[cfg(test)]
    pub fn new(
        item_id: impl Into<String>,
        match_score: f32,
        match_margin: f32,
        gate_quality: f32,
    ) -> Self {
        Self {
            item_id: item_id.into(),
            match_score,
            match_margin,
            gate_quality,
        }
    }
}

/// 一个已识别的槽位。
#[derive(Debug, Clone, PartialEq)]
pub struct ObservedSlot {
    pub row: u32,
    pub col: u32,
    pub rect: ImageRect,
    pub kind: SlotKind,
    /// 槽位上是否**有内容**。
    ///
    /// * Home：由既有识别器的空槽判据（`cell_is_empty`）给出 —— 它是这条
    ///   判定的唯一权威，本迁移不改它的阈值；
    /// * List：恒为 `true`（网格里报出来的每一格都是真实格子）。
    ///
    /// 与 `classification` 严格区分：`occupied == true, classification == None`
    /// 表示「有东西但认不出」，这种槽位**不得**当作空槽去点击，也**不得**
    /// 当作可确认目标（plan6 §2：Unknown 不驱动输入）。
    pub occupied: bool,
    /// 分类失败时为 `None`（= Unknown）。**绝不用默认 ID 填充**。
    pub classification: Option<Classification>,
}

impl ObservedSlot {
    /// 可参与身份比对的槽位（有分类结果）。
    /// 仅测试与诊断使用。
    #[cfg(test)]
    pub fn is_identity(&self) -> bool {
        self.classification.is_some()
    }

    /// 是否是「可填的空槽」（有位置、没内容）。
    pub fn is_empty(&self) -> bool {
        !self.occupied
    }

    /// 转成 `click_plan` 使用的引用形式。
    pub fn as_slot_ref(&self) -> crate::loadout_sync::direct_select::click_plan::SlotRef {
        crate::loadout_sync::direct_select::click_plan::SlotRef {
            row: self.row,
            col: self.col,
            rect: self.rect,
            selectable: true,
            occupied: self.occupied,
            classification: self.classification.clone(),
        }
    }

    /// 转成 `page_relation` 使用的跟踪形式。
    pub fn as_tracked(&self) -> crate::loadout_sync::direct_select::page_relation::TrackedSlot {
        crate::loadout_sync::direct_select::page_relation::TrackedSlot {
            row: self.row,
            col: self.col,
            rect: self.rect,
            item_id: self.classification.as_ref().map(|c| c.item_id.clone()),
        }
    }
}

/// 一次 ROI 观察的结果（参考实现 `RoiObservation`）。
#[derive(Debug, Clone, PartialEq)]
pub struct RoiObservation {
    pub slots: Vec<ObservedSlot>,
    /// ROI 高度（用于把参考位移折算到当前分辨率）
    pub roi_height: f32,
    /// 帧指纹（[`crate::loadout_sync::direct_select::frame`]）
    pub fingerprint: Vec<u8>,
}

impl RoiObservation {
    pub fn new(slots: Vec<ObservedSlot>, roi_height: f32, fingerprint: Vec<u8>) -> Self {
        Self {
            slots,
            roi_height,
            fingerprint,
        }
    }

    /// 全部可参与身份比对的槽位。
    /// 仅测试与诊断使用。
    #[cfg(test)]
    pub fn classified(&self) -> impl Iterator<Item = &ObservedSlot> {
        self.slots.iter().filter(|s| s.is_identity())
    }

    /// 仅测试与诊断使用。
    #[cfg(test)]
    pub fn classified_count(&self) -> usize {
        self.classified().count()
    }

    /// 是否存在指定 ID 的槽位。
    /// 仅测试与诊断使用。
    #[cfg(test)]
    pub fn contains_item(&self, item_id: &str) -> bool {
        self.classified().any(|s| {
            s.classification
                .as_ref()
                .is_some_and(|c| c.item_id == item_id)
        })
    }

    /// 转成 `page_relation` 的跟踪输入。
    pub fn tracked(&self) -> Vec<crate::loadout_sync::direct_select::page_relation::TrackedSlot> {
        self.slots.iter().map(|s| s.as_tracked()).collect()
    }

    /// 转成 `click_plan` 的槽位输入。
    pub fn slot_refs(&self) -> Vec<crate::loadout_sync::direct_select::click_plan::SlotRef> {
        self.slots.iter().map(|s| s.as_slot_ref()).collect()
    }
}

/// 界面状态（参考实现 `UiState`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiState {
    /// Home 且还有空槽
    HomeEmpty,
    /// Home 且已填满
    HomeFilled,
    /// 战备列表
    StratagemList,
    /// Booster 列表
    BoosterList,
}

impl UiState {
    pub fn label(self) -> &'static str {
        match self {
            Self::HomeEmpty => "home_empty",
            Self::HomeFilled => "home_filled",
            Self::StratagemList => "stratagem_list",
            Self::BoosterList => "booster_list",
        }
    }

    /// 仅测试与诊断使用。
    #[cfg(test)]
    pub fn is_home(self) -> bool {
        matches!(self, Self::HomeEmpty | Self::HomeFilled)
    }

    /// 该状态对应的列表类型（Home 时没有列表）。
    /// 仅测试与诊断使用。
    #[cfg(test)]
    pub fn list_kind(self) -> Option<ItemKind> {
        match self {
            Self::StratagemList => Some(ItemKind::Stratagem),
            Self::BoosterList => Some(ItemKind::Booster),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_kind_separates_stratagem_and_booster() {
        assert!(SlotKind::ListStratagem.is_selectable_item_for(ItemKind::Stratagem));
        assert!(!SlotKind::ListStratagem.is_selectable_item_for(ItemKind::Booster));
        assert!(SlotKind::ListBooster.is_selectable_item_for(ItemKind::Booster));
        assert!(!SlotKind::ListBooster.is_selectable_item_for(ItemKind::Stratagem));
        assert!(SlotKind::HomeBooster.is_selectable_item_for(ItemKind::Booster));
        assert!(SlotKind::HomeBooster.is_booster());
        assert!(SlotKind::ListBooster.is_booster());
        assert!(!SlotKind::HomeStratagem.is_booster());
    }

    #[test]
    fn list_kind_maps_from_item_kind() {
        assert_eq!(
            SlotKind::list_kind_for(ItemKind::Stratagem),
            SlotKind::ListStratagem
        );
        assert_eq!(
            SlotKind::list_kind_for(ItemKind::Booster),
            SlotKind::ListBooster
        );
        assert_eq!(
            UiState::StratagemList.list_kind(),
            Some(ItemKind::Stratagem)
        );
        assert_eq!(UiState::BoosterList.list_kind(), Some(ItemKind::Booster));
        assert_eq!(UiState::HomeFilled.list_kind(), None);
        assert!(UiState::HomeEmpty.is_home());
        assert!(UiState::HomeFilled.is_home());
        assert!(!UiState::StratagemList.is_home());
    }

    #[test]
    fn unknown_slots_stay_none_and_do_not_block_identity_checks() {
        let unknown = ObservedSlot {
            row: 0,
            col: 0,
            rect: ImageRect::new(0, 0, 10, 10),
            kind: SlotKind::ListStratagem,
            occupied: true,
            classification: None,
        };
        assert!(!unknown.is_identity());
        let obs = RoiObservation::new(vec![unknown], 832.0, vec![]);
        assert_eq!(obs.classified_count(), 0);
        assert!(!obs.contains_item("anything"));
        // 未分类槽位仍然出现在 tracked 里，但 item_id 为 None
        assert_eq!(obs.tracked().len(), 1);
        assert!(obs.tracked()[0].item_id.is_none());
    }

    #[test]
    fn observation_reports_classified_items() {
        let s = |id: &str, row: u32| ObservedSlot {
            row,
            col: 0,
            rect: ImageRect::new(0, row as i32 * 90, 80, 80),
            kind: SlotKind::ListStratagem,
            occupied: true,
            classification: Some(Classification::new(id, 0.8, 0.1, 1.0)),
        };
        let obs = RoiObservation::new(
            vec![s("machine-gun", 0), s("autocannon", 1)],
            832.0,
            vec![1],
        );
        assert_eq!(obs.classified_count(), 2);
        assert!(obs.contains_item("autocannon"));
        assert!(!obs.contains_item("railgun"));
        assert_eq!(obs.slot_refs().len(), 2);
        assert_eq!(obs.tracked()[1].item_id.as_deref(), Some("autocannon"));
    }

    #[test]
    fn empty_and_unknown_slots_are_different_states() {
        let home = |occupied: bool| ObservedSlot {
            row: 0,
            col: 0,
            rect: ImageRect::new(0, 0, 80, 80),
            kind: SlotKind::HomeStratagem,
            occupied,
            classification: None,
        };
        let empty = home(false);
        let unknown = home(true);
        assert!(empty.is_empty());
        assert!(!empty.is_identity());
        // 有内容但认不出（Mod 图标 / 分割失败）既不是空槽，也不是可确认目标
        assert!(!unknown.is_empty());
        assert!(!unknown.is_identity());
        // 可填性必须原样传到 `click_plan` 的视图里，否则会点开已填槽位
        assert!(unknown.as_slot_ref().occupied);
        assert!(!empty.as_slot_ref().occupied);
    }
}
