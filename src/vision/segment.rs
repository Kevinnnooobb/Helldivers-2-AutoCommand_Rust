// 前景分割（需求 §8 / §9 / §10 / §11）—— 整个视觉系统的核心。
//
// 四路信息：HSV + Lab + 局部对比度 + 空间先验；每像素得到一个可解释的前景分数：
//
//   foreground_score = w_orange·orange + w_white·white + w_contrast·contrast + w_center·center
//   foreground       = foreground_score > threshold
//   combined_mask    = (orange_mask | white_mask) & foreground
//
// 为什么不能只用 RGB 阈值（实测）：游戏格子里同时存在「白字图标」（≈248,248,243）
// 与「分类色字形」（≈170,250,251），而槽位边框的亮白线亮度与白字几乎相同 ——
// 只靠亮度/色度阈值必然把边框吃进前景。因此这里额外用：
//   * Lab 距离（对 gamma / 亮度整体偏移不敏感）；
//   * 局部对比度（只保留比周围更亮的内部结构，压掉大面积底色与高亮块）；
//   * 中心先验（贴边的 UI 线条天然落在低权重区）。
use image::RgbaImage;

use super::config::SegmentationConfig;

/// HSV（H ∈ [0,360)，S/V ∈ [0,1]）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsv {
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

/// CIELAB（D65）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

/// sRGB(0~255) → HSV。
pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> Hsv {
    let (rf, gf, bf) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let delta = max - min;
    let h = if delta <= f32::EPSILON {
        0.0
    } else if (max - rf).abs() <= f32::EPSILON {
        60.0 * (((gf - bf) / delta) % 6.0)
    } else if (max - gf).abs() <= f32::EPSILON {
        60.0 * ((bf - rf) / delta + 2.0)
    } else {
        60.0 * ((rf - gf) / delta + 4.0)
    };
    Hsv {
        h: if h < 0.0 { h + 360.0 } else { h },
        s: if max <= f32::EPSILON {
            0.0
        } else {
            delta / max
        },
        v: max,
    }
}

/// sRGB(0~255) → CIELAB（D65 白点，标准 sRGB 传递函数）。
pub fn rgb_to_lab(r: u8, g: u8, b: u8) -> Lab {
    fn linearize(c: f32) -> f32 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    let (rf, gf, bf) = (
        linearize(r as f32 / 255.0),
        linearize(g as f32 / 255.0),
        linearize(b as f32 / 255.0),
    );
    // sRGB → XYZ (D65)
    let x = (0.412_456_4 * rf + 0.357_576_1 * gf + 0.180_437_5 * bf) / 0.950_47;
    let y = 0.212_672_9 * rf + 0.715_152_2 * gf + 0.072_175_0 * bf;
    let z = (0.019_333_9 * rf + 0.119_192_0 * gf + 0.950_304_1 * bf) / 1.088_83;
    fn f(t: f32) -> f32 {
        const EPS: f32 = 216.0 / 24_389.0;
        const KAPPA: f32 = 24_389.0 / 27.0;
        if t > EPS {
            t.cbrt()
        } else {
            (KAPPA * t + 16.0) / 116.0
        }
    }
    let (fx, fy, fz) = (f(x), f(y), f(z));
    Lab {
        l: 116.0 * fy - 16.0,
        a: 500.0 * (fx - fy),
        b: 200.0 * (fy - fz),
    }
}

pub fn lab_distance(a: Lab, b: Lab) -> f32 {
    ((a.l - b.l).powi(2) + (a.a - b.a).powi(2) + (a.b - b.b).powi(2)).sqrt()
}

/// 平滑阶跃（标准 smoothstep）。
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if (edge1 - edge0).abs() <= f32::EPSILON {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 带羽化的单边软阈值（`lo` 以下 0，`lo + feather` 以上 1）。
fn ramp_up(value: f32, lo: f32, feather: f32) -> f32 {
    smoothstep(lo - feather, lo + feather, value)
}

/// 带羽化的双边软阈值。
fn soft_range(value: f32, lo: f32, hi: f32, feather: f32) -> f32 {
    ramp_up(value, lo, feather) * (1.0 - ramp_up(value, hi, feather))
}

/// 色相软命中（支持跨 0° 的区间）。
fn hue_score(h: f32, h_min: f32, h_max: f32, feather: f32) -> f32 {
    let (lo, hi) = (h_min, h_max);
    let span = if lo <= hi { hi - lo } else { hi + 360.0 - lo };
    if span <= f32::EPSILON {
        return 0.0;
    }
    let rel = if lo <= hi {
        h - lo
    } else if h >= lo {
        h - lo
    } else {
        h + 360.0 - lo
    };
    let rel = if rel < 0.0 { rel + 360.0 } else { rel };
    // 到区间边界的绕行距离：区间内取到最近边界的距离，区间外取到最近边界的距离。
    // 边界处两侧都取 0.5，保证「恰好落在阈值上」不会突变（软命中）。
    let (inside, edge_dist) = if rel <= span {
        (true, rel.min(span - rel))
    } else {
        (false, (rel - span).min(360.0 - rel))
    };
    let t = if inside {
        smoothstep(0.0, feather, edge_dist)
    } else {
        -smoothstep(0.0, feather, edge_dist)
    };
    0.5 * (t + 1.0)
}

/// 参考实现口径的颜色相似度：色度方向距离 + 亮度斜坡（对整体亮度缩放不敏感）。
///
/// 用于 Booster 黄色判定（`ChromaProfile::BOOSTER_YELLOW`），也可用于其他固定色。
pub fn chroma_likeness(r: u8, g: u8, b: u8, profile: &crate::vision::config::ChromaProfile) -> f32 {
    let sum = r as f32 + g as f32 + b as f32;
    if sum <= 1.0 {
        return 0.0;
    }
    let chroma = [r as f32 / sum, g as f32 / sum, b as f32 / sum];
    let distance = ((chroma[0] - profile.chroma[0]).powi(2)
        + (chroma[1] - profile.chroma[1]).powi(2)
        + (chroma[2] - profile.chroma[2]).powi(2))
    .sqrt();
    let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
    let brightness = 0.35 + 0.65 * smoothstep(profile.luma_low, profile.luma_full, luma);
    let chromaticity = 1.0 - smoothstep(profile.distance_full, profile.distance_zero, distance);
    (brightness * chromaticity).clamp(0.0, 1.0)
}

/// 单像素的橙色/桃色前景分数（0~1）。
pub fn orange_score(r: u8, g: u8, b: u8, cfg: &SegmentationConfig) -> f32 {
    let hsv = rgb_to_hsv(r, g, b);
    if hsv.v < cfg.orange_hsv.v_min * 0.5 {
        return 0.0;
    }
    let hue = hue_score(hsv.h, cfg.orange_hsv.h_min, cfg.orange_hsv.h_max, 8.0);
    if hue <= 0.0 {
        return 0.0;
    }
    let sat = soft_range(hsv.s, cfg.orange_hsv.s_min, cfg.orange_hsv.s_max, 0.08);
    let val = soft_range(hsv.v, cfg.orange_hsv.v_min, cfg.orange_hsv.v_max, 0.10);
    let lab = rgb_to_lab(r, g, b);
    let reference = Lab {
        l: cfg.orange_lab.l,
        a: cfg.orange_lab.a,
        b: cfg.orange_lab.b,
    };
    let distance = lab_distance(lab, reference);
    let lab_sim = 1.0
        - smoothstep(
            cfg.orange_lab.distance_full,
            cfg.orange_lab.distance_zero,
            distance,
        );
    (hue * sat * val * lab_sim).clamp(0.0, 1.0)
}

/// 单像素的白色/浅色前景分数（0~1）。
pub fn white_score(r: u8, g: u8, b: u8, cfg: &SegmentationConfig) -> f32 {
    let hsv = rgb_to_hsv(r, g, b);
    let val = if hsv.v >= cfg.white_value_min {
        1.0
    } else {
        smoothstep(
            (cfg.white_value_min - 0.18).max(0.0),
            cfg.white_value_min,
            hsv.v,
        )
    };
    let sat = 1.0
        - smoothstep(
            cfg.white_saturation_max,
            cfg.white_saturation_max + 0.12,
            hsv.s,
        );
    if val <= 0.0 || sat <= 0.0 {
        return 0.0;
    }
    let lab = rgb_to_lab(r, g, b);
    let reference = Lab {
        l: cfg.white_lab.l,
        a: cfg.white_lab.a,
        b: cfg.white_lab.b,
    };
    let distance = lab_distance(lab, reference);
    let lab_sim = 1.0
        - smoothstep(
            cfg.white_lab.distance_full,
            cfg.white_lab.distance_zero,
            distance,
        );
    (val * sat * lab_sim).clamp(0.0, 1.0)
}

/// 四路分数的逐像素图（`row-major`，尺寸与输入一致）。
#[derive(Debug, Clone, PartialEq)]
pub struct ScoreMap {
    pub width: usize,
    pub height: usize,
    pub orange: Vec<f32>,
    pub white: Vec<f32>,
    pub contrast: Vec<f32>,
    pub center: Vec<f32>,
    /// 相对亮度证据：相对格内稳健背景亮度的超额（0~1）。
    pub luma: Vec<f32>,
    pub combined: Vec<f32>,
    /// 本次分割估计出的背景亮度（0~1），诊断用。
    pub background_luma: f32,
}

/// 分割结果：掩码 + 分数图（分数图在 Trace 级别才写盘）。
#[derive(Debug, Clone, PartialEq)]
pub struct ForegroundMasks {
    pub width: usize,
    pub height: usize,
    /// 橙色/桃色主体
    pub orange: Vec<bool>,
    /// 白色/浅色符号
    pub white: Vec<bool>,
    /// 融合后的最终前景（形态学处理前）
    pub combined: Vec<bool>,
    pub scores: ScoreMap,
}

impl ForegroundMasks {
    pub fn foreground_px(&self) -> usize {
        self.combined.iter().filter(|v| **v).count()
    }

    /// 仅测试与诊断使用的通道计数。
    #[cfg(test)]
    pub fn orange_px(&self) -> usize {
        self.orange.iter().filter(|v| **v).count()
    }

    /// 仅测试与诊断使用的通道计数。
    #[cfg(test)]
    pub fn white_px(&self) -> usize {
        self.white.iter().filter(|v| **v).count()
    }

    /// 前景像素占内裁剪面积的比例。
    pub fn foreground_ratio(&self) -> f32 {
        let total = (self.width * self.height).max(1);
        self.foreground_px() as f32 / total as f32
    }

    /// 指定窗口内的前景占比（空槽判定的中心活动度用）。
    pub fn ratio_in(&self, x0: usize, y0: usize, x1: usize, y1: usize) -> f32 {
        let x1 = x1.min(self.width);
        let y1 = y1.min(self.height);
        if x1 <= x0 || y1 <= y0 {
            return 0.0;
        }
        let mut hit = 0usize;
        for y in y0..y1 {
            for x in x0..x1 {
                if self.combined[y * self.width + x] {
                    hit += 1;
                }
            }
        }
        hit as f32 / ((x1 - x0) * (y1 - y0)) as f32
    }
}

/// 对单元格图像（已内裁剪）做分割。
///
/// `cell` 必须已经是**内裁剪之后**的图像：贴边 UI 在这里再被 `border_inset_ratio`
/// 强制清零一次（双保险，需求 §10）。
pub fn segment_cell(cell: &RgbaImage, cfg: &SegmentationConfig) -> ForegroundMasks {
    let width = cell.width() as usize;
    let height = cell.height() as usize;
    let n = width * height;
    let mut orange_s = vec![0.0f32; n];
    let mut white_s = vec![0.0f32; n];
    let mut luma = vec![0.0f32; n];

    for y in 0..height {
        for x in 0..width {
            let p = cell.get_pixel(x as u32, y as u32).0;
            let i = y * width + x;
            orange_s[i] = orange_score(p[0], p[1], p[2], cfg);
            white_s[i] = white_score(p[0], p[1], p[2], cfg);
            luma[i] = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0;
        }
    }

    let local = box_blur(&luma, width, height, cfg.contrast_radius);
    let mut contrast = vec![0.0f32; n];
    for i in 0..n {
        let diff = (luma[i] - local[i]).max(0.0);
        let soft = if diff <= cfg.contrast_threshold {
            0.0
        } else {
            ((diff - cfg.contrast_threshold) * cfg.contrast_gain).min(1.0)
        };
        contrast[i] = soft;
    }

    let center = center_prior(width, height, cfg.center_sigma);
    let background_luma = background_luma(
        &luma,
        width,
        height,
        cfg.background_percentile,
        inset_of(width, height, cfg),
    );
    let luma_s = luma_evidence(&luma, background_luma, cfg.luma_delta);
    let weights = cfg.weights;
    let weight_sum =
        (weights.orange + weights.white + weights.contrast + weights.center + weights.luma)
            .max(1e-6);
    let mut combined_score = vec![0.0f32; n];
    let mut orange = vec![false; n];
    let mut white = vec![false; n];
    let mut combined = vec![false; n];

    let inset = inset_of(width, height, cfg);
    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            let weighted = (weights.orange * orange_s[i]
                + weights.white * white_s[i]
                + weights.contrast * contrast[i]
                + weights.center * center[i]
                + weights.luma * luma_s[i])
                / weight_sum;
            // 「最佳单通道证据」：一个通道给出高置信度就足以说明该像素是前景，
            // 不能被其它通道的加权平均稀释掉。
            //
            // 实机教训：游戏把图标白字渲染成逐行断开的 2px 短横线，其白色分数
            // 接近 1.0，但橙/对比/中心三个通道都很低；加权平均后只有约 0.31，
            // 恰好压着 0.30 的阈值，抗锯齿像素全部掉线 → 连通域碎成短划线 →
            // bbox 只剩底座（38×14）→ 与模板 IoU 掉到 0.5。
            let best_channel = orange_s[i].max(white_s[i]).max(luma_s[i]);
            let score = weighted.max(best_channel * 0.85);
            combined_score[i] = score;
            if x < inset
                || y < inset
                || x >= width.saturating_sub(inset)
                || y >= height.saturating_sub(inset)
            {
                continue; // 贴边 UI 一律判背景
            }
            // 前景判定：色度证据命中，或相对亮度证据命中。
            // 实机图标主体由「浅灰字形 + 铜色底座」两类色度截然不同的部件组成，
            // 只靠绝对颜色阈值必然漏掉其中之一（实测底座整块丢失 → bbox 只剩 14px 高）。
            let chroma_hit =
                orange_s[i] >= cfg.foreground_threshold || white_s[i] >= cfg.foreground_threshold;
            let luma_hit = luma_s[i] >= cfg.foreground_threshold;
            orange[i] = orange_s[i] >= cfg.foreground_threshold;
            white[i] = white_s[i] >= cfg.foreground_threshold;
            combined[i] = (chroma_hit || luma_hit) && score >= cfg.foreground_threshold;
        }
    }

    ForegroundMasks {
        width,
        height,
        orange,
        white,
        combined,
        scores: ScoreMap {
            width,
            height,
            orange: orange_s,
            white: white_s,
            contrast,
            center,
            luma: luma_s,
            combined: combined_score,
            background_luma,
        },
    }
}

/// 采样区四边内缩像素数（贴边 UI 一律判背景）。
fn inset_of(width: usize, height: usize, cfg: &SegmentationConfig) -> usize {
    (width.min(height) as f32 * cfg.border_inset_ratio.clamp(0.0, 0.45)).round() as usize
}

/// 格内稳健背景亮度：忽略贴边 UI 后的亮度分位数。
fn background_luma(
    luma: &[f32],
    width: usize,
    height: usize,
    percentile: f32,
    inset: usize,
) -> f32 {
    let mut values: Vec<f32> = Vec::with_capacity(width * height);
    for y in inset..height.saturating_sub(inset) {
        for x in inset..width.saturating_sub(inset) {
            values.push(luma[y * width + x]);
        }
    }
    if values.is_empty() {
        values.extend_from_slice(luma);
    }
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p = percentile.clamp(0.0, 0.5);
    // 向下取整避免把略亮于底色的像素算进「背景」。
    let index = ((values.len() - 1) as f32 * p).floor() as usize;
    values[index.min(values.len() - 1)]
}

/// 相对背景亮度的超额证据（软阈值，`delta` 处为 0.5）。
fn luma_evidence(luma: &[f32], background: f32, delta: f32) -> Vec<f32> {
    let feather = (delta * 0.5).max(1e-3);
    luma.iter()
        .map(|v| ramp_up(*v - background, delta, feather))
        .collect()
}

/// 中心先验：到格子中心的归一化距离的高斯衰减（sigma 为半边长比例）。
pub fn center_prior(width: usize, height: usize, sigma: f32) -> Vec<f32> {
    let (cx, cy) = (width as f32 / 2.0, height as f32 / 2.0);
    let sigma = sigma.max(0.05);
    let mut out = vec![0.0f32; width * height];
    for y in 0..height {
        for x in 0..width {
            let dx = (x as f32 + 0.5 - cx) / cx.max(1.0);
            let dy = (y as f32 + 0.5 - cy) / cy.max(1.0);
            let d2 = dx * dx + dy * dy;
            out[y * width + x] = (-d2 / (2.0 * sigma * sigma)).exp();
        }
    }
    out
}

/// 均值平滑（局部对比度的参考底）。
pub fn box_blur(src: &[f32], width: usize, height: usize, radius: i32) -> Vec<f32> {
    if radius <= 0 || width == 0 || height == 0 {
        return src.to_vec();
    }
    let mut out = vec![0.0f32; src.len()];
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let mut sum = 0.0f32;
            let mut count = 0.0f32;
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let sx = x + dx;
                    let sy = y + dy;
                    if sx >= 0 && sy >= 0 && sx < width as i32 && sy < height as i32 {
                        sum += src[sy as usize * width + sx as usize];
                        count += 1.0;
                    }
                }
            }
            out[y as usize * width + x as usize] = sum / count.max(1.0);
        }
    }
    out
}

// ─── 形态学（需求 §11） ───

/// 掩码的 3×3/5×5 形态学运算。半径以配置为准（默认 1，即 3×3）。
pub fn morphology(
    mask: &mut [bool],
    width: usize,
    height: usize,
    radius: i32,
    iterations: u32,
    dilate: bool,
) {
    if radius <= 0 || iterations == 0 || width == 0 || height == 0 {
        return;
    }
    let mut current = mask.to_vec();
    let mut next = current.clone();
    for _ in 0..iterations {
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                let mut hit = false;
                'scan: for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let sx = x + dx;
                        let sy = y + dy;
                        let inside = sx >= 0 && sy >= 0 && sx < width as i32 && sy < height as i32;
                        let v = inside && current[sy as usize * width + sx as usize];
                        if dilate && v {
                            hit = true;
                            break 'scan;
                        }
                        if !dilate && inside && !v {
                            hit = true;
                            break 'scan;
                        }
                    }
                }
                // dilate：窗口内存在前景 → 置前景；
                // erode ：窗口内存在背景 → 置背景（即全部为前景才保留）。
                next[y as usize * width + x as usize] = if dilate { hit } else { !hit };
            }
        }
        std::mem::swap(&mut current, &mut next);
    }
    mask.copy_from_slice(&current);
}

/// 开运算（先腐蚀后膨胀）：去孤立噪点与细刺。
pub fn opening(mask: &mut [bool], width: usize, height: usize, radius: i32, iterations: u32) {
    morphology(mask, width, height, radius, iterations, false);
    morphology(mask, width, height, radius, iterations, true);
}

/// 保形开运算：**只清除孤立噪点**，不收缩细长结构。
///
/// 为什么不能用各向同性开运算（实机实测教训）：游戏把战备图标渲染成
/// 2px 宽的短横线/短竖线（每段之间还有 1px 的抗锯齿缝隙）。3×3 腐蚀要求
/// 「一个像素的四邻都在前景」，会把所有 2px 宽的笔画整条抹掉 ——
/// 实测 slot 1 的前景从 487px 掉到 314px，只留下铜色底座，bbox 变成 38×14，
/// 归一化后与模板的 IoU 掉到 0.5，识别只能弃权。
///
/// 这里改为「孤立点清除」：仅当像素的 8 邻域内没有任何其它前景像素时才清除。
/// 单像素噪点被去掉，而宽度 ≥2px 的笔画完整保留。
pub fn opening_preserve_thin(mask: &mut [bool], width: usize, height: usize, iterations: u32) {
    if width == 0 || height == 0 || iterations == 0 {
        return;
    }
    for _ in 0..iterations {
        let current = mask.to_vec();
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                let i = y as usize * width + x as usize;
                if !current[i] {
                    continue;
                }
                let mut neighbour = false;
                'scan: for dy in -1..=1i32 {
                    for dx in -1..=1i32 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let (sx, sy) = (x + dx, y + dy);
                        if sx >= 0
                            && sy >= 0
                            && sx < width as i32
                            && sy < height as i32
                            && current[sy as usize * width + sx as usize]
                        {
                            neighbour = true;
                            break 'scan;
                        }
                    }
                }
                if !neighbour {
                    mask[i] = false;
                }
            }
        }
    }
}

/// 闭运算（先膨胀后腐蚀）：补内部小孔洞。
pub fn closing(mask: &mut [bool], width: usize, height: usize, radius: i32, iterations: u32) {
    morphology(mask, width, height, radius, iterations, true);
    morphology(mask, width, height, radius, iterations, false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn cfg() -> SegmentationConfig {
        SegmentationConfig::default()
    }

    fn img_with(f: impl Fn(u32, u32) -> [u8; 4], w: u32, h: u32) -> RgbaImage {
        let mut img = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.put_pixel(x, y, Rgba(f(x, y)));
            }
        }
        img
    }

    #[test]
    fn hsv_and_lab_conversions_are_sane() {
        let hsv = rgb_to_hsv(255, 0, 0);
        assert!((hsv.h - 0.0).abs() < 1e-3);
        assert!((hsv.s - 1.0).abs() < 1e-6);
        let hsv_green = rgb_to_hsv(0, 255, 0);
        assert!((hsv_green.h - 120.0).abs() < 1e-3);
        let white = rgb_to_lab(255, 255, 255);
        assert!((white.l - 100.0).abs() < 0.5);
        let black = rgb_to_lab(0, 0, 0);
        assert!(black.l.abs() < 0.5);
        assert!(lab_distance(white, black) > 90.0);
    }

    #[test]
    fn orange_glyph_beats_dark_cell_background() {
        // 实机实测：橙色主体 ≈ (232,160,90)；格子底色 ≈ (80,80,78)
        let glyph = orange_score(232, 160, 90, &cfg());
        let background = orange_score(80, 80, 78, &cfg());
        let border = orange_score(210, 210, 205, &cfg());
        assert!(glyph > 0.8, "橙色主体应判为橙色前景，实际 {glyph}");
        assert!(background < 0.1, "暗底色不得判为前景，实际 {background}");
        assert!(border < 0.2, "无彩色亮边框不得判为橙色，实际 {border}");
    }

    #[test]
    fn white_glyph_beats_bright_border_by_contrast() {
        // 白字与亮边框色度相同 —— 区分靠局部对比度与空间先验，而不是单一阈值
        let c = cfg();
        let white_px = white_score(248, 248, 243, &c);
        assert!(white_px > 0.8);
        // 灰底 → 不判白
        assert!(white_score(120, 120, 118, &c) < 0.5);
        // 饱和色（分类色字形）不应落入白通道
        assert!(white_score(170, 250, 251, &c) < 0.5);
    }

    #[test]
    fn segmentation_finds_glyph_and_ignores_uniform_background() {
        let c = cfg();
        // 80×80 灰底，中间 20×20 橙色方块（模拟图标主体）
        let img = img_with(
            |x, y| {
                if (30..50).contains(&x) && (30..50).contains(&y) {
                    [232, 160, 90, 255]
                } else {
                    [80, 80, 78, 255]
                }
            },
            80,
            80,
        );
        let masks = segment_cell(&img, &c);
        assert!(masks.foreground_px() > 200, "应提取到图标主体");
        assert!(masks.foreground_ratio() < 0.25, "不应把底色算成前景");
        assert!(masks.orange_px() > masks.white_px());
        // 中心活动度高 → 不是空槽
        assert!(masks.ratio_in(28, 28, 52, 52) > 0.5);
    }

    #[test]
    fn uniform_cell_is_empty_like() {
        let c = cfg();
        let img = img_with(|_, _| [80, 80, 78, 255], 80, 80);
        let masks = segment_cell(&img, &c);
        assert_eq!(masks.foreground_px(), 0);
        assert_eq!(masks.foreground_ratio(), 0.0);
        assert_eq!(masks.ratio_in(20, 20, 60, 60), 0.0);
    }

    #[test]
    fn border_ring_is_forced_to_background() {
        let c = cfg();
        // 整图橙色（模拟被边框污染的格子）：贴边一圈必须被内缩规则清掉
        let img = img_with(|_, _| [232, 160, 90, 255], 40, 40);
        let masks = segment_cell(&img, &c);
        let inset = (40.0 * c.border_inset_ratio).round() as usize;
        assert!(inset >= 1);
        for x in 0..masks.width {
            assert!(!masks.combined[x], "第一行必须被清空");
        }
        for y in 0..masks.height {
            assert!(!masks.combined[y * masks.width + masks.width - 1]);
        }
    }

    #[test]
    fn opening_removes_isolated_noise_and_closing_fills_holes() {
        let (w, h) = (16usize, 16usize);
        let mut mask = vec![false; w * h];
        mask[w + 1] = true; // 孤立噪点（远离主体）
        for y in 4..12 {
            for x in 4..12 {
                mask[y * w + x] = true;
            }
        }
        mask[7 * w + 7] = false; // 内部孔洞
        opening(&mut mask, w, h, 1, 1);
        assert!(!mask[w + 1], "开运算应去掉孤立噪点");
        assert!(mask[5 * w + 5], "主体必须保留");
        closing(&mut mask, w, h, 1, 1);
        assert!(mask[7 * w + 7], "闭运算应补上 1px 孔洞");
    }

    #[test]
    fn center_prior_decreases_towards_edges() {
        let prior = center_prior(21, 21, 0.6);
        let center = prior[10 * 21 + 10];
        let edge = prior[0];
        assert!(center > 0.99);
        assert!(edge < 0.3);
    }
}
