// Loadout Sync 基础类型 —— 坐标系统、标定、槽位、UI 状态。
//
// plan6 §5.3 之后，整轮装配的判定都在 `direct_select`；本模块里的
// 槽位/网格便捷访问器（`GameLoadoutSlots::stratagem`、`ListGrid::row_cells` 等）
// 现在只被底层适配契约的回归测试调用。整体豁免 dead_code 告警。
#![allow(dead_code)]
//
// 坐标约定（三者严格区分，禁止混用）：
//   * ImageRect   —— 截图（帧）像素坐标，原点在捕获帧左上角
//   * ScreenRect  —— 虚拟桌面物理像素坐标，鼠标注入最终使用这一套
//   * RoiPoint    —— 标定 ROI 内部的参考坐标（2560x1440 参考帧下的像素）
use serde::{Deserialize, Serialize};

use crate::loadout_sync::error::LoadoutSyncError;
use crate::loadout_sync::selection::LoadoutItem;

// ─── 矩形 ───

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ImageRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl ImageRect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }

    pub fn is_valid(&self) -> bool {
        self.w > 0 && self.h > 0
    }

    pub fn inset(&self, d: i32) -> Self {
        Self {
            x: self.x + d,
            y: self.y + d,
            w: (self.w - 2 * d).max(0),
            h: (self.h - 2 * d).max(0),
        }
    }

    /// 图像坐标 → 虚拟桌面物理像素坐标。
    pub fn to_screen(self, origin: ScreenPoint) -> ScreenRect {
        ScreenRect {
            x: origin.x + self.x,
            y: origin.y + self.y,
            w: self.w,
            h: self.h,
        }
    }

    /// 裁剪到帧范围（越界视为不可用）。
    pub fn clamp_to_frame(&self, frame_w: i32, frame_h: i32) -> Option<Self> {
        if self.x < 0 || self.y < 0 || self.right() > frame_w || self.bottom() > frame_h {
            return None;
        }
        self.is_valid().then_some(*self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScreenPoint {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl ScreenRect {
    pub fn center(&self) -> ScreenPoint {
        ScreenPoint {
            x: self.x + self.w / 2,
            y: self.y + self.h / 2,
        }
    }
}

// ─── 标定 ───

/// 标定模型支持三种缩放轴与锚点：当前默认只用 Fit + TopCenter，
/// 其余组合保留为可配置能力（config 中手工指定时使用）。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleAxis {
    Width,
    Height,
    Fit,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoiAnchor {
    TopLeft,
    TopCenter,
    Center,
}

/// 参考帧 → 实际帧的仿射参数（用于把 ROI 参考坐标换算成帧像素坐标）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoiMapping {
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl RoiMapping {
    pub fn point(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.offset_x + x * self.scale,
            self.offset_y + y * self.scale,
        )
    }

    pub fn rect(&self, r: ImageRect) -> ImageRect {
        let (x0, y0) = self.point(r.x as f32, r.y as f32);
        let (x1, y1) = self.point(r.right() as f32, r.bottom() as f32);
        ImageRect::new(
            x0.round() as i32,
            y0.round() as i32,
            ((x1 - x0).round() as i32).max(1),
            ((y1 - y0).round() as i32).max(1),
        )
    }
}

/// 游戏配装界面在屏幕上的标定模型。
///
/// 默认值来自实测：
///   * 参考帧 2560x1440（16:9），窗口 / 无边框 / 全屏共用同一 UI 布局；
///   * ROI 取左侧配装面板（含标题与准备就绪条），锚点 top_center、缩放取 fit；
///   * 槽位几何（相对 ROI 左上角，参考像素）：
///       - Home 四个战备槽：x = 9/122/236/349，边长 106，行顶 y = 636
///       - Home Booster 六边形外接框：x = 458，边长 106
///       - 列表视图：4 列 x = 77/190/304/417，可滚动区 y = 120..700，行距约 113
///
/// 实测来源：2559x1439 无边框截图（与 2560x1440 参考误差 < 2px）。
///
/// 注意：这是「先验 + 逐帧验证」模型 —— 识别器会在同一帧内搜索边框线来
/// 修正/确认位置，得分不足时直接失败，绝不按先验坐标盲点。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Calibration {
    pub reference_w: u32,
    pub reference_h: u32,
    pub roi: ImageRect,
    pub scale_axis: ScaleAxis,
    pub anchor: RoiAnchor,
    pub slot_size: i32,
    pub home_cols: [i32; 4],
    pub home_row_top: i32,
    pub booster_x: i32,
    pub list_cols: [i32; 4],
    pub list_top: i32,
    pub list_bottom: i32,
    pub row_pitch: i32,
    /// Booster 六边形宽/高占槽位边长的比例（实测：0.877 / 0.755）
    pub booster_hex_w_ratio: f32,
    pub booster_hex_h_ratio: f32,
    /// 六边形中心相对槽位中心的偏移（占槽位边长的比例，实测校正量）
    pub booster_center_dx: f32,
    pub booster_center_dy: f32,
}

impl Default for Calibration {
    fn default() -> Self {
        Self {
            reference_w: 2560,
            reference_h: 1440,
            roi: ImageRect::new(64, 480, 576, 832),
            scale_axis: ScaleAxis::Fit,
            anchor: RoiAnchor::TopCenter,
            slot_size: 106,
            home_cols: [9, 122, 236, 349],
            home_row_top: 636,
            booster_x: 458,
            list_cols: [77, 190, 304, 417],
            list_top: 120,
            list_bottom: 700,
            row_pitch: 113,
            booster_hex_w_ratio: 0.877,
            booster_hex_h_ratio: 0.755,
            booster_center_dx: -0.024,
            booster_center_dy: 0.009,
        }
    }
}

impl Calibration {
    /// 计算当前帧尺寸下的 ROI 与坐标映射。
    pub fn resolve(
        &self,
        frame_w: u32,
        frame_h: u32,
    ) -> Result<(ImageRect, RoiMapping), LoadoutSyncError> {
        if frame_w == 0 || frame_h == 0 {
            return Err(LoadoutSyncError::CaptureFailed {
                detail: "画面尺寸为 0".into(),
            });
        }
        let sx = frame_w as f32 / self.reference_w as f32;
        let sy = frame_h as f32 / self.reference_h as f32;
        let scale = match self.scale_axis {
            ScaleAxis::Width => sx,
            ScaleAxis::Height => sy,
            ScaleAxis::Fit => sx.min(sy),
        };
        if !(scale.is_finite() && scale > 0.05) {
            return Err(LoadoutSyncError::CaptureFailed {
                detail: format!("画面尺寸异常 {frame_w}x{frame_h}"),
            });
        }
        let scaled_w = self.reference_w as f32 * scale;
        let scaled_h = self.reference_h as f32 * scale;
        let (offset_x, offset_y) = match self.anchor {
            RoiAnchor::TopLeft => (0.0, 0.0),
            RoiAnchor::TopCenter => ((frame_w as f32 - scaled_w) * 0.5, 0.0),
            RoiAnchor::Center => (
                (frame_w as f32 - scaled_w) * 0.5,
                (frame_h as f32 - scaled_h) * 0.5,
            ),
        };
        let mapping = RoiMapping {
            scale,
            offset_x,
            offset_y,
        };
        let roi = mapping.rect(self.roi);
        let clamped = roi
            .clamp_to_frame(frame_w as i32, frame_h as i32)
            .ok_or_else(|| {
                // 非标准分辨率 / UI 裁剪时安全失败：绝不带着偏移的 ROI 继续点击
                LoadoutSyncError::CaptureFailed {
                    detail: format!(
                        "标定 ROI {roi:?} 超出画面 {frame_w}x{frame_h}（可能为非 16:9 裁剪或 UI 缩放异常）"
                    ),
                }
            })?;
        Ok((clamped, mapping))
    }

    /// Home 界面槽位矩形（ROI 参考坐标，未映射）。
    pub fn home_slot_rects(&self) -> ([ImageRect; 4], ImageRect) {
        let s = self.slot_size;
        let strat = self
            .home_cols
            .map(|x| ImageRect::new(x, self.home_row_top, s, s));
        let booster = ImageRect::new(self.booster_x, self.home_row_top, s, s);
        (strat, booster)
    }

    /// ROI 内部参考坐标（参考帧像素单位，例如 slot_size=106）→ 帧像素坐标。
    ///
    /// 标定表中的槽位几何（home_cols / list_cols / home_row_top ...）都是
    /// 「相对 ROI 左上角」的参考像素，必须先把 ROI 原点加回去再映射，
    /// 否则在非参考分辨率下位置会整体偏移（曾导致 720p 下全部槽位识别失败）。
    /// 映射过程中会乘 scale，因此传入的矩形必须是参考单位，不要预先缩放。
    pub fn roi_to_frame(&self, mapping: &RoiMapping, rel: ImageRect) -> ImageRect {
        mapping.rect(ImageRect::new(
            self.roi.x + rel.x,
            self.roi.y + rel.y,
            rel.w,
            rel.h,
        ))
    }

    /// ROI 内部参考点 → 帧像素坐标。
    pub fn roi_point(&self, mapping: &RoiMapping, x: f32, y: f32) -> (f32, f32) {
        mapping.point(self.roi.x as f32 + x, self.roi.y as f32 + y)
    }

    /// Booster 六边形外接框（ROI 参考坐标，以槽位中心为基准计算）。
    pub fn booster_hex_rect(&self, slot_rect: ImageRect) -> ImageRect {
        let s = slot_rect.w.max(1) as f32;
        let (cx, cy) = slot_rect.center();
        let cx = cx as f32 + self.booster_center_dx * s;
        let cy = cy as f32 + self.booster_center_dy * s;
        let w = (s * self.booster_hex_w_ratio).round().max(4.0) as i32;
        let h = (s * self.booster_hex_h_ratio).round().max(4.0) as i32;
        ImageRect::new(
            (cx - w as f32 / 2.0).round() as i32,
            (cy - h as f32 / 2.0).round() as i32,
            w,
            h,
        )
    }
}

// ─── 槽位 / 网格 ───

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlotRegion {
    pub rect: ImageRect,
    /// 边框线响应得分（0.0~1.0），过低表示槽位几何不可信。
    pub score: f32,
}

impl SlotRegion {
    pub fn center(&self) -> (i32, i32) {
        self.rect.center()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GameLoadoutSlots {
    pub stratagems: [SlotRegion; 4],
    pub booster: SlotRegion,
    /// Home 识别总分（用于日志与阈值判断）。
    pub home_score: f32,
}

impl GameLoadoutSlots {
    pub fn stratagem(&self, index: usize) -> Option<&SlotRegion> {
        self.stratagems.get(index)
    }

    /// 全部 5 个槽位（测试断言用）。
    #[cfg(test)]
    pub fn all(&self) -> Vec<SlotRegion> {
        let mut v: Vec<SlotRegion> = self.stratagems.to_vec();
        v.push(self.booster);
        v
    }
}

/// 列表视图中的一个可见单元格。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListCell {
    pub rect: ImageRect,
    pub row: u32,
    pub col: u32,
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ListGrid {
    pub cells: Vec<ListCell>,
    pub rows: u32,
    pub cols: u32,
}

impl ListGrid {
    pub fn from_cells(cells: Vec<ListCell>) -> Self {
        let rows = cells.iter().map(|c| c.row + 1).max().unwrap_or(0);
        let cols = cells.iter().map(|c| c.col + 1).max().unwrap_or(0);
        Self { cells, rows, cols }
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn row_cells(&self, row: u32) -> Vec<&ListCell> {
        let mut v: Vec<&ListCell> = self.cells.iter().filter(|c| c.row == row).collect();
        v.sort_by_key(|c| c.col);
        v
    }
}

/// 图标匹配结果（目标 ID + 屏幕上的位置 + 置信度）。
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub item: LoadoutItem,
    pub rect: ImageRect,
    pub score: f32,
    /// 与次优模板的分数差 —— 相似图标（如多种哨戒炮）必须靠 margin 区分。
    pub margin: f32,
    pub cell_row: u32,
    pub cell_col: u32,
}

impl Match {
    pub fn is_confident(&self, threshold: f32) -> bool {
        self.score >= threshold && self.margin >= MIN_MATCH_MARGIN
    }
}

/// 次优模板分数差下限：低于该值说明「认出来了但分不清是哪一个」，必须拒绝点击。
///
/// 该值来自合成 fixture 的实测混淆矩阵（12 个图标、识别矩形下的真实评分路径）：
/// 正确目标 0.61~0.82、最接近的相似图标分差最小 0.02~0.03（自动加农炮 vs 电磁炮）。
/// 真实游戏画面（尤其是列表视图）需要重新标定这两个阈值。
pub const MIN_MATCH_MARGIN: f32 = 0.03;

/// 整库判别时的「近似打平」容差。
///
/// 实测（720p 合成 fixture，53px 格子）：
///   * 目标确实在列表里、但与另一图标近似打平：目标 0.607 / 干扰项 0.632 → 差 -0.025；
///   * 目标不在列表里、被相似图标冒充：目标 0.722 / 干扰项 0.901 → 差 -0.179。
///
/// 因此取 0.03 作为分界：差在容差内视为「打平」（接受），超过则判定「这一格是别的战备」（拒绝）。
pub const LIBRARY_TIE_EPSILON: f32 = 0.03;

// ─── UI 状态 ───

/// 本功能需要识别的界面状态。BoosterList 当前与 StratagemList 共用同一套
/// 列表识别（列表几何一致），保留该状态用于日志与后续细分。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiState {
    Unknown,
    LoadoutHome,
    StratagemList,
    BoosterList,
}

impl UiState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::LoadoutHome => "LoadoutHome",
            Self::StratagemList => "StratagemList",
            Self::BoosterList => "BoosterList",
        }
    }
}

// ─── 游戏窗口 ───

/// 游戏窗口快照。hwnd 用 isize 保存，避免把 Windows 类型带进状态机与测试。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameWindowInfo {
    pub hwnd: isize,
    /// 客户区（渲染区）在虚拟桌面物理像素中的位置与尺寸。
    pub client: ScreenRect,
    pub dpi: u32,
}

impl GameWindowInfo {
    pub fn width(&self) -> u32 {
        self.client.w.max(0) as u32
    }

    pub fn height(&self) -> u32 {
        self.client.h.max(0) as u32
    }
}
