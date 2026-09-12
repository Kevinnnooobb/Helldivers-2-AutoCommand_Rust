// ROI 提取与调试图绘制（需求 §5 / §23）。
//
// 只处理战备面板：识别器永远看不到整张截图，这条约束由本模块的 API 形状保证
// （所有下游函数都接收 `RoiImage`，拿不到全屏帧）。
use image::{Rgba, RgbaImage};

use crate::loadout_sync::capture::CapturedFrame;
use crate::loadout_sync::types::ImageRect;

use super::calibration::FrameGeometry;
use super::error::VisionError;
use super::grid::{SlotKind, SlotPlan};

/// 从捕获帧裁出 ROI（帧像素坐标）。
pub fn extract_roi(frame: &CapturedFrame, geom: &FrameGeometry) -> Result<RgbaImage, VisionError> {
    crop_rect(frame, geom.roi)
}

/// 从捕获帧裁剪任意矩形（越界 → 明确失败，不做静默补边）。
pub fn crop_rect(frame: &CapturedFrame, rect: ImageRect) -> Result<RgbaImage, VisionError> {
    let (w, h) = (frame.rgba.width() as i32, frame.rgba.height() as i32);
    let clamped = rect
        .clamp_to_frame(w, h)
        .ok_or(VisionError::ImageProcessing {
            stage: "crop",
            detail: format!(
                "裁剪区域 ({},{},{},{}) 超出帧 {w}x{h}",
                rect.x, rect.y, rect.w, rect.h
            ),
        })?;
    let mut out = RgbaImage::new(clamped.w as u32, clamped.h as u32);
    for y in 0..clamped.h {
        for x in 0..clamped.w {
            let p = frame
                .rgba
                .get_pixel((clamped.x + x) as u32, (clamped.y + y) as u32);
            out.put_pixel(x as u32, y as u32, *p);
        }
    }
    Ok(out)
}

/// 调试图配色（与 UI 主题无关，只为在 PNG 上可辨认）。
pub const COLOR_ROI: [u8; 4] = [0, 200, 255, 255];
pub const COLOR_SLOT: [u8; 4] = [255, 210, 0, 255];
pub const COLOR_BOOSTER: [u8; 4] = [180, 120, 255, 255];
pub const COLOR_ICON_BBOX: [u8; 4] = [80, 255, 120, 255];

/// 在图上绘制 1px 矩形（`origin` 为图相对帧的偏移，用于把帧坐标画到 ROI 图上）。
pub fn draw_rect(img: &mut RgbaImage, rect: ImageRect, color: [u8; 4], origin: (i32, i32)) {
    let x0 = rect.x - origin.0;
    let y0 = rect.y - origin.1;
    let x1 = x0 + rect.w - 1;
    let y1 = y0 + rect.h - 1;
    let w = img.width() as i32;
    let h = img.height() as i32;
    let mut put = |x: i32, y: i32| {
        if x >= 0 && y >= 0 && x < w && y < h {
            img.put_pixel(x as u32, y as u32, Rgba(color));
        }
    };
    for x in x0..=x1 {
        put(x, y0);
        put(x, y1);
    }
    for y in y0..=y1 {
        put(x0, y);
        put(x1, y);
    }
}

/// 在图上绘制十字（槽位中心 / 图标中心标记用）。
pub fn draw_cross(
    img: &mut RgbaImage,
    center: (i32, i32),
    arm: i32,
    color: [u8; 4],
    origin: (i32, i32),
) {
    let (cx, cy) = (center.0 - origin.0, center.1 - origin.1);
    let w = img.width() as i32;
    let h = img.height() as i32;
    let mut put = |x: i32, y: i32| {
        if x >= 0 && y >= 0 && x < w && y < h {
            img.put_pixel(x as u32, y as u32, Rgba(color));
        }
    };
    for d in -arm..=arm {
        put(cx + d, cy);
        put(cx, cy + d);
    }
}

/// 生成 ROI 叠加图：ROI 边界 + 网格 + 槽位框（+ 图标 bbox）。
pub fn overlay_slots(
    roi: &RgbaImage,
    geom: &FrameGeometry,
    plan: &SlotPlan,
    icon_boxes: &[(usize, ImageRect)],
) -> RgbaImage {
    let mut out = roi.clone();
    let origin = (geom.roi.x, geom.roi.y);
    draw_rect(&mut out, geom.roi, COLOR_ROI, (0, 0));
    for slot in &plan.slots {
        let color = if slot.kind == SlotKind::Booster {
            COLOR_BOOSTER
        } else {
            COLOR_SLOT
        };
        draw_rect(&mut out, slot.rect, color, origin);
        draw_cross(&mut out, slot.center(), 2, color, origin);
    }
    for (_, bbox) in icon_boxes {
        draw_rect(&mut out, *bbox, COLOR_ICON_BBOX, origin);
    }
    out
}

/// 写 PNG（失败带路径上下文）。
pub fn save_png(img: &RgbaImage, path: &std::path::Path) -> Result<(), VisionError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| VisionError::Io {
            path: parent.display().to_string(),
            detail: format!("创建目录失败: {e}"),
        })?;
    }
    img.save(path).map_err(|e| VisionError::Io {
        path: path.display().to_string(),
        detail: format!("写入 PNG 失败: {e}"),
    })
}

/// 从磁盘 PNG 构造一个「捕获帧」（CLI 与数据集工具的唯一入口）。
///
/// 灰度图由彩色图推导，保证与真实捕获路径同一口径。
pub fn load_frame(path: &std::path::Path) -> Result<CapturedFrame, VisionError> {
    let bytes = std::fs::read(path).map_err(|e| VisionError::Io {
        path: path.display().to_string(),
        detail: format!("读取截图失败: {e}"),
    })?;
    let img = image::load_from_memory(&bytes).map_err(|e| VisionError::ImageProcessing {
        stage: "load_frame",
        detail: format!("解码 {} 失败: {e}", path.display()),
    })?;
    let rgba = img.to_rgba8();
    if rgba.width() == 0 || rgba.height() == 0 {
        return Err(VisionError::ZeroSizedFrame);
    }
    let gray = image::DynamicImage::ImageRgba8(rgba.clone()).to_luma8();
    Ok(CapturedFrame {
        gray,
        rgba,
        origin: crate::loadout_sync::types::ScreenPoint { x: 0, y: 0 },
        backend: crate::loadout_sync::capture::CaptureBackend::Wgc,
    })
}

/// 灰度图写 PNG（掩码/边缘产物用）。
pub fn save_gray_png(img: &image::GrayImage, path: &std::path::Path) -> Result<(), VisionError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| VisionError::Io {
            path: parent.display().to_string(),
            detail: format!("创建目录失败: {e}"),
        })?;
    }
    img.save(path).map_err(|e| VisionError::Io {
        path: path.display().to_string(),
        detail: format!("写入灰度 PNG 失败: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadout_sync::capture::CaptureBackend;
    use crate::loadout_sync::types::ScreenPoint;
    use crate::vision::calibration::StratagemLayout;

    fn frame_from_rgba(rgba: RgbaImage) -> CapturedFrame {
        let gray = image::DynamicImage::ImageRgba8(rgba.clone()).to_luma8();
        CapturedFrame {
            gray,
            rgba,
            origin: ScreenPoint { x: 0, y: 0 },
            backend: CaptureBackend::Wgc,
        }
    }

    fn solid_frame(w: u32, h: u32, color: [u8; 4]) -> CapturedFrame {
        let mut img = RgbaImage::new(w, h);
        for (_, _, p) in img.enumerate_pixels_mut() {
            *p = Rgba(color);
        }
        frame_from_rgba(img)
    }

    #[test]
    fn roi_extraction_matches_calibrated_rect() {
        let layout = StratagemLayout::default();
        let geom = layout.resolve(1920, 1080).expect("解析");
        let frame = solid_frame(1920, 1080, [10, 20, 30, 255]);
        let roi = extract_roi(&frame, &geom).expect("ROI");
        assert_eq!(roi.dimensions(), (geom.roi.w as u32, geom.roi.h as u32));
        assert_eq!(roi.get_pixel(0, 0).0, [10, 20, 30, 255]);
    }

    #[test]
    fn out_of_frame_crop_fails_with_context() {
        let frame = solid_frame(64, 64, [0, 0, 0, 255]);
        let err = crop_rect(&frame, ImageRect::new(60, 60, 32, 32)).expect_err("越界必须失败");
        assert_eq!(err.code(), "VisionImageProcessing");
        assert!(err.message().contains("超出帧"));
    }

    #[test]
    fn overlay_draws_inside_bounds_only() {
        let layout = StratagemLayout::default();
        let geom = layout.resolve(2560, 1440).expect("解析");
        let plan = super::super::grid::calibration_home_plan(&layout, &geom).expect("网格");
        let roi = RgbaImage::new(geom.roi.w as u32, geom.roi.h as u32);
        let overlaid = overlay_slots(&roi, &geom, &plan, &[(0, ImageRect::new(0, 0, 10, 10))]);
        assert_eq!(
            overlaid.dimensions(),
            (geom.roi.w as u32, geom.roi.h as u32)
        );
    }

    #[test]
    fn gray_png_roundtrip() {
        let dir = std::env::temp_dir().join("h2ac_vision_test_gray");
        let path = dir.join("mask.png");
        let mut img = image::GrayImage::new(4, 4);
        img.put_pixel(0, 0, image::Luma([255]));
        save_gray_png(&img, &path).expect("写盘");
        let back = image::open(&path).expect("读回").to_luma8();
        assert_eq!(back.get_pixel(0, 0)[0], 255);
        let _ = std::fs::remove_file(&path);
    }
}
