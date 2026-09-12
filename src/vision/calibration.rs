// 参考坐标系与标定 —— 视觉层的唯一坐标权威（需求 §4）。
//
// 坐标约定（三者严格区分，禁止混用）：
//   * Reference  —— 参考帧坐标（默认 2560×1440），配置里所有几何数字都用它表达；
//   * RoiRel     —— ROI 内部的参考坐标（相对 ROI 左上角，单位仍是参考像素）；
//   * Frame      —— 捕获帧像素坐标（1920×1080 / 1914×1076 / 3440×1440 …）。
//
// 业务代码只允许使用 `FrameGeometry` 提供的换算函数，禁止自己乘 scale、禁止裸坐标。
use crate::loadout_sync::types::{ImageRect, RoiAnchor, ScaleAxis};

use super::config::{BoosterProfile, GridProfile, InnerCropConfig, LayoutConfig};
use super::error::VisionError;

/// 战备面板版面的运行时形式（`LayoutConfig` 的几何视角，需求 §4 的 `StratagemLayout`）。
#[derive(Debug, Clone, PartialEq)]
pub struct StratagemLayout {
    pub reference_w: u32,
    pub reference_h: u32,
    /// 参考帧坐标下的面板 ROI。
    pub panel_roi: ImageRect,
    pub scale_axis: ScaleAxis,
    pub anchor: RoiAnchor,
    pub home: GridProfile,
    pub list: GridProfile,
    pub booster: BoosterProfile,
    pub inner_padding: InnerCropConfig,
}

impl Default for StratagemLayout {
    fn default() -> Self {
        Self::from_config(&LayoutConfig::default())
    }
}

impl StratagemLayout {
    pub fn from_config(cfg: &LayoutConfig) -> Self {
        Self {
            reference_w: cfg.reference_w,
            reference_h: cfg.reference_h,
            panel_roi: cfg.panel_roi,
            scale_axis: cfg.scale_axis,
            anchor: cfg.anchor,
            home: cfg.home.clone(),
            list: cfg.list.clone(),
            booster: cfg.booster,
            inner_padding: cfg.crop,
        }
    }

    /// home 网格原点（ROI 参考坐标）—— 需求 §4 的 `grid_origin`。
    #[cfg(test)]
    pub fn grid_origin(&self) -> (i32, i32) {
        (self.home.origin_x, self.home.origin_y)
    }

    /// 格子边长（参考像素）—— 需求 §4 的 `cell_size`。
    #[cfg(test)]
    pub fn cell_size(&self) -> i32 {
        self.home.cell_size
    }

    /// 格间距（参考像素）—— 需求 §4 的 `cell_gap`。
    #[cfg(test)]
    pub fn cell_gap(&self) -> i32 {
        self.home.cell_gap
    }

    /// 列数 —— 需求 §4 的 `columns`。
    #[cfg(test)]
    pub fn columns(&self) -> u32 {
        self.home.columns
    }

    /// home 固定行数 —— 需求 §4 的 `rows`（列表区行数由视口决定，见 `grid.rs`）。
    #[cfg(test)]
    pub fn rows(&self) -> u32 {
        self.home.rows
    }

    /// 解析当前帧的 ROI 与映射。
    ///
    /// 与 `loadout_sync::Calibration::resolve` 的区别：这里允许 **非等比缩放**
    /// （scale_x / scale_y 分开持有），因此 1921×1076 这类带 1px 边框的帧
    /// 不会再因为 fit 缩放被整体平移，从而在长边上少切一刀。
    pub fn resolve(&self, frame_w: u32, frame_h: u32) -> Result<FrameGeometry, VisionError> {
        if frame_w == 0 || frame_h == 0 {
            return Err(VisionError::ZeroSizedFrame);
        }
        let sx = frame_w as f32 / self.reference_w as f32;
        let sy = frame_h as f32 / self.reference_h as f32;
        let (scale_x, scale_y) = match self.scale_axis {
            ScaleAxis::Width => (sx, sx),
            ScaleAxis::Height => (sy, sy),
            ScaleAxis::Fit => {
                let s = sx.min(sy);
                (s, s)
            }
        };
        if !(scale_x.is_finite() && scale_y.is_finite()) || scale_x <= 0.05 || scale_y <= 0.05 {
            return Err(VisionError::InvalidGrid {
                detail: format!("画面尺寸异常 {frame_w}x{frame_h}（推导缩放 {scale_x}x{scale_y}）"),
            });
        }
        let scaled_w = self.reference_w as f32 * scale_x;
        let scaled_h = self.reference_h as f32 * scale_y;
        let (offset_x, offset_y) = match self.anchor {
            RoiAnchor::TopLeft => (0.0, 0.0),
            RoiAnchor::TopCenter => ((frame_w as f32 - scaled_w) * 0.5, 0.0),
            RoiAnchor::Center => (
                (frame_w as f32 - scaled_w) * 0.5,
                (frame_h as f32 - scaled_h) * 0.5,
            ),
        };
        let mut geometry = FrameGeometry {
            frame_w,
            frame_h,
            roi_ref: self.panel_roi,
            roi: ImageRect::default(),
            scale_x,
            scale_y,
            offset_x,
            offset_y,
        };
        let roi = geometry.rect_ref(self.panel_roi);
        roi.clamp_to_frame(frame_w as i32, frame_h as i32)
            .ok_or(VisionError::RoiOutsideFrame {
                roi,
                frame_w,
                frame_h,
            })?;
        geometry.roi = roi;
        Ok(geometry)
    }
}

/// 参考帧 → 捕获帧的仿射参数（支持 scale_x / scale_y 不同）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameGeometry {
    pub frame_w: u32,
    pub frame_h: u32,
    /// 参考帧坐标下的 ROI（配置原值，便于反查）。
    pub roi_ref: ImageRect,
    /// 帧像素坐标下的 ROI（已裁剪到画面内）。
    pub roi: ImageRect,
    pub scale_x: f32,
    pub scale_y: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl FrameGeometry {
    /// 参考帧坐标点 → 帧像素（不做取整，供需要亚像素的调用方使用）。
    pub fn point_ref(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.offset_x + x * self.scale_x,
            self.offset_y + y * self.scale_y,
        )
    }

    /// 参考帧坐标矩形 → 帧像素矩形。
    pub fn rect_ref(&self, r: ImageRect) -> ImageRect {
        let (x0, y0) = self.point_ref(r.x as f32, r.y as f32);
        let (x1, y1) = self.point_ref(r.right() as f32, r.bottom() as f32);
        ImageRect::new(
            x0.round() as i32,
            y0.round() as i32,
            ((x1 - x0).round() as i32).max(1),
            ((y1 - y0).round() as i32).max(1),
        )
    }

    /// ROI 内部参考坐标 → 帧像素（几何表里的槽位坐标都是这一套）。
    pub fn roi_rect(&self, rel: ImageRect) -> ImageRect {
        self.rect_ref(ImageRect::new(
            self.roi_ref.x + rel.x,
            self.roi_ref.y + rel.y,
            rel.w,
            rel.h,
        ))
    }

    /// 帧像素纵坐标 → ROI 内部参考纵坐标（列表行位置回推用）。
    #[cfg(test)]
    pub fn roi_rel_y(&self, frame_y: i32) -> i32 {
        let origin = self.offset_y + self.roi_ref.y as f32 * self.scale_y;
        ((frame_y as f32 - origin) / self.scale_y).round() as i32
    }

    /// 帧像素横坐标 → ROI 内部参考横坐标。
    #[cfg(test)]
    pub fn roi_rel_x(&self, frame_x: i32) -> i32 {
        let origin = self.offset_x + self.roi_ref.x as f32 * self.scale_x;
        ((frame_x as f32 - origin) / self.scale_x).round() as i32
    }

    /// 帧像素矩形 → ROI 内部参考矩形（调试图与日志用）。
    #[cfg(test)]
    pub fn frame_rect_to_roi(&self, r: ImageRect) -> ImageRect {
        let x0 = self.roi_rel_x(r.x);
        let y0 = self.roi_rel_y(r.y);
        let x1 = self.roi_rel_x(r.right());
        let y1 = self.roi_rel_y(r.bottom());
        ImageRect::new(x0, y0, (x1 - x0).max(1), (y1 - y0).max(1))
    }

    /// 该帧是否与参考分辨率等比（等比时 `RoiMapping` 与几何映射完全一致）。
    #[cfg(test)]
    pub fn is_uniform(&self) -> bool {
        (self.scale_x - self.scale_y).abs() <= f32::EPSILON
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> StratagemLayout {
        StratagemLayout::default()
    }

    #[test]
    fn reference_resolution_maps_identity() {
        let geom = layout().resolve(2560, 1440).expect("解析");
        assert_eq!(geom.roi, ImageRect::new(64, 480, 576, 832));
        assert!((geom.scale_x - 1.0).abs() < 1e-6);
        assert!(geom.is_uniform());
        let home0 = geom.roi_rect(ImageRect::new(9, 636, 106, 106));
        assert_eq!(home0, ImageRect::new(73, 1116, 106, 106));
    }

    #[test]
    fn scaling_keeps_roi_proportional() {
        let l = layout();
        let g1080 = l.resolve(1920, 1080).expect("1080p");
        // fit 缩放 = 0.75，锚点 top_center → x 偏移 0
        assert!((g1080.scale_x - 0.75).abs() < 1e-6);
        assert_eq!(g1080.roi, ImageRect::new(48, 360, 432, 624));
        let g2160 = l.resolve(3840, 2160).expect("4K");
        assert!((g2160.scale_x - 1.5).abs() < 1e-6);
        assert_eq!(g2160.roi, ImageRect::new(96, 720, 864, 1248));
    }

    #[test]
    fn ultrawide_keeps_vertical_fit_and_centers() {
        let g = layout().resolve(3440, 1440).expect("21:9");
        assert!((g.scale_y - 1.0).abs() < 1e-6);
        assert!((g.scale_x - 1.0).abs() < 1e-6);
        // fit 缩放严格等比 → scale_x == scale_y，偏移把参考帧水平居中
        assert!(g.is_uniform());
        assert_eq!(g.offset_x, (3440.0 - 2560.0) * 0.5);
    }

    #[test]
    fn odd_screenshot_with_border_is_supported() {
        // 1921×1076：等比 fit（0.7471…），不再因取整被整体平移
        let g = layout().resolve(1921, 1076).expect("带边框截图");
        assert!(g.scale_x < 0.76 && g.scale_x > 0.73);
        assert!(g.roi.x >= 0 && g.roi.right() <= 1921);
        assert!(g.roi.bottom() <= 1076);
    }

    #[test]
    fn zero_sized_frame_fails_safely() {
        assert_eq!(layout().resolve(0, 0), Err(VisionError::ZeroSizedFrame));
    }

    #[test]
    fn tiny_frame_fails_with_context() {
        let err = layout().resolve(100, 60).expect_err("过小画面必须失败");
        assert!(matches!(err, VisionError::InvalidGrid { .. }));
        assert!(err.message().contains("画面尺寸异常"));
    }

    #[test]
    fn panel_roi_outside_reference_fails_with_context() {
        let mut l = layout();
        l.panel_roi = ImageRect::new(2500, 1300, 576, 832);
        let err = l.resolve(2560, 1440).expect_err("ROI 越界必须失败");
        assert!(matches!(err, VisionError::RoiOutsideFrame { .. }));
        assert!(err.message().contains("超出画面"));
    }

    #[test]
    fn roi_relative_roundtrip_is_stable() {
        let l = layout();
        let g = l.resolve(1914, 1080).expect("实机帧");
        let rel = ImageRect::new(77, 120, 106, 106);
        let frame = g.roi_rect(rel);
        let back = g.frame_rect_to_roi(frame);
        assert!((back.x - rel.x).abs() <= 1);
        assert!((back.y - rel.y).abs() <= 1);
        assert!((back.w - rel.w).abs() <= 2);
    }

    #[test]
    fn layout_exposes_canonical_grid_accessors() {
        let l = layout();
        assert_eq!(l.grid_origin(), (11, 636));
        assert_eq!(l.cell_size(), 104);
        assert_eq!(l.cell_gap(), 9);
        assert_eq!(l.columns(), 4);
        assert_eq!(l.rows(), 1);
        assert_eq!(l.home.column_x(3), 350);
    }
}
