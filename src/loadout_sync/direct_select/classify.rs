//! 目录键控的分类器（参考实现 `vision/classifier.rs` 的**适配层**）。
//!
//! ## 它做什么
//!
//! 参考实现的状态机要求每个槽位的分类结果带**目录 ID**
//! （`Classification.item_id`）。当前项目的 `loadout_sync::matcher::IconMatcher`
//! 是按**图标键**打分的，两者需要一个明确的桥。
//!
//! 本模块就是这座桥，且**不重复实现打分**：
//!
//! 1. 用 `IconCatalog` 的 `item_id` → 当前图标键（规范化 + 显式别名表）；
//! 2. 图标键 → 复用既有的 `IconMatcher::score_cell_for` 拿分数；
//! 3. 组成本模块的 `Classification { item_id, match_score, match_margin, gate_quality }`。
//!
//! ## 门质量（`gate_quality`）
//!
//! 参考实现把「几何/分割是否可信」与「像哪个图标」分开：
//! 前者只影响**可信度**，不能把不达标的候选抬过门槛。
//! 这里沿用同一语义：前景包围盒不合理（太小/几乎占满格子）或前景过少时
//! `gate_quality` 降为 0，调用方据此拒绝目标。
//!
//! ## 迁移期定位
//!
//! 这是 plan6 §3.1 `ReferenceVision` 的分类部分。参考的
//! `TemplateClassifier`（961 行，像素层灰度/梯度相关）**尚未迁移**；
//! 在那之前，本适配层让新状态机能真正跑起来，同时**不改变**既有的打分实现
//! （不引入未经真实帧验证的阈值）。
use crate::loadout_sync::matcher::{CellKind, IconMatcher};
use crate::loadout_sync::reference_vision::{Classification, ItemKind};
use crate::loadout_sync::selection::LoadoutItem;
use crate::loadout_sync::types::ImageRect;
use crate::vision::reference_catalog::{IconCatalog, ItemKind as CatalogItemKind};

/// 图标键 → 目录 ID 的桥。
pub struct CatalogClassifier {
    catalog: IconCatalog,
    matcher: IconMatcher,
    /// 目录 `item_id` → 当前项目图标键（只含可解析的）
    resolved: Vec<(String, String)>,
    /// 目录 `item_id` → `LoadoutItem`（喂给 matcher）
    items: Vec<LoadoutItem>,
    /// 无法解析到当前图标键的目录条目（诊断用，不静默）
    pub unresolved: Vec<String>,
}

impl CatalogClassifier {
    /// 由目录构建分类器。
    ///
    /// 只加载能解析到当前图标键的条目；其余记录在 [`Self::unresolved`]。
    pub fn load(catalog: IconCatalog) -> Self {
        let keys = crate::icons::all_icon_keys();
        let mut resolved: Vec<(String, String)> = Vec::new();
        let mut unresolved: Vec<String> = Vec::new();
        for entry in catalog
            .by_kind(CatalogItemKind::Stratagem)
            .chain(catalog.by_kind(CatalogItemKind::Booster))
        {
            match catalog.resolve_current_key(&entry.item_id, &keys) {
                Some(key) => resolved.push((entry.item_id.clone(), key.to_string())),
                None => unresolved.push(entry.item_id.clone()),
            }
        }
        let items: Vec<LoadoutItem> = resolved
            .iter()
            .map(|(id, key)| LoadoutItem {
                name: id.clone(),
                icon: key.clone(),
                base_index: None,
            })
            .collect();
        let refs: Vec<&LoadoutItem> = items.iter().collect();
        let matcher = IconMatcher::for_targets_only(&refs);
        Self {
            catalog,
            matcher,
            resolved,
            items,
            unresolved,
        }
    }

    /// 可分类的目录条目数。
    pub fn classifiable(&self) -> usize {
        self.items.len()
    }

    /// preset 图标键 → 目录 `item_id`（状态机的目标标识）。
    ///
    /// 只在**双向都能解析**时返回：必须也在 `resolved` 表里，
    /// 否则会出现「目标认得出来但选不中」的不一致。
    pub fn catalog_id_for_icon(&self, icon_key: &str) -> Option<&str> {
        let item_id = self.catalog.resolve_item_id(icon_key)?;
        self.resolved
            .iter()
            .any(|(resolved_id, _)| resolved_id == item_id)
            .then_some(item_id)
    }

    /// Home 槽位的分类（用 `HomeStratagem` / `HomeBooster` 的资源渲染比例）。
    ///
    /// 与列表必须区分：`HOME_GLYPH_SCALE` 与 `LIST_GLYPH_SCALE` 不同，
    /// 用错比例会让同一个槽位得到不同的分数（实测差 0.03~0.05）。
    pub fn classify_cell_home(
        &self,
        frame: &crate::loadout_sync::capture::CapturedFrame,
        cell: ImageRect,
        kind: ItemKind,
    ) -> Option<Classification> {
        self.classify_cell_with(frame, cell, home_cell_kind_for(kind))
    }

    /// 把一格的打分结果汇总成目录 ID 的 `Classification`。
    ///
    /// `gate_quality`：前景包围盒占格子比例落在可信区间内且前景像素足够 → 1.0，
    /// 否则 0.0（调用方据此拒绝，而不是让外观分把它抬过去）。
    pub fn classify_cell(
        &self,
        frame: &crate::loadout_sync::capture::CapturedFrame,
        cell: ImageRect,
        kind: ItemKind,
    ) -> Option<Classification> {
        self.classify_cell_with(frame, cell, cell_kind_for(kind))
    }

    fn classify_cell_with(
        &self,
        frame: &crate::loadout_sync::capture::CapturedFrame,
        cell: ImageRect,
        cell_kind: CellKind,
    ) -> Option<Classification> {
        let masks = self.matcher.cell_masks_pub(frame, cell)?;
        if masks.is_empty_like() {
            return None;
        }
        let gate_quality = gate_quality(&masks);

        let mut scored: Vec<(usize, f32)> = Vec::with_capacity(self.items.len());
        for (index, item) in self.items.iter().enumerate() {
            if let Some(score) = self.matcher.score_cell_for(frame, cell, item, cell_kind) {
                if score.is_finite() {
                    scored.push((index, score));
                }
            }
        }
        if scored.is_empty() {
            return None;
        }
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        let (best_index, best_score) = scored[0];
        let second = scored.get(1).map(|(_, s)| *s).unwrap_or(0.0);
        let item_id = self.resolved.get(best_index)?.0.clone();
        Some(Classification {
            item_id,
            match_score: best_score,
            match_margin: (best_score - second).max(0.0),
            gate_quality,
        })
    }
}

/// `ItemKind` → `CellKind`（列表与 Home 用不同的资源渲染比例）。
pub fn cell_kind_for(kind: ItemKind) -> CellKind {
    match kind {
        ItemKind::Stratagem => CellKind::ListStratagem,
        ItemKind::Booster => CellKind::ListBooster,
    }
}

/// Home 界面的 `CellKind`。
pub fn home_cell_kind_for(kind: ItemKind) -> CellKind {
    match kind {
        ItemKind::Stratagem => CellKind::HomeStratagem,
        ItemKind::Booster => CellKind::HomeBooster,
    }
}

/// 门质量：前景包围盒占比可信且前景像素足够 → 1.0，否则 0.0。
///
/// 阈值复用既有的 `MIN_CARD_RATIO` / `MAX_CARD_RATIO` 语义，
/// 不引入新的魔法数字。
fn gate_quality(masks: &crate::loadout_sync::matcher::CellMasks) -> f32 {
    if masks.is_empty_like() {
        return 0.0;
    }
    let Some((x0, y0, x1, y1)) = masks.bbox() else {
        return 0.0;
    };
    let w = (x1 - x0 + 1) as f32 / masks.width().max(1) as f32;
    let h = (y1 - y0 + 1) as f32 / masks.height().max(1) as f32;
    let ok = (0.10..=0.98).contains(&w) && (0.10..=0.98).contains(&h);
    if ok {
        1.0
    } else {
        0.0
    }
}
