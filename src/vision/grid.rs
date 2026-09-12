// 网格与槽位（需求 §6）。
//
// 职责边界：
//   * 几何负责回答「在哪里」——本模块只把几何检测器的结果翻译成「槽位计划」，
//     不做分割、不做分类、不做任何视觉判断；
//   * 主路径 = `geometry::detect_home` / `detect_list` 的**固定锚点 + 边线验证**结果；
//   * 回退路径 = 纯标定坐标（明确标记为未验证，分数为 0），只在前者验证失败且
//     配置允许回退时使用，调用方可以据此拒绝结果。
use crate::loadout_sync::types::ImageRect;

use super::calibration::{FrameGeometry, StratagemLayout};
use super::error::VisionError;
use super::geometry::{DetectedKind, GeometryDetection};

/// home 界面的战备槽数量（游戏固定 4 个）。
pub const HOME_STRATAGEM_SLOTS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    /// 战备槽（矩形）
    Stratagem,
    /// Booster 槽（六边形）
    Booster,
}

impl SlotKind {
    pub fn is_booster(self) -> bool {
        matches!(self, Self::Booster)
    }

    /// 人类可读标签（仅测试与诊断使用）。
    #[cfg(test)]
    pub fn label(self) -> &'static str {
        match self {
            Self::Stratagem => "Stratagem",
            Self::Booster => "Booster",
        }
    }
}

/// 一个槽位（帧像素坐标）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    /// 在该版面内从 0 开始的序号（home：0..=3 战备，4 = Booster）。
    pub index: usize,
    pub row: u32,
    pub col: u32,
    pub kind: SlotKind,
    /// 槽位外框（帧像素）。
    pub rect: ImageRect,
    /// 边线验证分数（0~1）。0 表示该矩形来自纯标定、未经边线验证。
    pub score: f32,
    /// home 内容检查结果（几何层的独立证据，仅供参考；空/非空最终由分割层决定）。
    pub content_hint: bool,
}

impl Slot {
    pub fn center(&self) -> (i32, i32) {
        self.rect.center()
    }
}

/// 几何来源：固定锚点验证 vs 纯标定回退。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometrySource {
    /// 由边线响应验证通过（正常路径）
    Verified,
    /// 验证失败后回退到标定坐标（未验证，调用方可拒绝）
    Calibration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotPlanKind {
    Home,
    List,
}

/// 一次识别要处理的槽位计划。
#[derive(Debug, Clone, PartialEq)]
pub struct SlotPlan {
    pub kind: SlotPlanKind,
    pub geometry_source: GeometrySource,
    pub slots: Vec<Slot>,
    pub rows: u32,
    pub cols: u32,
    /// 几何检测的最强行分数（诊断用）
    pub best_row_score: f32,
}

impl SlotPlan {
    /// 仅测试与诊断使用的便捷访问器。
    #[cfg(test)]
    pub fn stratagems(&self) -> impl Iterator<Item = &Slot> {
        self.slots.iter().filter(|s| !s.kind.is_booster())
    }

    /// 仅测试与诊断使用的便捷访问器。
    #[cfg(test)]
    pub fn booster(&self) -> Option<&Slot> {
        self.slots.iter().find(|s| s.kind.is_booster())
    }
}

/// 几何检测结果 → home 槽位计划（4 战备 + Booster）。
pub fn plan_from_home(
    detection: &GeometryDetection,
    geom: &FrameGeometry,
) -> Result<SlotPlan, VisionError> {
    if !detection.verified() {
        return Err(VisionError::InvalidGrid {
            detail: "home 几何未通过边线验证".into(),
        });
    }
    let columns = detection.cols;
    let mut slots = Vec::with_capacity(detection.slots.len());
    let mut booster: Option<Slot> = None;
    for detected in &detection.slots {
        let rect = geom.roi_rect(detected.rect);
        ensure_cell(detected.col as usize, rect, geom)?;
        let slot = match detected.kind {
            DetectedKind::Stratagem => Slot {
                index: detected.col as usize,
                row: detected.row,
                col: detected.col,
                kind: SlotKind::Stratagem,
                rect,
                score: detected.score,
                content_hint: detected.content,
            },
            DetectedKind::Booster => Slot {
                index: HOME_STRATAGEM_SLOTS,
                row: detected.row,
                col: detected.col,
                kind: SlotKind::Booster,
                rect,
                score: detected.score,
                content_hint: detected.content,
            },
        };
        if slot.kind.is_booster() {
            booster = Some(slot);
        } else {
            slots.push(slot);
        }
    }
    if slots.len() < HOME_STRATAGEM_SLOTS || booster.is_none() {
        return Err(VisionError::InvalidGrid {
            detail: format!(
                "home 几何不完整：战备槽 {} / Booster {}",
                slots.len(),
                if booster.is_some() { 1 } else { 0 }
            ),
        });
    }
    slots.sort_by_key(|s| s.index);
    slots.push(booster.expect("已在上面确认存在"));
    Ok(SlotPlan {
        kind: SlotPlanKind::Home,
        geometry_source: GeometrySource::Verified,
        slots,
        rows: detection.rows as u32,
        cols: columns,
        best_row_score: detection.best_row_score,
    })
}

/// 几何检测结果 → 列表槽位计划。
pub fn plan_from_list(
    detection: &GeometryDetection,
    geom: &FrameGeometry,
) -> Result<SlotPlan, VisionError> {
    if !detection.verified() {
        return Err(VisionError::InvalidGrid {
            detail: "列表几何未通过边线验证".into(),
        });
    }
    let columns = detection.cols.max(1);
    let mut slots = Vec::with_capacity(detection.slots.len());
    for detected in &detection.slots {
        let rect = geom.roi_rect(detected.rect);
        let index = detected.row as usize * columns as usize + detected.col as usize;
        ensure_cell(index, rect, geom)?;
        slots.push(Slot {
            index,
            row: detected.row,
            col: detected.col,
            kind: SlotKind::Stratagem,
            rect,
            score: detected.score,
            content_hint: true,
        });
    }
    slots.sort_by_key(|s| s.index);
    Ok(SlotPlan {
        kind: SlotPlanKind::List,
        geometry_source: GeometrySource::Verified,
        slots,
        rows: detection.rows as u32,
        cols: columns,
        best_row_score: detection.best_row_score,
    })
}

/// 回退：home 版面完全由标定推导（**未经边线验证**，分数 0）。
pub fn calibration_home_plan(
    layout: &StratagemLayout,
    geom: &FrameGeometry,
) -> Result<SlotPlan, VisionError> {
    let mut slots = Vec::with_capacity(HOME_STRATAGEM_SLOTS + 1);
    for index in 0..HOME_STRATAGEM_SLOTS {
        let rect = geom.roi_rect(layout.home.cell_rect(0, index as u32));
        ensure_cell(index, rect, geom)?;
        slots.push(Slot {
            index,
            row: 0,
            col: index as u32,
            kind: SlotKind::Stratagem,
            rect,
            score: 0.0,
            content_hint: false,
        });
    }
    let booster_rect = geom.roi_rect(booster_hex_rect(layout, booster_slot_rect(layout)));
    ensure_cell(HOME_STRATAGEM_SLOTS, booster_rect, geom)?;
    slots.push(Slot {
        index: HOME_STRATAGEM_SLOTS,
        row: 0,
        col: layout.home.columns,
        kind: SlotKind::Booster,
        rect: booster_rect,
        score: 0.0,
        content_hint: false,
    });
    Ok(SlotPlan {
        kind: SlotPlanKind::Home,
        geometry_source: GeometrySource::Calibration,
        slots,
        rows: 1,
        cols: layout.home.columns,
        best_row_score: 0.0,
    })
}

/// 回退：列表版面由标定给出的行位置构造（**未经边线验证**）。
pub fn calibration_list_plan(
    layout: &StratagemLayout,
    geom: &FrameGeometry,
    row_tops: &[i32],
) -> Result<SlotPlan, VisionError> {
    let columns = layout.list.columns.max(1);
    let mut slots = Vec::with_capacity(row_tops.len() * columns as usize);
    for (row, top) in row_tops.iter().enumerate() {
        for col in 0..columns {
            let rel = ImageRect::new(
                layout.list.column_x(col),
                *top,
                layout.list.cell_size,
                layout.list.cell_size,
            );
            let rect = geom.roi_rect(rel);
            let index = row * columns as usize + col as usize;
            ensure_cell(index, rect, geom)?;
            slots.push(Slot {
                index,
                row: row as u32,
                col,
                kind: SlotKind::Stratagem,
                rect,
                score: 0.0,
                content_hint: true,
            });
        }
    }
    if slots.is_empty() {
        return Err(VisionError::InvalidGrid {
            detail: "列表行位置为空".into(),
        });
    }
    Ok(SlotPlan {
        kind: SlotPlanKind::List,
        geometry_source: GeometrySource::Calibration,
        slots,
        rows: row_tops.len() as u32,
        cols: columns,
        best_row_score: 0.0,
    })
}

/// Booster 槽方形外接框（ROI 参考坐标）：home 第 4 列右移 `column_offset`。
pub fn booster_slot_rect(layout: &StratagemLayout) -> ImageRect {
    let last = layout.home.column_x(layout.home.columns.saturating_sub(1));
    ImageRect::new(
        last + layout.booster.column_offset,
        layout.home.origin_y,
        layout.home.cell_size,
        layout.home.cell_size,
    )
}

/// Booster 六边形外接框（ROI 参考坐标）。
pub fn booster_hex_rect(layout: &StratagemLayout, slot: ImageRect) -> ImageRect {
    let s = slot.w.max(1) as f32;
    let (cx, cy) = slot.center();
    let cx = cx as f32 + layout.booster.center_dx * s;
    let cy = cy as f32 + layout.booster.center_dy * s;
    let w = (s * layout.booster.hex_w_ratio).round().max(4.0) as i32;
    let h = (s * layout.booster.hex_h_ratio).round().max(4.0) as i32;
    ImageRect::new(
        (cx - w as f32 / 2.0).round() as i32,
        (cy - h as f32 / 2.0).round() as i32,
        w,
        h,
    )
}

/// 格子是否可用：尺寸为正且完整落在画面内。
fn cell_usable(rect: ImageRect, geom: &FrameGeometry) -> bool {
    rect.is_valid()
        && rect.x >= 0
        && rect.y >= 0
        && rect.right() <= geom.frame_w as i32
        && rect.bottom() <= geom.frame_h as i32
}

fn ensure_cell(index: usize, rect: ImageRect, geom: &FrameGeometry) -> Result<(), VisionError> {
    if cell_usable(rect, geom) {
        Ok(())
    } else {
        Err(VisionError::InvalidCell { slot: index, rect })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vision::geometry::{self, DetectedKind};
    use crate::vision::{config::GeometryConfig, config::SegmentationConfig};
    use image::{Rgba, RgbaImage};

    fn layout() -> StratagemLayout {
        StratagemLayout::default()
    }

    /// canonical 尺寸的假 ROI：暗底 + 指定槽位亮边框。
    fn canonical_roi(cells: &[ImageRect], cfg: &GeometryConfig) -> RgbaImage {
        let mut img = RgbaImage::new(cfg.canonical_w, cfg.canonical_h);
        for (_, _, p) in img.enumerate_pixels_mut() {
            *p = Rgba([30, 33, 38, 255]);
        }
        for cell in cells {
            for y in cell.y..cell.bottom() {
                for x in cell.x..cell.right() {
                    if x < 0 || y < 0 || x >= cfg.canonical_w as i32 || y >= cfg.canonical_h as i32
                    {
                        continue;
                    }
                    let edge = x == cell.x
                        || y == cell.y
                        || x == cell.right() - 1
                        || y == cell.bottom() - 1;
                    let color = if edge {
                        [200, 200, 196, 255]
                    } else {
                        [70, 74, 80, 255]
                    };
                    img.put_pixel(x as u32, y as u32, Rgba(color));
                }
            }
        }
        img
    }

    #[test]
    fn home_plan_maps_verified_slots_to_frame_pixels() {
        let cfg = GeometryConfig::default();
        let mut cells: Vec<ImageRect> = cfg
            .home_cols
            .iter()
            .map(|x| ImageRect::new(*x, 636, cfg.slot_size, cfg.slot_size))
            .collect();
        cells.push(ImageRect::new(
            cfg.home_booster_x,
            636,
            cfg.slot_size,
            cfg.slot_size,
        ));
        let detection = geometry::detect_home(
            &canonical_roi(&cells, &cfg),
            &cfg,
            &SegmentationConfig::default(),
        );
        let geom = layout().resolve(2560, 1440).expect("几何");
        let plan = plan_from_home(&detection, &geom).expect("计划");
        assert_eq!(plan.kind, SlotPlanKind::Home);
        assert_eq!(plan.geometry_source, GeometrySource::Verified);
        assert_eq!(plan.slots.len(), 5);
        // 参考坐标 + ROI 原点 = 帧坐标（2560×1440 是 1:1）
        assert_eq!(
            plan.slots[0].rect,
            ImageRect::new(64 + 11, 480 + 636, 104, 104)
        );
        assert_eq!(plan.slots[4].kind, SlotKind::Booster);
        assert_eq!(plan.slots[4].index, 4);
        assert!(plan.slots[0].score > 0.26);
        assert!(plan.stratagems().count() == 4);
    }

    #[test]
    fn home_plan_scales_with_resolution() {
        let cfg = GeometryConfig::default();
        let mut cells: Vec<ImageRect> = cfg
            .home_cols
            .iter()
            .map(|x| ImageRect::new(*x, 636, cfg.slot_size, cfg.slot_size))
            .collect();
        cells.push(ImageRect::new(
            cfg.home_booster_x,
            636,
            cfg.slot_size,
            cfg.slot_size,
        ));
        let detection = geometry::detect_home(
            &canonical_roi(&cells, &cfg),
            &cfg,
            &SegmentationConfig::default(),
        );
        let geom = layout().resolve(1920, 1080).expect("几何");
        let plan = plan_from_home(&detection, &geom).expect("计划");
        // 0.75 缩放：(64+11)*0.75 = 56，边长 78
        assert_eq!(plan.slots[0].rect.x, 56);
        assert_eq!(plan.slots[0].rect.w, 78);
    }

    #[test]
    fn list_plan_indexes_rows_and_columns() {
        let cfg = GeometryConfig::default();
        let mut cells = Vec::new();
        for y in [120, 233] {
            for x in cfg.list_cols {
                cells.push(ImageRect::new(x, y, cfg.slot_size, cfg.slot_size));
            }
        }
        let detection = geometry::detect_list(&canonical_roi(&cells, &cfg), &cfg);
        let geom = layout().resolve(2560, 1440).expect("几何");
        let plan = plan_from_list(&detection, &geom).expect("计划");
        assert_eq!(plan.rows, 2);
        assert_eq!(plan.cols, 4);
        assert_eq!(plan.slots.len(), 8);
        assert_eq!(plan.slots[5].index, 5);
        assert_eq!(plan.slots[5].row, 1);
        assert_eq!(plan.slots[5].col, 1);
    }

    #[test]
    fn unverified_detection_is_rejected() {
        let detection = GeometryDetection::empty(4);
        let geom = layout().resolve(2560, 1440).expect("几何");
        assert!(plan_from_home(&detection, &geom).is_err());
        assert!(plan_from_list(&detection, &geom).is_err());
    }

    #[test]
    fn calibration_fallback_is_marked_unverified() {
        let geom = layout().resolve(2560, 1440).expect("几何");
        let plan = calibration_home_plan(&layout(), &geom).expect("回退计划");
        assert_eq!(plan.geometry_source, GeometrySource::Calibration);
        assert_eq!(plan.slots.len(), 5);
        assert_eq!(plan.slots[0].score, 0.0);
        assert_eq!(
            plan.slots[0].rect,
            ImageRect::new(64 + 11, 480 + 636, 104, 104)
        );
        assert!(plan.slots[4].kind.is_booster());
        // Booster 六边形外接框比槽位方框小
        assert!(plan.slots[4].rect.w < plan.slots[0].rect.w);
    }

    #[test]
    fn invalid_cell_geometry_fails_with_slot_index() {
        let mut l = layout();
        l.home.column_offsets = vec![-500, 124, 237, 350];
        let geom = l.resolve(2560, 1440).expect("几何");
        let err = calibration_home_plan(&l, &geom).expect_err("越界槽位必须失败");
        match err {
            VisionError::InvalidCell { slot, .. } => assert_eq!(slot, 0),
            other => panic!("期望 InvalidCell，实际 {other:?}"),
        }
    }

    #[test]
    fn detected_kind_maps_to_slot_kind() {
        assert!(DetectedKind::Booster.is_booster());
        assert!(!DetectedKind::Stratagem.is_booster());
        assert_eq!(SlotKind::Booster.label(), "Booster");
    }
}
