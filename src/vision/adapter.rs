// 视觉层 → 自动化层的适配器（需求 §33 / §35）。
//
// 边界约束：
//   * 视觉层只输出 `StratagemDetection`（ID + confidence + bbox + slot + top-k）；
//   * 本模块只做「识别结果 → 现有 Loadout 数据结构」的转换，
//     **不**按键、**不**点击、**不**修改任何 loadout 状态；
//   * 因此新识别管线可以独立替换/回退，旧 `loadout_sync` 自动化完全不受影响。
//
// plan6 §5.3 之后，装配路径不再调用本适配器（controller 只走 `direct_select`）；
// 这里保留「识别结果 → LoadoutItem」的纯转换契约，供单元测试与后续工具复用。
#![allow(dead_code)]
use crate::loadout_sync::selection::{LoadoutItem, LoadoutSyncSelection, GAME_STRATAGEM_SLOTS};
use crate::stratagems::STRATAGEMS;

use super::id::StratagemId;
#[cfg(test)]
use super::recognize::DetectionOutcome;
use super::recognize::{LoadoutRecognition, StratagemDetection};
/// `StratagemId` → 现有 `LoadoutItem`（保持 H2AC 的身份语义：内置索引或插件名）。
pub fn item_for(id: &StratagemId) -> LoadoutItem {
    let key = id.as_str();
    match STRATAGEMS.iter().position(|s| s.icon == key) {
        Some(index) => LoadoutItem {
            name: STRATAGEMS[index].name.to_string(),
            icon: key.to_string(),
            base_index: Some(index),
        },
        None => LoadoutItem {
            name: id.display_name().unwrap_or(key).to_string(),
            icon: key.to_string(),
            base_index: None,
        },
    }
}

/// 单槽识别结果 → `LoadoutItem`（未识别 / 空格 / 弃权 → None）。
pub fn item_for_detection(det: &StratagemDetection) -> Option<LoadoutItem> {
    det.outcome.id().map(item_for)
}

/// 整次识别 → `LoadoutSyncSelection`（只填「已识别」的槽位，其余保持 None）。
///
/// 语义上这是「游戏当前战备栏」的读取结果，可以直接交给现有 loadout manager 做
/// 对比、校验或预设导入（需求 §35 的 adapter）。
pub fn selection_from_recognition(rec: &LoadoutRecognition) -> LoadoutSyncSelection {
    let mut selection = LoadoutSyncSelection::default();
    for det in rec.stratagems() {
        if det.slot >= GAME_STRATAGEM_SLOTS {
            continue;
        }
        selection.stratagems[det.slot] = item_for_detection(det);
    }
    selection.booster = rec.booster().and_then(item_for_detection);
    selection
}

/// 该结果是否包含任何弃权（上层可据此决定是否重试/回退到旧匹配器）。
#[cfg(test)]
pub fn has_abstention(rec: &LoadoutRecognition) -> bool {
    rec.detections
        .iter()
        .any(|d| matches!(d.outcome, DetectionOutcome::Unknown { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadout_sync::types::ImageRect;
    use crate::vision::grid::SlotKind;
    use crate::vision::recognize::{Alternative, DetectionOutcome, RecognitionMethod};
    use crate::vision::template::MethodScores;

    fn detection(slot: usize, id: Option<&str>) -> StratagemDetection {
        let outcome = match id {
            Some(key) => DetectionOutcome::Recognized {
                id: StratagemId::new(key),
                confidence: 0.9,
                method: RecognitionMethod::Template,
                template_score: 0.9,
                margin: 0.1,
                geometry_score: 1.0,
                methods: MethodScores::default(),
                alternatives: Vec::<Alternative>::new(),
            },
            None => DetectionOutcome::Empty,
        };
        StratagemDetection {
            slot,
            row: 0,
            col: slot as u32,
            kind: if slot == 4 {
                SlotKind::Booster
            } else {
                SlotKind::Stratagem
            },
            cell: ImageRect::new(0, 0, 100, 100),
            inner: ImageRect::new(10, 10, 80, 80),
            bbox: None,
            outcome,
            foreground_ratio: 0.2,
            components_kept: 1,
        }
    }

    fn recognition(detections: Vec<StratagemDetection>) -> LoadoutRecognition {
        LoadoutRecognition {
            screen: (2560, 1440),
            roi: ImageRect::new(64, 480, 576, 832),
            plan: crate::vision::grid::SlotPlanKind::Home,
            geometry_source: crate::vision::grid::GeometrySource::Verified,
            detections,
            elapsed_ms: 1,
            from_cache: false,
        }
    }

    #[test]
    fn detection_maps_to_existing_loadout_item() {
        let id = StratagemId::new("grenade_launcher");
        let item = item_for(&id);
        assert_eq!(item.icon, "grenade_launcher");
        assert_eq!(item.name, "榴弹发射器");
        assert!(item.base_index.is_some());
        assert!(item.key().starts_with("base:"));
    }

    #[test]
    fn icon_keys_without_a_stratagem_entry_still_map_to_items() {
        // 图标库（assets/icons）比 STRATAGEMS 更大：部分图标键没有内置战备条目。
        // 这类 ID 必须仍能转成 LoadoutItem（无内置索引），而不是丢结果。
        //
        // 断言用的是「当前确实没有内置条目」的键，因此先从图标库动态挑一个，
        // 避免以后把该键补进 STRATAGEMS 时测试变成假的失败。
        let orphan = crate::icons::all_icon_keys()
            .into_iter()
            .find(|key| {
                !crate::stratagems::STRATAGEMS.iter().any(|s| s.icon == *key)
                    && crate::vision::id::StratagemId::new(key)
                        .display_name()
                        .is_none()
            })
            .expect("图标库应当至少有一个没有内置条目的键");
        let id = StratagemId::new(orphan);
        assert!(id.display_name().is_none());
        let item = item_for(&id);
        assert_eq!(item.icon, orphan);
        assert!(item.base_index.is_none());
    }

    #[test]
    fn unknown_id_still_produces_an_item_without_base_index() {
        let id = StratagemId::new("some_modded_stratagem");
        let item = item_for(&id);
        assert_eq!(item.icon, "some_modded_stratagem");
        assert!(item.base_index.is_none());
        assert!(item.key().starts_with("plugin:"));
    }

    #[test]
    fn selection_is_built_from_recognized_slots_only() {
        let rec = recognition(vec![
            detection(0, Some("orbital_precision_strike")),
            detection(1, None),
            detection(2, Some("eagle_airstrike")),
            detection(3, Some("railgun")),
        ]);
        let selection = selection_from_recognition(&rec);
        assert_eq!(
            selection.stratagems[0].as_ref().map(|i| i.icon.as_str()),
            Some("orbital_precision_strike")
        );
        assert!(selection.stratagems[1].is_none(), "未识别槽位必须保持空");
        assert_eq!(selection.filled_stratagem_count(), 3);
        assert!(selection.booster.is_none());
    }

    #[test]
    fn booster_maps_to_the_booster_field() {
        let rec = recognition(vec![detection(4, Some("stamina_enhancement"))]);
        let selection = selection_from_recognition(&rec);
        assert!(selection.booster.is_some());
        assert_eq!(selection.filled_stratagem_count(), 0);
    }

    #[test]
    fn abstention_is_visible_to_the_caller() {
        let mut det = detection(0, None);
        det.outcome = DetectionOutcome::Unknown {
            reason: crate::vision::recognize::UnknownReason::LowConfidence,
            best: Some(StratagemId::new("railgun")),
            best_score: 0.4,
            margin: 0.01,
        };
        let rec = recognition(vec![det]);
        assert!(has_abstention(&rec));
        assert!(selection_from_recognition(&rec).stratagems[0].is_none());
    }
}
