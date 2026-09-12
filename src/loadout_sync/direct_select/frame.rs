//! 参考实现的帧指纹（`hd2-preset-helper-0.1.4/src/loadout/frame.rs`）。
//!
//! 用途：**触发**语义扫描，而不是单独宣布「滚动成功」。
//! 指纹只回答「画面变了没有」；「视口是否按预期移动」由
//! [`super::page_relation`] 用「共同 identity 的垂直位移」回答。
//!
//! 这一点必须说清楚，否则很容易退化成参考实现明确避免的做法：
//! 拿一个整屏相似度阈值当滚动的成功判据。
use image::RgbaImage;

/// 指纹采样网格边长（参考实现 `ROI_FINGERPRINT_SAMPLES = 32`）。
pub const ROI_FINGERPRINT_SAMPLES: u32 = 32;

/// 参考实现的「画面变化」阈值（`PAGE_CHANGE_THRESHOLD = 6.0`）。
pub const PAGE_CHANGE_THRESHOLD: f32 = 6.0;

/// ROI 的 32×32 亮度指纹（luma601，逐点采样）。
pub fn image_fingerprint(rgba: &RgbaImage) -> Vec<u8> {
    let samples = ROI_FINGERPRINT_SAMPLES;
    let mut out = Vec::with_capacity((samples * samples) as usize);
    if rgba.width() == 0 || rgba.height() == 0 {
        return out;
    }
    for row in 0..samples {
        for col in 0..samples {
            let x = ((col as f32 + 0.5) * rgba.width() as f32 / samples as f32) as u32;
            let y = ((row as f32 + 0.5) * rgba.height() as f32 / samples as f32) as u32;
            let x = x.min(rgba.width() - 1);
            let y = y.min(rgba.height() - 1);
            let [r, g, b, _] = rgba.get_pixel(x, y).0;
            out.push(luma601_u8(r, g, b));
        }
    }
    out
}

/// luma601（与当前项目其他 luma 计算同一系数）。
pub fn luma601_u8(r: u8, g: u8, b: u8) -> u8 {
    ((77u32 * r as u32 + 150 * g as u32 + 29 * b as u32 + 128) >> 8) as u8
}

/// 两个指纹的平均绝对差。长度不一致 / 为空 → `INFINITY`（视为「完全变了」）。
pub fn fingerprint_distance(left: &[u8], right: &[u8]) -> f32 {
    if left.is_empty() || left.len() != right.len() {
        return f32::INFINITY;
    }
    let total: u32 = left
        .iter()
        .zip(right)
        .map(|(a, b)| a.abs_diff(*b) as u32)
        .sum();
    total as f32 / left.len() as f32
}

/// 画面是否发生了值得做语义扫描的变化。
pub fn frame_changed(left: &[u8], right: &[u8]) -> bool {
    fingerprint_distance(left, right) >= PAGE_CHANGE_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn solid(w: u32, h: u32, v: u8) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([v, v, v, 255]))
    }

    #[test]
    fn fingerprint_is_stable_for_identical_images() {
        let a = solid(64, 64, 100);
        let b = solid(64, 64, 100);
        let fa = image_fingerprint(&a);
        let fb = image_fingerprint(&b);
        assert_eq!(
            fa.len(),
            (ROI_FINGERPRINT_SAMPLES * ROI_FINGERPRINT_SAMPLES) as usize
        );
        assert_eq!(fingerprint_distance(&fa, &fb), 0.0);
        assert!(!frame_changed(&fa, &fb));
    }

    #[test]
    fn fingerprint_detects_a_change_above_threshold() {
        let a = solid(64, 64, 100);
        let b = solid(64, 64, 110); // 差 10 > 阈值 6
        assert!(frame_changed(
            &image_fingerprint(&a),
            &image_fingerprint(&b)
        ));
        // 差 3 < 阈值：不算变化（避免被抗锯齿/压缩噪声触发）
        let c = solid(64, 64, 103);
        assert!(!frame_changed(
            &image_fingerprint(&a),
            &image_fingerprint(&c)
        ));
    }

    #[test]
    fn fingerprint_handles_degenerate_input() {
        assert!(image_fingerprint(&RgbaImage::new(0, 0)).is_empty());
        assert_eq!(fingerprint_distance(&[], &[]), f32::INFINITY);
        assert_eq!(fingerprint_distance(&[1, 2], &[1]), f32::INFINITY);
        // 空指纹 → 视为完全变化，绝不误判为「没动」
        assert!(frame_changed(&[], &[1, 2]));
    }

    #[test]
    fn luma601_matches_weights() {
        assert_eq!(luma601_u8(255, 255, 255), 255);
        assert_eq!(luma601_u8(0, 0, 0), 0);
        // 绿色权重最高
        assert!(luma601_u8(0, 255, 0) > luma601_u8(255, 0, 0));
        assert!(luma601_u8(0, 255, 0) > luma601_u8(0, 0, 255));
    }
}
