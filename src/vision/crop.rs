// 内裁剪（需求 §5 / §7）—— 格子的四个外边不允许进入识别器。
//
// 剔除对象：白色槽位边框、黄色选中框、UI 分隔线、阴影、格子底色边缘。
// padding 只在这里定义，调用方禁止再写 `cell.inset(6)` 这类裸内缩。
use crate::loadout_sync::types::ImageRect;

use super::config::InnerCropConfig;

/// 一次内裁剪的实测结果（调试图与日志用）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InnerCrop {
    /// 原始格子（帧像素）
    pub cell: ImageRect,
    /// 内裁剪后的图标区（帧像素）
    pub rect: ImageRect,
    /// 四边实际内缩的像素数（左、上、右、下）
    pub insets: (i32, i32, i32, i32),
}

/// 按配置从格子算出内裁剪矩形。
///
/// 规则：
///   * 单边内缩 = `px`（显式像素优先）或 `pct × 格子边长`；
///   * 再统一叠加 `border_ratio × 格子边长`（槽位边框本身的厚度）；
///   * 结果至少保留 4×4 像素，且不允许越出原格子。
pub fn inner_crop(cell: ImageRect, cfg: &InnerCropConfig) -> InnerCrop {
    if !cell.is_valid() {
        return InnerCrop {
            cell,
            rect: ImageRect::new(cell.x, cell.y, 1, 1),
            insets: (0, 0, 0, 0),
        };
    }
    let side = cell.w.min(cell.h).max(1) as f32;
    let border = (side * cfg.border_ratio.clamp(0.0, 0.45)).round() as i32;
    let pick = |px: i32, pct: f32| -> i32 {
        let base = if px > 0 {
            px
        } else {
            (cell.w.max(cell.h) as f32 * pct.clamp(0.0, 0.45)).round() as i32
        };
        (base + border).max(0)
    };
    let left = pick(cfg.left_px, cfg.left_pct).min(cell.w / 2 - 1).max(0);
    let right = pick(cfg.right_px, cfg.right_pct).min(cell.w / 2 - 1).max(0);
    let top = pick(cfg.top_px, cfg.top_pct).min(cell.h / 2 - 1).max(0);
    let bottom = pick(cfg.bottom_px, cfg.bottom_pct)
        .min(cell.h / 2 - 1)
        .max(0);
    let rect = ImageRect::new(
        cell.x + left,
        cell.y + top,
        (cell.w - left - right).max(4),
        (cell.h - top - bottom).max(4),
    );
    InnerCrop {
        cell,
        rect,
        insets: (left, top, right, bottom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell() -> ImageRect {
        // 实机 1080p 下实测的格子尺寸：80×80
        ImageRect::new(100, 200, 80, 80)
    }

    #[test]
    fn default_crop_removes_border_ring() {
        let cfg = InnerCropConfig::default();
        let crop = inner_crop(cell(), &cfg);
        // 10% × 80 = 8，再加 4% 边框 = 3 → 11
        assert_eq!(crop.insets, (11, 11, 11, 11));
        assert_eq!(crop.rect, ImageRect::new(111, 211, 58, 58));
    }

    #[test]
    fn explicit_pixels_win_over_percentage() {
        let cfg = InnerCropConfig {
            left_px: 2,
            right_px: 4,
            top_px: 6,
            bottom_px: 8,
            border_ratio: 0.0,
            ..InnerCropConfig::default()
        };
        let crop = inner_crop(cell(), &cfg);
        assert_eq!(crop.insets, (2, 6, 4, 8));
        assert_eq!(crop.rect, ImageRect::new(102, 206, 74, 66));
    }

    #[test]
    fn absurd_padding_never_produces_empty_rect() {
        let cfg = InnerCropConfig {
            left_pct: 0.45,
            right_pct: 0.45,
            top_pct: 0.45,
            bottom_pct: 0.45,
            border_ratio: 0.45,
            ..InnerCropConfig::default()
        };
        let crop = inner_crop(cell(), &cfg);
        assert!(crop.rect.w >= 4 && crop.rect.h >= 4);
        assert!(crop.rect.x >= cell().x && crop.rect.right() <= cell().right());
    }

    #[test]
    fn invalid_cell_degrades_to_one_pixel() {
        let crop = inner_crop(ImageRect::new(0, 0, 0, 0), &InnerCropConfig::default());
        assert_eq!(crop.rect.w, 1);
        assert_eq!(crop.insets, (0, 0, 0, 0));
    }
}
