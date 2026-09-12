// 图标归一化（需求 §13 / §14）。
//
// 规则：
//   * **保持纵横比**：绝不把宽扁形状拉伸成正方形（纵横比本身是主要判别特征，
//     实测把宽条拉伸后「任何宽条」的相似度都会很高）；
//   * 缩放到 128×128 画布的 `1 - 2·padding` 内并居中；
//   * 同时保留 raw / foreground / mask / edge / normalized 五个通道 ——
//     模板匹配与未来的 embedding/分类器都从同一份产物取数据。
//
// 部分通道字段只被调试工具读取（见 `vision::debug`），故豁免 dead_code 告警。
#![allow(dead_code)]
use image::{GrayImage, Luma, RgbaImage};

use crate::loadout_sync::types::ImageRect;

use super::config::NormalizationConfig;
use super::error::VisionError;
use super::segment::ForegroundMasks;

/// 一次图标归一化的全部产物。
#[derive(Debug, Clone)]
pub struct IconImage {
    /// 图标主体包围盒（内裁剪图像的局部坐标）
    pub bbox: ImageRect,
    /// 图标主体包围盒（帧像素坐标，用于日志与 UI 叠加）
    pub bbox_frame: ImageRect,
    /// 包围盒区域的原图
    pub raw: RgbaImage,
    /// 包围盒区域的前景分数（长度 = w·h）
    pub foreground: Vec<f32>,
    /// 包围盒区域的二值掩码（255 = 前景）
    pub mask: GrayImage,
    /// 包围盒区域的边缘图（255 = 强边缘）
    pub edge: GrayImage,
    pub normalized_rgb: RgbaImage,
    pub normalized_mask: GrayImage,
    pub normalized_edge: GrayImage,
    /// 包围盒 → 归一化画布的等比缩放系数
    pub scale: f32,
    /// 归一化画布内前景像素占比
    pub normalized_foreground_ratio: f32,
}

/// 归一化：从内裁剪图像 + 掩码 + 包围盒构建 `IconImage`。
pub fn build_icon_image(
    cell: &RgbaImage,
    cell_frame: ImageRect,
    bbox_local: ImageRect,
    masks: &ForegroundMasks,
    cfg: &NormalizationConfig,
) -> Result<IconImage, VisionError> {
    let (cw, ch) = (cell.width() as i32, cell.height() as i32);
    if cw <= 0 || ch <= 0 {
        return Err(VisionError::ImageProcessing {
            stage: "normalize",
            detail: "图标裁剪图像尺寸为 0".into(),
        });
    }
    let bbox = ImageRect::new(
        bbox_local.x.clamp(0, cw.saturating_sub(1)),
        bbox_local.y.clamp(0, ch.saturating_sub(1)),
        bbox_local.w.max(1),
        bbox_local.h.max(1),
    );
    let bbox = ImageRect::new(
        bbox.x,
        bbox.y,
        bbox.w.min(cw - bbox.x).max(1),
        bbox.h.min(ch - bbox.y).max(1),
    );
    if !bbox.is_valid() {
        return Err(VisionError::InvalidCell {
            slot: 0,
            rect: bbox_local,
        });
    }

    let (bw, bh) = (bbox.w as usize, bbox.h as usize);
    let mut raw = RgbaImage::new(bw as u32, bh as u32);
    let mut mask = GrayImage::new(bw as u32, bh as u32);
    let mut foreground = vec![0.0f32; bw * bh];
    for y in 0..bh {
        for x in 0..bw {
            let sx = bbox.x as usize + x;
            let sy = bbox.y as usize + y;
            let inside = sx < masks.width && sy < masks.height;
            let si = if inside { sy * masks.width + sx } else { 0 };
            raw.put_pixel(
                x as u32,
                y as u32,
                *cell.get_pixel((bbox.x + x as i32) as u32, (bbox.y + y as i32) as u32),
            );
            let is_fg = inside && masks.combined[si];
            mask.put_pixel(x as u32, y as u32, Luma([if is_fg { 255 } else { 0 }]));
            foreground[y * bw + x] = if inside {
                masks.scores.combined[si]
            } else {
                0.0
            };
        }
    }

    let edge = edge_map(&raw, cfg.edge_threshold);

    let size = cfg.size.max(16);
    let padding = cfg.padding_ratio.clamp(0.0, 0.40);
    let inner = (size as f32 * (1.0 - 2.0 * padding)).max(4.0);
    let scale = inner / bw.max(bh).max(1) as f32;
    let placed_w = ((bw as f32 * scale).round() as u32).clamp(1, size);
    let placed_h = ((bh as f32 * scale).round() as u32).clamp(1, size);
    let x0 = (size - placed_w) / 2;
    let y0 = (size - placed_h) / 2;

    let mut normalized_rgb = RgbaImage::new(size, size);
    let mut normalized_mask = GrayImage::new(size, size);
    let mut normalized_edge = GrayImage::new(size, size);
    let mut fg_px = 0usize;
    for dy in 0..placed_h {
        for dx in 0..placed_w {
            let src_x = dx as f32 / scale;
            let src_y = dy as f32 / scale;
            let pixel = sample_bilinear(&raw, src_x, src_y);
            normalized_rgb.put_pixel(x0 + dx, y0 + dy, pixel);
            let sx = (src_x.floor() as u32).min(bw as u32 - 1);
            let sy = (src_y.floor() as u32).min(bh as u32 - 1);
            let m = mask.get_pixel(sx, sy)[0];
            normalized_mask.put_pixel(x0 + dx, y0 + dy, Luma([m]));
            if m > 127 {
                fg_px += 1;
            }
            let e = edge.get_pixel(sx, sy)[0];
            normalized_edge.put_pixel(x0 + dx, y0 + dy, Luma([e]));
        }
    }

    Ok(IconImage {
        bbox,
        bbox_frame: ImageRect::new(cell_frame.x + bbox.x, cell_frame.y + bbox.y, bbox.w, bbox.h),
        raw,
        foreground,
        mask,
        edge,
        normalized_rgb,
        normalized_mask,
        normalized_edge,
        scale,
        normalized_foreground_ratio: fg_px as f32 / (size * size) as f32,
    })
}

/// 双线性采样（越界按边缘钳制）。
fn sample_bilinear(img: &RgbaImage, x: f32, y: f32) -> image::Rgba<u8> {
    let (w, h) = (img.width() as f32, img.height() as f32);
    if w <= 0.0 || h <= 0.0 {
        return image::Rgba([0, 0, 0, 0]);
    }
    let x = x.clamp(0.0, w - 1.0);
    let y = y.clamp(0.0, h - 1.0);
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(img.width() - 1);
    let y1 = (y0 + 1).min(img.height() - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let p00 = img.get_pixel(x0, y0).0;
    let p10 = img.get_pixel(x1, y0).0;
    let p01 = img.get_pixel(x0, y1).0;
    let p11 = img.get_pixel(x1, y1).0;
    let mut out = [0u8; 4];
    for c in 0..4 {
        let top = p00[c] as f32 * (1.0 - fx) + p10[c] as f32 * fx;
        let bottom = p01[c] as f32 * (1.0 - fx) + p11[c] as f32 * fx;
        out[c] = (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8;
    }
    image::Rgba(out)
}

/// Sobel 边缘图（归一化后按阈值二值化；255 = 边缘）。
pub fn edge_map(src: &RgbaImage, threshold: f32) -> GrayImage {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let mut luma = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let p = src.get_pixel(x as u32, y as u32).0;
            luma[y * w + x] =
                (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0;
        }
    }
    let mut out = GrayImage::new(w as u32, h as u32);
    if w < 3 || h < 3 {
        return out;
    }
    let mut magnitudes = vec![0.0f32; w * h];
    let mut max = 0.0f32;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let at = |dx: usize, dy: usize| luma[(y + dy - 1) * w + (x + dx - 1)];
            let gx = -at(0, 0) + at(2, 0) - 2.0 * at(0, 1) + 2.0 * at(2, 1) - at(0, 2) + at(2, 2);
            let gy = -at(0, 0) - 2.0 * at(1, 0) - at(2, 0) + at(0, 2) + 2.0 * at(1, 2) + at(2, 2);
            let magnitude = (gx * gx + gy * gy).sqrt();
            magnitudes[y * w + x] = magnitude;
            max = max.max(magnitude);
        }
    }
    if max <= 1e-6 {
        return out;
    }
    let threshold = threshold.clamp(0.0, 1.0) * max;
    for y in 0..h {
        for x in 0..w {
            let v = if magnitudes[y * w + x] >= threshold && magnitudes[y * w + x] > 0.0 {
                255
            } else {
                0
            };
            out.put_pixel(x as u32, y as u32, Luma([v]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vision::config::SegmentationConfig;
    use crate::vision::segment::segment_cell;
    use image::Rgba;

    fn cfg() -> NormalizationConfig {
        NormalizationConfig::default()
    }

    fn striped_cell() -> (RgbaImage, ForegroundMasks) {
        // 40×20 的橙色矩形（宽扁形状），周围暗底
        let mut img = RgbaImage::new(80, 80);
        for y in 0..80u32 {
            for x in 0..80u32 {
                let color = if (30..70).contains(&x) && (30..50).contains(&y) {
                    [232, 160, 90, 255]
                } else {
                    [80, 80, 78, 255]
                };
                img.put_pixel(x, y, Rgba(color));
            }
        }
        let masks = segment_cell(&img, &SegmentationConfig::default());
        (img, masks)
    }

    fn glyph_cell() -> (RgbaImage, ForegroundMasks) {
        let mut img = RgbaImage::new(80, 80);
        for y in 0..80u32 {
            for x in 0..80u32 {
                let color = if (30..50).contains(&x) && (30..50).contains(&y) {
                    [232, 160, 90, 255]
                } else {
                    [80, 80, 78, 255]
                };
                img.put_pixel(x, y, Rgba(color));
            }
        }
        let masks = segment_cell(&img, &SegmentationConfig::default());
        (img, masks)
    }

    #[test]
    fn normalized_output_has_requested_size_and_channels() {
        let (cell, masks) = glyph_cell();
        let bbox = ImageRect::new(30, 30, 20, 20);
        let icon = build_icon_image(&cell, ImageRect::new(0, 0, 80, 80), bbox, &masks, &cfg())
            .expect("归一化");
        assert_eq!(icon.normalized_rgb.dimensions(), (128, 128));
        assert_eq!(icon.normalized_mask.dimensions(), (128, 128));
        assert_eq!(icon.normalized_edge.dimensions(), (128, 128));
        assert_eq!(icon.raw.dimensions(), (20, 20));
        assert_eq!(icon.mask.dimensions(), (20, 20));
        assert_eq!(icon.foreground.len(), 400);
        assert_eq!(icon.bbox_frame, ImageRect::new(30, 30, 20, 20));
    }

    #[test]
    fn aspect_ratio_is_preserved_not_stretched() {
        let (cell, masks) = striped_cell();
        let bbox = ImageRect::new(30, 30, 40, 20);
        let icon = build_icon_image(&cell, ImageRect::new(0, 0, 80, 80), bbox, &masks, &cfg())
            .expect("归一化");
        // 40:20 = 2:1，缩放到 128 画布后内容仍应是 2:1
        let fg_cols: Vec<u32> = (0..128)
            .filter(|x| (0..128).any(|y| icon.normalized_mask.get_pixel(*x, y)[0] > 127))
            .collect();
        let fg_rows: Vec<u32> = (0..128)
            .filter(|y| (0..128).any(|x| icon.normalized_mask.get_pixel(x, *y)[0] > 127))
            .collect();
        let w = fg_cols.len() as f32;
        let h = fg_rows.len() as f32;
        assert!((w / h - 2.0).abs() < 0.15, "纵横比应保持 2:1，实际 {w}:{h}");
        assert!(w > h, "宽条不得被拉伸成正方形");
    }

    #[test]
    fn padding_keeps_content_away_from_edges() {
        let (cell, masks) = glyph_cell();
        let bbox = ImageRect::new(30, 30, 20, 20);
        let icon = build_icon_image(&cell, ImageRect::new(0, 0, 80, 80), bbox, &masks, &cfg())
            .expect("归一化");
        let pad = (128.0 * cfg().padding_ratio).floor() as u32;
        for y in 0..128 {
            for x in 0..128 {
                if icon.normalized_mask.get_pixel(x, y)[0] > 127 {
                    assert!(x >= pad && y >= pad && x < 128 - pad && y < 128 - pad);
                }
            }
        }
    }

    #[test]
    fn mask_matches_source_foreground() {
        let (cell, masks) = glyph_cell();
        let bbox = ImageRect::new(30, 30, 20, 20);
        let icon = build_icon_image(&cell, ImageRect::new(0, 0, 80, 80), bbox, &masks, &cfg())
            .expect("归一化");
        assert!(icon.normalized_foreground_ratio > 0.05);
        assert!(icon.normalized_foreground_ratio < 0.85);
        // 源掩码里每个前景像素都应为 255
        let mut fg = 0;
        for y in 0..20u32 {
            for x in 0..20u32 {
                if icon.mask.get_pixel(x, y)[0] > 127 {
                    fg += 1;
                }
            }
        }
        assert_eq!(fg, masks.foreground_px());
    }

    #[test]
    fn edge_map_detects_border_of_block() {
        let (cell, _) = glyph_cell();
        // 取包含图标边界的窗口（方块位于 30..50，向外各留 5px 底色）
        let raw = image::imageops::crop_imm(&cell, 25, 25, 30, 30).to_image();
        let edge = edge_map(&raw, 0.2);
        assert_eq!(edge.dimensions(), (30, 30));
        let mut hits = 0;
        for y in 0..30u32 {
            for x in 0..30u32 {
                if edge.get_pixel(x, y)[0] > 0 {
                    hits += 1;
                }
            }
        }
        assert!(hits > 10, "方形边界应产生边缘像素");
    }

    #[test]
    fn out_of_range_bbox_degrades_to_valid_rect() {
        let (cell, masks) = glyph_cell();
        let bbox = ImageRect::new(75, 75, 20, 20);
        let icon = build_icon_image(&cell, ImageRect::new(0, 0, 80, 80), bbox, &masks, &cfg())
            .expect("归一化");
        assert!(icon.bbox.right() <= 80 && icon.bbox.bottom() <= 80);
        assert!(icon.bbox.w >= 1 && icon.bbox.h >= 1);
    }

    #[test]
    fn zero_sized_cell_returns_error_instead_of_panicking() {
        let cell = RgbaImage::new(0, 0);
        let masks = segment_cell(&cell, &SegmentationConfig::default());
        let err = build_icon_image(
            &cell,
            ImageRect::new(0, 0, 0, 0),
            ImageRect::new(0, 0, 1, 1),
            &masks,
            &cfg(),
        )
        .expect_err("空图像必须显式失败");
        assert!(matches!(
            err,
            VisionError::ImageProcessing {
                stage: "normalize",
                ..
            }
        ));
    }
}
